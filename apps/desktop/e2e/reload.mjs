// Reloading the window keeps the open connection: the backend still has it, so the screen attaches again instead of asking to connect.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1400, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
await page.addInitScript({ content: "localStorage.setItem('mylodb.tourDone','1'); window.__openConnections = JSON.parse(sessionStorage.getItem('open') || '[]');\n" + fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
const connects = () => page.evaluate(() => window.__calls.filter((x) => x.cmd === "connect").map((x) => x.args));

// nothing is open at first: the window stays on the connection list
await page.waitForTimeout(800);
assert.equal((await connects()).length, 0, "no connection is opened by itself");
ok("with nothing open, a fresh start does not connect");

// connect by hand; the backend now holds the session (the mock keeps that across a reload in sessionStorage)
await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".qtab");
await page.evaluate(() => sessionStorage.setItem("open", "[1]"));
await page.reload();
await page.waitForFunction(() => window.__calls.some((x) => x.cmd === "connect"));
const after = await connects();
assert.equal(after.at(-1).id, 1, "the open connection is attached again");
assert.equal(after.at(-1).reuse, true, "it reuses the open session instead of reconnecting");
await page.waitForSelector(".qtab");
assert.equal(await page.locator(".conn-bar select").inputValue(), "1", "the connection is selected again");
ok("after a reload the open connection comes back by itself, reusing the session, without picking it again");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL RELOAD CHECKS PASSED");
