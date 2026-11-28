// UI test of remote connections (URL form, "Show all databases") and the result tabs under the console.
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
const calls = (cmd) => page.evaluate((c) => window.__calls.filter((x) => x.cmd === c), cmd);
const text = (loc) => loc.allInnerTexts().then((t) => t.map((x) => x.replace(/\s+/g, " ").trim()));
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
const field = (label) => page.locator(`.conn-dialog label:has-text('${label}') input:not([type=radio])`).first();

// ---- remote connection: the URL form
await page.click("button[title='New connection']");
await page.waitForSelector(".conn-dialog");
await page.locator(".conn-dialog label:has-text('URL') input[type=radio]").check();
const url = page.locator("input[aria-label='Connection URL']");
await url.fill("jdbc:mysql://srv1832.hstgr.io:3306/u807414057_erp_db");
assert.equal(await field("Host").inputValue(), "srv1832.hstgr.io");
assert.equal(await field("Port").inputValue(), "3306");
assert.equal(await field("Default database").inputValue(), "u807414057_erp_db");
await url.fill("mysql://bob:p%40ss@db.example.com:3307/shop?ssl-mode=REQUIRED");
assert.equal(await field("Host").inputValue(), "db.example.com");
assert.equal(await field("Port").inputValue(), "3307");
assert.equal(await field("User").inputValue(), "bob");
assert.equal(await field("Password").inputValue(), "p@ss");
assert.equal(await field("Default database").inputValue(), "shop");
await url.fill("ftp://nope");
assert.match(await page.locator(".conn-dialog .error").innerText(), /Not a connection URL/);
await page.locator(".conn-dialog label:has-text('Host') input[type=radio]").check();
assert.equal(await page.locator("input[aria-label='Connection URL']").inputValue(), "jdbc:mysql://db.example.com:3307/shop", "the URL is shown for the host form (without the password)");
await page.click(".conn-dialog .dz-tabs button:has-text('SSL')");
assert.equal(await page.locator(".conn-dialog label:has-text('SSL mode') select").inputValue(), "required", "ssl-mode=REQUIRED in the URL turns SSL on");
ok("a connection can be entered by host or by pasting a JDBC / mysql:// URL; SSL and credentials are picked up");

// ---- show all databases
await page.click(".conn-dialog .dz-tabs button:has-text('General')");
const showAll = page.locator(".conn-dialog label:has-text('Show all databases') input");
assert.equal(await showAll.isChecked(), false, "off for a new connection with a default database");
await field("Default database").fill("");
assert.equal(await showAll.isDisabled(), true, "no default database: everything is listed");
await field("Default database").fill("u807414057_erp_db");
assert.equal(await showAll.isEnabled(), true);
await field("Name").fill("Hosting");
await page.click(".conn-dialog button:text-is('Save')");
await page.waitForFunction(() => window.__calls.some((c) => c.cmd === "save_connection"));
const saved = (await calls("save_connection")).at(-1).args.connection;
assert.deepEqual([saved.host, saved.port, saved.database, saved.show_all_databases], ["db.example.com", 3307, "u807414057_erp_db", false]);
ok("'Show all databases' is off for a hosted account with a default database (only that database is listed) and is saved");
await page.reload();

// ---- result tabs of a query console
await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
const tabs = () => text(page.locator(".result-tabs button"));
assert.deepEqual(await tabs(), ["Results", "Profile", "Messages", "Info", "History"]);
await page.click(".cm-content");
await page.evaluate(() => { window.__runSets = [{ columns: [{ name: "one", type_name: "LONGLONG" }], rows: [["1"]], total_rows: 1, truncated: false }]; });
await page.keyboard.type("SELECT 1 AS one");
await page.keyboard.press("ControlOrMeta+Enter");
await page.waitForFunction(() => document.querySelector(".result-tabs button.active")?.textContent.trim() === "Results" && document.querySelector("table.grid th"));
assert.equal(await page.locator("table.grid th:has-text('one')").count(), 1);
ok("a query console has Results, Profile, Messages, Info and History tabs, and shows the rows in Results");

await page.click(".result-tabs button:has-text('Messages')");
assert.match(await page.locator(".messages").innerText(), /SELECT 1 AS one/);
await page.click(".result-tabs button:has-text('Info')");
const info = await page.locator(".info-pane").innerText();
assert.match(info, /root@127\.0\.0\.1/);
assert.match(info, /Development/);
assert.match(info, /SELECT 1 AS one/);
assert.match(await page.locator(".info-pane table.info:not(.kv) tbody tr").first().innerText(), /^1\s+SELECT 1 AS one\s+1\s+\d+ ms$/);
await page.click(".result-tabs button:has-text('History')");
assert.match(await page.locator(".result-body .list").innerText(), /SELECT 1 AS one/);
ok("Messages lists the statement, Info shows the connection and each statement's rows and time, History lists what was run");

await page.click(".result-tabs button:has-text('Profile')");
assert.match(await page.locator(".result-body").innerText(), /Profile runs the statement at the cursor/);
await page.click(".result-body button:has-text('Profile the statement')");
await page.waitForFunction(() => window.__calls.some((c) => c.cmd === "profile_query"));
await page.waitForFunction(() => document.querySelector(".result-tabs button.active")?.textContent.trim() === "Profile");
ok("Profile offers to profile the statement and shows the result in the same tab");

// two result sets
await page.evaluate(() => { const s = { columns: [{ name: "a", type_name: "LONGLONG" }], rows: [["1"]], total_rows: 1, truncated: false }; window.__runSets = [s, s]; });
await page.click(".cm-content");
await page.keyboard.press("ControlOrMeta+Enter");
await page.waitForFunction(() => document.querySelector(".result-tabs button:nth-child(2)")?.textContent.trim() === "Result 2");
assert.deepEqual((await tabs()).slice(0, 2), ["Result 1", "Result 2"]);
ok("several result sets get Result 1, Result 2 … tabs");

// ---- a table tab adds Table Data, in the order you asked for
await page.click(".tree .db:has-text('shop')");
await page.dblclick(".row-item.leaf:has-text('orders')");
await page.waitForSelector("table.dg tbody tr");
assert.deepEqual(await tabs(), ["Table Data"], "a table tab starts with its data alone");
assert.equal(await page.locator(".result-tabs button.active").innerText().then((t) => t.trim()), "Table Data");
// running a query in the tab brings the rest
await page.click(".cm-content");
await page.keyboard.press("ControlOrMeta+Enter");
await page.waitForFunction(() => document.querySelectorAll(".result-tabs button").length > 1);
// (the mock still returns two result sets from the step above, hence Result 1 and Result 2)
assert.deepEqual((await tabs()).slice(-5), ["Profile", "Messages", "Table Data", "Info", "History"]);
assert.match((await tabs())[0], /^Result/);
await page.click(".result-tabs button:has-text('Info')");
assert.match(await page.locator(".info-pane").innerText(), /shop\.orders[\s\S]*InnoDB/);
await page.click(".info-pane button:has-text('Open Properties')");
await page.waitForSelector(".props-head");
ok("a table tab shows only Table Data until a query runs, then Results, Profile, Messages, Table Data, Info and History; Info summarises the table and links to Properties");

await browser.close();
assert.deepEqual(errors, [], "browser errors: " + errors.join(" | "));
console.log("ALL CONNECT / RESULT TAB UI CHECKS PASSED");
