// UI test of the Databases page: list, sizes, sort, check all, bulk drop through the Preview SQL dialog, create.
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

await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
await page.click(".tree .db", { button: "right" });
await page.click(".ctx-label:has-text('Databases (list')");
await page.waitForSelector("table[aria-label=Databases]");
const names = () => page.locator("table[aria-label=Databases] tbody tr td:nth-child(2) button").allInnerTexts().then((t) => t.map((x) => x.trim()));
assert.deepEqual(await names(), ["blog", "old_2019", "shop"], "system databases are hidden and the list is sorted");
await page.waitForSelector("td:has-text('50.0 MB'), td:has-text('50 MB')");
ok("lists databases with collation, table count and size; system ones hidden");

await page.click("th:has-text('Size')");
await page.click("th:has-text('Size')");
assert.deepEqual(await names(), ["old_2019", "shop", "blog"], "sorted by size, largest first");
await page.check("text=Show system databases");
assert.equal((await names()).length, 5);
assert.equal(await page.locator("input[aria-label='Select mysql']").isDisabled(), true, "system databases cannot be selected");
await page.uncheck("text=Show system databases");
ok("sorts by size; system databases are never selectable");

await page.fill("input[aria-label='Search databases']", "o");
await page.fill("input[aria-label='Search databases']", "sh"); assert.deepEqual(await names(), ["shop"], "search narrows the list");
await page.fill("input[aria-label='Search databases']", "");
await page.check("text=Check all");
assert.equal(await page.locator("table[aria-label=Databases] tbody input:checked").count(), 3);
await page.uncheck("input[aria-label='Select shop']");
await page.click("button.danger:has-text('Drop')");
await page.waitForSelector(".preview-dialog");
const sql = await page.locator(".preview-sql").innerText();
assert.ok(sql.includes("DROP DATABASE `blog`;") && sql.includes("DROP DATABASE `old_2019`;") && !sql.includes("`shop`"), sql);
assert.equal(await g("__runPlans"), undefined);
await page.click(".preview-dialog button.danger");
await page.waitForSelector(".plan-results li");
await page.click(".preview-dialog button:text-is('Done')");
await page.waitForFunction(() => document.querySelectorAll("table[aria-label=Databases] tbody tr").length === 1);
assert.deepEqual(await names(), ["shop"]);
ok("check all, uncheck one, bulk drop shows the SQL first, then runs and refreshes");

await page.fill("input[aria-label='New database name']", "fresh");
await page.click("button.primary:text-is('Create')");
await page.waitForSelector(".preview-dialog");
assert.equal(await page.locator(".preview-sql").innerText(), "CREATE DATABASE `fresh`;");
await page.click(".preview-dialog button:text-is('Cancel')");
ok("create database previews its SQL");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL DATABASES PAGE UI CHECKS PASSED");
