// UI end-to-end test of autocomplete, the formatter, EXPLAIN / EXPLAIN ANALYZE and the profiler,
// against the fake Tauri bridge in mock.js. Needs the dev server (`npm run dev`) and CHROME_PATH.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await browser.newPage({ viewport: { width: 1500, height: 900 } });
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
page.on("console", (m) => m.type() === "error" && !m.text().includes("404") && errors.push("console: " + m.text()));
await page.addInitScript({ content: fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");

const calls = (cmd) => page.evaluate((c) => window.__calls.filter((x) => x.cmd === c), cmd);
const last = async (cmd) => (await calls(cmd)).at(-1)?.args;
const doc = () => page.evaluate(() => document.querySelector(".cm-content").innerText.replace(/ /g, " "));
const settle = () => page.waitForTimeout(450);
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
const clear = async () => { await page.click(".cm-content"); await page.keyboard.press("Meta+a"); await page.keyboard.press("Backspace"); };

await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
await page.waitForSelector(".cm-content");

// ---------- autocomplete ----------
await clear();
await page.keyboard.type("SELECT * FROM or");
await page.waitForSelector(".cm-tooltip-autocomplete li");
const options = await page.locator(".cm-tooltip-autocomplete li").allInnerTexts();
assert.ok(options.some((o) => o.includes("orders")) && options.some((o) => o.includes("order items")), `popup: ${options}`);
assert.ok(!options.some((o) => o.includes("customers")), "the backend ranks and filters; the popup shows only what it returned");
const call = await last("complete_sql");
assert.equal(call.sql, "SELECT * FROM or");
assert.equal(call.cursor, 16, "cursor is sent as a UTF-16 offset");
assert.equal(call.id, 1);
ok("typing opens a popup with the backend's suggestions and sends the cursor position");

await page.keyboard.press("Enter");
assert.equal((await doc()).trim(), "SELECT * FROM orders");
ok("accepting a suggestion replaces the typed word using the backend's range");

// a quoted insert is applied verbatim
await clear();
await page.keyboard.type("SELECT * FROM ord");
await page.waitForSelector(".cm-tooltip-autocomplete li");
await page.locator(".cm-tooltip-autocomplete li", { hasText: "order items" }).click();
assert.equal((await doc()).trim(), "SELECT * FROM `order items`");
ok("names that need quoting are inserted quoted");

// the database selector is part of the request
await page.selectOption(".dbsel select", "shop");
await clear();
await page.keyboard.type("SELECT * FROM cu");
await page.waitForSelector(".cm-tooltip-autocomplete li");
assert.equal((await last("complete_sql")).database, "shop");
await page.keyboard.press("Escape");
ok("the selected database is used to resolve names");

// ---------- formatter ----------
await clear();
await page.keyboard.type("select a,b from t where x=1");
await page.click("button:text-is('Format')");
await settle();
let fmt = await last("format_sql");
assert.equal(fmt.sql, "select a,b from t where x=1");
assert.deepEqual(fmt.options, { indent: { style: "spaces", width: 2 }, keyword_case: "upper", lines_between_statements: 1 });
assert.match(await doc(), /SELECT a,b\s+FROM t\s+WHERE x=1/);
ok("Format sends the document with the default options and replaces it");

// options persist and are used
await page.click("button[title='Formatting options']");
await page.locator(".popup label:has-text('Indent') select").selectOption("tab");
await page.locator(".popup label:has-text('Keywords') select").selectOption("lower");
await page.mouse.move(900, 600);
assert.deepEqual(JSON.parse(await page.evaluate(() => localStorage.getItem("mylodb.formatOptions"))).indent, { style: "tab" });
await clear();
await page.keyboard.type("SELECT 1 FROM t");
await page.keyboard.press("Meta+Shift+f");
await settle();
fmt = await last("format_sql");
assert.equal(fmt.options.keyword_case, "lower");
assert.deepEqual(fmt.options.indent, { style: "tab" });
assert.match(await doc(), /select 1\s+from t/);
ok("options are saved locally, and ⇧⌘F formats");

// only the selection is formatted when there is one
await clear();
await page.keyboard.type("select 1; select 2 from u");
for (let i = 0; i < 15; i++) await page.keyboard.press("Shift+ArrowLeft");
await page.click("button:text-is('Format')");
await settle();
assert.equal((await last("format_sql")).sql, "select 2 from u");
assert.match(await doc(), /^select 1;/, "text outside the selection is untouched");
ok("formatting applies to the selection only");

// a formatter error is shown, not swallowed
await page.evaluate(() => { const h = window.__TAURI_INTERNALS__.invoke; window.__TAURI_INTERNALS__.invoke = async (c, a) => { if (c === "format_sql") { window.__calls.push({ cmd: c, args: a }); throw "this script uses DELIMITER; select a single statement to format"; } return h(c, a); }; });
await page.click("button:text-is('Format')");
await page.waitForSelector("text=uses DELIMITER");
ok("formatter errors are reported");

// ---------- explain ----------
await clear();
await page.keyboard.type("SELECT * FROM orders; SELECT 2");
await page.keyboard.press("ArrowLeft");
await page.keyboard.press("Meta+ArrowUp");           // cursor at the start: first statement
await page.click("button:text-is('Explain')");
await page.waitForSelector(".hints .hint");
let ex = await last("explain_query");
assert.equal(ex.sql, "SELECT * FROM orders", "explains the statement at the cursor");
assert.equal(ex.analyze, false);
assert.equal(await page.locator(".hint.warning").count(), 1);
assert.equal(await page.locator(".hint.good").count(), 1);
assert.match(await page.locator(".result-tabs.sub button.active").innerText(), /Plan \(1 ⚠\)/);
assert.equal(await page.locator(".analysis-body table.grid tbody tr").count(), 1);
ok("Explain targets the statement at the cursor and lists plain-language hints");

await page.click(".result-tabs.sub button:text-is('JSON')");
assert.ok(await page.locator(".jrow.hot:has-text('access_type')").count() > 0, "tuning keys are highlighted");
await page.locator(".jrow.toggle").first().click();
ok("the JSON plan renders as a collapsible tree");

// selection beats cursor
await clear();
await page.keyboard.type("SELECT 1; SELECT * FROM orders");
for (let i = 0; i < 20; i++) await page.keyboard.press("Shift+ArrowLeft");
await page.click("button:text-is('Analyze')");
await page.waitForSelector(".result-tabs.sub button:text-is('Timed tree')");
ex = await last("explain_query");
assert.equal(ex.analyze, true);
assert.equal(ex.sql, "SELECT * FROM orders", "the selection (not the whole script) is what gets analysed");
assert.match(await page.locator(".tree-text").innerText(), /actual time/);
ok("Analyze requests EXPLAIN ANALYZE and shows the timed tree");

// errors
await clear();
await page.keyboard.type("SELECT BAD");
await page.click("button:text-is('Explain')");
await page.waitForSelector(".analysis-wrap .error");
assert.match(await page.locator(".analysis-wrap .error").innerText(), /syntax error/);
ok("explain errors are shown in the analysis pane");

// ---------- profile ----------
await clear();
await page.keyboard.type("SELECT * FROM orders");
await page.click("button:text-is('Profile')");
await page.waitForSelector(".analysis.profile");
assert.equal((await last("profile_query")).sql, "SELECT * FROM orders");
assert.match(await page.locator(".summary").innerText(), /42\s*ms[\s\S]*3\s*row/);
assert.equal(await page.locator("table.stages tr").count(), 2);
assert.match(await page.locator("table.stages tr").first().innerText(), /executing[\s\S]*30\.00 ms[\s\S]*100%/);
assert.match(await page.locator("table.counters").innerText(), /Innodb_rows_read\s+20,000/);
assert.equal(await page.locator("table.counters tr.warn").count(), 1, "table scans are flagged");
ok("Profile shows total time, stage bars and the work the statement caused");

await clear();
await page.keyboard.type("DELETE FROM t");
await page.click("button:text-is('Profile')");
await page.waitForSelector("text=only SELECT statements can be profiled");
ok("profiling a write is refused with a clear message");

// ---------- schema changes refresh autocomplete ----------
const before = (await page.evaluate(() => window.__refreshed)) || 0;
await clear();
await page.keyboard.type("CREATE TABLE z (id int)");
await page.keyboard.press("Meta+Enter");
await settle();
assert.equal((await page.evaluate(() => window.__refreshed)) || 0, before + 1, "DDL triggers an index refresh");
await clear();
await page.keyboard.type("SELECT 1");
await page.keyboard.press("Meta+Enter");
await settle();
assert.equal((await page.evaluate(() => window.__refreshed)) || 0, before + 1, "a plain SELECT does not");
await page.click("button[title='Refresh objects and rebuild autocomplete']");
await settle();
assert.equal((await page.evaluate(() => window.__refreshed)) || 0, before + 2, "the tree button rebuilds on demand");
ok("DDL (and the refresh button) rebuild the object index that autocomplete uses");

await browser.close();
assert.deepEqual(errors, [], "browser errors: " + errors.join(" | "));
console.log("ALL EDITOR UI CHECKS PASSED");
