// UI test of the light / dark / system theme switch. Needs the dev server (`npm run dev`) and CHROME_PATH.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const mock = fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8");
const errors = [];
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);

async function open(colorScheme) {
  const ctx = await browser.newContext({ colorScheme, viewport: { width: 1300, height: 800 } });
  const page = await ctx.newPage();
  page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
  page.on("console", (m) => m.type() === "error" && !m.text().includes("404") && errors.push("console: " + m.text()));
  await page.addInitScript({ content: mock });
  await page.goto(process.env.APP_URL || "http://localhost:1420/");
  return { ctx, page };
}
const theme = (page) => page.evaluate(() => document.documentElement.dataset.theme);
const bg = (page) => page.evaluate(() => getComputedStyle(document.body).backgroundColor);
const pref = (page) => page.evaluate(() => localStorage.getItem("mylodb.theme"));

// "System" follows the operating system
let { ctx, page } = await open("light");
assert.equal(await theme(page), "light");
assert.equal(await bg(page), "rgb(255, 255, 255)");
await ctx.close();
({ ctx, page } = await open("dark"));
assert.equal(await theme(page), "dark");
assert.equal(await bg(page), "rgb(30, 31, 34)");
ok("with no choice made, the theme follows the system appearance");

// cycling: system -> light -> dark -> system
const toggle = page.locator(".theme-toggle");
assert.match(await toggle.getAttribute("title"), /Theme: system/);
await toggle.click();
assert.deepEqual([await theme(page), await pref(page), await bg(page)], ["light", "light", "rgb(255, 255, 255)"], "light overrides a dark system");
await toggle.click();
assert.deepEqual([await theme(page), await pref(page)], ["dark", "dark"]);
await toggle.click();
assert.deepEqual([await theme(page), await pref(page)], ["dark", "system"], "back to following the system (dark here)");
ok("the toggle cycles system → light → dark and applies it immediately");

// the choice is remembered and applied before first paint
await toggle.click();                                  // light
await page.reload();
assert.equal(await theme(page), "light");
assert.equal(await bg(page), "rgb(255, 255, 255)");
ok("the choice survives a reload");

// the SQL editor follows the theme without being recreated (text and undo history stay)
await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".cm-content");
await page.click(".cm-content");
await page.keyboard.type("SELECT 1");
const editorBg = () => page.evaluate(() => getComputedStyle(document.querySelector(".cm-editor")).backgroundColor);
assert.equal(await editorBg(), "rgb(255, 255, 255)");
await page.locator(".theme-toggle").click();            // dark
assert.equal(await theme(page), "dark");
assert.notEqual(await editorBg(), "rgb(255, 255, 255)", "the editor switched to dark colours");
assert.match(await page.locator(".cm-content").innerText(), /SELECT 1/, "typed text is still there");
await page.click(".cm-content");
await page.keyboard.press("Meta+z");
assert.doesNotMatch(await page.locator(".cm-content").innerText(), /SELECT 1/, "undo history survived the theme change");
ok("the editor changes theme live and keeps its text and undo history");

// readable in light: text contrasts with the background on key surfaces
await page.locator(".theme-toggle").click();            // system (dark in this context)
await page.locator(".theme-toggle").click();            // light
const contrast = await page.evaluate(() => {
  const lum = (rgb) => { const [r, g, b] = rgb.match(/\d+/g).map(Number).map((v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }); return 0.2126 * r + 0.7152 * g + 0.0722 * b; };
  const ratio = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
  const pair = (sel) => { const el = document.querySelector(sel); const cs = getComputedStyle(el); return ratio(cs.color, cs.backgroundColor === "rgba(0, 0, 0, 0)" ? getComputedStyle(document.body).backgroundColor : cs.backgroundColor); };
  return { body: pair("body"), tabs: pair(".qtab.active"), dim: pair(".statusbar") };
});
assert.ok(contrast.body >= 7 && contrast.tabs >= 7 && contrast.dim >= 3.5, JSON.stringify(contrast));
ok(`light theme text is readable (contrast ${contrast.body.toFixed(1)}:1 body, ${contrast.dim.toFixed(1)}:1 status bar)`);
await ctx.close();

await browser.close();
assert.deepEqual(errors, [], "browser errors: " + errors.join(" | "));
console.log("ALL THEME UI CHECKS PASSED");
