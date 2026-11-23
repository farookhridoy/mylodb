// In-app updates: a new version is offered with its release notes; Update now downloads, installs and restarts; Later and Skip this version
// are remembered; Help → Check for Updates always answers.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1400, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
const BODY = "**Added**\n* **SQLite writes.** Edit rows and run SQL that changes the file.\n* A `Connecting` spinner.\n\n**Changed**\n* Grid headers show only the name.";
const offer = `window.__update = localStorage.getItem("__noUpdate") ? null : { version: "0.4.0", date: "2026-11-24T10:00:00Z", body: ${JSON.stringify(BODY)} };\n`;
await page.addInitScript({ content: offer + fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
const dialog = page.locator(".update-dialog");
const log = () => page.evaluate(() => window.__updateLog);

// a newer version is found a few seconds after launch and shown with what changed
await dialog.waitFor({ timeout: 12000 });
assert.match(await dialog.innerText(), /MyLoDB 0\.4\.0 is available/);
assert.match(await dialog.innerText(), /You have version 0\.3\.0/);
assert.equal(await dialog.locator(".update-notes h4").count(), 2, "the Added and Changed headings");
assert.equal(await dialog.locator(".update-notes li").count(), 3);
assert.equal(await dialog.locator(".update-notes li b").first().innerText(), "SQLite writes.");
assert.equal(await dialog.locator(".update-notes code").innerText(), "Connecting");
ok("a new version is offered with its release notes rendered");

// Later closes it and remembers nothing
await dialog.locator("button:has-text('Later')").click();
assert.equal(await dialog.count(), 0);
assert.equal(await page.evaluate(() => localStorage.getItem("mylodb.skipUpdate")), null);
ok("Later closes the dialog and does not skip the version");

// Help → Check for Updates offers it again
await page.evaluate(() => window.__emit("menu-action", "check_updates"));
await dialog.waitFor();
ok("Help → Check for Updates shows the update again");

// Skip this version: a quiet check no longer offers it, a manual one still does
await dialog.locator("button:has-text('Skip this version')").click();
assert.equal(await page.evaluate(() => localStorage.getItem("mylodb.skipUpdate")), "0.4.0");
await page.reload();
await page.waitForTimeout(6000);
assert.equal(await dialog.count(), 0, "the skipped version is not offered on the next launch");
await page.evaluate(() => window.__emit("menu-action", "check_updates"));
await dialog.waitFor();
ok("a skipped version stays quiet on launch but a manual check still offers it");

// Update now: progress while downloading, then install, then restart, in that order; the buttons are locked meanwhile
await page.evaluate(() => { window.__downloadMs = 1200; });
await dialog.locator("button:has-text('Update now')").click();
await dialog.locator("[role=progressbar]").waitFor();
assert.match(await dialog.innerText(), /Downloading 1\.0 MB of 4\.0 MB/);
for (const b of ["Skip this version", "Later", "Update now"]) assert.equal(await dialog.locator(`button:has-text('${b}')`).isDisabled(), true, b);
await page.waitForFunction(() => window.__updateLog.includes("restart"));
assert.deepEqual(await log(), ["download", "install", "restart"]);
ok("Update now downloads with progress, installs, then restarts");

// automatic checks can be turned off in About; a manual check on an up-to-date copy says so
await page.evaluate(() => localStorage.setItem("__noUpdate", "1"));
await page.reload();
await page.evaluate(() => window.__emit("menu-action", "check_updates"));
await page.waitForSelector("text=is up to date");
await page.evaluate(() => window.__emit("menu-action", "about"));
const auto = page.locator("label:has-text('Check for updates automatically') input");
assert.equal(await auto.isChecked(), true);
await auto.uncheck();
assert.equal(await page.evaluate(() => localStorage.getItem("mylodb.autoUpdate")), "0");
await page.click(".about button:has-text('Check for updates')");
await page.waitForSelector("text=is up to date");
ok("an up-to-date copy says so, and automatic checks can be switched off in About");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL UPDATE UI CHECKS PASSED");
