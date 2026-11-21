// Takes the screenshots used by README.md and docs/MANUAL.md.
// They show the real UI running against the demo data of e2e/mock.js (no real server, no real names).
// Needs `npm run dev` running and CHROME_PATH set:  node scripts/screenshots.mjs
import { chromium } from "playwright-core";
import fs from "node:fs";

const out = new URL("../../../docs/images/", import.meta.url).pathname;
fs.mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const ctx = await browser.newContext({ viewport: { width: 1320, height: 820 }, deviceScaleFactor: 2 });
const page = await ctx.newPage();
const mock = fs.readFileSync(new URL("../e2e/mock.js", import.meta.url), "utf8");
await page.addInitScript({ content: "window.__tour = true;\n" + mock });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
const shot = async (name, sel) => { await page.waitForTimeout(350); await (sel ? page.locator(sel).first() : page).screenshot({ path: `${out}${name}.png` }); console.log("saved", name); };
const key = (k) => page.keyboard.press(k);
const palette = async (text) => { await key("Meta+k"); await page.fill("input[aria-label=Command]", text); await key("Enter"); };

// 1. the first-run tour on a fresh start
await page.waitForSelector(".tour-card");
await shot("tour");
await page.click(".tour-card >> text=Skip");

// 2. connect and browse
await page.click("button[title='New connection']");
await page.waitForSelector(".dialog");
await shot("connection-dialog");
await page.click(".dialog >> text=Safety").catch(() => {});
await shot("connection-safety");
await page.click(".dialog .actions button:has-text('Cancel')");
await page.waitForSelector(".dialog", { state: "detached" });
await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".tree .db");
await page.click(".tree .db:has-text('shop')");
await page.waitForSelector(".row-item.leaf");
await page.click(".row-item.leaf:has-text('customers')");
await page.click("button:has-text('Info')");
await page.waitForSelector(".info-pane");
await shot("browser-info");

// 3. a table: console + grid
await page.dblclick(".row-item.leaf:has-text('customers')");
await page.waitForSelector("table.dg, .grid");
await shot("table-data");

// 4. a query with results
await page.evaluate(() => {
  window.__runSets = [{ columns: [{ name: "id", type_name: "BIGINT" }, { name: "name", type_name: "VARCHAR" }, { name: "email", type_name: "VARCHAR" }],
    rows: [["3", "Carla Gomez", "carla@example.com"], ["2", "Ben Okafor", "ben@example.com"], ["1", "Ana Petrova", "ana@example.com"]], total_rows: 3, truncated: false }];
});
await palette("new query tab");
await page.waitForTimeout(300);
await page.click(".cm-content");
await page.keyboard.type("SELECT id, name, email\nFROM customers\nORDER BY id DESC\nLIMIT 100;");
await key("ControlOrMeta+Enter");
await page.waitForTimeout(800);
await shot("query");

// 5. admin pages
await page.click(".tree .db:has-text('shop')", { button: "right" });
await page.click(".ctx-label:has-text('Databases (list')");
await page.waitForSelector("table[aria-label=Databases]");
await shot("databases");
await page.click(".tree .db:has-text('shop')", { button: "right" });
await page.click(".ctx-label:has-text('Structure')");
await page.waitForTimeout(600);
await shot("structure");

// 6. command palette, preferences, manual, about
await key("Meta+k");
await page.fill("input[aria-label=Command]", "snippet");
await page.waitForFunction((q) => (document.querySelector(".palette li")?.innerText ?? "").toLowerCase().includes(q.split(" ")[0].toLowerCase()), "snippet");
await shot("command-palette");
await key("Escape");
await palette("preferences");
await page.waitForSelector("[role=dialog]");
await shot("preferences");
await page.click("[role=dialog] .actions button:has-text('Cancel')");
await page.waitForSelector("[role=dialog]", { state: "detached" });
await palette("user manual");
await page.waitForSelector("[aria-label='User manual']");
await shot("manual");
await page.click("[aria-label='Close manual']");
await palette("about mylodb");
await page.waitForSelector("[aria-label='About MyLoDB']");
await shot("about");
await page.click("[aria-label='About MyLoDB'] .primary");

// 6b. SQLite in the connection dialog
await page.click("button[title='New connection']");
await page.waitForSelector(".conn-dialog");
await page.click(".conn-dialog .ss-trigger");
await page.click(".ss-opt:has-text('SQLite file')");
await page.fill(".conn-dialog input >> nth=0", "Shop file");
await page.fill("input[placeholder='/path/to/data.sqlite']", "/Users/you/data/shop.sqlite");
await shot("sqlite-dialog");
await page.click(".conn-dialog .actions button:has-text('Cancel')");

// 6c. the visual tools: ER diagram and query builder
await page.click(".tree .db:has-text('shop')", { button: "right" });
await page.click(".ctx-label:has-text('ER diagram')");
await page.waitForSelector(".dgm-box[data-table=customers]");
await shot("er-diagram");
await page.click(".tree .db:has-text('shop')", { button: "right" });
await page.click(".ctx-label:has-text('Query builder')");
await page.waitForSelector("h2:has-text('Query builder')");
await page.locator("label:has-text('From') .ss-trigger").click();
await page.click(".ss-opt:has-text('customers')");
await page.locator("label:has-text('Join another table') .ss-trigger").click();
await page.click(".ss-opt:has-text('orders')");
await page.click("button:has-text('+ Column')");
await page.selectOption("select[aria-label='Column 1 table']", "c");
await page.selectOption("select[aria-label='Column 1']", "name");
await page.click("button:has-text('+ Condition')");
await page.selectOption("select[aria-label='Condition 1 column']", "name");
await page.selectOption("select[aria-label='Condition 1 operator']", "contains");
await page.fill("input[aria-label='Condition 1 value']", "Ann");
await shot("query-builder");

// 7. dark theme
await palette("theme: dark");
await page.waitForTimeout(400);
await shot("dark-theme");
await browser.close();
