// Preferences: fonts per area apply live, persist, can be cancelled and reset; theme is chosen there too.
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
const css = (sel, prop) => page.evaluate(([s, p]) => getComputedStyle(document.querySelector(s))[p], [sel, prop]);

await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
assert.equal(await css(".sidebar", "fontSize"), "12px");
await page.keyboard.press("Meta+,");
await page.waitForSelector("[role=dialog][aria-label=Preferences]");
await page.fill("input[aria-label='Sidebar (object tree) size']", "15");
assert.equal(await css(".sidebar", "fontSize"), "15px", "applies while editing");
await page.check("input[aria-label='Sidebar (object tree) bold']");
assert.equal(await css(".sidebar", "fontWeight"), "600");
await page.fill("input[aria-label='Query editor size']", "17");
assert.equal(await css(".cm-scroller", "fontSize"), "17px");
await page.click("[role=dialog] button:text-is('Cancel')");
assert.equal(await css(".sidebar", "fontSize"), "12px", "Cancel puts the old values back");
assert.equal(await css(".cm-scroller", "fontSize"), "13px");
ok("fonts apply live and Cancel restores them");

await page.click("button[aria-label=Preferences]");
await page.fill("input[aria-label='Data grid size']", "14");
await page.fill("input[aria-label='Editor line height']", "1.8");
await page.click("[role=dialog] button:text-is('Save')");
const saved = JSON.parse(await page.evaluate(() => localStorage.getItem("mylodb.prefs")));
assert.equal(saved.grid.size, 14); assert.equal(saved.editorLineHeight, 1.8);
await page.reload();
await page.waitForSelector(".conn-bar select");
assert.equal(await page.evaluate(() => document.documentElement.style.getPropertyValue("--fs-grid")), "14px", "restored at startup");
ok("saved preferences survive a restart");

await page.click("button[aria-label=Preferences]");
await page.click("[role=dialog] button:text-is('Reset to defaults')");
assert.equal(await page.evaluate(() => document.documentElement.style.getPropertyValue("--fs-grid")), "12px");
await page.selectOption("[role=dialog] select[title='Theme']", "dark");
assert.equal(await page.evaluate(() => document.documentElement.dataset.theme), "dark");
await page.click("[role=dialog] button:text-is('Save')");
ok("reset restores the defaults; theme is selectable in Preferences");

// damaged storage falls back to defaults
await page.evaluate(() => localStorage.setItem("mylodb.prefs", '{"grid":{"size":999},"ui":"x"}'));
await page.reload();
assert.equal(await page.evaluate(() => document.documentElement.style.getPropertyValue("--fs-grid")), "16px", "out-of-range size is clamped");
ok("a damaged or out-of-range saved value is clamped, not trusted");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL PREFERENCES UI CHECKS PASSED");
