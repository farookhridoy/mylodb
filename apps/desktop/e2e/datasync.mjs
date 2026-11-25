// Data compare and sync, and copying a database to another connection.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1400, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
await page.addInitScript({ content: "localStorage.setItem('mylodb.tourDone','1');\n" + fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
const calls = (cmd) => page.evaluate((c) => window.__calls.filter((x) => x.cmd === c).map((x) => x.args), cmd);
await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");

// compare two tables
await page.evaluate(() => window.__emit("menu-action", "data_compare"));
await page.waitForSelector(".datacompare");
assert.equal(await page.locator(".datacompare button:has-text('Compare')").first().isDisabled(), true, "nothing to compare until both tables are chosen");
await page.selectOption("select[aria-label='Source database']", "shop");
await page.selectOption("select[aria-label='Source table']", "orders");
await page.selectOption("select[aria-label='Target database']", "shop");
await page.selectOption("select[aria-label='Target table']", "customers");
await page.waitForSelector(".dc-key input:checked");
assert.deepEqual(await page.locator(".dc-key input:checked").evaluateAll((els) => els.map((e) => e.closest("label").innerText.trim())), ["id (primary key)"], "the primary key is chosen by default");
await page.click(".datacompare .actions button:has-text('Compare')");
await page.waitForSelector(".dc-result");
assert.match(await page.locator(".dc-chips").innerText(), /10\s+identical[\s\S]*1\s+different[\s\S]*2\s+only in the source[\s\S]*1\s+only in the target/);
assert.match(await page.locator(".dc-result").innerText(), /Not compared \(in one table only\): legacy_flag/);
assert.match(await page.locator(".dc-result").innerText(), /Gust/);
const req = (await calls("data_compare")).at(-1).request;
assert.deepEqual([req.source_table, req.target_table, req.key, req.delete_extra], ["orders", "customers", ["id"], false]);
ok("two tables are compared on the primary key and the differences are shown by kind");

// apply: confirmed, in one transaction
await page.click(".dc-result button:has-text('Make the target match')");
await page.waitForSelector("[aria-label='Make the target match']");
assert.match(await page.locator("[aria-label='Make the target match']").innerText(), /2 row\(s\) inserted, 1 updated/);
await page.click("[aria-label='Make the target match'] button.primary");
await page.waitForSelector(".toast:has-text('Synced shop.customers: 2 inserted, 1 updated, 0 deleted')");
ok("applying asks first, says what will change, and reports what it did");

// deleting extra rows is opt-in and changes the count
await page.check(".datacompare label:has-text('Also delete') input");
await page.click(".datacompare .actions button:has-text('Compare')");
await page.waitForSelector(".dc-result");
assert.equal((await calls("data_compare")).at(-1).request.delete_extra, true);
assert.match(await page.locator(".dc-result").innerText(), /will be deleted/);
ok("deleting the rows only the target has is a separate choice");

// copy a database
await page.keyboard.press("Escape").catch(() => {});
await page.click(".tree .db:has-text('shop')");
await page.evaluate(() => window.__emit("menu-action", "copy_database"));
await page.waitForSelector("[aria-label='Copy database']");
assert.equal(await page.locator("[aria-label='Copy database'] button.primary").isDisabled(), true, "the source itself cannot be the target");
assert.match(await page.locator("[aria-label='Copy database']").innerText(), /That is the source itself/);
await page.fill("input[aria-label='Target database name']", "shop_copy");
await page.click("[aria-label='Copy database'] button.primary");
await page.waitForSelector("[aria-label='Copy database'] .banner.ok");
assert.match(await page.locator("[aria-label='Copy database'] .banner.ok").innerText(), /2 table\(s\), 6,600 rows/);
const copy = (await calls("copy_database")).at(-1);
assert.deepEqual([copy.sourceDb, copy.targetDb, copy.withData], ["shop", "shop_copy", true]);
ok("a database is copied under a new name, and the source itself is refused as a target");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL DATA SYNC CHECKS PASSED");
