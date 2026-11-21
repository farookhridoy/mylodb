// UI end-to-end test of Backup, Restore, Import CSV and Export, against the fake Tauri bridge in mock.js
// (which emits real progress events and honours Cancel). Needs the dev server (`npm run dev`) and CHROME_PATH.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
page.on("console", (m) => m.type() === "error" && !m.text().includes("404") && errors.push("console: " + m.text()));
await page.addInitScript({ content: fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
const g = (n) => page.evaluate((k) => window[k], n);
const set = (n, v) => page.evaluate(([k, x]) => { window[k] = x; }, [n, v]);
const settle = (ms = 500) => page.waitForTimeout(ms);
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);

await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
await page.click(".tree .db");

// ======================================================= backup
await page.click("button[title='Backup, restore, import and export']");
await page.waitForSelector(".transfer");
assert.equal(await page.locator(".transfer .dz-tabs button.active").innerText(), "Backup");
assert.equal(await page.locator("button:text-is('Start backup')").isDisabled(), true, "nothing chosen yet");
await page.check(".db-pick label:has-text('shop') input");
assert.equal(await page.locator("button:text-is('Start backup')").isDisabled(), true, "no folder yet");
await set("__nextOpen", "/Users/me/backups");
await page.click("button:text-is('Browse…')");
await page.waitForFunction(() => document.querySelector(".path-field input")?.value === "/Users/me/backups");
assert.equal(await page.locator(".xfer-form label:has-text('File name') input").inputValue(), "shop", "named after the first database picked");
assert.equal(await page.locator("button:text-is('Start backup')").isDisabled(), false);
ok("backup needs at least one database and a folder; the file name defaults to the database");

await page.click("button:text-is('Start backup')");
await page.waitForSelector(".job-progress");
assert.match(await page.locator(".job-line").innerText(), /Backing up · 50% · shop\.orders/);
await page.waitForSelector(".result-card");
let b = await g("__backup");
assert.deepEqual(b.options.databases, [{ name: "shop", tables: null }]);
assert.equal(b.dir, "/Users/me/backups");
assert.equal(b.name, "shop");
for (const k of ["structure", "data", "drop_if_exists", "create_database", "views", "routines", "triggers", "events", "snapshot", "disable_fk_checks", "strip_definer"]) assert.equal(b.options[k], true, k);
assert.equal(b.options.file_per_table, false);
assert.equal(b.options.compress, false);
assert.equal(b.options.rows_per_insert, 100);
const card = await page.locator(".result-card").innerText();
assert.match(card, /Backup complete[\s\S]*1 database\(s\), 2 table\(s\), 1,234 row\(s\), 5\.0 MB/);
assert.match(card, /shop-20261120-101500\.sql/);
assert.match(card, /MyISAM/, "server warnings are shown");
ok("backup sends the safe defaults, shows live progress, and reports what was written");

// options and table subset
await page.check("label:has-text('One file per table') input");
await page.check("label:has-text('Compress into a .zip') input");
await page.uncheck("label:has-text('Table data') input");
await page.locator(".db-pick .mini").click();
await page.uncheck(".tbl-sub label:has-text('All tables') input");
await page.uncheck(".tbl-sub label:has-text('customers') input");
await page.click("button:text-is('Start backup')");
await page.waitForSelector(".result-card");
b = await g("__backup");
assert.deepEqual(b.options.databases, [{ name: "shop", tables: ["orders", "v_orders"] }], "views are backed up with the tables");
assert.deepEqual([b.options.file_per_table, b.options.compress, b.options.data, b.options.structure], [true, true, false, true]);
ok("table selection and output options are passed through");

// cancel
await set("__jobMs", 5000);
await page.click("button:text-is('Start backup')");
await page.waitForSelector(".job-progress");
await page.click(".job-line button:text-is('Cancel')");
await page.waitForSelector(".banner:has-text('Backup cancelled')");
assert.equal(await g("__cancelled"), (await g("__backup")).jobId, "the cancel request names the job that was running");
assert.equal(await page.locator(".job-progress").count(), 0);
assert.equal(await page.locator("button:text-is('Start backup')").isDisabled(), false, "the form is usable again");
await set("__jobMs", undefined);
ok("cancelling stops the job, names it, and re-enables the form");

// ======================================================= restore
await page.click(".transfer .dz-tabs button:text-is('Restore / run script')");
assert.equal(await page.locator("button:text-is('Run script…')").isDisabled(), true);
await set("__nextOpen", "/Users/me/backups/dump.sql");
await page.click("button:text-is('File…')");
await page.waitForFunction(() => document.querySelector(".path-field input")?.value.endsWith("dump.sql"));
const refreshed = (await g("__refreshed")) || 0;
await page.click("button:text-is('Run script…')");
assert.match(await page.locator(".dialog").innerText(), /Run dump\.sql now\?[\s\S]*cannot be undone/);
await page.click(".dialog button:text-is('Cancel')");
assert.equal(await g("__run"), undefined, "nothing runs until confirmed");
await page.click("button:text-is('Run script…')");
await page.click(".dialog button.danger, .dialog button.primary");
await page.waitForSelector(".result-card");
const run = await g("__run");
assert.equal(run.path, "/Users/me/backups/dump.sql");
assert.deepEqual(run.options, { stop_on_error: false, disable_fk_checks: true, database: null });
assert.match(await page.locator(".result-card").innerText(), /Finished with errors[\s\S]*10 statement\(s\)[\s\S]*2 failed/);
assert.equal(await page.locator(".err-table tbody tr").count(), 2);
assert.match(await page.locator(".err-table tbody tr").first().innerText(), /dump\.sql\s+5\s+INSERT INTO t[\s\S]*Duplicate entry/);
assert.ok(((await g("__refreshed")) || 0) > refreshed, "the object tree is refreshed after a restore");
ok("restore asks first, shows per-statement errors with line numbers, and refreshes the tree");

await page.check("label:has-text('Stop at the first error') input");
await page.selectOption(".xfer-form label:has-text('Run in database') select", "shop");
await page.click("button:text-is('Run script…')");
await page.click(".dialog button.danger, .dialog button.primary");
await page.waitForSelector(".result-card");
assert.deepEqual((await g("__run")).options, { stop_on_error: true, disable_fk_checks: true, database: "shop" });
ok("restore options (stop on error, target database) are sent");

// ======================================================= import CSV from a table's menu
await page.click(".row-item.leaf:has-text('orders')", { button: "right" });
await page.click(".ctx-item:text-is('Import CSV into this table…')");
await page.waitForSelector(".transfer .dz-tabs button.active:text-is('Import CSV')");
assert.equal(await page.locator(".xfer-form label:has-text('Table') select").first().inputValue(), "orders", "the table is preselected");
await set("__nextOpen", "/Users/me/orders.csv");
await page.click("button:text-is('Browse…')");
await page.waitForSelector(".map-table table");
const pv = (await g("__previews")).at(-1);
assert.equal(pv.format.delimiter, ";", "the delimiter was detected from the file");
assert.equal(await page.locator(".xfer-form label:has-text('Delimiter') select").inputValue(), ";");
const maps = await page.locator(".map-table thead tr:first-child select").evaluateAll((els) => els.map((e) => e.value));
assert.deepEqual(maps, ["id", "", ""], "columns are mapped by header name; unknown ones are skipped");
assert.equal(await page.locator(".map-table tbody tr").count(), 2);
ok("import: the file's delimiter is detected, the preview is shown, and columns map by name");

await page.locator(".map-table thead tr:first-child select").nth(2).selectOption("total");
await page.selectOption(".xfer-form label:has-text('If a row with the same key exists') select", "update");
await page.fill(".xfer-form label:has-text('Text meaning NULL') input", "\\N");
await page.selectOption(".xfer-form label:has-text('Encoding') select", "windows-1252");
await page.uncheck("label:has-text('Stop at the first bad row') input");
await page.click("button:has-text('Import (2 columns)')");
await page.waitForSelector(".job-progress");
await page.waitForSelector(".result-card");
let imp = await g("__import");
assert.equal(imp.path, "/Users/me/orders.csv");
assert.deepEqual(imp.plan.mapping, ["id", null, "total"]);
assert.deepEqual([imp.plan.database, imp.plan.table, imp.plan.mode, imp.plan.stop_on_error, imp.plan.truncate_first, imp.plan.empty_as_null], ["shop", "orders", "update", false, false, true]);
assert.deepEqual([imp.format.delimiter, imp.format.encoding, imp.format.null_text, imp.format.has_header], [";", "windows-1252", "\\N", true]);
assert.match(await page.locator(".result-card").innerText(), /Import finished[\s\S]*2 row\(s\) read, 2 affected/);
assert.match(await page.locator(".result-card").innerText(), /MyISAM/);
ok("import sends the mapping, mode and CSV format exactly as chosen");

// truncating needs confirmation
await page.check("label:has-text('Empty the table first') input");
await page.click("button:has-text('Import (2 columns)')");
assert.match(await page.locator(".dialog").innerText(), /Empty shop\.orders before importing/);
await page.click(".dialog button:text-is('Cancel')");
const before = await g("__import");
await set("__import", undefined);
await page.click("button:has-text('Import (2 columns)')");
await page.click(".dialog button.primary");
await page.waitForSelector(".result-card");
assert.equal((await g("__import")).plan.truncate_first, true);
assert.equal(before.plan.truncate_first, false);
ok("emptying the table first is confirmed");

// bad rows are listed
await set("__importErrors", [{ line: 3, message: "Data too long for column 'name' (1406)" }, { line: 9, message: "Incorrect integer value (1366)" }]);
await page.uncheck("label:has-text('Empty the table first') input");
await page.click("button:has-text('Import (2 columns)')");
await page.waitForSelector(".result-card:has-text('2 bad row(s)')");
assert.match(await page.locator(".result-card").innerText(), /line 3: Data too long[\s\S]*line 9: Incorrect integer/);
await set("__importErrors", undefined);
ok("bad rows are reported with their line numbers");

// ---- import into a NEW table
await page.check("label:has-text('Create a new table from the file') input");
await page.waitForSelector(".map-table thead input");
assert.equal(await page.locator(".xfer-form label:has-text('New table name') input").inputValue(), "orders", "named after the file");
await page.fill(".xfer-form label:has-text('New table name') input", "orders_2026");
const typeInputs = page.locator(".map-table thead tr:nth-child(2) input");
assert.deepEqual(await typeInputs.evaluateAll((e) => e.map((x) => x.value)), ["int", "varchar(255)", "int"]);
await typeInputs.nth(2).fill("bigint");
await page.locator(".map-table thead tr:nth-child(3) input").nth(1).fill("full_name");
await page.click("button:has-text('Import (3 columns)')");
await page.waitForSelector(".result-card");
const created = await g("__created");
assert.deepEqual([created.db, created.table], ["shop", "orders_2026"]);
assert.deepEqual(created.columns, [{ name: "id", column_type: "int" }, { name: "full_name", column_type: "varchar(255)" }, { name: "qty", column_type: "bigint" }], "the table is created exactly as edited");
imp = await g("__import");
assert.equal(imp.plan.table, "orders_2026");
assert.deepEqual(imp.plan.mapping, ["id", "full_name", "qty"], "renaming a column keeps its mapping");
ok("import into a new table: inferred columns can be edited, the table is created, then loaded");

// ======================================================= export a query
await page.click(".transfer .dz-tabs button:text-is('Export query')");
await page.fill(".sql-box", "SELECT id, total FROM orders WHERE total > 5");
await page.selectOption(".xfer-form label:has-text('Format') select", "xlsx");
await set("__nextSave", "/Users/me/out.xlsx");
await page.click("button:text-is('Export…')");
await page.waitForSelector(".result-card");
const eq = await g("__exportQuery");
assert.equal(eq.sql, "SELECT id, total FROM orders WHERE total > 5");
assert.deepEqual([eq.options.format, eq.path, eq.database], ["xlsx", "/Users/me/out.xlsx", null]);
assert.match(await page.locator(".result-card").innerText(), /Exported 42 row\(s\)[\s\S]*out\.xlsx/);
await page.fill(".sql-box", "DELETE FROM orders");
await set("__nextSave", "/Users/me/x.csv");
await page.selectOption(".xfer-form label:has-text('Format') select", "csv");
await page.click("button:text-is('Export…')");
await page.waitForSelector(".banner.bad:has-text('only SELECT')");
await set("__nextSave", null);
await page.click("button:text-is('Export…')");
await settle(300);
assert.equal(await page.locator(".result-card").count(), 0, "cancelling the save dialog exports nothing");
ok("export query: format and path are sent; a non-SELECT error is shown; cancelling the dialog does nothing");

// ======================================================= grid export has every format
await page.click(".row-item.leaf:has-text('orders')", { button: "right" });
await page.click(".ctx-item:text-is('Open data')");
await page.waitForSelector("table.dg");
await page.click("button:text-is('Export ▾')");
assert.deepEqual((await page.locator(".popup button.link").allInnerTexts()).map((t) => t.replace("…", "")), ["CSV", "JSON", "SQL INSERT", "XML", "HTML table", "Excel (XLSX)"]);
await set("__nextSave", "/Users/me/orders.xml");
await page.click(".popup button.link:has-text('XML')");
await page.waitForSelector(".banner.ok:has-text('Exported')");
const et = await g("__exportTable");
assert.deepEqual([et.options.format, et.path, et.table], ["xml", "/Users/me/orders.xml", "orders"]);
assert.match(await page.locator(".banner.ok").innerText(), /Exported 3 row\(s\) to \/Users\/me\/orders\.xml — 1 cell\(s\) longer than 32767/);
ok("the grid's export menu offers CSV, JSON, SQL, XML, HTML and Excel and shows format warnings");

await browser.close();
assert.deepEqual(errors, [], "browser errors: " + errors.join(" | "));
console.log("ALL TRANSFER UI CHECKS PASSED");
