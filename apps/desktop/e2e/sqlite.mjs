// SQLite: the connection dialog takes a file, and the toolbar disables what a file cannot do, with the reason.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1400, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
const no = (reason) => ({ available: false, reason });
const yes = { available: true, reason: null };
const sqliteCaps = { flavor: "sqlite", schemas: no("one main database"), routines: no("SQLite has no stored procedures or functions"), triggers: yes, events: no("SQLite has no scheduled events"),
  users: no("SQLite has no users or privileges: access is the file's permissions"), replication: no("SQLite has no replication"), explain_analyze: no("SQLite offers EXPLAIN QUERY PLAN only"),
  read_only_session: yes, row_estimates: no("exact counts"), server_tools: no("SQLite is a file, not a server: there is no process list, variables or log"),
  dump_restore: no("backup, restore and CSV import are MySQL-only for now; export works"), data_export: yes, writes: no("SQLite files open read-only in this version") };
const extra = [{ id: 7, name: "Shop file", color: "#4f8cff", engine: "sqlite", file: "/tmp/shop.sqlite", host: "", port: 0, user: "", database: null, ssl: "disabled" }];
await page.addInitScript({ content: `window.__extraConns = ${JSON.stringify(extra)}; window.__caps = ${JSON.stringify(sqliteCaps)};\n` + fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
let step = 0;
const calls = (cmd) => page.evaluate((c) => window.__calls.filter((x) => x.cmd === c).map((x) => x.args), cmd);
const ok = (m) => console.log(`ok ${++step} ${m}`);

// the dialog: choosing SQLite replaces host / user / SSL with a file
await page.click("button[title='New connection']");
await page.waitForSelector(".conn-dialog");
assert.ok(await page.locator("text=Host").count() > 0 || await page.locator(".dz-tabs").count() > 0, "MySQL shows the tabs by default");
await page.click(".conn-dialog .ss-trigger");
await page.click(".ss-opt:has-text('SQLite file')");
assert.equal(await page.locator(".conn-dialog .dz-tabs").count(), 0, "no SSH / SSL tabs for a file");
assert.ok(await page.locator("text=Database file").count() > 0);
const save = page.locator(".conn-dialog .primary");
assert.equal(await save.isDisabled(), true, "a name and a file are required");
await page.fill(".conn-dialog input >> nth=0", "Shop file");
await page.fill("input[placeholder='/path/to/data.sqlite']", "/tmp/shop.sqlite");
assert.equal(await save.isDisabled(), false);
await page.click(".conn-dialog .actions button:has-text('Test connection')");
await page.waitForSelector(".banner.ok");
await page.click(".conn-dialog .actions button:has-text('Cancel')");
const saved = await page.evaluate(() => window.__calls.filter((c) => c.cmd === "test_connection").map((c) => c.args.connection));
assert.deepEqual([saved[0].engine, saved[0].file], ["sqlite", "/tmp/shop.sqlite"]);
ok("the dialog switches to a file, requires a name and a file, and tests it");

// a connected SQLite session: server-only toolbar buttons are disabled and say why
await page.selectOption(".conn-bar select", "7");
await page.waitForSelector(".tree .db");
await page.waitForFunction(() => document.querySelector("button[title*='no process list']"));
for (const [label, reason] of [["Users", /no users or privileges/], ["Server", /no process list/], ["Home", /no process list/]]) {
  const b = page.locator(`header.topbar button:has-text('${label}')`);
  assert.equal(await b.isDisabled(), true, label);
  assert.match(await b.getAttribute("title"), reason);
}
assert.equal(await page.locator("header.topbar button:has-text('New query')").isDisabled(), false);
ok("Users, Server and Home are disabled with the engine's reason; New query stays on");

// Transfer stays on for a file, with only the export tab (backup, restore and import are MySQL-only)
const transfer = page.locator("header.topbar button:has-text('Transfer')");
assert.equal(await transfer.isDisabled(), false);
await transfer.click();
await page.waitForSelector(".transfer .dz-tabs");
assert.deepEqual(await page.locator(".transfer .dz-tabs button").allInnerTexts(), ["Export query"]);
ok("Transfer is available for a SQLite file with only the Export tab");
// upkeep: integrity check and VACUUM from the database's right-click menu, with the result as a flash message
await page.locator(".tree .db").first().click({ button: "right" });
await page.waitForSelector(".ctx");
assert.ok((await page.locator(".ctx .ctx-label").allInnerTexts()).some((t) => /Compact the file \(VACUUM\)/.test(t)));
await page.click(".ctx .ctx-label:text-is('Check integrity')");
await page.waitForSelector(".toast:has-text('Integrity check: ok')");
assert.equal((await page.evaluate(() => window.__calls.filter((c) => c.cmd === "sqlite_maintain").map((c) => c.args.op))).at(-1), "integrity");
await page.locator(".tree .db").first().click({ button: "right" });
await page.click(".ctx .ctx-label:text-is('Compact the file (VACUUM)')");
await page.waitForSelector(".toast:has-text('VACUUM done')");
ok("a SQLite file can be checked and compacted from the database menu, and the result is announced");

// the table designer works for a SQLite file: its own types, no engine or character set, SQLite's index kinds
await page.locator(".tree .db").first().click();
await page.evaluate(() => window.__emit("menu-action", "new_table"));
await page.waitForSelector(".designer");
const heads = await page.locator(".dz-grid thead th").allInnerTexts();
assert.ok(!heads.includes("Unsigned") && !heads.includes("Comment"), `no MySQL-only columns: ${heads}`);
assert.equal(await page.locator(".dz-tabs button:has-text('Options')").count(), 0, "no engine or character set page");
await page.locator(".dz-grid tbody tr").first().locator(".ss-trigger, .ss").nth(0).waitFor();
assert.deepEqual((await calls("server_options")).length, 0, "no server options are asked for a file");
await page.locator(".dz-name").fill("notes");
await page.waitForFunction(() => window.__calls.some((c) => c.cmd === "ddl_preview" && c.args.sqlite === true));
await page.click(".dz-tabs button:has-text('Indexes')");
await page.click(".dz-item >> nth=0");
const kinds = await page.locator(".dz-edit select.ss-native").first().evaluate((el) => [...el.options].map((o) => o.value));
assert.deepEqual(kinds, ["primary", "unique", "index"], "SQLite has no fulltext or spatial indexes");
ok("the table designer works for a SQLite file with SQLite's own types and index kinds");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL SQLITE UI CHECKS PASSED");
