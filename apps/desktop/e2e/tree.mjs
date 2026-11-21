// UI test of the object tree and its filter on a very large fake server (about 90,000 objects):
// the filter is scoped to the selected database, columns load on demand, and the page stays small and fast.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await browser.newPage({ viewport: { width: 1500, height: 950 } });
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
page.on("console", (m) => m.type() === "error" && !m.text().includes("404") && errors.push("console: " + m.text()));
await page.addInitScript({ content: "window.__bigServer = { dbs: 2000, tables: 40, hugeDb: 6000 };\n" + fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
const g = (n) => page.evaluate((k) => window[k], n);
const rows = () => page.locator(".tree li").count();
const filter = page.locator(".filter input");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
const calls = async () => (await g("__filterCalls")) ?? [];

assert.equal(await filter.isDisabled(), true, "no connection yet");
const t0 = Date.now();
await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
const connectMs = Date.now() - t0;
assert.ok(connectMs < 4000, `connecting to a 90k-object server took ${connectMs} ms`);
assert.ok(await rows() <= 620, `the database list is capped (${await rows()} rows)`);
assert.equal(await g("__loadCalls"), undefined, "connecting loads no columns");
assert.equal(await filter.getAttribute("placeholder"), "Filter databases");
ok(`a 90,000-object server connects in ${connectMs} ms with a bounded tree and no column loading`);

// ---- no database selected: the filter only looks at database names
await filter.fill("db00");
await page.waitForFunction(() => window.__filterCalls?.length > 0);
await page.waitForTimeout(250);
let c = await calls();
assert.deepEqual(c.at(-1), { query: "db00", scope: null });
assert.equal(await page.locator(".tree .db").count(), 100, "db0000 … db0099");
assert.equal(await page.locator(".row-item.leaf").count(), 0, "no tables are listed while filtering databases");
ok("with nothing selected the filter matches database names only");

// ---- the reported scenario: search databases, click one, and it is locked
await filter.fill("db00");
await page.waitForFunction(() => window.__filterCalls?.some((x) => x.query === "db00"));
await page.click(".tree .db:has-text('db0005')");
assert.equal(await filter.getAttribute("placeholder"), "Filter tables");
assert.equal(await filter.inputValue(), "", "the box is now for tables");
assert.equal(await page.locator(".tree .db").count(), 1, "only the locked database is listed, not all 2,000 again");
assert.equal(await page.locator(".scope-name").innerText(), "db0005");
assert.equal(await page.locator(".row-item.leaf").count(), 40, "its tables are listed");
await filter.fill("t_0001");
await page.waitForFunction(() => window.__filterCalls?.some((x) => x.query === "t_0001" && x.scope === "db0005"));
await page.waitForTimeout(250);
assert.equal(await page.locator(".row-item.leaf").count(), 10, "t_00010 … t_00019");
assert.match(await page.locator(".row-item.group:has-text('Tables') .badge").innerText(), /^10$/);
await filter.fill("");
await page.waitForFunction(() => document.querySelectorAll(".row-item.leaf").length === 40);
ok("search databases, click one: it is locked, only it is listed, tables are searched, clearing the text shows all its tables");

// ---- typing stays responsive
const timing = await page.evaluate(async () => {
  const input = document.querySelector(".filter input");
  const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
  const worst = [];
  for (const q of ["t", "t_", "t_0", "t_00", "t_001", "t_0012", "t_00123", "t_001234"]) {
    const before = (window.__filterCalls || []).length;
    const start = performance.now();
    set.call(input, q); input.dispatchEvent(new Event("input", { bubbles: true }));
    await new Promise((r) => { const tick = () => ((window.__filterCalls || []).length > before ? setTimeout(r, 30) : setTimeout(tick, 10)); tick(); });
    worst.push(performance.now() - start);
  }
  return { worst: Math.max(...worst), rows: document.querySelectorAll(".tree li").length };
});
assert.ok(timing.worst < 1200, `slowest keystroke took ${timing.worst.toFixed(0)} ms`);
assert.ok(timing.rows < 700, `rows in the tree: ${timing.rows}`);
ok(`typing is responsive on a 90k-object server (slowest keystroke ${timing.worst.toFixed(0)} ms, ${timing.rows} rows)`);

// ---- a database with thousands of tables is shown a page at a time
await filter.fill("");
await page.waitForTimeout(250);
await page.click(".row-item.root");                              // release the lock: all databases again
await page.click(".tree .db:has-text('db0000')");
assert.equal(await page.locator(".row-item.leaf").count(), 300);
const moreTables = page.locator(".row-item.more", { hasText: /of 5,7\d\d…/ });
assert.match(await moreTables.innerText(), /Show 300 more of 5,7\d\d…/);
await moreTables.click();
assert.equal(await page.locator(".row-item.leaf").count(), 600);
assert.ok(await rows() < 1500);
ok("6,040 tables render 300 at a time, with a 'show more' row");

// ---- columns load on demand: details per table when selected, whole-database names only for col: filters
await page.click(".row-item.root");
await page.click(".tree .db:has-text('shop')");
assert.equal(await g("__loadCalls"), undefined, "expanding a database loads no columns");
assert.equal(await g("__detailCalls"), undefined, "nor any column details");
await page.click(".row-item.leaf:has-text('orders')");
await page.waitForTimeout(300);
await page.waitForSelector(".row-item.col:has-text('total')");
await page.click(".scope .x");                                  // orders is locked; release it to pick another table
await page.click(".row-item.leaf:has-text('customers')");
await page.waitForTimeout(300);
await page.waitForSelector(".row-item.col:has-text('email')");
assert.deepEqual(await g("__detailCalls"), ["shop.orders", "shop.customers"], "one query per table, not per database");
assert.equal(await g("__loadCalls"), undefined, "selecting tables never loads the whole database's columns");
ok("selecting a table fetches that table's columns only");

// ---- column filter inside the selected database
await page.click(".scope .x");                                  // release the table lock: back to the database level
await filter.fill("col:total");
await page.waitForFunction(() => window.__filterCalls?.some((x) => x.query === "col:total" && x.scope === "shop"));
await page.waitForTimeout(250);
assert.deepEqual(await page.locator(".row-item.leaf").allInnerTexts().then((t) => t.map((x) => x.replace(/[▾▸]/g, "").split("\n").map((l) => l.trim()).find(Boolean))), ["orders"], "only the table that has a matching column");
assert.ok((await page.locator(".row-item.col").allInnerTexts()).some((t) => t.includes("total")));
ok("col: filters by column inside the selected database");

// ---- shortcuts
await filter.fill("");
await page.locator("body").click({ position: { x: 900, y: 600 } });
await page.keyboard.press("Meta+Shift+b");
await page.waitForFunction(() => document.activeElement?.matches(".filter input"), null, { timeout: 2000 });
await filter.fill("zzz");
await page.keyboard.press("Escape");
assert.equal(await filter.inputValue(), "", "Esc clears the filter");
ok("⇧⌘B focuses the filter and Esc clears it");

await browser.close();
assert.deepEqual(errors, [], "browser errors: " + errors.join(" | "));
console.log("ALL TREE UI CHECKS PASSED");
