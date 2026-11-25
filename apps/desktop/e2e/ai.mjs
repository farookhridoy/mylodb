// Editor help: Tab takes a suggestion, problems are underlined, and the AI assistant writes a query from a sentence.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1400, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
await page.addInitScript({ content: "localStorage.setItem('mylodb.tourDone','1');\n" + fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
const calls = (cmd) => page.evaluate((c) => window.__calls.filter((x) => x.cmd === c).map((x) => x.args), cmd);
const doc = () => page.evaluate(() => document.querySelector(".cm-content").innerText.replace(/ /g, " "));
const clear = async () => { await page.click(".cm-content"); await page.keyboard.press("ControlOrMeta+a"); await page.keyboard.press("Backspace"); };

await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".cm-content");

// Tab takes the highlighted suggestion and stays in the editor
await clear();
await page.keyboard.type("SELECT * FROM ord");
await page.waitForSelector(".cm-tooltip-autocomplete li");
await page.keyboard.press("Tab");
assert.match(await doc(), /FROM orders/);
assert.equal(await page.evaluate(() => !!document.activeElement.closest(".cm-editor")), true, "focus stays in the editor");
ok("Tab accepts the highlighted suggestion and focus stays in the editor");

// with no suggestion open, Tab indents instead of moving focus
await clear();
await page.keyboard.type("x");
await page.keyboard.press("Escape");
await page.keyboard.press("Tab");
assert.equal(await page.evaluate(() => !!document.activeElement.closest(".cm-editor")), true, "focus stays in the editor");
assert.match(await doc(), /^\s+x|x\s*$/);
assert.ok((await doc()).length > 1, "Tab inserted indentation");
ok("Tab without a suggestion indents and does not leave the editor");

// problems are underlined, with the message on hover
await clear();
await page.evaluate(() => { window.__diag = [{ from: 14, to: 20, severity: "warning", message: "Table 'ordres' was not found in 'shop'" }]; });
await page.keyboard.type("SELECT * FROM ordres");
await page.waitForSelector(".cm-lintRange-warning", { timeout: 5000 });
assert.ok((await calls("diagnose_sql")).length > 0);
await page.hover(".cm-lintRange-warning");
await page.waitForSelector(".cm-tooltip-lint");
assert.match(await page.locator(".cm-tooltip-lint").innerText(), /was not found in 'shop'/);
ok("a problem found by the server side is underlined and explained on hover");
await page.evaluate(() => { window.__diag = []; });

// AI: off until set up
await clear();
await page.click("button:has-text('Ask AI')");
await page.waitForSelector(".ai-dialog");
assert.match(await page.locator(".ai-dialog").innerText(), /assistant is off/);
await page.click(".ai-dialog button:has-text('Set up the assistant')");
await page.waitForSelector(".ai-page");
assert.match(await page.locator(".ai-page").innerText(), /It is off until you choose a service/);
ok("Ask AI says the assistant is off and leads to its settings");

// set up Ollama (local) and save
await page.selectOption(".ai-page select", "ollama");
assert.equal(await page.locator(".ai-page input").first().inputValue(), "http://localhost:11434/v1");
await page.click(".ai-page button:has-text('Save')");
await page.waitForSelector(".ai-page .banner:has-text('Saved')");
assert.deepEqual(JSON.parse(await page.evaluate(() => localStorage.getItem("mylodb.ai"))).preset, "ollama");
await page.click(".prefs-actions button:has-text('Cancel'), .dialog button:text-is('Cancel')");
ok("the assistant is set up on a model on this computer (no key needed)");

// ask: a local model needs no consent step; the SQL is shown, then inserted, never run
await page.waitForSelector("button:has-text('Ask AI')");
await page.click("button:has-text('Ask AI')");
await page.waitForSelector(".ai-dialog textarea");
await page.fill(".ai-dialog textarea", "top 10 customers by total orders");
await page.click(".ai-dialog button:has-text('Write query')");
await page.waitForSelector(".ai-sql");
assert.match(await page.locator(".ai-sql").innerText(), /SELECT c\.name/);
const sent = (await calls("ai_write_query")).at(-1);
assert.equal(sent.request, "top 10 customers by total orders");
assert.equal(sent.settings.endpoint, "http://localhost:11434/v1");
await page.click(".ai-dialog button:has-text('Insert in editor')");
await page.waitForSelector(".ai-dialog", { state: "detached" });
await page.waitForFunction(() => /customer_id/.test(document.querySelector(".cm-content").innerText));
assert.match(await doc(), /JOIN orders o ON o\.customer_id = c\.id/);
assert.equal((await calls("run_query")).length, 0, "nothing was run");
ok("the question goes to the assistant, the SQL is shown for review, inserted into the editor and not run");

// a service on the internet asks for consent first
await page.evaluate(() => localStorage.setItem("mylodb.ai", JSON.stringify({ preset: "groq", endpoint: "https://api.groq.com/openai/v1", model: "m" })));
await page.click("button:has-text('Ask AI')");
await page.waitForSelector(".ai-dialog");
assert.match(await page.locator(".ai-dialog").innerText(), /api\.groq\.com/);
assert.match(await page.locator(".ai-dialog").innerText(), /No rows or values are sent/);
await page.click(".ai-dialog button:has-text('Allow and continue')");
await page.waitForSelector(".ai-dialog textarea");
await page.evaluate(() => { window.__aiFail = "The service refused the key (401)."; });
await page.fill(".ai-dialog textarea", "anything");
await page.click(".ai-dialog button:has-text('Write query')");
await page.waitForSelector(".ai-dialog .banner.bad");
assert.match(await page.locator(".ai-dialog .banner.bad").innerText(), /refused the key/);
ok("an internet service asks for consent once, and its errors are shown plainly");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL EDITOR AI CHECKS PASSED");
