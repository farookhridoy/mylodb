// Table Data: TEXT, MEDIUMTEXT, LONGTEXT and JSON cells open in an editor window, like SQLyog's; short types stay in the cell.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await browser.newPage({ viewport: { width: 1500, height: 900 } });
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
await page.addInitScript({ content: fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
const calls = (cmd) => page.evaluate((c) => window.__calls.filter((x) => x.cmd === c), cmd);

// orders gets a LONGTEXT and a JSON column next to its TEXT note; row 1 has a cut (long) value in the LONGTEXT column
await page.evaluate(() => {
  window.__meta.orders.columns.push(["body", "longtext", 0, 1], ["doc", "json", 0, 1]);
  window.__data.orders[0].push("first 100 chars…", '{"a":1,"b":[1,2]}');
  window.__data.orders[1].push(null, "not json {");
  window.__data.orders[2].push("short", null);
  window.__longCells = [[0, 4]];
  window.__fullText = "FULL TEXT\n".repeat(500);
});
await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
await page.click(".tree .db");
await page.dblclick(".row-item.leaf:has-text('orders')");
await page.waitForSelector("table.dg tbody tr");
const cell = (r, c) => page.locator(`table.dg tbody tr:nth-child(${r}) td:nth-child(${c + 2})`);
const modal = page.locator(".cell-modal");
const applyBtn = modal.locator("button.primary");

// a varchar-like cell keeps the one-line editor; a TEXT cell opens the window
await cell(1, 2).dblclick();   // total (decimal)
await page.waitForSelector(".cell-edit input");
assert.equal(await modal.count(), 0, "short types edit in the cell");
await page.press(".cell-edit input", "Escape");
await cell(1, 3).dblclick();   // note (text)
await modal.waitFor();
assert.equal(await page.locator(".cell-edit input").count(), 0);
assert.match(await modal.innerText(), /note\s*text/);
assert.equal(await modal.locator("textarea").inputValue(), "first");
ok("a decimal cell edits in place and a TEXT cell opens the editor window");

// Apply stages the value (nothing is saved until Submit); Escape cancels
await modal.locator("textarea").fill("line 1\nline 2");
assert.match(await modal.innerText(), /13 characters/);
await applyBtn.click();
assert.equal(await modal.count(), 0);
assert.equal((await cell(1, 3).getAttribute("class")).includes("edited"), true);
assert.equal((await calls("apply_changes")).length, 0, "staged only");
await cell(2, 3).dblclick(); await modal.waitFor(); await modal.locator("textarea").fill("zzz"); await page.keyboard.press("Escape");
assert.equal(await modal.count(), 0);
assert.equal((await cell(2, 3).getAttribute("class")).includes("edited"), false, "Escape discards");
ok("Apply stages a multi-line value; Escape throws the edit away");

// LONGTEXT: the cut value is fetched in full before the window opens
await cell(1, 4).dblclick(); await modal.waitFor();
assert.equal(await modal.locator("textarea").inputValue(), window_text());
function window_text() { return "FULL TEXT\n".repeat(500); }
const fetched = await page.evaluate(() => window.__cellFetches);
assert.equal(fetched.length, 1);
assert.equal(fetched[0].column, "body");
assert.deepEqual(fetched[0].key, [{ column: "id", value: { kind: "text", value: "1" } }]);
assert.match(await modal.innerText(), /5,000 characters/);
await page.keyboard.press("Escape");
ok("a long value is fetched in full when its window opens");

// JSON: validity badge, Format, Minify, and an invalid document cannot be applied
await cell(1, 5).dblclick(); await modal.waitFor();
assert.match(await modal.locator(".json-badge").innerText(), /Valid JSON/);
await modal.locator("button:has-text('Format')").click();
assert.equal(await modal.locator("textarea").inputValue(), JSON.stringify({ a: 1, b: [1, 2] }, null, 2));
await modal.locator("button:has-text('Minify')").click();
assert.equal(await modal.locator("textarea").inputValue(), '{"a":1,"b":[1,2]}');
await page.keyboard.press("Escape");
await cell(2, 5).dblclick(); await modal.waitFor();
assert.match(await modal.locator(".json-badge").innerText(), /Invalid JSON/);
await modal.locator("textarea").fill('{"x": 1');
assert.equal(await applyBtn.isDisabled(), true, "an invalid JSON column value is not applied");
await modal.locator("textarea").fill('{"x": 1}');
assert.equal(await applyBtn.isDisabled(), false);
await page.keyboard.press("Meta+Enter");
assert.equal(await modal.count(), 0, "⌘↵ applies");
const staged = page.locator("button.primary:has-text('Submit')");
assert.match(await staged.innerText(), /Submit \(2\)/);
await staged.click();
await page.waitForTimeout(450);
const changes = (await calls("apply_changes")).at(-1).args.changes;
assert.deepEqual(changes.map((c) => c.set.map((s) => [s.column, s.value.value])), [[["note", "line 1\nline 2"]], [["doc", '{"x": 1}']]]);
ok("JSON gets a validity badge, Format and Minify; an invalid document is blocked; Submit sends both staged values");

// Set NULL from the window
await cell(3, 4).dblclick(); await modal.waitFor();
await modal.locator("button:has-text('Set NULL')").click();
assert.equal(await modal.locator("textarea").getAttribute("placeholder"), "NULL");
await applyBtn.click();
await page.click("button.primary:has-text('Submit')");
await page.waitForTimeout(450);
assert.deepEqual((await calls("apply_changes")).at(-1).args.changes[0].set, [{ column: "body", value: { kind: "null" } }]);
ok("Set NULL in the window stages a NULL");

// the context menu says "Edit cell" and opens the same window
await cell(1, 3).click({ button: "right" });
await page.click(".ctx-label:text-is('Edit cell')");
await modal.waitFor();
await page.keyboard.press("Escape");
ok("Edit cell in the context menu opens the window too");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL CELL EDITOR UI CHECKS PASSED");
