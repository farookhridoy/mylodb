// Checks the website before it is published: every local link, image and script in site/index.html exists, every image has alt text.
import fs from "node:fs";
import path from "node:path";
const root = new URL("../site/", import.meta.url).pathname;
const html = fs.readFileSync(path.join(root, "index.html"), "utf8");
const problems = [];
for (const m of html.matchAll(/(?:src|href)="([^"#?]+)(?:[#?][^"]*)?"/g)) {
  const u = m[1];
  if (/^(https?:|mailto:|data:|\/\/)/.test(u) || u === "./") continue;
  if (u.startsWith("/")) problems.push(`absolute path ${u} (the site lives under /mylodb/, use a relative path)`);
  else if (!fs.existsSync(path.join(root, u))) problems.push(`missing file ${u}`);
}
for (const m of html.matchAll(/<img\b[^>]*>/g)) if (!/\balt="[^"]*"/.test(m[0])) problems.push(`image without alt text: ${m[0].slice(0, 80)}`);
for (const need of ["farookhridoy.com", "thelarasoft.com", "github.com/farookhridoy/mylodb"]) if (!html.includes(need)) problems.push(`missing link to ${need}`);
for (const f of ["styles.css", "app.js"]) if (!fs.existsSync(path.join(root, f))) problems.push(`missing ${f}`);
// the release notes on the page must start with the version in the app, so a release cannot go out without its entry
const appVersion = JSON.parse(fs.readFileSync(new URL("../apps/desktop/package.json", import.meta.url), "utf8")).version;
const firstNote = html.match(/<ol class="log"[^>]*>\s*<li>\s*<h3>(\d+\.\d+\.\d+)/)?.[1];
if (firstNote !== appVersion) problems.push(`the newest release note is ${firstNote ?? "missing"} but the app is at ${appVersion}: add the ${appVersion} entry to the Release notes section of site/index.html`);
for (const m of html.matchAll(/(?:data-version>|MyLoDB_)(\d+\.\d+\.\d+)/g)) if (m[1] !== appVersion) problems.push(`stale version ${m[1]} in site/index.html (the app is at ${appVersion})`);
if (problems.length) { console.error("site check failed:\n- " + problems.join("\n- ")); process.exit(1); }
console.log("site check ok");
