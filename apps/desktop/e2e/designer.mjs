// UI end-to-end test of the table designer, table operations and the users manager,
// against the fake Tauri bridge in mock.js. Needs the dev server (`npm run dev`) and CHROME_PATH.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await browser.newPage({ viewport: { width: 1600, height: 950 } });
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
page.on("console", (m) => m.type() === "error" && !m.text().includes("404") && errors.push("console: " + m.text()));
await page.addInitScript({ content: fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");

const calls = (cmd) => page.evaluate((c) => window.__calls.filter((x) => x.cmd === c), cmd);
const last = async (cmd) => (await calls(cmd)).at(-1)?.args;
const settle = () => page.waitForTimeout(450);
const g = (name) => page.evaluate((n) => window[n], name);
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);

await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
await page.click(".tree .db");

// ---------- designer ----------
await page.click(".row-item.leaf:has-text('orders')", { button: "right" });
for (const label of ["View Data", "Design table…", "Read data in SQL console", "Duplicate…", "Rename…", "Truncate…", "Drop…"]) {
  assert.equal(await page.locator(`.ctx-label:text-is('${label}')`).count(), 1, label);
}
await page.click(".ctx-label:text-is('Design table…')");
await page.waitForSelector(".designer .dz-grid");
assert.equal(await page.locator(".dz-grid tbody tr").count(), 4);
assert.equal(await page.locator(".dz-name").inputValue(), "orders");
assert.equal(await page.locator(".dz-grid tbody tr:nth-child(1) input[type=checkbox]").first().isChecked(), true, "id is the primary key");
assert.equal(await page.locator("button:text-is('Apply changes')").isDisabled(), true, "nothing changed yet");
ok("table opens in the designer with its columns and primary key");

// the buttons and the column titles stay put while the rows scroll
await page.setViewportSize({ width: 1600, height: 330 });
const top = (sel) => page.locator(sel).first().evaluate((e) => Math.round(e.getBoundingClientRect().top));
const before = [await top(".dz-actions"), await top(".dz-grid thead th")];
const scrolled = await page.locator(".dz-scroll").evaluate((e) => { e.scrollTop = 500; return e.scrollTop; });
assert.ok(scrolled > 0, "the rows scroll");
assert.equal(await top(".dz-actions"), before[0]);
assert.ok(Math.abs((await top(".dz-grid thead th")) - before[1]) <= 2, "the titles stay (to within the border)");
await page.setViewportSize({ width: 1600, height: 950 });
ok("the column buttons and titles stay in place while the rows scroll");

// refresh reads the table again; with unapplied changes it asks first
const reads = (await calls("table_def")).length;
await page.click(".dz-head button:has-text('Refresh')");
await settle();
assert.equal((await calls("table_def")).length, reads + 1);
await page.locator(".dz-name").fill("orders2");
await settle();
await page.click(".dz-head button:has-text('Refresh')");
assert.equal((await calls("table_def")).length, reads + 1, "not yet");
await page.click(".dz-head button:has-text('Discard changes?')");
await settle();
assert.equal(await page.locator(".dz-name").inputValue(), "orders");
assert.equal((await calls("table_def")).length, reads + 2);
ok("Refresh reads the table again, and asks before discarding changes");

// an AUTO_INCREMENT column is put at the front of the primary key
const cell = (n, k) => page.locator(`.dz-grid tbody tr:nth-child(${n}) input[type=checkbox]`).nth(k);
await cell(1, 0).uncheck();
await cell(1, 3).uncheck();
await cell(2, 0).check();
await cell(1, 0).check();
await cell(1, 3).check();
await settle();
let key = (await last("ddl_preview")).desired.indexes.find((i) => i.kind === "primary").columns.map((c) => c.name);
assert.equal(key[0], (await last("ddl_preview")).desired.columns[0].name, `the auto column leads the key: ${key}`);
assert.equal(key.length, 2);
await page.click(".dz-head button:has-text('Revert')");
await settle();
ok("ticking Auto inc puts that column first in the primary key");

// rename a column, widen another
const row = (n) => page.locator(`.dz-grid tbody tr:nth-child(${n})`);
await row(4).locator("input.cell-input").first().fill("remark");
await settle();
assert.equal(await page.locator("button:text-is('Apply changes')").isDisabled(), false);
await page.click("button:text-is('SQL •')");
assert.match(await page.locator(".dz-sql .preview").innerText(), /ALTER orders/);
await page.click("button:has-text('Columns')");
ok("editing marks the design as changed and shows the generated SQL");

// foreign key follows a column rename
await row(2).locator("input.cell-input").first().fill("cust");
await page.click("button:has-text('Foreign keys')");
assert.match(await page.locator(".dz-item").first().innerText(), /\(cust\)/);
await page.click("button:has-text('Columns')");
ok("renaming a column updates foreign keys that use it");

// type change resets parameters and flags
await row(3).locator("select").first().selectOption("varchar");
assert.equal(await row(3).locator("input.cell-input").nth(1).inputValue(), "255");
await row(3).locator("select").first().selectOption("int");
assert.equal(await row(3).locator("input.cell-input").nth(1).inputValue(), "");
assert.equal(await row(3).locator("input[type=checkbox]").nth(1).isEnabled(), true, "unsigned is available for int");
await row(3).locator("select").first().selectOption("decimal");
assert.equal(await row(3).locator("input.cell-input").nth(1).inputValue(), "10,2");
ok("changing a column type sets sensible parameters");

// primary key toggle edits the PRIMARY index
await row(3).locator("input[type=checkbox]").first().check();
await page.click("button:has-text('Indexes')");
assert.match(await page.locator(".dz-item").first().innerText(), /PRIMARY[\s\S]*id, total/);
await page.click("button:has-text('Columns')");
ok("PK checkbox adds the column to the primary key");

// add + remove a column
await page.click("button:text-is('＋ Add column')");
assert.equal(await page.locator(".dz-grid tbody tr").count(), 5);
await page.click("button:text-is('－ Remove')");
assert.equal(await page.locator(".dz-grid tbody tr").count(), 4);
ok("add and remove column");

// apply -> payload carries original and desired, and original_name tracks renames
await page.click("button:text-is('Apply changes')");
await settle();
const applied = await g("__applied");
assert.equal(applied.db, "shop");
assert.equal(applied.original.columns.find((c) => c.name === "note").original_name, "note");
const renamed = applied.desired.columns.find((c) => c.name === "remark");
assert.equal(renamed.original_name, "note", "a rename keeps the server-side name so the diff can emit CHANGE COLUMN");
assert.equal(applied.desired.foreign_keys[0].columns[0], "cust");
assert.equal(applied.desired.indexes[0].columns.map((c) => c.name).join(), "id,total");
assert.ok((await g("__refreshed")) >= 1, "the tree index is refreshed after applying");
assert.equal(await page.locator("button:text-is('Apply changes')").isDisabled(), true, "after applying, the design is clean");
ok("apply sends original + desired; tree refreshes; designer resets");

// ---------- new table ----------
await page.click(".tree .db", { button: "right" });
await page.hover(".ctx-item.sub:has-text('Create')");
await page.click(".ctx-nested .ctx-label:text-is('New table…')");
await page.waitForSelector(".qtab.active:has-text('New table')");
assert.equal(await page.locator("button:text-is('Create table')").isDisabled(), true, "needs a name");
assert.match(await page.locator(".banner.bad").innerText(), /table name is empty/);
await page.locator(".dz-name").fill("invoices");
await settle();
assert.equal(await page.locator("button:text-is('Create table')").isDisabled(), false);
await page.click("button:text-is('Create table')");
await settle();
const created = await g("__applied");
assert.equal(created.original, null);
assert.equal(created.desired.name, "invoices");
assert.equal(created.desired.columns[0].name, "id");
assert.equal(created.desired.indexes[0].kind, "primary");
ok("new table starts with an id primary key and is created from a blank definition");

// ---------- table operations ----------
await page.click(".row-item.leaf:has-text('customers')", { button: "right" });
await page.click(".ctx-label:text-is('Drop…')");
await page.waitForSelector(".preview-dialog");
assert.equal(await page.locator(".preview-sql").innerText(), "DROP TABLE `shop`.`customers`;", "the exact statement is shown before anything runs");
assert.equal(await g("__runPlans"), undefined, "nothing ran yet");
await page.click("button.danger:text-is('Drop table')");
await page.waitForSelector(".plan-results li");
assert.deepEqual((await g("__runPlans")).at(-1), ["DROP TABLE `shop`.`customers`;"]);
await page.click(".preview-dialog button:text-is('Done')");
await settle();
assert.equal(await page.locator(".row-item.leaf:has-text('customers')").count(), 0, "the table is gone from the tree");
ok("drop shows the exact SQL first, runs it on Run, and refreshes the tree");

await page.click(".row-item.leaf:has-text('orders')", { button: "right" });
await page.click(".ctx-label:text-is('Duplicate…')");
await page.fill(".dialog input:not([type=checkbox])", "orders_bak");
await page.check(".dialog input[type=checkbox]");
await page.click(".dialog button.primary");
await page.waitForSelector(".preview-dialog");
assert.equal(await page.locator(".preview-sql").innerText(), "CREATE TABLE `shop`.`orders_bak` LIKE `shop`.`orders`;\nINSERT INTO `shop`.`orders_bak` SELECT * FROM `shop`.`orders`;");
await page.click("button.danger:text-is('Duplicate')");
await page.waitForSelector(".plan-results li");
await page.click(".preview-dialog button:text-is('Done')");
await page.click(".row-item.leaf:has-text('orders')", { button: "right" });
await page.click(".ctx-label:text-is('Rename…')");
await page.fill(".dialog input", "orders2");
await page.click(".dialog button.primary");
await page.waitForSelector(".preview-dialog");
assert.equal(await page.locator(".preview-sql").innerText(), "RENAME TABLE `shop`.`orders` TO `shop`.`orders2`;");
await page.click("button.danger:text-is('Rename')");
await page.waitForSelector(".plan-results li");
await page.click(".preview-dialog button:text-is('Done')");
await settle();
assert.equal(await page.locator(".qtab:has-text('orders2')").count() > 0, true, "open tabs are retitled after a rename");
ok("duplicate (with data) and rename are previewed as SQL, then run");

// create / drop database
await page.click(".tree .db", { button: "right" });
await page.click(".ctx-label:text-is('Create database…')");
await page.fill(".dialog input", "newdb");
await page.click(".dialog button.primary");
await settle();
assert.deepEqual((await g("__dbOps")).at(-1), { op: "create", name: "newdb", charset: "utf8mb4", collation: null });
ok("create database");

// ---------- users ----------
await page.click("button[title='Users and privileges']");
await page.waitForSelector(".users .u-list li");
assert.equal(await page.locator(".u-list li").count(), 2);
assert.equal(await page.locator(".u-list li:has-text('report') .icon-lock").count(), 1, "locked accounts are marked");
await page.click(".u-list li:has-text('app')");
await page.waitForSelector("text=Account locked");
await page.click(".dz-tabs button:text-is('Privileges')");
assert.equal(await page.locator(".dz-item").count(), 2, "database scope + table scope");
assert.match(await page.locator(".dz-item").first().innerText(), /Database shop[\s\S]*2 privilege/);
assert.equal(await page.locator("summary:text-is('role')").count(), 0);
assert.equal(await page.locator("details summary:has-text('not editable')").count(), 1, "unsupported grants are listed read-only");
// toggle INSERT on the database scope
await page.locator(".dz-item").first().click();
const insert = page.locator(".priv-grid label:has-text('INSERT') input").first();
assert.equal(await insert.isChecked(), false);
await insert.check();
await settle();
const pv = await g("__lastPreview");
const dbScope = pv.desired.find((s) => s.level === "database");
assert.deepEqual([...dbScope.privileges].sort(), ["INSERT", "SELECT", "UPDATE"]);
assert.equal(pv.original.user, "app");
ok("privilege checkboxes edit the scope and the preview receives the full desired set");

// column privileges on the table scope
await page.locator(".dz-item").nth(1).click();
assert.match(await page.locator(".col-privs").innerText(), /SELECT \(id\)/);
await page.locator(".col-privs details:has(summary:has-text('UPDATE')) summary").click();
await page.locator(".col-privs details:has(summary:has-text('UPDATE')) label:has-text('total') input").check();
await settle();
const pv2 = await g("__lastPreview");
assert.deepEqual(pv2.desired.find((s) => s.level === "table").columns.UPDATE, ["total"]);
ok("column-level privileges can be granted per column");

// ALL PRIVILEGES disables the individual boxes
await page.locator(".dz-item").first().click();
await page.check(".privs .all input");
assert.equal(await page.locator(".priv-grid label:has-text('DELETE') input").first().isDisabled(), true);
await page.uncheck(".privs .all input");

// apply
await page.click("button:text-is('Apply changes')");
await settle();
const ap = await g("__accountApplied");
assert.equal(ap.edit.user, "app");
assert.equal(ap.edit.password, null, "an empty password field means unchanged");
assert.ok(Array.isArray(ap.desired));
ok("apply sends the account edit and the desired privileges");

// new user with a password; the SQL tab never shows it
await page.click("button:text-is('＋ New user')");
await page.locator(".dz-form label:has-text('User name') input").fill("newbie");
await page.locator(".dz-form input[type=password]").fill("s3cret");
await settle();
await page.click(".dz-tabs button:has-text('SQL')");
assert.ok(!(await page.locator(".dz-sql").innerText()).includes("s3cret"));
await page.click("button:text-is('Create user')");
await settle();
const created2 = await g("__accountApplied");
assert.equal(created2.original, null);
assert.equal(created2.edit.password, "s3cret");
assert.equal(created2.edit.host, "localhost");
ok("new user: password is sent to apply but never displayed");

// drop user needs confirmation
await page.click(".u-list li:has-text('report')");
await page.click("button.danger:text-is('Drop user')");
await page.click(".dialog button.primary:text-is('Drop user')");
await settle();
assert.deepEqual(await g("__dropped"), { id: 1, user: "report", host: "localhost" });
ok("drop user is confirmed first");

await browser.close();
assert.deepEqual(errors, [], "browser errors: " + errors.join(" | "));
console.log("ALL DESIGNER/USERS UI CHECKS PASSED");
