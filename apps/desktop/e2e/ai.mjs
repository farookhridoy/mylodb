// Editor help: Tab takes a suggestion, problems are underlined, and the AI assistant writes a query from a sentence.
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1400, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
await page.addInitScript({ content: "localStorage.setItem('mylodb.tourDone','1');\n" + fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);
const calls = (cmd) => page.evaluate((c) => window.__calls.filter((x) => x.cmd === c).map((x) => x.args), cmd);
const doc = () => page.evaluate(() => document.querySelector(".cm-content").innerText.replace(/ /g, " "));
const clear = async () => { await page.click(".cm-content"); await page.keyboard.press("ControlOrMeta+a"); await page.keyboard.press("Backspace"); };

await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".cm-content");

// Tab takes the highlighted suggestion and stays in the editor
await clear();
await page.keyboard.type("SELECT * FROM ord");
await page.waitForSelector(".cm-tooltip-autocomplete li");
await page.keyboard.press("Tab");
assert.match(await doc(), /FROM orders/);
assert.equal(await page.evaluate(() => !!document.activeElement.closest(".cm-editor")), true, "focus stays in the editor");
ok("Tab accepts the highlighted suggestion and focus stays in the editor");

// with no suggestion open, Tab indents instead of moving focus
await clear();
await page.keyboard.type("x");
await page.keyboard.press("Escape");
await page.keyboard.press("Tab");
assert.equal(await page.evaluate(() => !!document.activeElement.closest(".cm-editor")), true, "focus stays in the editor");
assert.match(await doc(), /^\s+x|x\s*$/);
assert.ok((await doc()).length > 1, "Tab inserted indentation");
ok("Tab without a suggestion indents and does not leave the editor");

// snippets have tab stops: the first name is selected, a name used twice changes everywhere, Tab goes to the next
await clear();
await page.keyboard.press("Meta+k");
await page.fill("input[aria-label=Command]", "GROUP BY with count");
await page.waitForFunction(() => (document.querySelector(".palette li")?.innerText ?? "").includes("GROUP BY"));
await page.keyboard.press("Enter");
await page.waitForFunction(() => /column_name/.test(document.querySelector(".cm-content").innerText));
await page.keyboard.type("city");
await page.keyboard.press("Tab");
await page.keyboard.type("customers");
const grouped = await doc();
assert.match(grouped, /SELECT city, COUNT\(\*\) AS total\s+FROM customers\s+GROUP BY city\s+ORDER BY total DESC;/, grouped);
ok("a snippet selects its first name, a repeated name changes in every place, and Tab moves to the next name");

// problems are underlined, with the message on hover
await clear();
await page.evaluate(() => { window.__diag = [{ from: 14, to: 20, severity: "warning", message: "Table 'ordres' was not found in 'shop'" }]; });
await page.keyboard.type("SELECT * FROM ordres");
await page.waitForSelector(".cm-lintRange-warning", { timeout: 5000 });
assert.ok((await calls("diagnose_sql")).length > 0);
await page.hover(".cm-lintRange-warning");
await page.waitForSelector(".cm-tooltip-lint");
assert.match(await page.locator(".cm-tooltip-lint").innerText(), /was not found in 'shop'/);
ok("a problem found by the server side is underlined and explained on hover");
await page.evaluate(() => { window.__diag = []; });

// Ask AI: a toolbar button after Server opens a panel on the right
await clear();
const labels = await page.locator("header.topbar button").allInnerTexts();
const iServer = labels.findIndex((t) => /Server/.test(t)), iAi = labels.findIndex((t) => /Ask AI/.test(t));
assert.ok(iAi === iServer + 1, `Ask AI comes right after Server (${labels.join("|")})`);
assert.equal(await page.locator("button[aria-label=Settings]").count(), 1, "the gear is called Settings");
await page.click("header.topbar button:has-text('Ask AI')");
await page.waitForSelector(".ai-panel");
assert.match(await page.locator(".ai-panel").innerText(), /Service/, "not set up yet: the panel shows the setup first");
ok("Ask AI is in the toolbar after Server, opens a right-hand panel and starts with the setup; the gear is named Settings");

// a service with a key: Get a key opens the browser, Connect checks the key and lists the free models
await page.selectOption(".ai-setup select >> nth=0", "openrouter");
await page.click(".ai-panel button:has-text('Get a key in your browser')");
assert.deepEqual((await calls("open_url")).at(-1), { url: "https://openrouter.ai/keys" });
await page.fill(".ai-setup input[type=password]", "sk-test");
await page.click(".ai-panel button:has-text('Connect')");
await page.waitForSelector(".ai-status:has-text('Connected')");
const opts = await page.locator(".ai-setup select[aria-label=Model] option").allInnerTexts();
assert.ok(opts.some((o) => /meta\/free-big · free · 131k/.test(o)), `free models with their window are listed: ${opts}`);
assert.ok(!opts.some((o) => /paid\/model/.test(o)), "paid models are hidden until asked for");
await page.check(".ai-setup label:has-text('Show paid models') input");
await page.waitForFunction(() => [...document.querySelectorAll(".ai-setup select[aria-label=Model] option")].some((o) => /paid\/model/.test(o.textContent)));
await page.uncheck(".ai-setup label:has-text('Show paid models') input");
const saved = JSON.parse(await page.evaluate(() => localStorage.getItem("mylodb.ai")));
assert.equal(saved.preset, "openrouter");
assert.equal(saved.auto, true, "switching between models is on by default");
ok("Get a key opens the provider's key page; Connect checks the key and fills the model list with the free models");

// a wrong key is refused and nothing stays connected
await page.click(".ai-status button:has-text('Disconnect')");
await page.evaluate(() => { window.__aiBadKey = true; });
await page.fill(".ai-setup input[type=password]", "bad");
await page.click(".ai-panel button:has-text('Connect')");
await page.waitForSelector(".ai-setup .banner.bad");
assert.match(await page.locator(".ai-setup .banner.bad").innerText(), /refused the key/);
assert.equal(await page.locator(".ai-status:has-text('Connected')").count(), 0);
await page.evaluate(() => { window.__aiBadKey = false; });
await page.click(".ai-panel button:has-text('Connect')");
await page.waitForSelector(".ai-status:has-text('Connected')");
ok("a refused key is explained and does not count as connected");

// Claude is offered with a key too
await page.selectOption(".ai-setup select >> nth=0", "anthropic");
assert.match(await page.locator(".ai-setup").innerText(), /billed separately from a Claude subscription/);
await page.click(".ai-panel button:has-text('Get a key in your browser')");
assert.deepEqual((await calls("open_url")).at(-1), { url: "https://console.anthropic.com/settings/keys" });
await page.selectOption(".ai-setup select >> nth=0", "openrouter");
await page.waitForSelector(".ai-status:has-text('Connected')");
ok("Claude is offered with a link to make an API key");

// chat: consent first for an internet service, then the SQL arrives with its buttons, and is never run
await page.click(".ai-panel button:has-text('Done')");
await page.waitForSelector(".ai-composer textarea");
assert.match(await page.locator(".ai-consent").innerText(), /openrouter\.ai/);
assert.equal(await page.locator(".ai-composer button.primary").isDisabled(), true, "no sending before the service is allowed");
await page.click(".ai-consent button:has-text('Allow')");
await page.fill(".ai-composer textarea", "top 10 customers by total orders");
await page.keyboard.press("Enter");
await page.waitForSelector(".ai-code pre");
assert.match(await page.locator(".ai-code pre").innerText(), /SELECT c\.name/);
const sent = (await calls("ai_chat")).at(-1);
assert.equal(sent.request, "top 10 customers by total orders");
assert.equal(sent.settings.preset, "openrouter");
assert.deepEqual(sent.settings.fallbacks.map((f) => f.id), ["meta/free-small"], "the other models are the fallbacks");
await page.click(".ai-code button:has-text('Insert in editor')");
await page.waitForFunction(() => /customer_id/.test(document.querySelector(".cm-content").innerText));
assert.equal((await calls("run_query")).length, 0, "nothing was run");
ok("a question is answered with SQL that can be inserted into the editor; other models are the fallbacks; nothing is run");

// a follow-up carries the conversation, and a switch of model is announced
await page.evaluate(() => { window.__aiAnswerModel = "meta/free-small"; window.__aiNote = "Switched to meta/free-small because the first model failed: Too many requests."; });
await page.fill(".ai-composer textarea", "only for 2024");
await page.keyboard.press("Enter");
await page.waitForSelector(".ai-note");
assert.match(await page.locator(".ai-note").innerText(), /Switched to meta\/free-small/);
const follow = (await calls("ai_chat")).at(-1);
assert.deepEqual(follow.history.map((h) => h.role), ["user", "assistant"]);
ok("a follow-up sends the earlier turns, and a switch to another model is shown");

// errors appear in the chat; the panel closes and reopens with its conversation
await page.evaluate(() => { window.__aiFail = "Too many requests, or the free limit is used up (429)."; });
await page.fill(".ai-composer textarea", "again");
await page.keyboard.press("Enter");
await page.waitForSelector(".ai-msg.error");
await page.click("button[aria-label='Close Ask AI']");
assert.equal(await page.locator(".ai-panel").count(), 0);
await page.click("header.topbar button:has-text('Ask AI')");
await page.waitForSelector(".ai-panel .ai-msg.error");
ok("an error is shown in the chat, and the conversation is still there when the panel is reopened");

// the layout holds at a narrow window with the panel open: nothing spills out, the Send button is visible, answers are formatted
await page.evaluate(() => { window.__aiFail = null; });
await page.setViewportSize({ width: 1000, height: 800 });
await page.evaluate(() => { window.__aiText = "Here is why it may be empty:\n\n1. **Entry status** - `entries.is_reversed = 1` or `NULL`.\n2. Another point with a_very_long_identifier_name_that_would_never_wrap_by_itself_in_a_narrow_panel_unless_forced.\n\n```sql\nSELECT 1;\n```"; });
await page.fill(".ai-composer textarea", "x ".repeat(150) + "a_very_long_word_without_any_spaces_".repeat(4));
const geo = await page.evaluate(() => {
  const panel = document.querySelector(".ai-panel").getBoundingClientRect(), send = document.querySelector(".ai-composer .primary").getBoundingClientRect();
  const comp = document.querySelector(".ai-composer").getBoundingClientRect(), pick = document.querySelector(".ai-model-pick").getBoundingClientRect();
  const tb = document.querySelector(".toolbar");
  return { panelRight: panel.right, sendRight: send.right, compRight: comp.right, pickRight: pick.right, vw: innerWidth, tbOver: tb.scrollWidth - tb.clientWidth, pageScroll: document.documentElement.scrollWidth - innerWidth };
});
assert.ok(geo.sendRight <= geo.panelRight + 1, `Send stays inside the panel (${geo.sendRight} <= ${geo.panelRight})`);
assert.ok(geo.compRight <= geo.panelRight + 1 && geo.pickRight <= geo.compRight, "the message box and the model list stay inside the panel");
assert.ok(geo.tbOver <= 1, `the editor toolbar does not overflow (${geo.tbOver}px)`);
assert.ok(geo.pageScroll <= 1, "the page does not scroll sideways");
await page.fill(".ai-composer textarea", "why is it empty");
await page.keyboard.press("Enter");
await page.waitForSelector(".ai-text strong");
assert.equal(await page.locator(".ai-text strong").last().innerText(), "Entry status");
assert.ok((await page.locator(".ai-text code").count()) >= 2, "inline code is formatted");
assert.equal(await page.locator(".ai-text ol li").count(), 2);
assert.doesNotMatch(await page.locator(".ai-msg.assistant").last().innerText(), /\*\*/, "no raw ** markers");
const names = await page.locator(".toolbar button").evaluateAll((bs) => bs.map((b) => b.getAttribute("title")));
assert.ok(["Show exactly what will run, choose foreign key checks, then run", "Cancel running query"].every((t) => names.includes(t)), "toolbar buttons keep their hover text");
ok("at a narrow width nothing spills out of the panel or the toolbar, answers are formatted, and toolbar buttons are icons with hover text");

assert.deepEqual(errors, []);
await browser.close();
console.log("ALL EDITOR AI CHECKS PASSED");
