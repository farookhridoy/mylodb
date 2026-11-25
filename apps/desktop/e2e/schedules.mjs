// Scheduled backups: create a job, see when it runs next, run it, read its log, pause and delete it.
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
await page.waitForSelector(".cm-content");

await page.evaluate(() => window.__emit("menu-action", "schedules"));
await page.waitForSelector(".schedules-dialog");
assert.match(await page.locator(".schedules-dialog").innerText(), /No scheduled backups yet/);
ok("File → Scheduled Backups opens the list, empty at first");

// a job without a name or database is refused with a plain message
await page.click(".schedules-dialog button:has-text('New scheduled backup')");
await page.uncheck(".sched-dbs input >> nth=0").catch(() => {});
await page.click(".sched-form button:has-text('Save')");
await page.waitForSelector(".schedules-dialog .banner.bad");
assert.match(await page.locator(".schedules-dialog .banner.bad").innerText(), /Give the job a name/);
ok("a job with no name is refused with a plain message");

// make a weekly job
await page.fill(".sched-form input[placeholder='Nightly shop backup']", "Weekly shop");
await page.selectOption(".sched-form select[aria-label=When]", "weekly");
await page.selectOption(".sched-form select[aria-label=Day]", "6");
await page.fill(".sched-form input[type=time]", "04:30");
await page.fill(".sched-form input[placeholder='/Users/you/Backups']", "/Users/me/Backups");
await page.fill(".sched-form input[type=number]", "3");
await page.check(".sched-dbs label:has-text('shop') input");
await page.click(".sched-form button:has-text('Save')");
await page.waitForSelector(".sched-list li");
const saved = (await calls("schedule_save")).at(-1).job;
assert.deepEqual(saved.rule, { kind: "weekly", weekday: 6, hour: 4, minute: 30 });
assert.equal(saved.keep, 3);
assert.deepEqual(saved.databases, ["shop"]);
assert.match(await page.locator(".sched-list li").innerText(), /every Sunday at 04:30/);
assert.match(await page.locator(".sched-list li").innerText(), /Next: 2026-11-26 03:00:00/);
ok("a weekly job is saved with its day, time, folder and keep-the-last-3, and shows when it runs next");

// run now and read the log
await page.click(".sched-list button:has-text('Run now')");
await page.waitForFunction(() => /Last: 2026-11-25 10:00:00/.test(document.querySelector(".sched-list li").innerText));
await page.click(".sched-list button:has-text('Log')");
assert.match(await page.locator(".sched-log").innerText(), /12 table\(s\).*removed 1 old backup/);
ok("Run now makes a backup and its result is in the log, including how many old backups were removed");

// pause, then delete
await page.click(".sched-list button:has-text('Pause')");
await page.waitForSelector(".sched-list li.off");
assert.match(await page.locator(".sched-list > li").innerText(), /Paused/);
await page.click(".sched-list button[title^='Delete the job']");
await page.waitForFunction(() => /No scheduled backups yet/.test(document.querySelector(".schedules-dialog").innerText));
ok("a job can be paused and deleted");

// a background run is announced
await page.keyboard.press("Escape");
await page.evaluate(() => window.__emit("schedule-done", { name: "Weekly shop", ok: false, message: "could not connect: Connection refused" }));
await page.waitForSelector(".toast.error");
assert.match(await page.locator(".toast.error").innerText(), /backup "Weekly shop" failed: could not connect/);
ok("a failed background backup is announced as a flash message");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL SCHEDULED BACKUP CHECKS PASSED");
