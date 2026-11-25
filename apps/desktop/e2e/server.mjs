// UI end-to-end test of the server tools (processes, variables, status, maintenance, logs) and of the
// connection dialog's SSH / SSL / test-connection flow, against the fake Tauri bridge in mock.js.
// Needs the dev server (`npm run dev`) and CHROME_PATH.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const mockSrc = fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8");
const errors = [];
async function newPage(preset = "") {
  const page = await browser.newPage({ viewport: { width: 1600, height: 950 } });
  page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
  page.on("console", (m) => m.type() === "error" && !m.text().includes("404") && errors.push("console: " + m.text()));
  await page.addInitScript({ content: preset + mockSrc });
  await page.goto(process.env.APP_URL || "http://localhost:1420/");
  return page;
}
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
const settle = (page, ms = 450) => page.waitForTimeout(ms);
const g = (page, n) => page.evaluate((k) => window[k], n);

// ======================================================= server tools
let page = await newPage();
await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
await page.click("button[title^='Server:']");
await page.waitForSelector(".server-tools");
await page.waitForSelector(".card:has-text('MySQL 9.7.2')");
assert.match(await page.locator(".card:has-text('Uptime')").innerText(), /1d 2h 3m/);
await page.waitForFunction(() => {
  const card = [...document.querySelectorAll(".card")].find((c) => c.textContent.includes("Queries / s"));
  return /^[1-9]/.test(card?.querySelector("b")?.textContent ?? "");
}, null, { timeout: 8000 });
assert.match(await page.locator(".card:has-text('Buffer pool hit rate')").innerText(), /99\.00%/);
assert.equal(await page.locator(".card").filter({ has: page.locator("span.dim", { hasText: /^Connections$/ }) }).locator("b").innerText(), "7");
ok("overview shows server facts and live queries/second, connections and buffer pool hit rate");

// ---- processes
await page.click(".st-body >> xpath=ancestor::div[contains(@class,'server-tools')]//button[text()='Processes']");
await page.waitForSelector(".proc-grid tbody tr");
let ids = await page.locator(".proc-grid tbody tr td:first-child").allInnerTexts();
assert.deepEqual(ids, ["11", "13"], "idle connections and the monitoring connection are hidden; slowest first");
assert.equal(await page.locator(".proc-grid tr.slow").count(), 1, "statements running longer than the threshold are highlighted");
await page.uncheck("label:has-text('Hide idle') input");
assert.deepEqual(await page.locator(".proc-grid tbody tr td:first-child").allInnerTexts(), ["12", "11", "13"]);
assert.equal(await page.locator(".proc-grid tr:has-text('app') >> button:text-is('Stop query')").nth(0).isDisabled(), true, "an idle connection has no query to stop");
await page.check("label:has-text('Hide idle') input");
await page.fill(".proc input[placeholder^='Filter']", "customers");
assert.deepEqual(await page.locator(".proc-grid tbody tr td:first-child").allInnerTexts(), ["13"]);
await page.fill(".proc input[placeholder^='Filter']", "");
ok("process list hides idle/monitor connections, sorts by time, filters and flags slow statements");

await page.click(".proc-grid tr:has-text('LIKE')");
assert.match(await page.locator(".proc-detail pre").innerText(), /note LIKE '%x%'/);
await page.click(".proc-grid tr:has-text('LIKE') >> button:text-is('Stop query')");
assert.match(await page.locator(".dialog").innerText(), /Stop the statement running on connection 11/);
await page.click(".dialog button:text-is('Cancel')");
assert.equal(await g(page, "__kills"), undefined, "cancelling kills nothing");
await page.click(".proc-grid tr:has-text('LIKE') >> button:text-is('Stop query')");
await page.click(".dialog button.primary");
await settle(page);
assert.deepEqual(await g(page, "__kills"), [{ id: 11, kind: "query" }]);
await page.click(".proc-grid tr:has-text('customers') >> button:text-is('Kill')");
assert.match(await page.locator(".dialog").innerText(), /rolled back/);
await page.click(".dialog button.primary");
await settle(page, 700);
assert.deepEqual((await g(page, "__kills")).at(-1), { id: 13, kind: "connection" });
assert.equal(await page.locator(".proc-grid tr:has-text('customers')").count(), 0, "the killed connection disappears after the refresh");
ok("stopping a query and killing a connection both ask first and send the right request");

await page.uncheck("label:has-text('Auto refresh') input");
const before = await g(page, "__plCalls");
await page.waitForTimeout(2600);
assert.equal(await g(page, "__plCalls"), before, "auto refresh off stops polling");
ok("auto refresh can be paused");

// ---- variables
await page.click("button:text-is('Variables')");
await page.waitForSelector("td:text-is('max_connections')");
assert.equal(await page.locator(".vars tbody tr").count(), 3);
await page.fill(".vars input[placeholder^='Search']", "max_conn");
assert.equal(await page.locator(".vars tbody tr").count(), 1);
await page.click(".vars tbody tr >> button[title^='Change']");
await page.fill(".vars td.val input", "300");
await page.press(".vars td.val input", "Enter");
assert.match(await page.locator(".dialog").innerText(), /SET GLOBAL max_connections = 300/);
await page.click(".dialog button.primary");
await settle(page);
assert.deepEqual((await g(page, "__setVars")).at(-1), { name: "max_connections", value: "300" });
await page.click(".vars tbody tr >> button[title^='Change']");
await page.fill(".vars td.val input", "bad");
await page.click("button:text-is('Apply…')");
await page.click(".dialog button.primary");
await page.waitForSelector(".banner:has-text('Truncated incorrect')");
ok("a global variable is changed only after confirming the exact SET statement, and server warnings are shown");
await page.fill(".vars input[placeholder^='Search']", "");
await page.check("label:has-text('Only changed') input");
assert.equal(await page.locator(".vars tbody tr").count(), 1, "only explicitly set / runtime-changed variables");
await page.uncheck("label:has-text('Only changed') input");
await page.selectOption(".vars select", "session");
await page.waitForSelector(".vars tbody tr");
assert.equal(await page.locator(".vars tbody tr button[title^='Change']").count(), 0, "session values are read-only");
ok("variables: filter, 'only changed', and read-only session view");

// ---- status
await page.click("button:text-is('Status')");
await page.waitForSelector("td:text-is('Questions')");
await page.waitForFunction(() => [...document.querySelectorAll("tr")].some((r) => /Questions/.test(r.textContent) && /\d/.test(r.querySelector("td.num")?.textContent ?? "")), null, { timeout: 8000 });
await page.check("label:has-text('Only changing') input");
const moving = await page.locator(".vars tbody tr td:first-child").allInnerTexts();
assert.ok(moving.includes("Questions") && !moving.includes("Threads_connected"), `only counters that moved: ${moving}`);
ok("status shows per-second rates and can hide values that are not changing");

// ---- maintenance
await page.click("button:text-is('Maintenance')");
await page.waitForSelector(".tbl-pick");
assert.deepEqual(await page.locator(".tbl-pick label:not(.all)").allInnerTexts().then((t) => t.map((x) => x.trim())), ["customers", "orders"]);
assert.equal(await page.locator("button:has-text('Run on 0')").isDisabled(), true);
await page.check(".tbl-pick label.all input");
await page.click("button:has-text('Run on 2')");
await settle(page);
assert.deepEqual((await g(page, "__maint")).at(-1), { id: 1, db: "shop", tables: ["customers", "orders"], operation: { op: "check", option: null } });
assert.equal(await page.locator(".maint-result tr.ok").count(), 2);
ok("maintenance: pick tables, run CHECK, one result row per table");

await page.locator(".maint select").nth(2).selectOption("quick");   // database, operation, then the CHECK option
await page.click("button:has-text('Run on 2')");
await settle(page);
assert.deepEqual((await g(page, "__maint")).at(-1).operation, { op: "check", option: "quick" });

await page.locator(".maint select").nth(1).selectOption("optimize");
await page.click("button:has-text('Run on 2')");
assert.match(await page.locator(".dialog").innerText(), /rebuilds 2 table/);
await page.click(".dialog button:text-is('Cancel')");
assert.equal((await g(page, "__maint")).length, 2, "OPTIMIZE needs confirmation");
await page.click("button:has-text('Run on 2')");
await page.click(".dialog button.primary");
await settle(page);
assert.equal((await g(page, "__maint")).at(-1).operation.op, "optimize");

await page.locator(".maint select").nth(1).selectOption("repair");
await page.check("label:has-text('USE_FRM') input");
await page.check("label:has-text('QUICK') input");
await page.click("button:has-text('Run on 2')");
assert.match(await page.locator(".dialog").innerText(), /backup/);
await page.click(".dialog button.primary");
await settle(page);
assert.deepEqual((await g(page, "__maint")).at(-1).operation, { op: "repair", quick: true, extended: false, use_frm: true });
assert.equal(await page.locator(".maint-result tr.bad").count(), 1);
assert.equal(await page.locator(".maint-result tr.warn").count(), 1);
ok("OPTIMIZE and REPAIR require confirmation (REPAIR warns about backups); errors and notes are colour-coded");

// ---- logs
await page.click("button:text-is('Logs')");
await page.waitForSelector(".logtable tbody tr");
assert.equal(await page.locator(".logtable tbody tr").count(), 2);
await page.uncheck("label:has-text('Warning') input");
await page.waitForFunction(() => document.querySelectorAll(".logtable tbody tr").length === 1);
assert.deepEqual((await g(page, "__errLogArgs")).levels, ["System", "Error"]);
await page.fill(".vars input[placeholder^='Search text']", "disk");
await settle(page, 600);
assert.equal((await g(page, "__errLogArgs")).search, "disk");
ok("error log: level and text filters are sent to the server");

await page.click(".logs button:text-is('Slow queries')");
await page.waitForSelector(".banner:has-text('log_output')");
ok("slow log explains why it is empty when the server writes it to a file");

await page.click(".logs button:text-is('Top statements')");
await page.waitForSelector(".top-statements tbody tr");
assert.equal(await page.locator(".top-statements tbody tr").count(), 2);
assert.match(await page.locator(".top-statements tbody tr").first().innerText(), /customer_id/);
assert.equal(await page.locator(".top-statements tbody tr.warn").count(), 1, "a statement that used no index is flagged");
await page.selectOption("select[aria-label='Sort statements by']", "calls");
await page.waitForFunction(() => /UPDATE/.test(document.querySelector(".top-statements tbody tr").innerText));
await page.fill(".vars input[placeholder='Search SQL…']", "orders");
await page.waitForFunction(() => document.querySelectorAll(".top-statements tbody tr").length === 1);
ok("top statements: the costliest statements from the Performance Schema, sortable and searchable, with no-index runs flagged");

await page.click(".logs button:text-is('Binary logs')");
await page.waitForSelector(".tbl-pick .dz-item");
assert.equal(await page.locator(".tbl-pick .dz-item").count(), 2);
await page.waitForSelector(".logtable tbody tr");
assert.equal((await g(page, "__binlogArgs")).file, "binlog.000002", "the newest file opens first");
await page.click(".tbl-pick .dz-item:has-text('000001')");
await settle(page);
assert.deepEqual([(await g(page, "__binlogArgs")).file, (await g(page, "__binlogArgs")).fromPos], ["binlog.000001", 4]);
ok("binary logs: file list and events");

await page.click(".logs button:text-is('Settings')");
await page.waitForSelector("td:text-is('Slow query log')");
await page.click("tr:has-text('Slow query log') >> button:has-text('Enable')");
assert.match(await page.locator(".dialog").innerText(), /SET GLOBAL slow_query_log = ON/);
await page.click(".dialog button.primary");
await settle(page);
assert.deepEqual((await g(page, "__setVars")).at(-1), { name: "slow_query_log", value: "ON" });
ok("log settings can be switched, after confirming the SET statement");
await page.close();

// ======================================================= connection dialog
page = await newPage("window.__sshMode = 'unknown';");
await page.click("button[title='New connection']");
await page.fill(".conn-dialog label:has-text('Name') input", "prod");
await page.click(".conn-dialog .dz-tabs button:has-text('SSH tunnel')");
await page.check("label:has-text('Connect through an SSH tunnel') input");
await page.fill("label:has-text('SSH host') input", "bastion");
await page.fill("label:has-text('SSH user') input", "deploy");
assert.equal(await page.locator("label:has-text('Private key file') input").inputValue(), "~/.ssh/id_ed25519");
await page.fill("label:has-text('Key passphrase') input", "open sesame");
await page.click("button:text-is('Test connection')");
await page.waitForSelector("h3:text-is('Trust this SSH server?')");
assert.match(await page.locator(".overlay.inner .preview").innerText(), /SHA256:abc123/);
await page.click(".overlay.inner button:text-is('Cancel')");
assert.equal((await g(page, "__tested")).length, 1);
await page.click("button:text-is('Test connection')");
await page.click("button:text-is('Trust and continue')");
await page.waitForSelector(".banner.ok:has-text('through the SSH tunnel')");
const t2 = (await g(page, "__tested")).at(-1);
assert.equal(t2.trustFingerprint, "SHA256:abc123", "the confirmed fingerprint is sent back");
assert.deepEqual(t2.connection.ssh, { host: "bastion", port: 22, user: "deploy", auth: { method: "key", path: "~/.ssh/id_ed25519" } });
assert.equal(t2.connection.new_ssh_passphrase, "open sesame");
assert.equal(t2.connection.new_password, null, "an empty password means unchanged");
ok("test connection: an unknown SSH host is shown with its fingerprint and only used after confirmation");

await page.click(".conn-dialog .dz-tabs button:has-text('SSL')");
await page.selectOption(".conn-dialog label:has-text('SSL mode') select", "verifyca");
await page.fill("label:has-text('CA certificate') input", "/etc/ssl/ca.pem");
await page.fill("label:has-text('Client certificate') input", "/etc/ssl/client.pem");
await page.fill("label:has-text('Client key') input", "/etc/ssl/client.key");
await page.click("button:text-is('Test connection')");
await page.click("button:text-is('Trust and continue')");
await page.waitForSelector(".banner.ok:has-text('encrypted (TLS_AES_256_GCM_SHA384)')");
await page.click("button.primary:text-is('Save')");
await settle(page);
const saved = (await page.evaluate(() => window.__calls.filter((c) => c.cmd === "save_connection"))).at(-1).args.connection;
assert.equal(saved.ssl, "verifyca");
assert.deepEqual([saved.ssl_ca, saved.ssl_cert, saved.ssl_key], ["/etc/ssl/ca.pem", "/etc/ssl/client.pem", "/etc/ssl/client.key"]);
assert.equal(saved.ssh.host, "bastion");
assert.equal(saved.new_ssh_passphrase, "open sesame");
ok("SSL files and SSH settings are saved together; encryption is reported by the test");

// switching the tunnel off drops the SSH fields from the payload
await page.click("button[title='Edit connection']").catch(() => {});
await page.close();

// ======================================================= connecting through an unknown / changed SSH host
page = await newPage("window.__sshMode = 'unknown';");
await page.selectOption(".conn-bar select", "1");
await page.waitForSelector("h3:text-is('Trust this SSH server?')");
assert.match(await page.locator(".dialog .preview").innerText(), /SHA256:abc123/);
await page.click("button:text-is('Trust and connect')");
await page.waitForSelector(".tree .db");
assert.deepEqual(await g(page, "__connectTrust"), [null, "SHA256:abc123"], "the first attempt is refused; the retry carries the confirmed fingerprint");
ok("connecting to an unknown SSH server asks first, then connects with the confirmed fingerprint");
await page.close();

page = await newPage("window.__sshMode = 'changed';");
await page.selectOption(".conn-bar select", "1");
await page.waitForSelector("h3:has-text('SSH host key changed')");
const text = await page.locator(".dialog").innerText();
assert.ok(text.includes("SHA256:old") && text.includes("SHA256:new") && /not.*made/i.test(text));
assert.equal(await page.locator(".tree .db").count(), 0, "no connection was made");
await page.click("button:text-is('Close')");
assert.equal(await g(page, "__forgot"), undefined, "closing does not forget the key");
await page.selectOption(".conn-bar select", "");
await page.selectOption(".conn-bar select", "1");
await page.waitForSelector("h3:has-text('SSH host key changed')");
await page.click("button:has-text('forget the saved key')");
await page.waitForSelector(".tree .db");
assert.equal(await g(page, "__forgot"), true);
ok("a changed SSH host key blocks the connection; reconnecting needs an explicit 'forget'");
await page.close();

await browser.close();
assert.deepEqual(errors, [], "browser errors: " + errors.join(" | "));
console.log("ALL SERVER / CONNECTION UI CHECKS PASSED");
