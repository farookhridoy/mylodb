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
if (problems.length) { console.error("site check failed:\n- " + problems.join("\n- ")); process.exit(1); }
console.log("site check ok");
