// Saved passwords: the Keychain by default, or an encrypted file behind a master password (unlock window, Preferences → Security).
import { chromium } from "playwright-core";
import fs from "node:fs";
import assert from "node:assert/strict";

const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1400, height: 900 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push("pageerror: " + e.message));
// start with the passwords already in the encrypted file and locked, as after a restart
await page.addInitScript({ content: "window.__vault = { mode: 'file', unlocked: false, pw: 'correct horse' };\n" + fs.readFileSync(new URL("./mock.js", import.meta.url), "utf8") });
await page.goto(process.env.APP_URL || "http://localhost:1420/");
let step = 0;
const ok = (m) => console.log(`ok ${++step} ${m}`);

// at start the unlock window asks for the master password; a wrong one is refused and explained
await page.waitForSelector(".unlock-dialog");
await page.fill(".unlock-dialog input[type=password]", "wrong wrong");
await page.click(".unlock-dialog .primary");
await page.waitForSelector(".unlock-dialog .banner.bad");
assert.match(await page.locator(".unlock-dialog .banner.bad").innerText(), /not right/);
assert.doesNotMatch(await page.locator(".unlock-dialog").innerText(), /vault_/, "the code in front of the message is not shown");
assert.equal(await page.locator(".unlock-dialog input[type=password]").inputValue(), "", "the wrong attempt is cleared");
ok("a locked password file asks for the master password at start, and a wrong one is refused plainly");

// the right one unlocks it
await page.fill(".unlock-dialog input[type=password]", "correct horse");
await page.keyboard.press("Enter");
await page.waitForSelector(".unlock-dialog", { state: "detached" });
assert.equal(await page.evaluate(() => window.__vault.unlocked), true);
ok("the right master password unlocks it");

// dismissing the window leaves it locked; a connection that needs a password asks again and then connects
await page.evaluate(() => { window.__vault.unlocked = false; });
await page.selectOption(".conn-bar select", "1");
await page.waitForSelector(".unlock-dialog");
await page.fill(".unlock-dialog input[type=password]", "correct horse");
await page.click(".unlock-dialog .primary");
await page.waitForSelector(".tree .db");
ok("opening a connection while locked asks for the password, then opens it");

// Preferences → Security: change the password, then move back to the Keychain
await page.evaluate(() => window.__emit("menu-action", "preferences"));
await page.click(".prefs-item:has-text('Saved passwords')");
await page.waitForSelector(".security-page");
assert.match(await page.locator(".security-page").innerText(), /in an encrypted file/);
const [cur, n1, n2] = await page.locator(".security-page input[type=password]").all();
await cur.fill("correct horse"); await n1.fill("battery staple"); await n2.fill("battery stapl");
assert.equal(await page.locator(".security-page button:has-text('Change password')").isDisabled(), true, "the two new passwords must match");
await n2.fill("battery staple");
await page.click(".security-page button:has-text('Change password')");
await page.waitForSelector(".security-page .banner:has-text('was changed')");
assert.equal(await page.evaluate(() => window.__vault.pw), "battery staple");
ok("the master password can be changed, and the two entries must match");

await page.click(".security-page button:has-text('Move passwords back')");
await page.click(".security-page .banner button.primary");
await page.waitForSelector(".security-page .banner:has-text('back into the Keychain')");
assert.equal(await page.evaluate(() => window.__vault.mode), "keychain");
assert.match(await page.locator(".security-page").innerText(), /in the macOS Keychain/);
ok("the passwords can be moved back to the Keychain");

// and from the Keychain to a file: the form needs 8 characters and a matching repeat
const [p1, p2] = await page.locator(".security-page input[type=password]").all();
const move = page.locator(".security-page button.primary");
await p1.fill("short"); await p2.fill("short");
assert.equal(await move.isDisabled(), true, "under 8 characters");
await p1.fill("a good master"); await p2.fill("a good maste");
assert.equal(await move.isDisabled(), true, "the repeat differs");
await p2.fill("a good master");
await move.click();
await page.waitForSelector(".security-page .banner:has-text('encrypted file')");
assert.equal(await page.evaluate(() => window.__vault.mode), "file");
const used = await page.evaluate(() => window.__calls.filter((c) => c.cmd === "secrets_use_file").map((c) => c.args));
assert.deepEqual(used, [{ password: "a good master" }]);
ok("moving to the encrypted file needs a matching master password of 8+ characters");

// forgot the master password: after a restart the window offers to delete the file, and the app carries on
await page.reload();
await page.waitForSelector(".unlock-dialog");
await page.click(".unlock-dialog button:has-text('I forgot')");
assert.match(await page.locator(".unlock-dialog").innerText(), /no way to recover/);
await page.click(".unlock-dialog button:has-text('Keep it')");
assert.equal(await page.evaluate(() => window.__vault.mode), "file", "keeping it changes nothing");
await page.click(".unlock-dialog button:has-text('I forgot')");
await page.click(".unlock-dialog button:has-text('Delete the file')");
await page.waitForSelector(".unlock-dialog", { state: "detached" });
assert.equal(await page.evaluate(() => window.__vault.mode), "keychain");
ok("a forgotten master password can be reset after a warning that nothing can be recovered");

// Esc closes the window without unlocking
await page.reload();
await page.waitForSelector(".unlock-dialog");
await page.keyboard.press("Escape");
await page.waitForSelector(".unlock-dialog", { state: "detached" });
assert.equal(await page.evaluate(() => window.__vault.unlocked), false);
ok("Esc dismisses the window and leaves the file locked");

assert.deepEqual(errors, [], errors.join("\n"));
console.log("ALL SAVED-PASSWORD UI CHECKS PASSED");
await browser.close();
