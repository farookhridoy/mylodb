// UI test of production safety: environment badges, read-only connections, review of destructive SQL, confirm-on-commit.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await browser.newPage({ viewport: { width: 1400, height: 900 } });
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
page.on("console", (m) => m.type() === "error" && !m.text().includes("404") && errors.push("console: " + m.text()));
const extra = [
  { id: 2, name: "Prod (read-only)", color: "#e5484d", host: "prod.example", port: 3306, user: "report", database: null, ssl: "disabled", environment: "production", read_only: true, query_timeout_secs: 30, confirm_destructive: true },
  { id: 3, name: "Prod (writable)", color: "#e5484d", host: "prod.example", port: 3306, user: "admin", database: null, ssl: "disabled", environment: "production", read_only: false, query_timeout_secs: null, confirm_destructive: true },
  { id: 4, name: "Staging", color: "#e0a03b", host: "stage.example", port: 3306, user: "dev", database: null, ssl: "disabled", environment: "staging", read_only: false, query_timeout_secs: null, confirm_destructive: true },
  { id: 5, name: "Dev quiet", color: "#4f8cff", host: "127.0.0.1", port: 3306, user: "root", database: null, ssl: "disabled", environment: "development", read_only: false, query_timeout_secs: null, confirm_destructive: false },
];
await page.addInitScript({ content: `window.__extraConns = ${JSON.stringify(extra)};\n` + fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
const calls = (cmd) => page.evaluate((c) => window.__calls.filter((x) => x.cmd === c), cmd);
const editorText = () => page.evaluate(() => document.querySelector(".cm-content")?.innerText ?? "");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
const connect = async (id) => {
  await page.selectOption(".conn-bar select", String(id));
  await page.waitForSelector(".tree .db");
  await page.waitForSelector(".env-bar");
};
const write = async (sql, replace = true) => {
  await page.click(".cm-content");
  if (replace) await page.keyboard.press("ControlOrMeta+a");
  await page.keyboard.type(sql);
};
const run = async () => { await page.click(".cm-content"); await page.keyboard.press("ControlOrMeta+Enter"); await page.waitForTimeout(350); };

// ---- the connection dialog has a Safety tab with safe defaults
await page.click("button[title='New connection']");
await page.waitForSelector(".conn-dialog");
await page.click(".conn-dialog .dz-tabs button:has-text('Safety')");
assert.equal(await page.locator(".conn-dialog label:has-text('Environment') select").inputValue(), "development");
assert.equal(await page.locator(".conn-dialog label:has-text('Read-only') input").isChecked(), false);
assert.equal(await page.locator(".conn-dialog label:has-text('Ask before running destructive SQL') input").isChecked(), true);
assert.equal(await page.locator(".conn-dialog label:has-text('Query timeout') input").inputValue(), "");
await page.locator(".conn-dialog label:has-text('Environment') select").selectOption("production");
await page.click(".conn-dialog .dz-tabs button:has-text('General')");
assert.equal(await page.locator(".conn-dialog input[type=color]").inputValue(), "#e5484d", "the tab colour follows the environment");
await page.fill(".conn-dialog label:has-text('Name') input", "Prod copy");
await page.click(".conn-dialog .dz-tabs button:has-text('Safety')");
await page.locator(".conn-dialog label:has-text('Read-only') input").check();
await page.locator(".conn-dialog label:has-text('Ask before running destructive SQL') input").uncheck();
await page.fill(".conn-dialog label:has-text('Query timeout') input", "45");
await page.click(".conn-dialog button:text-is('Save')");
await page.waitForFunction(() => window.__calls.some((c) => c.cmd === "save_connection"));
const saved = (await calls("save_connection")).at(-1).args.connection;
assert.deepEqual([saved.environment, saved.read_only, saved.query_timeout_secs, saved.confirm_destructive], ["production", true, 45, false]);
ok("the Safety tab sets environment, read-only, timeout and the destructive-SQL warning, and they are saved");
await page.reload();

// ---- a read-only production connection is unmistakable
await connect(2);
assert.equal((await page.locator(".env-bar .env-badge").innerText()).toLowerCase(), "production");
assert.match(await page.locator(".env-bar .ro-badge").innerText(), /Read-only/i);
assert.match(await page.locator(".env-bar").innerText(), /30 s limit/);
assert.match(await page.locator(".statusbar .env-chip").innerText(), /Production · read-only/i);
await page.click(".tree .db:has-text('shop')");
await page.dblclick(".row-item.leaf:has-text('orders')");
await page.waitForSelector("table.dg tbody tr");
assert.equal(await page.locator(".query-tabs").evaluate((e) => getComputedStyle(e).borderBottomColor), "rgb(229, 72, 77)", "production has a red band");
ok("production shows a badge, 'read-only', the time limit, a red band and a status-bar chip");

// ---- read-only: nothing in the app edits
assert.match(await page.locator(".datagrid .banner").first().innerText(), /Read-only connection/);
await page.locator("table.dg tbody tr").first().locator("td").nth(4).dblclick();
assert.equal(await page.locator(".cell-edit input").count(), 0, "cells cannot be edited");
assert.equal(await page.locator(".cell-modal-text[readonly]").count(), 1, "a long text cell opens as a read-only viewer");
assert.equal(await page.locator(".cell-modal button.primary").count(), 0, "a viewer has no Apply");
await page.keyboard.press("Escape");
for (const title of ["Add row", "Delete selected row(s) (Delete)", "Duplicate row"]) assert.equal(await page.locator(`button[title='${title}']`).isDisabled(), true, title);
assert.equal(await page.locator("button[title^='Submit changes']").isDisabled(), true);
await page.click(".row-item.leaf:has-text('orders')", { button: "right" });
for (const label of ["Import Data…", "Duplicate…", "Rename…", "Drop…"]) assert.equal(await page.locator(`.ctx:not(.ctx-nested) > .ctx-item.disabled .ctx-label:text-is('${label}')`).count(), 1, `${label} is disabled`);
await page.hover(".ctx:not(.ctx-nested) > .ctx-item.sub:has-text('Tools')");
for (const label of ["Analyze", "Optimize", "Repair", "Truncate…"]) assert.equal(await page.locator(`.ctx-nested .ctx-item.disabled .ctx-label:text-is('${label}')`).count(), 1, `${label} is disabled`);
await page.keyboard.press("Escape");
await page.mouse.click(900, 600);
ok("grid editing, row buttons and the write entries of the table menu are off on a read-only connection");

// ---- the console refuses writes with a clear message, and shows no review (nothing would run)
await page.click(".qtab:has-text('Query 1')");
await write("DELETE FROM orders");
await run();
assert.match(await page.locator(".results .error").innerText(), /read-only/);
assert.equal(await page.locator(".review-dialog").count(), 0);
await write("SELECT 1");
await run();
assert.equal(await page.locator(".results .error").count(), 0, "reads still run");
ok("a write on a read-only connection is refused with the reason; reads still run");

// ---- the designer can be looked at but not applied
await page.click(".row-item.leaf:has-text('orders')", { button: "right" });
await page.click(".ctx:not(.ctx-nested) > .ctx-item .ctx-label:text-is('Design table…')");
await page.waitForSelector(".designer");
assert.match(await page.locator(".designer .banner").first().innerText(), /Read-only connection/);
assert.equal(await page.locator(".designer button:text-is('Apply changes')").isDisabled(), true);
ok("the table designer shows the definition but cannot apply on a read-only connection");

// ---- writable production: review with typed confirmation
await connect(3);
assert.equal(await page.locator(".env-bar .ro-badge").count(), 0);
await page.click(".qtab:has-text('Query 1')").catch(() => {});
await write("DELETE FROM orders");
const runsBefore = (await calls("run_query")).length;
await run();
await page.waitForSelector(".review-dialog");
assert.match(await page.locator(".review-dialog h3").innerText(), /Production/i);
assert.match(await page.locator(".review-dialog .risk-sql").innerText(), /DELETE FROM orders/);
assert.match(await page.locator(".review-dialog .risk-why").innerText(), /every row/);
assert.match(await page.locator(".review-dialog .est").innerText(), /about 5 rows/);
assert.equal((await calls("run_query")).length, runsBefore, "nothing was sent yet");
const go = page.locator(".review-dialog button:text-is('Run anyway')");
assert.equal(await go.isDisabled(), true, "production asks for the word");
await page.fill(".review-dialog input", "prod");
assert.equal(await go.isDisabled(), true);
await page.click(".review-dialog button:text-is('Cancel')");
assert.equal((await calls("run_query")).length, runsBefore, "Cancel runs nothing");
await run();
await page.waitForSelector(".review-dialog");
await page.fill(".review-dialog input", "production");
await go.click();
await page.waitForFunction((n) => window.__calls.filter((c) => c.cmd === "run_query").length > n, runsBefore);
assert.equal((await calls("run_query")).at(-1).args.sql, "DELETE FROM orders");
ok("on production a destructive statement is reviewed (SQL, reason, estimated rows) and needs the word 'production'; Cancel runs nothing");

// ---- harmless statements are not interrupted
const n = (await calls("run_query")).length;
await write("SELECT 1");
await run();
await write("DELETE FROM orders WHERE id = 1");
await run();
assert.equal(await page.locator(".review-dialog").count(), 0);
assert.equal((await calls("run_query")).length, n + 2);
ok("SELECT and DELETE with a real WHERE run without a review");

// ---- confirm when saving grid changes on production
await page.click(".tree .db:has-text('shop')");
await page.dblclick(".row-item.leaf:has-text('orders')");
await page.waitForSelector("table.dg tbody tr");
await page.locator("table.dg tbody tr").first().locator("td").nth(4).dblclick();
await page.fill(".cell-modal-text", "changed");
await page.click(".cell-modal button.primary:has-text('Apply')");
await page.click("button[title^='Submit changes']");
await page.waitForSelector(".dialog:has-text('Production database')");
assert.match(await page.locator(".dialog p").innerText(), /1 change to shop\.orders will be saved/);
const applied = (await calls("apply_changes")).length;
await page.click(".dialog button:text-is('Cancel')");
assert.equal((await calls("apply_changes")).length, applied, "Cancel saves nothing");
await page.click("button[title^='Submit changes']");
await page.click(".dialog button:text-is('Save changes')");
await page.waitForFunction((a) => window.__calls.filter((c) => c.cmd === "apply_changes").length > a, applied);
ok("saving row changes on production asks first, and Cancel saves nothing");

// ---- staging and development: review without the typed word; opt-out
await connect(4);
assert.equal((await page.locator(".env-bar .env-badge").innerText()).toLowerCase(), "staging");
await page.click(".qtab:has-text('Query 1')").catch(() => {});
await write("TRUNCATE TABLE orders");
await run();
await page.waitForSelector(".review-dialog");
assert.equal(await page.locator(".review-dialog input").count(), 0, "no typed word outside production");
assert.equal(await page.locator(".review-dialog button:text-is('Run anyway')").isEnabled(), true);
await page.click(".review-dialog button:text-is('Cancel')");
await connect(5);
const before5 = (await calls("run_query")).length;
await page.click(".qtab:has-text('Query 1')").catch(() => {});
await write("DROP TABLE orders");
await run();
assert.equal(await page.locator(".review-dialog").count(), 0, "the warning is switched off for this connection");
assert.equal((await calls("run_query")).length, before5 + 1);
ok("staging reviews without the typed word; a connection can switch the warning off");

await browser.close();
assert.deepEqual(errors, [], "browser errors: " + errors.join(" | "));
console.log("ALL SAFETY UI CHECKS PASSED");
