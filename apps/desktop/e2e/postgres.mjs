// PostgreSQL, first step: the connection dialog, browsing schemas, reading a table with PostgreSQL quoting, and what is not there yet.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1400, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
const no = (reason) => ({ available: false, reason });
const yes = { available: true, reason: null };
const caps = { flavor: "postgres", schemas: yes, routines: yes, triggers: yes, events: no("PostgreSQL has no scheduled events"), users: no("managing roles for PostgreSQL is not available yet"),
  replication: no("not yet"), explain_analyze: yes, read_only_session: yes, row_estimates: yes, server_tools: no("server tools for PostgreSQL is not available yet"),
  dump_restore: yes, data_export: yes, writes: yes };
const extra = [{ id: 8, name: "Pg shop", color: "#4f8cff", engine: "postgres", host: "127.0.0.1", port: 5432, user: "postgres", database: "shop", ssl: "disabled" }];
await page.addInitScript({ content: `localStorage.setItem('mylodb.tourDone','1'); window.__extraConns = ${JSON.stringify(extra)}; window.__caps = ${JSON.stringify(caps)};\n` + fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
let step = 0;
const calls = (cmd) => page.evaluate((c) => window.__calls.filter((x) => x.cmd === c).map((x) => x.args), cmd);
const ok = (m) => console.log(`ok ${++step} ${m}`);

// the dialog: choosing PostgreSQL brings its port and account, and a URL fills the form
await page.click("button[title='New connection']");
await page.waitForSelector(".conn-dialog");
await page.click(".conn-dialog .ss-trigger");
await page.click(".ss-opt:has-text('PostgreSQL')");
assert.equal(await page.locator(".conn-dialog input[type=number]").first().inputValue(), "5432");
assert.equal(await page.locator(".conn-dialog label:has-text('Show all databases')").count(), 0);
await page.click(".connect-by label:has-text('URL') input");
await page.fill("input[aria-label='Connection URL']", "postgresql://ann:pw@db.example.com:6000/sales?sslmode=verify-full");
await page.fill(".conn-dialog input >> nth=0", "Sales");
await page.click(".conn-dialog .actions button:has-text('Test connection')");
await page.waitForSelector(".banner.ok");
const tested = (await calls("test_connection")).at(-1).connection;
assert.deepEqual([tested.engine, tested.host, tested.port, tested.user, tested.database, tested.ssl, tested.new_password], ["postgres", "db.example.com", 6000, "ann", "sales", "required", "pw"]);
assert.doesNotMatch(await page.locator(".banner.ok").innerText(), /MySQL/);
await page.click(".conn-dialog .actions button:has-text('Cancel')");
ok("the dialog takes a PostgreSQL server or URL and tests it");

await page.selectOption(".conn-bar select", "8");
await page.waitForSelector(".tree .db");
await page.waitForFunction(() => document.querySelector("button[title*='server tools for PostgreSQL']"));
for (const label of ["Users", "Server", "Home"]) assert.equal(await page.locator(`header.topbar button:has-text('${label}')`).isDisabled(), true, label);
ok("the pages PostgreSQL does not have yet are disabled and say why");

// opening a table reads it in the console, quoted the PostgreSQL way
await page.click(".tree .db:has-text('shop')");
await page.locator(".row-item.leaf:has-text('orders')").first().click({ button: "right" });
assert.equal(await page.locator(".ctx >> text=Truncate").count() + await page.locator(".ctx >> text=Dump table").count(), 0, "no MySQL-only entries");
await page.click(".ctx >> text=Read data in SQL console");
await page.waitForFunction(() => window.__calls.some((c) => c.cmd === "run_query"));
const ran = (await calls("run_query")).at(-1);
assert.match(ran.sql, /FROM "[^"]+"\."orders" LIMIT 1000/);
assert.doesNotMatch(ran.sql, /`/);
ok("a table is read with a SELECT quoted for PostgreSQL");

// the grid opens, with only the Data view, and copies rows as PostgreSQL SQL
await page.locator(".row-item.leaf:has-text('orders')").first().dblclick();
await page.waitForSelector("table.dg tbody tr");
assert.deepEqual((await page.locator(".subtabs button").allInnerTexts()).map((t) => t.trim()), ["Data"]);
assert.equal(await page.locator("select[aria-label='Foreign key checks']").count(), 0);
ok("a table opens in the data grid, without the views and switches PostgreSQL does not have");

// the designer uses PostgreSQL's types, offers no MySQL options, and keeps columns where they are
await page.locator(".row-item.leaf:has-text('orders')").first().click({ button: "right" });
await page.click(".ctx >> text=Design table");
await page.waitForSelector(".designer .dz-grid tbody tr");
assert.equal(await page.locator(".dz-grid th:has-text('Unsigned')").count(), 0);
assert.equal(await page.locator(".dz-actions button:text-is('↓')").isDisabled(), true, "columns cannot be moved");
await page.click(".dz-actions button:has-text('Add column')");
const types = await page.locator(".dz-grid tbody tr:last-child select option").allInnerTexts();
assert.ok(types.includes("timestamp with time zone") && types.includes("bytea") && !types.includes("tinyint"), types.join());
await page.waitForFunction(() => window.__calls.some((c) => c.cmd === "ddl_preview" && c.args.postgres === true && c.args.desired.columns.at(-1).data_type === "text"));
assert.equal(await page.locator(".dz-grid tbody tr:last-child .cell-input").first().inputValue() !== "", true, "the new column is at the end");
await page.click(".dz-tabs button:has-text('SQL')");
await page.waitForSelector(".dz-sql .preview");
ok("the table designer works with PostgreSQL's types and rules");

// functions and views have a definition and a drop; the database menu creates and backs up
await page.locator(".row-item:has-text('shop')").first().click({ button: "right" });
for (const entry of ["Create", "Back up this schema", "Restore a backup"]) assert.equal(await page.locator(`.ctx >> text=${entry}`).count(), 1, entry);
await page.hover(".ctx .ctx-item.sub:has-text('Create')");
await page.click(".ctx-nested .ctx-label:text-is('New function…')");
await page.waitForFunction(() => /LANGUAGE plpgsql/.test(document.querySelector(".cm-content")?.innerText ?? ""));
ok("the schema menu creates objects from PostgreSQL templates");

// Explain draws the PostgreSQL plan; Profile is not offered
await page.evaluate(() => (window.__pgPlan = JSON.stringify([{ Plan: { "Node Type": "Aggregate", "Total Cost": 60, "Plan Rows": 1, Plans: [{ "Node Type": "Seq Scan", "Relation Name": "orders", "Total Cost": 50, "Plan Rows": 3000, Filter: "(id > 1)" }] } }])));
assert.equal(await page.locator("button[title^='Run the SELECT and show stage timings']").count(), 0, "no Profile button");
await page.click("button[title^='Explain the selected statement']");
await page.waitForSelector(".plan-diagram");
const plan = await page.locator(".plan-diagram").innerText();
assert.match(plan, /Seq Scan on orders/);
assert.match(plan, /reads every row/);
ok("Explain shows the PostgreSQL plan as a diagram");

// backup and restore
await page.click("header.topbar button:has-text('Transfer')");
await page.waitForSelector(".transfer .dz-tabs");
assert.deepEqual(await page.locator(".transfer .dz-tabs button").allInnerTexts(), ["Backup", "Restore", "Export query"]);
await page.click(".transfer .dz-tabs button:has-text('Backup')");
await page.fill(".transfer input[placeholder='Choose a folder…']", "/Users/me/Backups");
await page.selectOption(".transfer label:has-text('Format') select", "custom");
await page.click(".transfer button:has-text('Start backup')");
await page.waitForSelector(".transfer .banner.ok");
const backup = (await calls("pg_backup_job")).at(-1);
assert.deepEqual([backup.options.format, backup.dir, backup.options.clean], ["custom", "/Users/me/Backups", false]);
await page.click(".transfer .dz-tabs button:has-text('Restore')");
await page.fill(".transfer input[aria-label='Backup file']", "/Users/me/Backups/shop.sql");
assert.equal(await page.locator(".transfer label:has-text('Drop each object')").count(), 0, "a script decides that itself");
await page.click(".transfer button:has-text('Restore…')");
assert.equal((await calls("pg_restore_job")).length, 0, "it asks first");
await page.click(".dialog button.primary:text-is('Restore'), .dialog button.danger:text-is('Restore')");
await page.waitForSelector(".transfer .banner.ok >> text=Restore complete");
assert.deepEqual((await calls("pg_restore_job")).at(-1).options, { clean: false, single_transaction: true, no_owner: false });
ok("backup writes with the chosen options, and restore asks before it runs");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL POSTGRESQL CHECKS PASSED");
