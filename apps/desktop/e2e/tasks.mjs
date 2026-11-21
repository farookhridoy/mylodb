// The footer's Tasks list: a running job shows progress and can be cancelled from there; finished ones keep their outcome.
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
const set = (k, v) => page.evaluate(([a, b]) => { window[a] = b; }, [k, v]);

await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
await page.click(".tree .db");
assert.match(await page.locator(".tasks-btn").innerText(), /^Tasks$/);
await page.click("button[title='Backup, restore, import and export']");
await page.waitForSelector(".transfer");
await page.check(".db-pick label:has-text('shop') input");
await set("__nextOpen", "/Users/me/backups");
await page.click("button:text-is('Browse…')");
await page.waitForFunction(() => document.querySelector(".path-field input")?.value === "/Users/me/backups");

await set("__jobMs", 4000);
await page.click("button:text-is('Start backup')");
await page.waitForSelector(".job-progress");
assert.match(await page.locator(".tasks-btn").innerText(), /1 running/);
await page.click(".tasks-btn");
await page.waitForSelector("[aria-label=Tasks] li.task.running");
assert.match(await page.locator("li.task.running").innerText(), /Backup shop[\s\S]*Backing up · shop\.orders/);
await page.click("li.task.running button:text-is('Cancel')");
await page.waitForSelector("li.task.cancelled");
assert.equal(await page.evaluate(() => window.__cancelled), (await page.evaluate(() => window.__backup)).jobId, "Cancel from the list stops the right job");
ok("a running backup appears in Tasks with its progress, and Cancel there stops it");

await set("__jobMs", undefined);
await page.click("button:text-is('Start backup')");
await page.waitForSelector("li.task.done");
assert.equal(await page.locator("li.task").count(), 2);
await page.click("button:text-is('Clear finished')");
assert.equal(await page.locator("li.task").count(), 0);
ok("finished tasks keep their outcome until cleared");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL TASKS PANEL UI CHECKS PASSED");
