// Fails when a UI suite in e2e/ is not part of `npm run e2e` (a suite that is not listed never runs in CI).
import fs from "node:fs";
const pkg = JSON.parse(fs.readFileSync(new URL("../package.json", import.meta.url), "utf8"));
const listed = new Set([...pkg.scripts.e2e.matchAll(/node e2e\/(\w+)\.mjs/g)].map((m) => m[1]));
const files = fs.readdirSync(new URL("../e2e/", import.meta.url)).filter((f) => f.endsWith(".mjs")).map((f) => f.slice(0, -4));
const missing = files.filter((f) => !listed.has(f));
const gone = [...listed].filter((f) => !files.includes(f));
if (missing.length || gone.length) {
  console.error(`npm run e2e is out of step with e2e/: not listed: ${missing.join(", ") || "-"}; listed but missing: ${gone.join(", ") || "-"}`);
  process.exit(1);
}
console.log(`ok: all ${files.length} UI suites are in npm run e2e`);
