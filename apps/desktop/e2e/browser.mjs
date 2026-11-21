// UI test of the object browser's three levels: connection root -> database -> table (columns with types), with sizes.
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
const filter = page.locator(".filter input");
const placeholder = () => filter.getAttribute("placeholder");
const text = (loc) => loc.allInnerTexts().then((t) => t.map((x) => x.replace(/\s+/g, " ").trim()));
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);

await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");

// ---- the connection is the root of the tree
const root = page.locator(".row-item.root");
assert.match((await text(root))[0], /root@127\.0\.0\.1/);
assert.equal(await root.locator(".badge").innerText(), "1", "database count");
assert.equal(await placeholder(), "Filter databases");
ok("the connection (user@host) is the root node, with the number of databases");

// ---- database: filter tables, sizes
await page.click(".tree .db:has-text('shop')");
assert.equal(await placeholder(), "Filter tables in shop");
await page.click(".row-item.group:has-text('Tables')");
await page.waitForSelector(".row-item.leaf .size");
assert.deepEqual(await g("__statCalls"), ["shop"], "one stats query for the database");
const leaves = await text(page.locator(".row-item.leaf"));
assert.ok(leaves.some((t) => /^▸? ?customers 5\.20 MB$/.test(t.replace(/^[▾▸] ?/, "▸ ").replace(/^▸ /, "")) || /customers.*5\.20 MB/.test(t)), JSON.stringify(leaves));
assert.ok(leaves.some((t) => /orders.*112 KB/.test(t)), JSON.stringify(leaves));
ok("table sizes appear in KB and MB next to each table (one query per database)");

// ---- table: columns with types
await page.click(".row-item.leaf:has-text('orders')");
assert.equal(await placeholder(), "Filter columns in orders");
await page.waitForSelector(".row-item.col:not(.idx):not(.idx-head) .ctype");
assert.deepEqual(await g("__detailCalls"), ["shop.orders"]);
let cols = await text(page.locator(".row-item.col:not(.idx):not(.idx-head)"));
assert.deepEqual(cols, ["🔑 id bigint unsigned", "customer_id int", "total decimal(10,2)", "note text"]);
ok("selecting a table lists its columns with type and key, and the filter now says 'Filter columns in orders'");

// ---- column filter: by name or by type, entirely in the browser
const before = (await g("__filterCalls"))?.length ?? 0;
await filter.fill("total");
assert.deepEqual(await text(page.locator(".row-item.col:not(.idx):not(.idx-head)")), ["total decimal(10,2)"]);
await filter.fill("bigint");
assert.deepEqual(await text(page.locator(".row-item.col:not(.idx):not(.idx-head)")), ["🔑 id bigint unsigned"], "matches the type");
await filter.fill("zzz");
assert.equal(await page.locator(".row-item.col:not(.idx):not(.idx-head)").count(), 0);
assert.deepEqual(await text(page.locator(".columns .empty")), ["No matching columns"]);
await filter.fill("");
assert.equal(await page.locator(".row-item.col:not(.idx):not(.idx-head) .ctype").count(), 4);
await page.waitForTimeout(300);
assert.equal((await g("__filterCalls"))?.length ?? 0, before, "column filtering never calls the backend");
ok("the column filter searches names and types without asking the server");

// ---- another table: new scope, empty box, details fetched once per table
await filter.fill("id");
await page.click(".row-item.leaf:has-text('customers')");
assert.equal(await placeholder(), "Filter columns in customers");
assert.equal(await filter.inputValue(), "", "the box starts empty at every level");
await page.waitForFunction(() => window.__detailCalls?.includes("shop.customers"));
assert.deepEqual(await g("__detailCalls"), ["shop.orders", "shop.customers"]);
assert.equal(await page.locator("li:has(> .row-item.leaf:has-text('orders')) .col:not(.idx):not(.idx-head)").count(), 4, "the previous table's columns are not filtered by the new box");
await page.click(".row-item.leaf:has-text('orders')");   // selecting orders again must not refetch
assert.deepEqual(await g("__detailCalls"), ["shop.orders", "shop.customers"], "cached");
ok("each table fetches its column details once; switching tables resets the filter box");

// ---- clicking the root resets everything
await filter.fill("x");
await root.click();
assert.equal(await placeholder(), "Filter databases");
assert.equal(await filter.inputValue(), "");
assert.equal(await page.locator(".row-item.group").count(), 0, "everything is collapsed");
assert.equal(await page.locator(".row-item.col:not(.idx):not(.idx-head)").count(), 0);
assert.equal(await page.locator(".row-item.db.selected, .row-item.db.current").count(), 0, "no database is selected");
assert.equal(await page.locator(".row-item.root.selected").count(), 1);
ok("clicking root@host resets the browser to the full database list");

// ---- after the reset the cycle starts again
await filter.fill("sho");
await page.waitForFunction(() => window.__filterCalls?.some((x) => x.query === "sho" && x.scope === null));
await page.click(".tree .db:has-text('shop')");
assert.equal(await filter.inputValue(), "", "choosing a database clears the database filter");
assert.equal(await placeholder(), "Filter tables in shop");
ok("filter databases first, then choose one to filter its tables");

await browser.close();
assert.deepEqual(errors, [], "browser errors: " + errors.join(" | "));
console.log("ALL BROWSER UI CHECKS PASSED");
