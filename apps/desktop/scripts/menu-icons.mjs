// Renders the menu-bar icons (PNG, 32 px for 16 px @2x) from the same line-icon set the UI uses (src/Icon.tsx).
// Run: node scripts/menu-icons.mjs   (needs CHROME_PATH or a Playwright chromium). The PNGs are committed.
import { chromium } from "playwright-core";
import fs from "node:fs";

const src = fs.readFileSync(new URL("../src/Icon.tsx", import.meta.url), "utf8");
const block = src.slice(src.indexOf("const PATHS"), src.indexOf("export type IconName"));
const names = ["settings", "terminal", "folder", "save", "close", "refresh", "import", "export", "transfer", "play", "sql", "search", "preview", "info", "plus", "edit", "stop", "home", "database", "table", "user", "pulse", "sun", "moon", "auto", "compare", "tools", "copy", "diagram"];
const out = new URL("../src-tauri/icons/menu/", import.meta.url);
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await (await browser.newContext({ viewport: { width: 32, height: 32 }, deviceScaleFactor: 1 })).newPage();
for (const n of names) {
  const m = new RegExp(`\\n  ${n}: ([\\s\\S]*?),\\n  [a-z]+: |\\n  ${n}: ([\\s\\S]*?),\\n};`).exec(block);
  const body = (m?.[1] ?? m?.[2] ?? "").replace(/<\/?>/g, "").replace(/\.(\d)00/g, ".$1").replace(/(\d)\.0+(?=\D)/g, "$1");
  if (!body) { console.error("missing icon", n); continue; }
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 16 16" fill="none" stroke="#8a8f98" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">${body}</svg>`;
  await page.setContent(`<body style="margin:0;background:transparent">${svg}</body>`);
  await page.screenshot({ path: new URL(`${n}.png`, out).pathname, omitBackground: true });
}
await browser.close();
console.log("icons written to", out.pathname);
