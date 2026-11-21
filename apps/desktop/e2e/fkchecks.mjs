// The console's Run can switch foreign key checks per tab; the choice reaches the backend call.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1400, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
await page.addInitScript({ content: fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
const lastRun = () => page.evaluate(() => window.__calls.filter((c) => c.cmd === "run_query").at(-1)?.args);

await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
await page.click(".cm-content");
await page.keyboard.type("SELECT 1");
await page.click("button:has-text('▶ Run')");
await page.waitForFunction(() => window.__calls.some((c) => c.cmd === "run_query"));
assert.equal((await lastRun()).foreignKeyChecks, null, "default leaves the server setting alone");
ok("by default the server's foreign key setting is left alone");

await page.selectOption(".toolbar select[title='Foreign key checks while this tab runs']", "off");
await page.click("button:has-text('▶ Run')");
await page.waitForFunction(() => window.__calls.filter((c) => c.cmd === "run_query").length === 2);
assert.equal((await lastRun()).foreignKeyChecks, false);
await page.selectOption(".toolbar select[title='Foreign key checks while this tab runs']", "on");
await page.click("button:has-text('▶ Run')");
await page.waitForFunction(() => window.__calls.filter((c) => c.cmd === "run_query").length === 3);
assert.equal((await lastRun()).foreignKeyChecks, true);
ok("off and on are sent with the run");

// ---- Preview button: the exact script, a foreign key switch, then run
await page.click("button:has-text('Preview')");
await page.waitForSelector(".preview-dialog");
assert.equal(await page.locator(".preview-sql").innerText(), "SELECT 1");
await page.uncheck(".preview-dialog input[type=checkbox]");
assert.match(await page.locator(".preview-sql").innerText(), /SET FOREIGN_KEY_CHECKS = 0;\nSELECT 1\nSET FOREIGN_KEY_CHECKS = 1;|SET FOREIGN_KEY_CHECKS = 0;[\s\S]*SELECT 1[\s\S]*SET FOREIGN_KEY_CHECKS = 1;/);
const runs = (await page.evaluate(() => window.__calls.filter((c) => c.cmd === "run_query").length));
await page.click(".preview-dialog button.primary, .preview-dialog button.danger");
await page.waitForSelector(".plan-results li");
assert.equal(await page.evaluate(() => window.__calls.filter((c) => c.cmd === "run_query").length), runs + 1);
await page.click(".preview-dialog button:text-is('Done')");
ok("Preview shows the exact SQL with the foreign key switch, then runs it");

// ---- toolbar and breadcrumb
await page.click(".topbar button:has-text('Home')");
await page.waitForSelector("table[aria-label=Server]");
assert.match(await page.locator("nav[aria-label=Location]").innerText(), /Local/);
await page.click(".tree .db:has-text('shop')");
await page.waitForFunction(() => document.querySelector("nav[aria-label=Location]")?.innerText.includes("shop"));
await page.click(".crumb:has-text('shop')");
await page.waitForSelector("table[aria-label=Tables]");
ok("the toolbar has Home, and the footer breadcrumb shows connection > database and opens its pages");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL FK CHECK UI CHECKS PASSED");
