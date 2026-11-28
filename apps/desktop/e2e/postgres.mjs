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
const caps = { flavor: "postgres", schemas: yes, routines: no("not yet"), triggers: no("not yet"), events: no("PostgreSQL has no scheduled events"), users: no("managing roles for PostgreSQL is not available yet"),
  replication: no("not yet"), explain_analyze: no("the Explain view for PostgreSQL is not available yet"), read_only_session: yes, row_estimates: yes, server_tools: no("server tools for PostgreSQL is not available yet"),
  dump_restore: no("backup, restore and import for PostgreSQL is not available yet"), data_export: no("export for PostgreSQL is not available yet"), writes: no("editing rows for PostgreSQL is not available yet") };
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
for (const label of ["Users", "Server", "Home", "Transfer"]) assert.equal(await page.locator(`header.topbar button:has-text('${label}')`).isDisabled(), true, label);
ok("the pages PostgreSQL does not have yet are disabled and say why");

// opening a table reads it in the console, quoted the PostgreSQL way
await page.click(".tree .db:has-text('shop')");
await page.locator(".row-item.leaf:has-text('orders')").first().click({ button: "right" });
assert.equal(await page.locator(".ctx >> text=Truncate").count() + await page.locator(".ctx >> text=Design table").count(), 0, "no MySQL-only entries");
await page.click(".ctx >> text=Read data in SQL console");
await page.waitForFunction(() => window.__calls.some((c) => c.cmd === "run_query"));
const ran = (await calls("run_query")).at(-1);
assert.match(ran.sql, /FROM "[^"]+"\."orders" LIMIT 1000/);
assert.doesNotMatch(ran.sql, /`/);
ok("a table is read with a SELECT quoted for PostgreSQL");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL POSTGRESQL CHECKS PASSED");
