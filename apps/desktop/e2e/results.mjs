// Result views: grid or mysql-style text, and Copy as TSV / CSV / JSON / Markdown / INSERT.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const ctx = await browser.newContext({ viewport: { width: 1400, height: 900 }, permissions: ["clipboard-read", "clipboard-write"] });
const page = await ctx.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
await page.addInitScript({ content: fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
const clip = () => page.evaluate(() => navigator.clipboard.readText());
const copyAs = async (label) => { await page.selectOption("select[title='Copy the result as…']", label); await page.waitForFunction(() => document.querySelector(".grid-tools")?.innerText.includes("Copied")); };

await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
await page.evaluate(() => {
  window.__runSets = [{ columns: [{ name: "id", type_name: "INT" }, { name: "name", type_name: "VARCHAR" }, { name: "note", type_name: "TEXT" }],
    rows: [["1", "Ann", "say \"hi\", ok"], ["2", "O'Neil", null], ["3", "Bo|b", "line1\nline2"]], total_rows: 3, truncated: false }];
});
await page.click(".cm-content"); await page.keyboard.type("SELECT * FROM t");
await page.click(".toolbar button.primary");
await page.waitForSelector("table.grid");

await page.click(".seg button:text-is('Text')");
const text = await page.locator("pre[aria-label='Text result']").innerText();
assert.equal(text.split("\n")[0], "+----+--------+--------------+");
assert.match(text, /\| id \| name   \| note         \|/);
assert.match(text, /\| 2  \| O'Neil \| NULL         \|/);
assert.match(text, /line1↵line2/);
ok("Text view draws the result like the mysql client, NULL and newlines visible");
await page.reload(); await page.waitForSelector(".conn-bar select");
await page.click(".seg button:text-is('Grid')").catch(() => {});

await page.selectOption(".conn-bar select", "1"); await page.waitForSelector(".tree .db");
await page.evaluate(() => { window.__runSets = [{ columns: [{ name: "id", type_name: "INT" }, { name: "name", type_name: "VARCHAR" }, { name: "note", type_name: "TEXT" }],
  rows: [["1", "Ann", "say \"hi\", ok"], ["2", "O'Neil", null], ["3", "Bo|b", "line1\nline2"]], total_rows: 3, truncated: false }]; });
await page.click(".cm-content"); await page.keyboard.type("SELECT * FROM t");
await page.click(".toolbar button.primary"); await page.waitForSelector(".grid-tools");

await copyAs("csv");
assert.equal(await clip(), 'id,name,note\n1,Ann,"say ""hi"", ok"\n2,O\'Neil,\n3,Bo|b,"line1\nline2"');
await copyAs("tsv");
assert.equal(await clip(), "id\tname\tnote\n1\tAnn\tsay \"hi\", ok\n2\tO'Neil\tNULL\n3\tBo|b\tline1 line2");
await copyAs("json");
const json = JSON.parse(await clip());
assert.deepEqual(json[1], { id: "2", name: "O'Neil", note: null });
await copyAs("markdown");
assert.match(await clip(), /\| 3 \| Bo\\\|b \| line1 line2 \|/);
await copyAs("insert");
const ins = (await clip()).split("\n");
assert.equal(ins[1], "INSERT INTO `table_name` (`id`, `name`, `note`) VALUES ('2', 'O''Neil', NULL);");
ok("Copy as CSV, TSV, JSON, Markdown and INSERT quote values correctly (NULL kept distinct)");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL RESULT VIEW UI CHECKS PASSED");
