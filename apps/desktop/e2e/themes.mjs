// Colour themes (Monokai, GitHub Dark, …) in Preferences, and the screenshots in the in-app manual.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1400, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
page.on("response", (r) => r.status() >= 400 && errors.push(`${r.status()}: ${r.url()}`));
await page.addInitScript({ content: "window.__tour = false;\n" + fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
const html = (a) => page.evaluate((x) => document.documentElement.dataset[x] ?? null, a);
const bg = () => page.evaluate(() => getComputedStyle(document.body).backgroundColor);

// the Theme page lists the colour themes as cards; picking one applies at once and is remembered
await page.evaluate(() => window.__emit("menu-action", "preferences"));
await page.waitForSelector(".theme-grid");
assert.equal(await page.locator(".theme-card").count(), 9);
await page.click(".theme-card:has-text('Monokai')");
assert.deepEqual([await html("theme"), await html("palette"), await bg()], ["dark", "monokai", "rgb(39, 40, 34)"]);
await page.click(".theme-card:has-text('GitHub Light')");
assert.deepEqual([await html("theme"), await html("palette"), await bg()], ["light", "github-light", "rgb(255, 255, 255)"]);
await page.click(".theme-card:has-text('GitHub Dark')");
assert.deepEqual([await html("theme"), await html("palette"), await bg()], ["dark", "github-dark", "rgb(13, 17, 23)"]);
assert.equal(await page.evaluate(() => localStorage.getItem("mylodb.theme")), "github-dark");
ok("colour themes apply at once: Monokai and GitHub Dark are dark, GitHub Light is light");

// the plain Light / Dark / System choices clear the colour theme
await page.locator(".prefs-page .ss-trigger").click();
await page.click(".ss-opt:text-is('Light')");
assert.deepEqual([await html("theme"), await html("palette")], ["light", null]);
ok("choosing plain Light removes the colour theme");

// the choice survives a restart, and the command palette lists the themes
await page.click(".theme-card:has-text('Dracula')");
await page.keyboard.press("Escape");
await page.reload();
await page.waitForTimeout(500);
assert.deepEqual([await html("theme"), await html("palette")], ["dark", "dracula"]);
await page.keyboard.press("Meta+k");
await page.fill("input[aria-label=Command]", "tokyo");
await page.waitForFunction(() => (document.querySelector(".palette li")?.innerText ?? "").includes("Tokyo Night"));
await page.keyboard.press("Enter");
assert.equal(await html("palette"), "tokyo-night");
ok("the theme is remembered and ⌘K switches themes");

// the manual shows its screenshots
await page.evaluate(() => window.__emit("menu-action", "manual"));
await page.waitForSelector(".dialog.manual");
const sections = await page.locator(".manual nav button:not(.tour-link)").count();
let shots = 0;
for (let i = 0; i < sections; i++) {
  await page.locator(".manual nav button:not(.tour-link)").nth(i).click();
  const imgs = page.locator(".manual article img");
  const n = await imgs.count();
  for (let j = 0; j < n; j++) {
    await imgs.nth(j).scrollIntoViewIfNeeded();
    await page.waitForFunction((el) => el.complete && el.naturalWidth > 100, await imgs.nth(j).elementHandle(), { timeout: 8000 });
    shots++;
  }
}
assert.ok(shots >= 14, `${shots} screenshots in the manual`);
ok(`all ${shots} screenshots in the manual load`);

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL THEME AND MANUAL UI CHECKS PASSED");
