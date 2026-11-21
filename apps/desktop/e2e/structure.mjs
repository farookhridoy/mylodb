// UI test of the database Structure page: table list, filter, row actions, With selected, create new table.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1400, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
await page.addInitScript({ content: fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
const g = (n) => page.evaluate((k) => window[k], n);
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
const names = () => page.locator("table[aria-label=Tables] tbody tr td:nth-child(2) button").allInnerTexts().then((t) => t.map((x) => x.trim()));
const withSelected = async (label) => { await page.click("button[aria-label='With selected']"); await page.click(`.ctx-label:text-is('${label}')`); };

await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
await page.click(".tree .db:has-text('shop')", { button: "right" });
await page.click(".ctx-label:text-is('Structure (all tables)')");
await page.waitForSelector("table[aria-label=Tables]");
assert.deepEqual(await names(), ["customers", "orders", "v_orders"]);
const rowText = (await page.locator("table[aria-label=Tables] tbody tr").allTextContents()).join(" | ");
assert.match(rowText, /5,400.*InnoDB.*utf8mb4_0900_ai_ci1\.00 MB4\.00 MB/);
ok("lists tables and views with rows, engine, collation, size and overhead");

await page.fill("input[aria-label='Filter tables']", "ord");
assert.deepEqual(await names(), ["orders", "v_orders"]);
await page.fill("input[aria-label='Filter tables']", "");
assert.equal(await page.locator("button[aria-label='With selected']").isDisabled(), true, "nothing selected yet");
ok("filters by word; With selected waits for a selection");

await page.check("input[aria-label='Select customers']");
await page.check("input[aria-label='Select orders']");
await withSelected("Drop…");
await page.waitForSelector(".preview-dialog");
assert.equal(await page.locator(".preview-sql").innerText(), "DROP TABLE `shop`.`customers`, `shop`.`orders`;");
await page.click(".preview-dialog button:text-is('Cancel')");
ok("With selected > Drop shows one DROP TABLE for the chosen tables");

await withSelected("Add prefix to table…");
await page.fill(".dialog input", "old_");
await page.click(".dialog button.primary");
await page.waitForSelector(".preview-dialog");
assert.equal(await page.locator(".preview-sql").innerText(), "RENAME TABLE `shop`.`customers` TO `shop`.`old_customers`, `shop`.`orders` TO `shop`.`old_orders`;");
await page.click(".preview-dialog button:text-is('Cancel')");
await withSelected("Copy table…");
await page.check(".dialog input[type=checkbox]");
await page.click(".dialog button.primary");
await page.waitForSelector(".preview-dialog");
assert.match(await page.locator(".preview-sql").innerText(), /CREATE TABLE `shop`\.`customers_copy` LIKE `shop`\.`customers`;\nINSERT INTO/);
await page.click(".preview-dialog button:text-is('Cancel')");
ok("prefix and copy operations preview their SQL");

await withSelected("Show create");
await page.waitForSelector("[role=dialog][aria-label='Show create']");
await page.click("[role=dialog] button:text-is('Close')");
await page.locator("table[aria-label=Tables] tbody tr:nth-child(1) button.link.danger", { hasText: "Drop" }).click();
await page.waitForSelector(".preview-dialog");
assert.equal(await page.locator(".preview-sql").innerText(), "DROP TABLE `shop`.`customers`;");
await page.click(".preview-dialog button.danger");
await page.waitForSelector(".plan-results li");
await page.click(".preview-dialog button:text-is('Done')");
await page.waitForFunction(() => ![...document.querySelectorAll("table[aria-label=Tables] tbody tr")].some((r) => r.innerText.includes("customers")));
ok("row Drop previews, runs and the list refreshes");

await page.fill("input[aria-label='New table name']", "invoices");
await page.fill("input[aria-label='Number of columns']", "3");
await page.click("button.primary:text-is('Create')");
await page.waitForSelector(".qtab:has-text('invoices')");
ok("Create new table opens the designer for it");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL STRUCTURE PAGE UI CHECKS PASSED");
