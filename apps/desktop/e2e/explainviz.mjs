// The query plan as a diagram: steps as a tree, the slowest marked, and a comparison with the previous run.
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
await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".cm-content");
await page.click(".cm-content");
await page.keyboard.type("SELECT * FROM orders");

// Explain (estimates): a tree from the JSON plan; a full scan is flagged
await page.click("button:text-is('Explain')");
await page.waitForSelector(".plan-diagram");
assert.match(await page.locator(".plan-diagram").innerText(), /Full scan on orders/);
assert.match(await page.locator(".plan-row.warn").first().innerText(), /reads every row/);
assert.match(await page.locator(".plan-badge").innerText(), /most costly step/);
ok("Explain draws the plan as a tree from the JSON plan, flags a full scan and marks the most costly step");

// Analyze: measured times; the step that took longest on its own is marked
await page.evaluate(() => {
  window.__tree = [
    "-> Sort: total DESC  (cost=900 rows=100) (actual time=60..61 rows=100 loops=1)",
    "    -> Nested loop inner join  (cost=800 rows=100) (actual time=1..55 rows=100 loops=1)",
    "        -> Table scan on c  (cost=10 rows=100) (actual time=0.1..1 rows=100 loops=1)",
    "        -> Index lookup on o using fk (customer_id=c.id)  (cost=5 rows=1) (actual time=0.4..0.5 rows=1 loops=100)",
  ].join("\n");
});
await page.click("button:text-is('Analyze')");
await page.waitForFunction(() => document.querySelectorAll(".plan-row").length === 4);
assert.match(await page.locator(".plan-row.hot").innerText(), /Index lookup on o/);
assert.match(await page.locator(".plan-row.hot").innerText(), /slowest step/);
assert.equal(await page.locator(".plan-row").nth(2).evaluate((e) => parseFloat(getComputedStyle(e).paddingLeft)) > await page.locator(".plan-row").nth(0).evaluate((e) => parseFloat(getComputedStyle(e).paddingLeft)), true, "children are indented");
ok("Analyze shows measured time per step and marks the step that took longest on its own");

// collapse a branch
await page.locator(".plan-row").nth(1).locator(".plan-fold").click();
assert.equal(await page.locator(".plan-row").count(), 2);
await page.locator(".plan-row").nth(1).locator(".plan-fold").click();
assert.equal(await page.locator(".plan-row").count(), 4);
ok("a branch can be collapsed and opened");

// run again, faster: the result says so
await page.evaluate(() => { window.__tree = "-> Table scan on orders  (cost=100 rows=100) (actual time=0.1..30 rows=100 loops=1)"; });
await page.click("button:text-is('Analyze')");
await page.waitForSelector(".plan-versus");
assert.match(await page.locator(".plan-versus").innerText(), /faster than the previous run/);
ok("a second run of the same statement says how much faster or slower it was");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL PLAN DIAGRAM CHECKS PASSED");
