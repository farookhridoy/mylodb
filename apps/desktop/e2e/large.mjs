// UI test of large tables: windowed rows, keyset paging, estimated counts, export jobs.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await browser.newPage({ viewport: { width: 1400, height: 900 } });
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
page.on("console", (m) => m.type() === "error" && !m.text().includes("404") && errors.push("console: " + m.text()));
await page.addInitScript({ content: "window.__bigRows = 12000; window.__estimate = 5000000; window.__jobMs = 1500;\n" + fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
const g = (n) => page.evaluate((k) => window[k], n);
const set = (k, v) => page.evaluate(([a, b]) => { window[a] = b; }, [k, v]);
const specs = async () => (await g("__pageSpecs")) ?? [];
const lastSpec = async () => (await specs()).at(-1);
const bodyRows = () => page.locator("table.dg tbody tr:not(.vspacer)");
const rownums = () => page.locator("table.dg tbody tr:not(.vspacer) td.rownum").allInnerTexts().then((t) => t.map((x) => Number(x.replace(/\D/g, ""))));
const pager = () => page.locator(".pager").innerText().then((t) => t.replace(/\s+/g, " ").trim());
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
const settle = () => page.waitForTimeout(450);

await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
await page.click(".tree .db:has-text('shop')");
await page.dblclick(".row-item.leaf:has-text('events')");
await page.waitForSelector("table.dg tbody tr");
await page.selectOption(".dg-toolbar select[title='Rows per page']", "5000");
await page.waitForFunction(() => document.querySelector(".pager")?.textContent.includes("5,000"));
await settle();

// ---- only the rows on screen are in the page
const drawn = await bodyRows().count();
assert.ok(drawn > 20 && drawn < 90, `${drawn} rows drawn for a page of 5,000`);
assert.equal((await rownums())[0], 1);
assert.equal(await page.locator("table.dg tr.vspacer").count(), 1, "a spacer stands in for the rows below");
ok(`a page of 5,000 rows draws only ${drawn} rows`);

// ---- scrolling brings later rows in; spacers keep the scrollbar honest
const scroll = (top) => page.evaluate((t) => { const el = document.querySelector(".dg-scroll"); el.scrollTop = t; }, top);
await scroll(2000 * 22);
await page.waitForFunction(() => [...document.querySelectorAll("table.dg tbody tr td.rownum")].some((td) => /^2,?0\d\d$/.test(td.textContent.trim()) || Number(td.textContent.replace(/\D/g, "")) > 1900));
const around = await rownums();
assert.ok(around.some((n) => n > 1990 && n < 2060), JSON.stringify(around.slice(0, 5)));
assert.ok((await bodyRows().count()) < 90);
const height = await page.evaluate(() => document.querySelector(".dg-scroll").scrollHeight);
assert.ok(height > 5000 * 22 - 200, `scroll height ${height} covers all 5,000 rows`);
ok("scrolling swaps the drawn rows; the scroll height still covers all 5,000");

// ---- keyboard: click a visible cell, End / Home / PageDown move and scroll
await page.locator("table.dg tbody tr:not(.vspacer)").nth(3).locator("td").nth(2).click();
await page.keyboard.press("Meta+End");
await page.waitForFunction(() => [...document.querySelectorAll("table.dg tbody tr td.rownum")].some((td) => td.textContent.replace(/\D/g, "") === "5000"));
assert.equal(await page.locator("td.active").count(), 1, "the active cell is drawn after jumping to the last row");
assert.equal(await page.locator("td.active").evaluate((td) => td.parentElement.querySelector(".rownum").textContent.replace(/\D/g, "")), "5000");
await page.keyboard.press("Meta+Home");
await page.waitForFunction(() => document.querySelector("td.active")?.parentElement.querySelector(".rownum")?.textContent.trim() === "1");
await page.keyboard.press("PageDown");
const afterPageDown = await page.locator("td.active").evaluate((td) => Number(td.parentElement.querySelector(".rownum").textContent.replace(/\D/g, "")));
assert.ok(afterPageDown > 10, `PageDown moved to row ${afterPageDown}`);
ok("Cmd+End, Cmd+Home and PageDown move through the rows and scroll the active one into view");

// ---- keyset paging: the requests carry a cursor, not an offset
assert.equal(await page.locator("button[title='First page']").isDisabled(), true, "nothing before the first page");
const before = (await specs()).length;
await page.locator("button[title='Next page']").click();
await page.waitForFunction((n) => window.__pageSpecs.length > n, before);
let spec = await lastSpec();
assert.deepEqual(spec.cursor, { after: ["5000"] }, "next page = after the last key");
await settle();
assert.equal((await rownums())[0], 5001);
await page.locator("button[title='Next page']").click();
await page.waitForFunction(() => window.__pageSpecs.at(-1).cursor?.after?.[0] === "10000");
await settle();
await page.locator("button[title='Previous page']").click();
await page.waitForFunction(() => window.__pageSpecs.at(-1).cursor?.before);
spec = await lastSpec();
assert.deepEqual(spec.cursor, { before: ["10001"] });
await settle();
assert.equal((await rownums())[0], 5001, "previous returns to the same rows");
await page.locator("button[title='Last page']").click();
await page.waitForFunction(() => window.__pageSpecs.at(-1).cursor === "end");
await settle();
const last = await rownums();
assert.equal(last.at(-1) >= 11990 || last[0] >= 7000, true, `the last page shows the end of the table: ${last[0]}`);
await page.locator("button[title='First page']").click();
await page.waitForFunction(() => window.__pageSpecs.at(-1).cursor === null && window.__pageSpecs.at(-1).offset === 0);
ok("Next, Previous, Last and First page by primary-key cursor ('after', 'before', 'end'), not by offset");

// ---- a user sort puts paging back on OFFSET
await page.locator("table.dg th:has-text('kind')").first().click();
await settle();
await page.locator("button[title='Next page']").click();
await page.waitForFunction(() => window.__pageSpecs.at(-1).offset === 5000);
assert.equal((await lastSpec()).cursor, null, "sorted results are not in key order");
await page.locator("table.dg th:has-text('kind')").first().click();   // desc
await page.locator("table.dg th:has-text('kind')").first().click();   // off
await settle();
ok("sorting by a column switches paging back to OFFSET");

// ---- the total is an estimate until you ask
assert.match(await pager(), /of ≈ 5,000,000/);
assert.equal(await g("__countCalls"), undefined, "no exact count was started: the estimate is big");
await set("__slowCount", true);
await page.locator(".count-btn").click();
await page.waitForSelector("text=counting…");
await page.locator("button:text-is('Stop')").click();
await page.waitForFunction(() => !document.body.innerText.includes("counting…"));
assert.match(await pager(), /of ≈ 5,000,000/, "stopping keeps the estimate");
assert.equal((await g("__cancelledRuns")).length, 1, "the count's query was cancelled");
await set("__slowCount", false);
await page.locator(".count-btn").click();
await page.waitForFunction(() => document.querySelector(".pager")?.textContent.includes("of 12,000"));
assert.equal(await page.locator(".count-btn").count(), 0);
ok("big tables show ≈ estimate at once; Count takes the exact number, Stop cancels it and keeps the estimate");

// ---- export is a job with progress, cancel and gzip
await page.click("button:text-is('Export ▾')");
await page.locator(".popup label:has-text('gzip') input").check();
await set("__nextSave", "/tmp/events.jsonl.gz");
await page.click(".popup button.link:has-text('JSON Lines')");
await page.waitForSelector(".dg-job .job-progress");
assert.match(await page.locator(".dg-job").innerText(), /120,000 rows written/);
await page.locator(".dg-job button:text-is('Cancel')").click();
await page.waitForSelector(".banner:has-text('Export cancelled')");
assert.equal(await page.locator(".dg-job").count(), 0, "the progress bar goes away");
const job = await g("__exportTable");
assert.deepEqual([job.options.format, job.options.gzip, job.path], ["json_lines", true, "/tmp/events.jsonl.gz"]);
assert.ok(job.jobId, "the job has an id the Cancel button can use");
await page.click("button:text-is('Export ▾')");
await set("__nextSave", "/tmp/events.jsonl.gz");
await page.click(".popup button.link:has-text('JSON Lines')");
await page.waitForSelector(".banner.ok:has-text('Exported 12,000 row(s) to /tmp/events.jsonl.gz')", { timeout: 8000 });
ok("an export shows progress, Cancel stops it, and the gzip choice is sent");

await browser.close();
assert.deepEqual(errors, [], "browser errors: " + errors.join(" | "));
console.log("ALL LARGE TABLE UI CHECKS PASSED");
