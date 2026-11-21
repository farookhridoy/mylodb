// The native menu bar reports item ids; the window opens the matching page or dialog.
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
const menu = (id) => page.evaluate((i) => window.__emit("menu-action", i), id);
await page.waitForSelector(".conn-bar select");

await menu("preferences");
await page.waitForSelector("[role=dialog][aria-label=Preferences]");
await page.click("[role=dialog] button:text-is('Cancel')");
await menu("new_connection");
await page.waitForSelector(".dialog");
await page.click(".dialog button:text-is('Cancel')");
ok("Preferences and New Connection open without a connection");

await menu("databases");
await page.waitForFunction(() => document.querySelector(".statusbar")?.innerText.includes("Connect first"));
ok("items that need a connection say so instead of failing");

await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
await menu("databases");
await page.waitForSelector("table[aria-label=Databases]");
await menu("catalog");
await page.waitForSelector("table[aria-label=Engines]");
await menu("server_tools");
await page.waitForSelector(".qtab:has-text('Server')");
await menu("home");
await page.waitForSelector("table[aria-label=Server]");
await menu("theme_dark");
await page.waitForFunction(() => document.documentElement.dataset.theme === "dark");
await menu("theme_light");
await page.waitForFunction(() => document.documentElement.dataset.theme === "light");
ok("Database, Tools, Home and View > Theme items open the right pages");

await menu("structure");
await page.waitForFunction(() => document.querySelector(".statusbar")?.innerText.includes("Select a database"));
await page.click(".tree .db:has-text('shop')");
await menu("structure");
await page.waitForSelector("table[aria-label=Tables]");
const tabs = await page.locator(".qtab").count();
await menu("new_query");
assert.equal(await page.locator(".qtab").count(), tabs + 1);
await menu("close_tab");
assert.equal(await page.locator(".qtab").count(), tabs);
ok("Structure needs a selected database; New Query and Close Tab work");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL MENU BAR UI CHECKS PASSED");
