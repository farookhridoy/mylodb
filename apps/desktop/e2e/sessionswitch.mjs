// Editing then switching connections at once still saves the edit under the connection it was made on.
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
await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
await page.click(".cm-content");
await page.keyboard.type("SELECT 'unsaved edit'");
await page.selectOption(".conn-bar select", "4"); // inside the 800 ms autosave delay
await page.waitForFunction(() => document.querySelectorAll(".conn-tab").length === 2);
const saved = await calls("session_set");
assert.ok(saved.some((a) => a.id === 1 && a.session.includes("unsaved edit")), "the edit was saved for connection 1: " + JSON.stringify(saved).slice(0, 300));
assert.ok(!saved.some((a) => a.id === 4 && a.session.includes("unsaved edit")), "and not under connection 4");
ok("switching connections right after typing saves the edit under the connection it belongs to");
assert.deepEqual(errors, []);
await browser.close();
console.log("ALL SESSION SWITCH UI CHECKS PASSED");
