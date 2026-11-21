// Schema compare page: pick a source, compare, choose tables (drops are never pre-selected), preview and apply through Preview SQL.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1400, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
await page.addInitScript({ content: fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") + "\nSIZES.orders = 900 * 1024 * 1024; // orders is the biggest table here" });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
await page.selectOption(".conn-bar select", "1");
await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
await page.click(".tree .db:has-text('shop')", { button: "right" });
await page.getByText("Compare schemas…").click();
await page.waitForSelector("h2:has-text('Compare schemas')");
const compareBtn = page.locator("button:text-is('Compare')");
assert.equal(await compareBtn.isDisabled(), true, "a source database is required");
// source database: the page lists the databases of the source connection
await page.locator("label:has-text('Source database') .ss-trigger").click();
await page.click(".ss-opt:has-text('blog')");
await compareBtn.click();
await page.waitForSelector("table[aria-label=Differences]");
assert.match(await page.locator("p[aria-label=Summary]").innerText(), /3 differences, 2 tables identical/);
const rows = await page.locator("table[aria-label=Differences] tbody tr").allInnerTexts();
assert.equal(rows.length, 3);
assert.equal(await page.locator("input[aria-label='Include old_log']").isChecked(), false, "a drop is never pre-selected");
assert.equal(await page.locator("input[aria-label='Include posts']").isChecked(), true);
ok("the comparison lists the differences; the destructive drop is unchecked by default");

await page.click("tr:has-text('posts') button:has-text('Show SQL')");
assert.match(await page.locator("pre.preview").innerText(), /CREATE TABLE `shop`\.`posts`[\s\S]*ADD CONSTRAINT `fk_user`/);
ok("Show SQL reveals a table's statements, the foreign key last");

await page.click("button:has-text('Preview and apply')");
await page.waitForSelector("text=Compare schemas: shop");
const dlg = await page.locator(".dialog").innerText();
assert.ok(dlg.includes("CREATE TABLE `shop`.`posts`") && dlg.includes("ADD COLUMN `email`"), dlg);
assert.ok(!dlg.includes("DROP TABLE"), "the unchecked drop is not in the script");
assert.ok(dlg.indexOf("ADD CONSTRAINT") > dlg.indexOf("ADD COLUMN `email`"), "foreign keys of new tables come after the alters");
ok("Preview SQL shows exactly the chosen statements, in a working order");

await page.click(".dialog button:has-text('Apply changes')").catch(async () => { await page.click(".dialog .primary"); });
await page.waitForFunction(() => window.__calls.some((c) => c.cmd === "run_plan"));
const ran = await page.evaluate(() => window.__calls.filter((c) => c.cmd === "run_plan").at(-1).args);
assert.equal(ran.id, 1);
assert.ok(ran.statements.every((s) => s.endsWith(";")) && !ran.statements.join("\n").includes("DROP TABLE"));
ok("applying runs those statements on the target connection");
assert.deepEqual(errors, []);
await browser.close();
console.log("ALL COMPARE UI CHECKS PASSED");
