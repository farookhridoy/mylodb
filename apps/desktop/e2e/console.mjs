// UI test of the workspace: resizable sidebar and console, table tab = console on top + data below,
// chevrons, name picking into the query, locked database = console database, sample queries.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await browser.newPage({ viewport: { width: 1400, height: 900 } });
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
page.on("console", (m) => m.type() === "error" && !m.text().includes("404") && errors.push("console: " + m.text()));
await page.addInitScript({ content: fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
const filter = page.locator(".filter input");
const editorText = () => page.evaluate(() => document.querySelector(".cm-content")?.innerText ?? "");
const width = () => page.locator(".sidebar").evaluate((e) => e.getBoundingClientRect().width);
const editorHeight = () => page.locator(".editor-wrap").evaluate((e) => e.getBoundingClientRect().height);
async function drag(sel, dx, dy) {
  const b = await page.locator(sel).boundingBox();
  const x = b.x + b.width / 2, y = b.y + b.height / 2;
  await page.mouse.move(x, y); await page.mouse.down(); await page.mouse.move(x + dx / 2, y + dy / 2); await page.mouse.move(x + dx, y + dy); await page.mouse.up();
}
const typeIntoEditor = async (text, replace = false) => {
  await page.click(".cm-content");
  if (replace) await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.type(text);
};

await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");

// ---- the sidebar can be resized by dragging, within limits, and the width is remembered
const w0 = await width();
await drag(".splitter-x", 120, 0);
const w1 = await width();
assert.ok(w1 > w0 + 100 && w1 < w0 + 140, `${w0} -> ${w1}`);
await drag(".splitter-x", -800, 0);
assert.equal(Math.round(await width()), 200, "never narrower than 200 px");
await drag(".splitter-x", 150, 0);
const saved = await width();
await page.reload();
await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
assert.equal(Math.round(await width()), Math.round(saved), "the width survives a restart");
ok("the sidebar is resizable by dragging, limited to a sensible range, and remembered");

// ---- a table opens as console (top) + data grid (bottom)
await page.click(".tree .db:has-text('shop')");
await page.dblclick(".row-item.leaf:has-text('orders')");
await page.waitForSelector("table.dg tbody tr");
assert.equal(await page.locator(".editor-wrap").isVisible(), true, "the console is above the data");
assert.match(await editorText(), /SELECT \* FROM `orders` LIMIT 1000;/);
assert.equal(await page.locator(".result-tabs button.active").innerText().then((t) => t.trim()), "Table Data");
assert.equal(await page.locator("table.dg tbody tr").count(), 3);
assert.equal(await page.locator(".dbsel select").inputValue(), "shop");
ok("a table opens as a console on top and its data grid below, in the table's database");

// ---- the two halves are resizable
const h0 = await editorHeight();
await drag(".splitter-y", 0, 90);
const h1 = await editorHeight();
assert.ok(h1 > h0 + 70 && h1 < h0 + 110, `${h0} -> ${h1}`);
ok("the console / results divider can be dragged");

// ---- running a live query shows its result next to the table data
await page.evaluate(() => { window.__runSets = [{ columns: [{ name: "one", type_name: "LONGLONG" }], rows: [["1"]], total_rows: 1, truncated: false }]; });
await typeIntoEditor("SELECT 1 AS one", true);
await page.keyboard.press("Meta+Enter");
await page.waitForSelector(".result-tabs button:text-is('Results')");
await page.waitForFunction(() => document.querySelector(".result-tabs button.active")?.textContent.trim() === "Results");
assert.equal(await page.locator("table.grid th:has-text('one')").count(), 1);
await page.click(".result-tabs button:has-text('Table Data')");
await page.waitForSelector("table.dg tbody tr");
ok("a query run in the console shows its result below; the table's data stays one click away");

// ---- chevrons open and close; the row selects
await page.click(".row-item.leaf:has-text('orders')");
await page.waitForSelector(".row-item.col:not(.idx):not(.idx-head)");
await page.click(".row-item.leaf:has-text('orders') .chev");
assert.equal(await page.locator(".row-item.col:not(.idx):not(.idx-head)").count(), 0, "the arrow collapses the table's columns");
assert.equal(await page.locator(".row-item.leaf.selected").count(), 1, "while the table stays selected");
await page.click(".row-item.leaf:has-text('orders') .chev");
assert.equal(await page.locator(".row-item.col:not(.idx):not(.idx-head)").count(), 4, "and opens them again");
await page.click(".row-item.group:has-text('Tables') .chev");
assert.equal(await page.locator(".row-item.leaf").count(), 0, "the arrow on Tables closes the list");
await page.click(".row-item.group:has-text('Tables') .chev");
assert.equal(await page.locator(".row-item.leaf").count(), 2);
await page.click(".row-item.db .chev");
assert.equal(await page.locator(".row-item.group").count(), 0, "the arrow on the database closes it");
assert.equal(await page.locator(".scope-name").innerText(), "shop", "the database stays locked");
await page.click(".row-item.db .chev");
assert.equal(await page.locator(".row-item.group:has-text('Tables')").count(), 1);
ok("the arrows open and close a table's columns, the Tables list and the database");

// ---- searching tables; selecting one does not lock the filter; the lock icon does
await filter.fill("ord");
await page.waitForFunction(() => window.__filterCalls?.some((x) => x.query === "ord" && x.scope === "shop"));
await page.waitForTimeout(250);
const orders = page.locator(".row-item.leaf").first();   // the Tables group comes first
assert.equal(await page.locator(".row-item.leaf:has-text('customers')").count(), 0, "only matching objects are listed");
await orders.click();
assert.equal(await filter.inputValue(), "ord", "the search text stays");
assert.equal(await filter.getAttribute("placeholder"), "Filter tables");
assert.equal(await page.locator(".lock-btn.on").count(), 0);
await orders.locator(".lock-btn").click();
assert.equal(await filter.getAttribute("placeholder"), "Filter columns");
assert.equal(await page.locator(".lock-btn.on").count(), 1);
await orders.locator(".lock-btn").click();   // the same icon unlocks
assert.equal(await filter.getAttribute("placeholder"), "Filter tables");
await filter.fill("");
ok("a table search keeps its results when a table is selected; the lock icon locks and unlocks the column search");

// ---- a new query tab runs in the locked database
await page.click(".query-tabs .plus");
assert.equal(await page.locator(".dbsel select").inputValue(), "shop", "queries run in the locked database");
ok("the locked database is the console's database");

// ---- clicking names pastes them into the query (only while the editor has the focus)
await typeIntoEditor("SELECT * FROM ", true);
await page.click(".row-item.leaf:has-text('orders')");
await page.waitForFunction(() => /FROM `orders`/.test(document.querySelector(".cm-content")?.innerText ?? ""));
assert.doesNotMatch(await editorText(), /`shop`/, "no database prefix inside the console's own database");
await typeIntoEditor(" WHERE ", false);
await page.click(".row-item.col:has-text('total')");
await page.waitForFunction(() => /WHERE `orders`\.`total`/.test(document.querySelector(".cm-content")?.innerText ?? ""));
const pasted = await editorText();
await page.locator(".statusbar").click();                       // focus leaves the editor
await page.click(".row-item.leaf:has-text('customers')");
await page.waitForTimeout(150);
assert.equal(await editorText(), pasted, "a click outside a query being written changes nothing");
await typeIntoEditor("\nSELECT ", false);
await page.click(".tree .db:has-text('shop')");
await page.waitForFunction(() => /SELECT `shop`/.test(document.querySelector(".cm-content")?.innerText ?? ""));
ok("clicking a table, column or database name pastes it into the query being written");

// ---- sample queries from the table menu
await page.click(".row-item.leaf:has-text('orders')", { button: "right" });
await page.hover(".ctx-item.sub:has-text('Generate SQL')");
for (const k of ["SELECT", "INSERT", "UPDATE", "DELETE"]) assert.equal(await page.locator(`.ctx-nested .ctx-label:text-is('${k}')`).count(), 1, k);
await page.click(".ctx-nested .ctx-label:text-is('INSERT')");
let t = await editorText();
assert.match(t, /INSERT INTO `orders`/);
assert.match(t, /\(`customer_id`, `total`, `note`\)/, "every column except the auto-increment id");
assert.match(t, /VALUES\s+\('customer_id', 'total', 'note'\)/);
await page.click(".row-item.leaf:has-text('orders')", { button: "right" });
await page.hover(".ctx-item.sub:has-text('Generate SQL')");
await page.click(".ctx-nested .ctx-label:text-is('UPDATE')");
t = await editorText();
assert.match(t, /UPDATE `orders`\s+SET\s+`customer_id` = 'customer_id',\s+`total` = 'total',\s+`note` = 'note'\s+WHERE `id` = 'id';/);
await page.click(".row-item.leaf:has-text('orders')", { button: "right" });
await page.hover(".ctx-item.sub:has-text('Generate SQL')");
await page.click(".ctx-nested .ctx-label:text-is('DELETE')");
assert.match(await editorText(), /DELETE FROM `orders`\s+WHERE `id` = 'id';/);
await page.click(".row-item.leaf:has-text('orders')", { button: "right" });
await page.hover(".ctx-item.sub:has-text('Generate SQL')");
await page.click(".ctx-nested .ctx-label:text-is('SELECT')");
assert.match(await editorText(), /SELECT\s+`id`,\s+`customer_id`,\s+`total`,\s+`note`\s+FROM `orders`/);
ok("the table menu offers SELECT, INSERT, UPDATE and DELETE samples with the real column names");

await browser.close();
assert.deepEqual(errors, [], "browser errors: " + errors.join(" | "));
console.log("ALL CONSOLE / WORKSPACE UI CHECKS PASSED");
