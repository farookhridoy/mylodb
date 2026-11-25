// UI end-to-end test of the data grid. Runs the real React app against a fake Tauri bridge (e2e/mock.js).
// Start the dev server first (`npm run dev`), then: CHROME_PATH=/path/to/chrome npm run e2e
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

// Set CHROME_PATH to a Chrome/Chromium binary, or install one with `npx playwright-core install chromium`.
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await browser.newPage({ viewport: { width: 1500, height: 900 } });
const errors = [];
page.on("response", (r) => r.status() === 404 && errors.push("404: " + r.url()));
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
page.on("console", (m) => m.type() === "error" && !m.text().includes("404") && errors.push("console: " + m.text()));
await page.addInitScript({ content: fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");

const calls = (cmd) => page.evaluate((c) => window.__calls.filter((x) => x.cmd === c), cmd);
const last = async (cmd) => (await calls(cmd)).at(-1).args;
const settle = () => page.waitForTimeout(450);
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);

// open connection, then the orders table
await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
await page.click(".tree .db");
await page.dblclick(".row-item.leaf:has-text('orders')");
await page.waitForSelector("table.dg tbody tr");
assert.equal(await page.locator("table.dg tbody tr").count(), 3);
assert.deepEqual(await page.locator("table.dg thead tr:first-child th .th-name").allTextContents(), ["id", "customer_id", "total", "note"]);
assert.equal(await page.locator("th:has-text('id') .ico-pk").count() > 0, true);
assert.equal(await page.locator("th:has-text('customer_id') .icon-link").count() > 0, true);
ok("table opens as a grid tab with PK/FK markers");

// ---- column-wise filter: text contains ----
const filterRow = page.locator("tr.filter-row th");
const colIndex = { id: 1, customer_id: 2, total: 3, note: 4 };
const fInput = (c) => filterRow.nth(colIndex[c]).locator("input").first();
await fInput("note").fill("x");
await settle();
assert.deepEqual((await last("table_page")).spec.filters, [{ column: "note", op: "contains", values: ["x"] }]);
assert.equal(await page.locator("table.dg tbody tr").count(), 1);
ok("column filter sends a contains filter and narrows rows");

// ---- operator change + second column filter combine with AND ----
await filterRow.nth(colIndex.total).locator("select").selectOption("gt");
await filterRow.nth(colIndex.total).locator("input").first().fill("1");
await settle();
let spec = (await last("table_page")).spec;
assert.deepEqual(spec.filters.map((f) => [f.column, f.op, f.values.join()]), [["total", "gt", "1"], ["note", "contains", "x"]].sort((a, b) => 0) && spec.filters.map((f) => [f.column, f.op, f.values.join()]));
assert.equal(spec.filters.length, 2);
ok("two column filters combine");

// clear all
await page.click("button[title='Clear all column filters']");
await settle();
assert.deepEqual((await last("table_page")).spec.filters, []);
assert.equal(await page.locator("table.dg tbody tr").count(), 3);
ok("clear filters restores rows");

// ---- NULL operator filter (no value input) ----
await filterRow.nth(colIndex.note).locator("select").selectOption("is_null");
await settle();
assert.deepEqual((await last("table_page")).spec.filters, [{ column: "note", op: "is_null", values: [] }]);
assert.equal(await page.locator("table.dg tbody tr").count(), 1);
await page.click("button[title='Clear all column filters']"); await settle();
ok("IS NULL filter works without a value");

// ---- sorting ----
await page.click("th:has(.th-name:text-is('total'))");
await settle();
assert.deepEqual((await last("table_page")).spec.sort, [{ column: "total", desc: false }]);
await page.click("th:has(.th-name:text-is('total'))");
await settle();
assert.deepEqual((await last("table_page")).spec.sort, [{ column: "total", desc: true }]);
await page.click("th:has(.th-name:text-is('id'))", { modifiers: ["Shift"] });
await settle();
assert.deepEqual((await last("table_page")).spec.sort, [{ column: "total", desc: true }, { column: "id", desc: false }]);
ok("click sorts asc/desc, shift-click adds a second sort key");
await page.click("text=ORDER BY >> .. >> button.mini"); await settle();
assert.deepEqual((await last("table_page")).spec.sort, []);

// ---- WHERE box ----
await page.fill(".dg-where input", "total > 6");
await page.press(".dg-where input", "Enter");
await settle();
assert.equal((await last("table_page")).spec.where_sql, "total > 6");
await page.fill(".dg-where input", ""); await page.press(".dg-where input", "Enter"); await settle();
assert.equal((await last("table_page")).spec.where_sql, null);
ok("WHERE box is sent and cleared");

// ---- context menu: filter by value / exclude ----
const cell = (r, c) => page.locator(`table.dg tbody tr:nth-child(${r}) td:nth-child(${c + 2})`);
// a TEXT cell opens the editor window (CellModal); type the value there and apply it
const editText = async (r, c, v) => { await cell(r, c).dblclick(); await page.fill(".cell-modal-text", v); await page.click(".cell-modal button.primary:has-text('Apply')"); };
await cell(1, 2).click({ button: "right" });
await page.click(".ctx-label:text-is('Filter by this value')");
await settle();
assert.deepEqual((await last("table_page")).spec.filters, [{ column: "total", op: "eq", values: ["10.50"] }]);
await page.click("button[title='Clear all column filters']"); await settle();
await cell(2, 3).click({ button: "right" });                     // a NULL note
await page.click(".ctx-label:text-is('Filter by this value')");
await settle();
assert.deepEqual((await last("table_page")).spec.filters, [{ column: "note", op: "is_null", values: [] }]);
await page.click("button[title='Clear all column filters']"); await settle();
ok("context menu filters by value (NULL becomes IS NULL)");

// ---- inline edit stages a change, submit sends exactly one update keyed by PK ----
await editText(1, 3, "edited note");
assert.equal(await cell(1, 3).getAttribute("class").then((c) => c.includes("edited")), true);
assert.match(await page.locator("button.primary:has-text('Submit')").innerText(), /Submit \(1\)/);
await page.click("button.primary:has-text('Submit')");
await settle();
const applied = (await last("apply_changes")).changes;
assert.deepEqual(applied, [{ op: "update", key: [{ column: "id", value: { kind: "text", value: "1" } }], set: [{ column: "note", value: { kind: "text", value: "edited note" } }] }]);
assert.equal(await cell(1, 3).innerText(), "edited note");
ok("inline edit -> staged -> submit sends a keyed update and reloads");

// reverting the cell to its original value un-stages it
await editText(1, 3, "changed");
await editText(1, 3, "edited note");
assert.equal(await page.locator("button.primary:has-text('Submit')").isDisabled(), true);
ok("editing back to the original value removes the pending change");

// ---- Set NULL / DEFAULT via menu ----
await cell(1, 3).click({ button: "right" });
await page.click(".ctx-label:text-is('Set NULL')");
assert.equal(await cell(1, 3).innerText(), "NULL");
await page.click("button[title='Revert pending changes']");
assert.equal(await cell(1, 3).innerText(), "edited note");
ok("Set NULL stages, revert undoes");

// ---- delete a row ----
await page.click("table.dg tbody tr:nth-child(3) td.rownum");
await page.keyboard.press("Delete");
assert.equal(await page.locator("tr.row-deleted").count(), 1);
await page.click("button.primary:has-text('Submit')"); await settle();
assert.deepEqual((await last("apply_changes")).changes, [{ op: "delete", key: [{ column: "id", value: { kind: "text", value: "3" } }] }]);
assert.equal(await page.locator("table.dg tbody tr").count(), 2);
ok("Delete key stages a delete; submit sends it");

// ---- add a row ----
await page.click("button[title='Add row']");
assert.equal(await page.locator("tr.row-new").count(), 1);
await page.locator("tr.row-new td:nth-child(3)").dblclick();
await page.fill(".cell-edit input", "2");
await page.press(".cell-edit input", "Tab");   // commits and moves the selection right
await page.keyboard.press("F2");               // F2 starts editing the next cell (total)
await page.fill(".cell-edit input", "42.00");
await page.press(".cell-edit input", "Enter");
await page.click("button.primary:has-text('Submit')"); await settle();
const ins = (await last("apply_changes")).changes;
assert.equal(ins.length, 1);
assert.equal(ins[0].op, "insert");
assert.deepEqual(ins[0].values, [{ column: "customer_id", value: { kind: "text", value: "2" } }, { column: "total", value: { kind: "text", value: "42.00" } }]);
ok("new row only sends the columns the user filled (others use DEFAULT)");

// ---- pending changes block navigation until confirmed ----
await editText(1, 3, "pending!");
const before = (await calls("table_page")).length;
await fInput("note").fill("zzz");
await settle();
assert.equal(await page.locator(".dialog:has-text('Discard 1 pending')").count(), 1);
assert.equal((await calls("table_page")).length, before, "no reload while the prompt is open");
await page.click(".dialog button:text-is('Cancel')");
assert.match(await page.locator("button.primary:has-text('Submit')").innerText(), /Submit \(1\)/);
ok("changing filters with pending edits asks before discarding");
await page.click("button[title='Revert pending changes']");
await page.click("button[title='Clear all column filters']").catch(() => {}); await settle();

// ---- FK navigation: Cmd/Ctrl-click opens the referenced row in a new tab ----
await fInput("note").fill("");
await settle();
await cell(1, 1).click({ modifiers: ["Meta"] });
await page.waitForSelector(".qtab:has-text('customers')");
await settle();
assert.deepEqual((await last("table_page")).spec.filters, [{ column: "id", op: "eq", values: ["1"] }]);
assert.equal(await page.locator("table.dg tbody tr").count(), 1);
ok("cmd-click on a foreign key opens the referenced row");

// ---- paging controls reflect has_more ----
await page.click(".qtab:has-text('orders')");
await page.waitForSelector("table.dg");
await page.selectOption(".dg-toolbar select[title='Rows per page']", "100");
assert.equal(await page.locator("button[title='Next page']").isDisabled(), true);
ok("pager disables Next when there is no more data");

// ---- value viewer + form view open without errors ----
await page.click("button:text-is('Value')");
await cell(1, 3).click();
assert.equal(await page.locator(".value-panel").count(), 1);
await page.click("button:text-is('Form')");
assert.equal(await page.locator(".record-fields label").count(), 4);
ok("value viewer and form view render");

// ---- session restore keeps table tabs + their filters ----
await fInput("note").fill("e"); await settle();
await page.waitForTimeout(1000);
const saved = JSON.parse((await last("session_set")).session);
const orders = saved.tabs.find((t) => t.kind === "table" && t.table.name === "orders");
assert.deepEqual(orders.grid.filters.note, { op: "contains", text: "e", text2: "" });
ok("table tabs and their filters are written to the saved session");

await browser.close();
assert.deepEqual(errors, [], "browser errors: " + errors.join(" | "));
console.log("ALL UI CHECKS PASSED");
