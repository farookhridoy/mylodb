// First-run tour, the in-app manual and the About box (author links).
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1400, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
await page.addInitScript({ content: "window.__tour = true;\n" + fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
const calls = (cmd) => page.evaluate((c) => window.__calls.filter((x) => x.cmd === c).map((x) => x.args), cmd);

// first run: the tour starts by itself and walks through the main controls
await page.waitForSelector(".tour-card");
assert.match(await page.locator(".tour-card").innerText(), /1\. Add a connection/);
assert.equal(await page.locator(".tour-spot").count(), 1, "the control is highlighted");
await page.click(".tour-card .primary");
assert.match(await page.locator(".tour-card").innerText(), /2\. Pick a connection/);
await page.keyboard.press("ArrowLeft");
assert.match(await page.locator(".tour-card").innerText(), /1\./);
for (let i = 0; i < 6; i++) await page.click(".tour-card .primary");
assert.equal(await page.locator(".tour").count(), 0);
assert.equal(await page.evaluate(() => localStorage.getItem("mylodb.tourDone")), "1");
ok("the first-run tour highlights each control, steps back and forth, and is remembered as seen");

await page.reload();
await page.waitForTimeout(1200);
assert.equal(await page.locator(".tour").count(), 0, "it does not start again");
ok("the tour does not start again on the next launch");

// replay from the command palette
await page.keyboard.press("Meta+k");
await page.fill("input[aria-label=Command]", "quick tour");
await page.waitForFunction((q) => (document.querySelector(".palette li")?.innerText ?? "").toLowerCase().includes(q.split(" ")[0].toLowerCase()), "quick tour");
await page.keyboard.press("Enter");
await page.waitForSelector(".tour-card");
await page.keyboard.press("Escape");
assert.equal(await page.locator(".tour").count(), 0);
ok("Quick tour can be replayed and closed with Esc");

// manual
await page.keyboard.press("Meta+k");
await page.fill("input[aria-label=Command]", "user manual");
await page.waitForFunction((q) => (document.querySelector(".palette li")?.innerText ?? "").toLowerCase().includes(q.split(" ")[0].toLowerCase()), "user manual");
await page.keyboard.press("Enter");
await page.waitForSelector("[aria-label='User manual']");
assert.match(await page.locator(".manual article").innerText(), /Getting started/);
await page.click(".manual nav button:has-text('Running SQL')");
assert.match(await page.locator(".manual article").innerText(), /⌘↵/);
await page.click(".manual nav button:has-text('Troubleshooting')");
assert.match(await page.locator(".manual article").innerText(), /Open Anyway/);
await page.click("[aria-label='Close manual']");
assert.equal(await page.locator("[aria-label='User manual']").count(), 0);
ok("the manual opens, switches sections and closes");

// about
await page.keyboard.press("Meta+k");
await page.fill("input[aria-label=Command]", "about mylodb");
await page.waitForFunction((q) => (document.querySelector(".palette li")?.innerText ?? "").toLowerCase().includes(q.split(" ")[0].toLowerCase()), "about mylodb");
await page.keyboard.press("Enter");
await page.waitForSelector("[aria-label='About MyLoDB']");
const about = await page.locator("[aria-label='About MyLoDB']").innerText();
assert.match(about, /Version 0\.1\.0/);
assert.match(about, /farookhridoy\.com/);
assert.match(about, /thelarasoft\.com/);
await page.click("[aria-label='About MyLoDB'] a:has-text('thelarasoft.com')");
assert.deepEqual((await calls("open_url")).at(-1), { url: "https://thelarasoft.com" });
await page.click("[aria-label='About MyLoDB'] a:has-text('farookhridoy.com')");
assert.deepEqual((await calls("open_url")).at(-1), { url: "https://farookhridoy.com" });
ok("About shows the version and the author links, which open through the backend");
assert.deepEqual(errors, []);
await browser.close();
console.log("ALL HELP UI CHECKS PASSED");
