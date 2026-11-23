// UI test of views / routines / triggers / events in the object browser, indexes, database size and restored state.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const ctx = await browser.newContext({ viewport: { width: 1400, height: 900 } });
const page = await ctx.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
page.on("console", (m) => m.type() === "error" && !m.text().includes("404") && errors.push("console: " + m.text()));
await page.addInitScript({ content: fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
const g = (n) => page.evaluate((k) => window[k], n);
const text = (loc) => loc.allInnerTexts().then((t) => t.map((x) => x.replace(/\s+/g, " ").trim()));
const menu = () => text(page.locator(".ctx button, .ctx .item, .context-menu *"));
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
const editorText = () => page.evaluate(() => document.querySelector(".cm-content")?.innerText ?? "");

await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
await page.click(".tree .db:has-text('shop')");

// ---- database size and every object group
await page.waitForSelector(".row-item.db .size");
assert.match(await page.locator(".row-item.db .size").first().innerText(), /5\.\d+ MB/, "customers 5.2 MB + orders 112 KB");
for (const label of ["Tables", "Views", "Stored Procs", "Functions", "Triggers", "Events"]) {
  assert.equal(await page.locator(`.row-item.group:has-text('${label}')`).count(), 1, label);
}
ok("the database shows its total size and all six object groups");

// ---- indexes under a table
await page.click(".row-item.leaf:has-text('orders')");
await page.waitForTimeout(300);          // the Tables group is already open
await page.waitForSelector(".idx-head");
assert.match(await page.locator(".idx-head").innerText(), /indexes\s*2/i);
assert.equal(await page.locator(".row-item.idx").count(), 0, "the Indexes group starts closed");
await page.click(".idx-head");
assert.deepEqual(await text(page.locator(".row-item.idx")), ["PRIMARY (id)", "idx_customer_total (customer_id, total)"]);
await page.click(".idx-head");
assert.equal(await page.locator(".row-item.idx").count(), 0, "and closes again");
ok("a table's indexes are a group that starts closed and lists them with their columns when opened");


// ---- show a view's definition: becomes a runnable CREATE OR REPLACE
await page.click(".row-item.group:has-text('Views')");
await page.click(".row-item.leaf:has-text('v_orders')", { button: "right" });
const viewMenu = await text(page.locator("body"));
assert.ok(viewMenu.join(" ").includes("Show / edit definition") && viewMenu.join(" ").includes("Drop…") && viewMenu.join(" ").includes("View Data"));
await page.click("text=Show / edit definition");
await page.waitForFunction(() => /CREATE OR REPLACE/.test(document.querySelector(".cm-content")?.innerText ?? ""));
assert.match(await editorText(), /CREATE OR REPLACE ALGORITHM=UNDEFINED/);
assert.deepEqual(await g("__defCalls"), ["view:v_orders"]);
ok("a view's definition opens in a new tab as CREATE OR REPLACE VIEW");

// ---- a procedure: DELIMITER-wrapped DROP + CREATE
await page.click(".row-item.group:has-text('Stored Procs')");
await page.dblclick(".row-item.leaf:has-text('p_total')");
await page.waitForFunction(() => /DELIMITER/.test(document.querySelector(".cm-content")?.innerText ?? ""));
const proc = await editorText();
assert.match(proc, /DELIMITER \$\$/);
assert.match(proc, /DROP PROCEDURE IF EXISTS `shop`\.`p_total`\$\$/);
assert.match(proc, /CREATE DEFINER=`root`@`localhost` PROCEDURE `p_total`\(\)[\s\S]*END\$\$/);
assert.match(proc, /DELIMITER ;/);
ok("double-clicking a procedure opens a runnable DROP + CREATE script with DELIMITER");

// ---- triggers, events and functions have the same menu
for (const [group, name] of [["Triggers", "trg_orders_bi"], ["Events", "ev_nightly"], ["Functions", "f_tax"]]) {
  await page.click(`.row-item.group:has-text('${group}')`);
  await page.click(`.row-item.leaf:has-text('${name}')`, { button: "right" });
  assert.ok((await text(page.locator("body"))).join(" ").includes("Show / edit definition"), name);
  await page.keyboard.press("Escape");
  await page.mouse.click(900, 700);
}
ok("functions, triggers and events have the definition menu too");

// ---- drop a view: the SQL is shown first, then it runs and disappears
await page.click(".row-item.leaf:has-text('v_orders')", { button: "right" });
await page.click(".ctx >> text=Drop…");
await page.waitForSelector(".preview-dialog");
assert.equal(await page.locator(".preview-sql").innerText(), "DROP VIEW `shop`.`v_orders`;");
assert.equal(await g("__runPlans"), undefined, "nothing ran yet");
await page.click("button.danger:text-is('Drop view')");
await page.waitForSelector(".plan-results li");
assert.deepEqual((await g("__runPlans")).at(-1), ["DROP VIEW `shop`.`v_orders`;"]);
await page.click(".preview-dialog button:text-is('Done')");
await page.waitForFunction(() => ![...document.querySelectorAll(".row-item.leaf")].some((e) => e.innerText.includes("v_orders")));
ok("dropping a view shows its SQL first, runs once, and removes it from the tree");

// ---- new object templates from the database menu
await page.click(".tree .db:has-text('shop')", { button: "right" });
await page.hover(".ctx-item.sub:has-text('Create')");
assert.ok((await text(page.locator("body"))).join(" ").includes("New procedure…"));
await page.click(".ctx-nested .ctx-label:text-is('New trigger…')");
await page.waitForFunction(() => /CREATE TRIGGER `shop`/.test(document.querySelector(".cm-content")?.innerText ?? ""));
assert.match(await editorText(), /DELIMITER \$\$[\s\S]*BEFORE INSERT ON `shop`\.`table_name`/);
ok("the database menu offers templates for new views, procedures, functions, triggers and events");

// ---- expansion and selection come back after reconnecting (a reload here)
await page.click(".row-item.leaf:has-text('orders')");
await page.waitForTimeout(300);            // select the table
await page.reload();
await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".idx-head", { timeout: 5000 });
assert.equal(await page.locator(".filter input").getAttribute("placeholder"), "Filter tables", "the selection is restored, the lock is not");
assert.ok(await page.locator(".row-item.group:has-text('Tables')").count() === 1);
ok("what was expanded and selected is restored the next time the connection opens");

// ---- F5 refreshes the object list
const before = (await g("__refreshed")) ?? 0;
await page.keyboard.press("F5");
await page.waitForFunction((b) => window.__refreshed > b, before);
ok("F5 refreshes the browser");

await browser.close();
assert.deepEqual(errors, [], "browser errors: " + errors.join(" | "));
console.log("ALL OBJECT BROWSER UI CHECKS PASSED");
