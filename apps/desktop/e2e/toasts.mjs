// Status messages also appear as flash messages: they can be copied from a hover button and closed.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1400, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
await page.addInitScript({ content: "localStorage.setItem('mylodb.tourDone','1'); window.__copied = []; Object.defineProperty(navigator, 'clipboard', { value: { writeText: async (t) => { window.__copied.push(t); } } });\n" + fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
await page.waitForTimeout(600);

// a message that used to live only in the status bar now pops up
await page.evaluate(() => window.__emit("menu-action", "structure"));
await page.waitForSelector(".toast");
assert.match(await page.locator(".toast").innerText(), /Connect first/);
assert.equal(await page.locator(".toast .toast-actions").isVisible(), false, "the buttons are hidden until the mouse is over it");
ok("a status message appears as a flash message, without buttons until hovered");

// hovering shows the copy button, and it copies the text
await page.hover(".toast");
await page.waitForSelector(".toast .toast-btn[aria-label='Copy message']", { state: "visible" });
await page.click(".toast .toast-btn[aria-label='Copy message']");
assert.deepEqual(await page.evaluate(() => window.__copied), ["Connect first"]);
ok("hovering shows a copy button that copies the message");

// close button
await page.click(".toast .toast-btn[aria-label='Dismiss message']");
assert.equal(await page.locator(".toast").count(), 0);
ok("the close button removes it");

// it goes away by itself, but not while the mouse is on it
await page.evaluate(() => window.__emit("menu-action", "structure"));
await page.waitForSelector(".toast");
await page.hover(".toast");
await page.waitForTimeout(6500);
assert.equal(await page.locator(".toast").count(), 1, "held while hovered");
await page.mouse.move(5, 5);
await page.waitForFunction(() => !document.querySelector(".toast"), null, { timeout: 8000 });
ok("it fades after a few seconds and is held while the mouse is over it");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL TOAST CHECKS PASSED");
