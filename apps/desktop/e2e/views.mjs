// UI test of the table views (Properties, Data, Diagram) and of dialogs staying above the rest of the page.
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
const text = (loc) => loc.allInnerTexts().then((t) => t.map((x) => x.replace(/\s+/g, " ").trim()));
const editorText = () => page.evaluate(() => document.querySelector(".cm-content")?.innerText ?? "");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);

await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
await page.click(".tree .db:has-text('shop')");
await page.dblclick(".row-item.leaf:has-text('orders')");
await page.waitForSelector("table.dg tbody tr");

// ---- three views, Data first
assert.deepEqual(await text(page.locator(".subtabs button")), ["Properties", "Data", "Diagram"]);
assert.equal(await page.locator(".subtabs button.active").innerText().then((t) => t.trim()), "Data");
assert.equal(await page.locator(".editor-wrap").isVisible(), true);
ok("a table has Properties, Data and Diagram views; Data (console + grid) is the default");

// ---- Properties
await page.click(".subtabs button:has-text('Properties')");
await page.waitForSelector(".props-head");
assert.equal(await page.locator(".editor-wrap").isVisible(), false, "the console is hidden outside the Data view");
const v = (label) => page.locator(`.props-head label:has-text('${label}') input`).inputValue();
assert.equal(await v("Table name"), "orders");
assert.equal(await v("Engine"), "InnoDB");
assert.equal(await v("Collation"), "utf8mb4_unicode_ci");
assert.equal(await v("Charset"), "utf8mb4");
assert.equal(await v("Auto increment"), "4");
assert.equal(await v("Description"), "customer orders");
assert.deepEqual(await text(page.locator(".props-nav button")), ["Columns 4", "Constraints 2", "Foreign Keys 1", "References 0", "Triggers 1", "Indexes 2", "Partitions 0", "Statistics", "DDL"]);
const rows = await text(page.locator("table.info tbody tr"));
assert.match(rows[0], /^id 1 bigint unsigned .*PRI auto_increment/);
assert.match(rows[2], /^total 3 decimal\(10,2\)/);
ok("Properties shows the settings and the columns with type, key and extras, and counts for every section");

await page.click(".props-nav button:has-text('Foreign Keys')");
assert.match((await text(page.locator("table.info tbody tr")))[0], /fk1 customer_id customers id CASCADE RESTRICT/);
await page.click(".props-nav button:has-text('Indexes')");
assert.deepEqual((await text(page.locator("table.info tbody tr"))).map((r) => r.replace(/\s+/g, " ")), ["PRIMARY BTREE id", "idx_customer_total BTREE customer_id, total"].map((x) => x));
await page.click(".props-nav button:has-text('Triggers')");
assert.match((await text(page.locator("table.info tbody tr")))[0], /trg_orders_bi BEFORE INSERT/);
await page.click(".props-nav button:has-text('Statistics')");
assert.ok((await text(page.locator("table.info.kv tr"))).some((r) => /^Data size 16 KB/.test(r)));
ok("foreign keys (with rules), indexes, triggers and statistics are listed");

// a foreign key's table opens from the list
await page.click(".props-nav button:has-text('Foreign Keys')");
await page.click("table.info button.link:has-text('customers')");
await page.waitForSelector(".qtab:has-text('customers')");
assert.equal(await page.locator(".subtabs .crumb").innerText(), "shop › customers");
await page.click(".subtabs button:has-text('Properties')");
await page.click(".props-nav button:has-text('References')");
assert.match((await text(page.locator("table.info tbody tr")))[0], /fk1 orders customer_id id CASCADE RESTRICT/);
ok("References lists the tables that point at this one, and a table name opens it");

// DDL
await page.click(".qtab:has-text('orders')");
await page.click(".subtabs button:has-text('Properties')");
await page.click(".props-nav button:has-text('DDL')");
assert.match(await page.locator(".ddl pre").innerText(), /CREATE TABLE `orders`/);
await page.click(".ddl-bar button:has-text('Open in console')");
await page.waitForFunction(() => /CREATE TABLE `orders`/.test(document.querySelector(".cm-content")?.innerText ?? ""));
assert.equal(await page.locator(".subtabs button.active").innerText().then((t) => t.trim()), "Data");
ok("the DDL is shown and can be opened in the console");

// ---- Diagram: orders references customers
await page.click(".subtabs button:has-text('Diagram')");
await page.waitForSelector(".dgm-box");
assert.deepEqual(await text(page.locator(".dgm-head")), ["orders", "customers"]);
assert.equal(await page.locator(".dgm-box.center .dgm-head").innerText().then((t) => t.trim()), "orders");
assert.equal(await page.locator(".dgm-edge.main").count(), 1);
assert.match(await page.locator(".dgm-bar .dim").innerText(), /2 tables · 1 relation/);
await page.waitForFunction(() => document.querySelectorAll(".dgm-box").length === 2 && [...document.querySelectorAll(".dgm-box")].every((b) => b.querySelectorAll(".dgm-row").length > 1));
assert.equal(await page.locator(".dgm-box.center .dgm-row.linked .name").innerText(), "customer_id", "the foreign key column is marked");
const customers = page.locator(".dgm-box:not(.center)");
const rightOf = await Promise.all([page.locator(".dgm-box.center").boundingBox(), customers.boundingBox()]);
assert.ok(rightOf[1].x > rightOf[0].x, "the referenced table is to the right");
ok("the diagram shows the table, the table it references, and the relation between the two columns");

// drag, zoom, open
const before = await customers.boundingBox();
const h = await customers.locator(".dgm-head").boundingBox();
await page.mouse.move(h.x + 40, h.y + 10); await page.mouse.down(); await page.mouse.move(h.x + 80, h.y + 70); await page.mouse.move(h.x + 120, h.y + 130); await page.mouse.up();
const after = await customers.boundingBox();
assert.ok(after.x - before.x > 60 && after.y - before.y > 100, "boxes can be dragged");
await page.click(".dgm-bar button:has-text('Reset layout')");
assert.ok(Math.abs((await customers.boundingBox()).x - before.x) < 2);
await page.click("button[aria-label='Zoom in']");
assert.equal(await page.locator(".dgm-bar .zoom").innerText(), "110%");
await customers.locator(".dgm-head").dblclick();
await page.waitForSelector(".qtab.active:has-text('customers')");
ok("boxes can be dragged and reset, the diagram zooms, and a double-click opens a table");

// customers is referenced by orders: orders is on the left
await page.click(".subtabs button:has-text('Diagram')");
await page.waitForSelector(".dgm-box");
const pos = await Promise.all([page.locator(".dgm-box.center").boundingBox(), page.locator(".dgm-box:not(.center)").boundingBox()]);
assert.ok(pos[1].x < pos[0].x, "tables that reference this one are on the left");
ok("tables that reference the table are drawn on the left");

// each table remembers its own view
const active = () => page.locator(".subtabs button.active").innerText().then((t) => t.trim());
await page.click(".qtab:has-text('orders')");
assert.equal(await active(), "Diagram", "orders was left on its diagram");
await page.click(".subtabs button:has-text('Properties')");
await page.click(".qtab:has-text('customers')");
assert.equal(await active(), "Diagram");
await page.click(".qtab:has-text('orders')");
assert.equal(await active(), "Properties");
ok("each table tab remembers which view it was on");

// ---- dialogs stay above everything else (splitters, sticky grid headers, popups)
await page.click(".subtabs button:has-text('Data')");
await page.waitForSelector("table.dg th");
await page.click("button[title='New connection']");
await page.waitForSelector(".overlay .dialog");
for (const sel of [".splitter-x", ".splitter-y", "table.dg th", ".sidebar .filter", ".subtabs button.active"]) {
  const b = await page.locator(sel).first().boundingBox();
  const covered = await page.evaluate(([x, y]) => !!document.elementFromPoint(x, y)?.closest(".overlay"), [b.x + b.width / 2, b.y + b.height / 2]);
  assert.equal(covered, true, `${sel} must be under the dialog's overlay`);
}
ok("the new-connection dialog covers splitters, grid headers and every other part of the page");

await browser.close();
assert.deepEqual(errors, [], "browser errors: " + errors.join(" | "));
console.log("ALL TABLE VIEW UI CHECKS PASSED");
