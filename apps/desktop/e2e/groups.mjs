// Connection groups: grouped in the picker under a folder heading, and the dialog offers existing groups.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1400, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
const mk = (id, name, group) => ({ id, name, group, color: "#4f8cff", host: `h${id}`, port: 3306, user: "u", database: null, ssl: "disabled" });
const extra = [mk(2, "Shop prod", "Clients"), mk(3, "Shop stage", "Clients"), mk(4, "Blog", "Personal")];
await page.addInitScript({ content: `window.__extraConns = ${JSON.stringify(extra)};\n` + fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);

await page.click(".conn-bar .ss-trigger");
await page.waitForSelector(".ss-pop");
const headers = await page.locator(".ss-group").allInnerTexts();
assert.deepEqual(headers.map((h) => h.trim().toLowerCase()), ["clients", "personal"]);
const opts = await page.locator(".ss-opt").allInnerTexts();
assert.deepEqual(opts.map((o) => o.trim()).filter((o) => !o.startsWith("Select")), ["Local", "Shop prod", "Shop stage", "Blog"]);
ok("the picker lists ungrouped connections first, then one heading per group");
await page.keyboard.press("Escape");

await page.click("button[title='New connection']");
await page.waitForSelector("input[list=connection-groups]");
const suggestions = await page.locator("#connection-groups option").evaluateAll((els) => els.map((e) => e.value));
assert.deepEqual(suggestions, ["Clients", "Personal"]);
ok("the connection dialog suggests the groups already in use");
assert.deepEqual(errors, []);
await browser.close();
console.log("ALL GROUP UI CHECKS PASSED");
