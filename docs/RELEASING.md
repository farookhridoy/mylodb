# Releasing MyLoDB

A release is a tag. Pushing `v<version>` runs `.github/workflows/release.yml`, which builds Apple Silicon and Intel, signs the update packages,
attaches the `.dmg` files (with SHA-256 checksums), the update packages and `latest.json` to a GitHub release whose notes are that version's
section of `CHANGELOG.md`, checks `latest.json`, and publishes the release. Installed copies (0.4.0 and later) then offer the update.

1. Pick the version (semver). Set it in `Cargo.toml` (`[workspace.package]`), `apps/desktop/package.json`, `apps/desktop/package-lock.json` and `apps/desktop/src-tauri/tauri.conf.json`. The About box and manual read it from `package.json`; the workflow refuses a tag that does not match them.
2. Add the version to the **Release notes** list in `site/index.html` (newest first) and update the version numbers there; `node scripts/check-site.mjs` fails until it matches. In `CHANGELOG.md` rename **Unreleased** to `## <version> (<date>)`. This section is what installed copies show as "What is new", so write it for users.
3. Check: `cargo test --workspace` (against a test database, see CLAUDE.md), `npm run e2e` (with `npm run dev` running and `CHROME_PATH` set), `npx tsc --noEmit`.
4. Regenerate screenshots if the UI changed: `node scripts/screenshots.mjs`.
5. Merge to `main` through a pull request (`main` is protected).
6. Tag and push: `git tag -a v<version> -m "MyLoDB <version>"` and `git push origin v<version>`. Watch the **Release** workflow; the two builds run one after the other (about 20 minutes). A failed run leaves a draft release: fix the cause and run the workflow again from the Actions page with the tag.
7. Install the published release once from the `.dmg` and, from the previous version, try **Help → Check for Updates…**.

## The update key

Updates are signed with a minisign key. The **public** key is in `apps/desktop/src-tauri/tauri.conf.json` (`plugins.updater.pubkey`). The **private** key and its password are the repository secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, and should also be kept somewhere safe off GitHub. If they are lost, installed copies can no longer be updated and users must install a new build by hand; if they leak, rotate the key (generate a new pair with `npx tauri signer generate`, ship one release signed with the old key that carries the new public key, then switch).

To build a signed package locally: `cd apps/desktop && TAURI_SIGNING_PRIVATE_KEY="$(cat <key file>)" TAURI_SIGNING_PRIVATE_KEY_PASSWORD=... npm run tauri build -- --bundles app,dmg` (add `--target x86_64-apple-darwin` for Intel after `rustup target add x86_64-apple-darwin`). Without the key the build fails, because updater artifacts are on.

## Signing and notarisation

The app is not signed with an Apple Developer ID. Gatekeeper warns on the first manual install (the release notes and README explain **Open Anyway**); updates installed by MyLoDB itself are not quarantined and open without that warning. Signing and notarisation need an Apple Developer ID and are on the roadmap.
