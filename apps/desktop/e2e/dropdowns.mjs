// UI test of the searchable dropdowns (select2-like): search box on long lists, keyboard, grouping, cap on huge lists.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await browser.newPage({ viewport: { width: 1400, height: 900 } });
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
page.on("console", (m) => m.type() === "error" && !m.text().includes("404") && errors.push("console: " + m.text()));
await page.addInitScript({ content: "window.__bigServer = { dbs: 2000, tables: 3 };\n" + fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);

// ---- a short list has no search box
await page.locator(".conn-bar .ss-trigger").click();
assert.equal(await page.locator(".ss-pop").count(), 1);
assert.equal(await page.locator(".ss-search").count(), 0, "short lists are not searchable");
assert.deepEqual(await page.locator(".ss-opt").allInnerTexts(), ["Select connection…", "Local"]);
await page.locator(".ss-opt:text-is('Local')").click();
await page.waitForSelector(".tree .db");
assert.equal(await page.locator(".conn-bar .ss-trigger").innerText(), "Local");
assert.equal(await page.locator(".conn-bar select").inputValue(), "1", "the hidden select follows");
assert.equal(await page.locator(".ss-pop").count(), 0, "choosing closes the list");
ok("short dropdowns open without a search box, choose and close, and keep the hidden select in sync");

// ---- a long list (2,000 databases) is searchable, capped, and keyboard friendly
const dbs = page.locator(".dbsel .ss-trigger");
await dbs.click();
assert.equal(await page.locator(".ss-search input").count(), 1, "long lists have a search box");
assert.equal(await page.evaluate(() => document.activeElement?.getAttribute("aria-label")), "Search options", "the search box has the focus");
assert.ok((await page.locator(".ss-opt").count()) <= 301, "at most 300 rows are drawn");
assert.match(await page.locator(".ss-none").innerText(), /more — type to narrow/);
await page.keyboard.type("db0042");
await page.waitForFunction(() => document.querySelectorAll(".ss-opt").length === 1);
assert.equal(await page.locator(".ss-opt mark").innerText(), "db0042", "the match is highlighted");
await page.keyboard.press("Enter");
assert.equal(await dbs.innerText(), "db0042");
assert.equal(await page.locator(".dbsel select").inputValue(), "db0042");
ok("a long list has a search box with focus, draws at most 300 rows, filters as you type, highlights and chooses with Enter");

// ---- more keyboard: arrows, no matches, Escape, outside click
await dbs.click();
await page.keyboard.type("db001");
await page.waitForFunction(() => document.querySelectorAll(".ss-opt").length === 10);
await page.keyboard.press("ArrowDown");
await page.keyboard.press("ArrowDown");
await page.keyboard.press("Enter");
assert.equal(await dbs.innerText(), "db0012", "two arrows down from the first match, then Enter");
await dbs.click();
await page.keyboard.type("zzzz");
assert.equal(await page.locator(".ss-none").innerText(), "No matches");
await page.keyboard.press("Escape");
assert.equal(await page.locator(".ss-pop").count(), 0);
assert.equal(await dbs.innerText(), "db0012", "Escape changes nothing");
await dbs.click();
await page.mouse.click(700, 500);
assert.equal(await page.locator(".ss-pop").count(), 0, "a click outside closes it");
await dbs.focus();
await page.keyboard.press("ArrowDown");
assert.equal(await page.locator(".ss-pop").count(), 1, "the keyboard opens it too");
await page.keyboard.press("Escape");
ok("arrows and Enter choose, 'No matches' is shown, Escape and an outside click close without changing, the keyboard opens it");

// ---- the console really uses the chosen database
assert.equal(await page.locator(".dbsel select").inputValue(), "db0012");

// ---- grouped options (the designer's data types) can be searched too
await page.click(".tree .db:has-text('shop')").catch(async () => { await page.fill(".filter input", "shop"); await page.waitForTimeout(300); await page.click(".tree .db:has-text('shop')"); });
await page.click(".row-item.leaf:has-text('orders')", { button: "right" });
await page.click(".ctx:not(.ctx-nested) > .ctx-item .ctx-label:text-is('Design table…')");
await page.waitForSelector(".designer .dz-grid");
const typeBox = page.locator(".dz-grid tbody tr").nth(2).locator(".ss-trigger").first();
await typeBox.click();
assert.ok((await page.locator(".ss-group").count()) >= 3, "the data types are grouped");
assert.equal(await page.locator(".ss-search input").count(), 1);
await page.keyboard.type("varc");
await page.waitForFunction(() => [...document.querySelectorAll(".ss-opt")].every((o) => /varchar/i.test(o.textContent)) && document.querySelectorAll(".ss-opt").length >= 1);
await page.keyboard.press("Enter");
assert.match(await typeBox.innerText(), /varchar/i);
ok("grouped options show their headings, are searchable, and choosing one updates the column");

await browser.close();
assert.deepEqual(errors, [], "browser errors: " + errors.join(" | "));
console.log("ALL DROPDOWN UI CHECKS PASSED");
