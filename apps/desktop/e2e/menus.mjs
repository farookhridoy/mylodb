// UI test of the filter options (what a search looks at, arrows that close filter-opened nodes) and the table context menu.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await browser.newPage({ viewport: { width: 1400, height: 900 } });
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
page.on("console", (m) => m.type() === "error" && !m.text().includes("404") && errors.push("console: " + m.text()));
await page.addInitScript({ content: fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
const g = (n) => page.evaluate((k) => window[k], n);
const text = (loc) => loc.allInnerTexts().then((t) => t.map((x) => x.replace(/\s+/g, " ").trim()));
const filter = page.locator(".filter input");
const editorText = () => page.evaluate(() => document.querySelector(".cm-content")?.innerText ?? "");
const leaf = (name) => page.locator(".row-item.leaf", { has: page.locator(`.name:has-text('${name}')`) });
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
const search = async (q) => {
  const before = (await g("__filterCalls"))?.length ?? 0;
  await filter.fill(q);
  await page.waitForFunction((b) => (window.__filterCalls?.length ?? 0) > b, before);
  await page.waitForTimeout(250);
};

await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
await page.click(".tree .db:has-text('shop')");

// ---- by default a search lists tables (and other objects), not columns
await search("cust");
assert.equal(await page.locator(".row-item.col:not(.idx):not(.idx-head)").count(), 0, "no column rows");
assert.equal(await page.locator(".row-item.leaf:has-text('customers')").count(), 1);
assert.equal(await page.locator(".row-item.leaf:has-text('orders')").count(), 0, "orders only matches through a column, which is off");
const kinds = (await g("__filterKinds")).at(-1);
assert.ok(kinds.includes("table") && !kinds.includes("column"), JSON.stringify(kinds));
assert.equal(await page.locator(".filter-opts.on").count(), 0, "default options: the icon is not highlighted");
ok("a search lists matching tables only; columns are not searched unless switched on");

// ---- the filter icon opens the options
await page.click(".filter-opts");
const rows = await text(page.locator(".filter-pop .pop-row"));
assert.deepEqual(rows, ["Tables", "Views", "Procedures and functions", "Triggers", "Events", "Columns"]);
const checked = await page.locator(".filter-pop input").evaluateAll((els) => els.map((e) => e.checked));
assert.deepEqual(checked, [true, true, true, true, true, false]);
await page.locator(".filter-pop .pop-row:has-text('Columns') input").check();
await page.waitForFunction(() => window.__filterKinds?.at(-1)?.includes("column"));
await page.waitForTimeout(300);
assert.equal(await page.locator(".filter-opts.on").count(), 1, "a non-default choice is shown on the icon");
assert.equal(await page.locator(".row-item.leaf:has-text('orders')").count(), 1, "orders now appears through its customer_id column");
assert.ok((await g("__loadCalls")).includes("shop"), "columns were loaded when switched on");
ok("the filter icon opens the options; switching Columns on searches them too");

// ---- the reported bug: a table opened by the search can be closed with its arrow
await page.keyboard.press("Escape");
const orders = leaf("orders");
assert.equal(await page.locator(".row-item.col:not(.idx):not(.idx-head)").count() > 0, true, "the matching column is shown under orders");
await orders.locator(".chev").click();
assert.equal(await page.locator(".row-item.col:not(.idx):not(.idx-head)").count(), 0, "the arrow closes it");
await orders.locator(".chev").click();
assert.equal(await page.locator(".row-item.col:not(.idx):not(.idx-head)").count() > 0, true, "and opens it again");
await page.click(".row-item.group:has-text('Tables') .chev");
assert.equal(await page.locator(".row-item.leaf").count(), 0, "the Tables group closes too while searching");
await search("cust_");
await search("cust");
assert.equal(await page.locator(".row-item.leaf:has-text('customers')").count(), 1, "a new search shows its results again");
ok("arrows close tables and groups that a search opened");

// ---- at least one kind stays on; the choice is remembered; reset
await page.click(".filter-opts");
for (const label of ["Tables", "Views", "Procedures and functions", "Triggers", "Events"]) await page.locator(`.filter-pop .pop-row:has-text('${label}') input`).uncheck();
await page.locator(".filter-pop .pop-row:has-text('Columns') input").click();   // refused
assert.equal(await page.locator(".filter-pop .pop-row:has-text('Columns') input").isChecked(), true, "the last kind cannot be switched off");
await page.locator(".filter-pop .pop-row:has-text('Tables') input").check();
await page.reload();
await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
await page.click(".filter-opts");
assert.deepEqual(await page.locator(".filter-pop input").evaluateAll((els) => els.map((e) => e.checked)), [true, false, false, false, false, true], "remembered");
await page.click(".filter-pop button:has-text('Reset to defaults')");
assert.deepEqual(await page.locator(".filter-pop input").evaluateAll((els) => els.map((e) => e.checked)), [true, true, true, true, true, false]);
await page.keyboard.press("Escape");
ok("at least one kind stays on, the choice survives a restart, and it can be reset");

// ---- the table menu: DBeaver-style sections with icons and shortcuts
await page.click(".tree .db:has-text('shop')");
await page.click(".row-item.leaf:has-text('orders')", { button: "right" });
const top = await text(page.locator(".ctx:not(.ctx-nested) > .ctx-item > .ctx-label"));
assert.deepEqual(top, ["Create", "View Table", "Design table…", "Lock the filter to this table", "View Diagram", "View Data", "Export Data…", "Import Data…", "Tools", "Generate SQL", "Read data in SQL console", "Copy name", "Copy Advanced Info", "Duplicate…", "Rename…", "Drop…", "Refresh"]);
assert.ok(await page.locator(".ctx:not(.ctx-nested) > .ctx-item .ctx-icon svg").count() >= 15, "every entry has an icon");
assert.deepEqual(await text(page.locator(".ctx:not(.ctx-nested) > .ctx-item > .ctx-key")), ["F4", "F2", "F5"]);
assert.ok(await page.locator(".ctx:not(.ctx-nested) > .ctx-sep").count() >= 5, "grouped by separators");
assert.equal(await page.locator(".ctx-item.danger").count(), 2, "Drop and Truncate are drawn as dangerous (Truncate sits in Tools)");
const sub = async (name) => { await page.hover(`.ctx:not(.ctx-nested) > .ctx-item.sub:has-text('${name}')`); return text(page.locator(`.ctx:not(.ctx-nested) > .ctx-item.sub:has-text('${name}') > .ctx-nested .ctx-label`)); };
assert.deepEqual(await sub("Create"), ["New table…", "New view…", "New procedure…", "New function…", "New trigger…", "New event…"]);
assert.deepEqual(await sub("Tools"), ["Dump table…", "Analyze", "Check", "Optimize", "Repair", "Truncate…", "Databases", "Server tools"]);
assert.deepEqual(await sub("Generate SQL"), ["SELECT", "INSERT", "UPDATE", "DELETE", "UPSERT", "DDL"]);
await page.keyboard.press("Escape");
ok("the table menu has Create, View Table / Diagram / Data, Export / Import, Tools and Generate SQL submenus, icons, shortcuts and separators");

// ---- actions
const tab = () => page.locator(".subtabs button.active").innerText().then((t) => t.trim());
await page.click(".row-item.leaf:has-text('orders')", { button: "right" });
await page.click(".ctx:not(.ctx-nested) > .ctx-item .ctx-label:text-is('View Table')");
await page.waitForSelector(".props-head");
assert.equal(await tab(), "Properties");
await page.click(".row-item.leaf:has-text('orders')", { button: "right" });
await page.click(".ctx:not(.ctx-nested) > .ctx-item .ctx-label:text-is('View Diagram')");
await page.waitForSelector(".dgm-box");
assert.equal(await tab(), "Diagram");
await page.click(".row-item.leaf:has-text('orders')", { button: "right" });
await page.click(".ctx:not(.ctx-nested) > .ctx-item .ctx-label:text-is('View Data')");
await page.waitForSelector("table.dg th");
assert.equal(await tab(), "Data");
ok("View Table, View Diagram and View Data open the matching view of the table's tab");

await page.click(".row-item.leaf:has-text('orders')", { button: "right" });
await page.hover(".ctx:not(.ctx-nested) > .ctx-item.sub:has-text('Tools')");
await page.click(".ctx-nested .ctx-label:text-is('Analyze')");
await page.waitForFunction(() => window.__maint?.some((m) => m.operation.op === "analyze" && m.tables[0] === "orders"));
await page.waitForFunction(() => /ANALYZE shop\.orders: OK/.test(document.querySelector(".statusbar")?.innerText ?? ""));
ok("Tools → Analyze runs on the table and reports in the status bar");

await page.click(".row-item.leaf:has-text('orders')", { button: "right" });
await page.hover(".ctx:not(.ctx-nested) > .ctx-item.sub:has-text('Generate SQL')");
await page.click(".ctx-nested .ctx-label:text-is('UPSERT')");
assert.match(await editorText(), /INSERT INTO `orders`[\s\S]*ON DUPLICATE KEY UPDATE[\s\S]*`total` = VALUES\(`total`\)/);
await page.click(".row-item.leaf:has-text('orders')", { button: "right" });
await page.hover(".ctx:not(.ctx-nested) > .ctx-item.sub:has-text('Generate SQL')");
await page.click(".ctx-nested .ctx-label:text-is('DDL')");
await page.waitForFunction(() => /CREATE TABLE `orders`/.test(document.querySelector(".cm-content")?.innerText ?? ""));
ok("Generate SQL adds UPSERT and the table's DDL");

await page.click(".row-item.leaf:has-text('orders')", { button: "right" });
await page.click(".ctx:not(.ctx-nested) > .ctx-item .ctx-label:text-is('Export Data…')");
await page.waitForSelector(".transfer");
assert.match(await page.locator(".transfer .dz-tabs button.active").innerText(), /Export/);
await page.click(".qtab:has-text('orders')");
ok("Export Data opens the export tool for the table");

// lock from the menu
await page.click(".row-item.leaf:has-text('orders')", { button: "right" });
await page.click(".ctx:not(.ctx-nested) > .ctx-item .ctx-label:text-is('Lock the filter to this table')");
assert.equal(await filter.getAttribute("placeholder"), "Filter columns");
assert.ok(await page.locator(".row-item.col:not(.idx):not(.idx-head)").count() >= 4, "the locked table is open");
assert.equal(await page.locator(".row-item.group:has-text('Views')").count(), 0);
await page.click(".scope .x");
ok("the menu can lock the filter to a table (and the table is opened)");

// ---- keyboard: F4 properties, F2 rename
await page.click(".tree .db:has-text('shop')");
await page.click(".row-item.leaf:has-text('customers')");
await page.keyboard.press("F4");
await page.waitForSelector(".props-head");
assert.equal(await page.locator(".subtabs .crumb").innerText(), "shop › customers");
await page.click(".row-item.leaf:has-text('customers')");
await page.keyboard.press("F2");
await page.waitForSelector(".dialog h3:has-text('Rename customers')");
await page.keyboard.press("Escape");
await page.click(".dialog button:has-text('Cancel')").catch(() => {});
ok("F4 opens the selected table's properties and F2 starts a rename");

await browser.close();
assert.deepEqual(errors, [], "browser errors: " + errors.join(" | "));
console.log("ALL MENU / FILTER OPTION UI CHECKS PASSED");
