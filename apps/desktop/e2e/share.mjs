// Sharing connections and snippets as a file: nothing secret is written, and importing skips what is already here.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1400, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
await page.addInitScript({ content: "localStorage.setItem('mylodb.tourDone','1'); localStorage.setItem('mylodb.snippets', JSON.stringify([{ name: 'Big orders', sql: 'SELECT * FROM orders WHERE total > 1000;' }]));\n" + fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
await page.waitForTimeout(600);

// export
await page.evaluate(() => { window.__nextSave = "/tmp/team.json"; window.__emit("menu-action", "export_share"); });
await page.waitForFunction(() => window.__written);
const written = await page.evaluate(() => window.__written);
assert.equal(written.path, "/tmp/team.json");
const file = JSON.parse(written.content);
assert.deepEqual([file.app, file.kind, file.version], ["MyLoDB", "share", 1]);
assert.equal(file.connections.length, 1);
assert.equal(file.connections[0].name, "Local");
assert.equal(file.connections[0].id, null, "ids are not shared");
assert.ok(!JSON.stringify(file).match(/password|passphrase/i), "no password of any kind is in the file");
assert.deepEqual(file.snippets, [{ name: "Big orders", sql: "SELECT * FROM orders WHERE total > 1000;" }]);
await page.waitForSelector(".toast:has-text('Passwords are not included')");
ok("export writes connections and snippets, and never a password");

// import: new things are added, what is already here is skipped, a name clash is renamed
const incoming = { app: "MyLoDB", kind: "share", version: 1,
  connections: [
    { id: null, name: "Local", color: "#4f8cff", host: "127.0.0.1", port: 3306, user: "root", database: null, ssl: "disabled" },
    { id: null, name: "Local", color: "#f00", host: "db.example.com", port: 3306, user: "app", database: "shop", ssl: "required" },
    { id: null, name: "Staging", color: "#e0a63a", host: "stg.example.com", port: 3306, user: "app", database: null, ssl: "required", environment: "staging" },
  ],
  snippets: [{ name: "Big orders", sql: "SELECT * FROM orders WHERE total > 1000;" }, { name: "Big orders", sql: "SELECT 2;" }, { name: "New one", sql: "SELECT 3;" }] };
await page.evaluate((t) => { window.__fileText = JSON.stringify(t); window.__nextOpen = "/tmp/team.json"; window.__emit("menu-action", "import_share"); }, incoming);
await page.waitForSelector(".toast:has-text('Imported 2 connection(s) and 2 snippet(s)')");
assert.match(await page.locator(".toast").last().innerText(), /2 already here were skipped|1 already here were skipped/);
const saved = await page.evaluate(() => window.__calls.filter((c) => c.cmd === "save_connection").map((c) => c.args.connection));
assert.deepEqual(saved.map((c) => c.name), ["Local (imported)", "Staging"], "the identical connection is skipped and the name clash is renamed");
assert.equal(saved.every((c) => c.id === null && !c.new_password), true);
const mine = JSON.parse(await page.evaluate(() => localStorage.getItem("mylodb.snippets")));
assert.deepEqual(mine.map((s) => s.name).sort(), ["Big orders", "Big orders (imported)", "New one"]);
ok("import adds new connections and snippets, skips duplicates and renames name clashes");

// a wrong file is explained
await page.evaluate(() => { window.__fileText = "{not json"; window.__nextOpen = "/tmp/bad.json"; window.__emit("menu-action", "import_share"); });
await page.waitForSelector(".toast.error:has-text('not a MyLoDB share file')");
ok("a file that is not a share file is refused with a plain message");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL SHARE FILE CHECKS PASSED");
