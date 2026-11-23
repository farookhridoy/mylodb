// Several connections stay open at once: tabs above the workspace, switching keeps sessions, closing disconnects one.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1400, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
const extra = [{ id: 4, name: "Staging", color: "#e0a03b", host: "stage.example", port: 3306, user: "dev", database: null, ssl: "disabled", environment: "staging", read_only: false, query_timeout_secs: null, confirm_destructive: true }];
await page.addInitScript({ content: `window.__extraConns = ${JSON.stringify(extra)};\n` + fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
const calls = (cmd) => page.evaluate((c) => window.__calls.filter((x) => x.cmd === c).map((x) => x.args), cmd);
const names = () => page.locator(".conn-tab-name").allInnerTexts().then((t) => t.map((x) => x.trim()));

await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
assert.deepEqual(await names(), ["Local"]);
await page.selectOption(".conn-bar select", "4");
await page.waitForFunction(() => document.querySelectorAll(".conn-tab").length === 2);
assert.deepEqual(await names(), ["Local", "Staging"]);
assert.equal((await calls("disconnect")).length, 0, "opening a second connection does not close the first");
assert.equal(await page.locator(".conn-tab.active").innerText().then((t) => t.trim()), "Staging");
ok("a second connection opens beside the first; nothing is disconnected");

await page.click(".conn-tab-name:has-text('Local')");
await page.waitForFunction(() => document.querySelector(".conn-tab.active")?.innerText.includes("Local"));
assert.equal((await calls("disconnect")).length, 0);
ok("switching to a connection tab keeps both sessions");

// switching between open connections reuses their sessions: no new connect (so no keychain read), the app asks the backend to reuse
let conns = (await calls("connect")).map((a) => [a.id, a.reuse]);
assert.deepEqual(conns, [[1, false], [4, false], [1, true]], "first connects reconnect, the switch back reuses");
await page.selectOption(".conn-bar select", "4");
await page.waitForFunction(() => document.querySelector(".conn-tab.active")?.innerText.includes("Staging"));
await page.selectOption(".conn-bar select", "1");
await page.waitForFunction(() => document.querySelector(".conn-tab.active")?.innerText.includes("Local"));
conns = (await calls("connect")).map((a) => [a.id, a.reuse]);
assert.deepEqual(conns.slice(3), [[4, true], [1, true]], "the connection picker reuses open sessions too");
ok("switching between open connections asks the backend to reuse the session, from the tabs and from the picker");

await page.click("button[aria-label='Disconnect Staging']");
await page.waitForFunction(() => document.querySelectorAll(".conn-tab").length === 1);
assert.deepEqual((await calls("disconnect")).map((a) => a.id), [4]);
assert.match(await page.locator(".conn-tab.active").innerText(), /Local/);
ok("closing a tab disconnects only that connection");

await page.click("button[aria-label='Disconnect Local']");
await page.waitForFunction(() => document.querySelectorAll(".conn-tab").length === 0);
assert.match(await page.locator("nav[aria-label=Location]").innerText(), /No connection/);
ok("closing the last tab leaves the app disconnected");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL MULTI CONNECTION UI CHECKS PASSED");
