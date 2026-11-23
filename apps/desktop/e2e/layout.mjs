// The sidebar can be hidden and shown (button before Home, ⌘B), and the row-number (SL) column of Table Data resizes and sorts.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await browser.newPage({ viewport: { width: 1500, height: 900 } });
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
await page.addInitScript({ content: fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
const calls = (cmd) => page.evaluate((c) => window.__calls.filter((x) => x.cmd === c), cmd);

await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");

// ---- sidebar toggle: first button of the toolbar, before Home
const toggle = page.locator("header.topbar button.sidebar-toggle");
assert.equal(await page.locator("header.topbar button").first().getAttribute("aria-label"), "Toggle sidebar");
assert.equal(await page.locator(".sidebar").isVisible(), true);
await toggle.click();
assert.equal(await page.locator(".sidebar").isVisible(), false);
assert.equal(await toggle.getAttribute("aria-pressed"), "false");
assert.equal(await page.locator("button[aria-label='Resize the sidebar'], [aria-label='Resize the sidebar']").count(), 0, "no splitter without the sidebar");
assert.equal(await page.evaluate(() => localStorage.getItem("mylodb.sidebarOpen")), "0");
await toggle.click();
assert.equal(await page.locator(".sidebar").isVisible(), true);
assert.equal(await page.locator(".tree .db").count() > 0, true, "the tree is still there");
ok("the toolbar button hides and shows the sidebar, and the tree keeps its state");

await page.keyboard.press("Meta+b");
assert.equal(await page.locator(".sidebar").isVisible(), false);
await page.reload();
await page.selectOption(".conn-bar select", "1").catch(() => {});
await page.waitForSelector("header.topbar");
assert.equal(await page.locator(".sidebar").isVisible(), false, "hidden stays hidden after a restart");
await page.keyboard.press("Meta+b");
assert.equal(await page.locator(".sidebar").isVisible(), true);
ok("⌘B toggles it and the choice is remembered");

// ---- SL column
await page.selectOption(".conn-bar select", "1").catch(() => {});
await page.waitForSelector(".tree .db");
await page.click(".tree .db");
await page.dblclick(".row-item.leaf:has-text('orders')");
await page.waitForSelector("table.dg tbody tr");
const sl = page.locator("table.dg thead tr:first-child th.rownum");
assert.equal((await sl.innerText()).trim(), "SL");
const colW = () => page.evaluate(() => document.querySelector("table.dg colgroup col").getBoundingClientRect().width);
const w0 = await colW();
const box = await sl.locator(".resizer").boundingBox();
await page.mouse.move(box.x + 2, box.y + 8); await page.mouse.down(); await page.mouse.move(box.x + 62, box.y + 8, { steps: 4 }); await page.mouse.up();
const w1 = await colW();
assert.ok(w1 >= w0 + 50, `SL widened from ${w0} to ${w1}`);
await page.mouse.move(box.x + 62, box.y + 8); await page.mouse.down(); await page.mouse.move(box.x - 200, box.y + 8, { steps: 4 }); await page.mouse.up();
assert.ok((await colW()) >= 36, "and cannot be dragged to nothing");
ok("the SL column is resizable like the others");

const lastSort = async () => (await calls("table_page")).at(-1).args.spec.sort;
await sl.click(); await page.waitForTimeout(450);
assert.deepEqual(await lastSort(), [{ column: "id", desc: false }]);
assert.match(await sl.innerText(), /▲/);
await sl.click(); await page.waitForTimeout(450);
assert.deepEqual(await lastSort(), [{ column: "id", desc: true }]);
assert.match(await sl.innerText(), /▼/);
await sl.click(); await page.waitForTimeout(450);
assert.deepEqual(await lastSort(), []);
assert.doesNotMatch(await sl.innerText(), /[▲▼]/);
ok("clicking SL sorts by the primary key ascending, descending, then off, with the arrow shown");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL LAYOUT UI CHECKS PASSED");
