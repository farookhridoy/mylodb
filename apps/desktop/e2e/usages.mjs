// "Where is it used?": the objects that mention a table or a column, and the warning before a rename.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1400, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
await page.addInitScript({ content: "localStorage.setItem('mylodb.tourDone','1'); window.__viewDeps = true;\n" + fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
const calls = (cmd) => page.evaluate((c) => window.__calls.filter((x) => x.cmd === c).map((x) => x.args), cmd);
await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
await page.click(".tree .db:has-text('shop')");

await page.click(".row-item.leaf:has-text('orders')", { button: "right" });
await page.click(".ctx .ctx-label:text-is('Where is it used…')");
await page.waitForSelector(".usages-dialog .usage-list section");
const groups = await page.locator(".usage-list section h4").allInnerTexts();
assert.deepEqual(groups.map((g) => g.replace(/\s*\(\d+\)/, "")), ["view", "trigger", "foreign key (points here)"]);
assert.match(await page.locator(".usage-list").innerText(), /v_big_orders/);
assert.deepEqual((await calls("find_usages")).at(-1), { id: 1, db: "shop", table: "orders", column: null });
ok("Where is it used lists the views, triggers and keys that mention a table, grouped by kind");

// narrow to a column
await page.selectOption(".usages-dialog select[aria-label=Column]", "total");
await page.waitForFunction(() => document.querySelectorAll(".usage-list li").length === 1);
assert.equal((await calls("find_usages")).at(-1).column, "total");
ok("it can be narrowed to one column");
await page.keyboard.press("Escape");

// renaming warns that other objects still use the old name
await page.click(".row-item.leaf:has-text('orders')");
await page.keyboard.press("F2");
await page.waitForSelector(".dialog h3:has-text('Rename orders')");
await page.fill(".dialog input", "purchases");
await page.click(".dialog button.primary:has-text('Rename')");
await page.waitForSelector(".preview-dialog");
assert.match(await page.locator(".dialog").last().innerText(), /3 other object\(s\) mention orders \(1 view, 1 trigger, 1 foreign key\)/);
assert.match(await page.locator(".preview-sql").innerText(), /RENAME TABLE[\s\S]*CREATE OR REPLACE VIEW `shop`\.`v_big_orders` AS select `shop`\.`purchases`/, "the views are pointed at the new name, after the rename");
ok("a rename says how many other objects use the old name, and updates the views after it");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL USAGES CHECKS PASSED");
