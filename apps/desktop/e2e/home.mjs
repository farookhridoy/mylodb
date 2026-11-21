// A fresh connection opens on the server home: server facts, collation, and links.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1400, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
await page.addInitScript({ content: "window.__fresh = true;\n" + fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);

await page.selectOption(".conn-bar select", "1");
await page.waitForSelector("table[aria-label=Server]");
const server = await page.locator("table[aria-label=Server]").innerText();
assert.match(server, /9\.7\.2/); assert.match(server, /root@localhost/); assert.match(server, /1 day|1 d/); assert.match(server, /TLS_AES_256/);
assert.match(await page.locator("table[aria-label=Collation]").innerText(), /This connection uses\s+utf8mb4_0900_ai_ci/);
assert.equal(await page.locator(".qtab:has-text('Home')").count(), 1);
ok("a fresh connection opens on the home page with version, account, uptime, SSL and collation");

await page.click("button.home-action:has-text('Databases')");
await page.waitForSelector("table[aria-label=Databases]");
ok("the quick links open the pages");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL HOME PAGE UI CHECKS PASSED");
