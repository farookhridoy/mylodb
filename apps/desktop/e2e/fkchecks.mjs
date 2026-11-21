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

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL FK CHECK UI CHECKS PASSED");
