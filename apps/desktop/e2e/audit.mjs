// Settings → Audit log: switch it on, see the file and the latest changes.
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
await page.waitForTimeout(500);
await page.click("button[aria-label=Settings]");
await page.waitForSelector("[role=dialog][aria-label=Settings]");
await page.fill(".prefs input[placeholder], .dialog.prefs input", "audit").catch(() => {});
await page.click(".dialog.prefs :text-is('Audit log')");
await page.waitForSelector(".audit-page");
assert.match(await page.locator(".audit-page").innerText(), /Turn the log on to start recording/);
assert.equal(await page.locator(".audit-page input[type=checkbox]").isChecked(), false, "off by default");
ok("the audit log is off by default and says how to turn it on");

await page.check(".audit-page input[type=checkbox]");
await page.waitForSelector(".audit-table tbody tr");
assert.equal(await page.locator(".audit-table tbody tr").count(), 2);
assert.match(await page.locator(".audit-page").innerText(), /audit\.log/);
assert.equal(await page.locator(".audit-table tbody tr.lvl-error").count(), 1, "a failed statement is marked");
assert.match(await page.locator(".audit-table").innerText(), /DELETE FROM orders WHERE id = 7/);
ok("turning it on shows the file and the latest entries, with failures marked");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL AUDIT LOG CHECKS PASSED");
