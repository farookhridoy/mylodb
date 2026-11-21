# Releasing MyLoDB

1. Pick the version (semver). Set it in `Cargo.toml` (`[workspace.package]`), `apps/desktop/package.json`, `apps/desktop/package-lock.json` and `apps/desktop/src-tauri/tauri.conf.json`. The About box and manual read it from `package.json`.
2. Update `CHANGELOG.md`, the download file name in `README.md` and `docs/MANUAL.md`.
3. Check: `cargo test --workspace`, `npm run e2e` (with `npm run dev` running and `CHROME_PATH` set), `npx tsc --noEmit`.
4. Regenerate screenshots if the UI changed: `node scripts/screenshots.mjs`.
5. Build: `cd apps/desktop && npm run tauri build -- --bundles dmg`. The file is `target/release/bundle/dmg/MyLoDB_<version>_aarch64.dmg`.
6. Checksum: `shasum -a 256 MyLoDB_<version>_aarch64.dmg > MyLoDB_<version>_aarch64.dmg.sha256`.
7. Tag and release as a draft, upload both files, then publish:
   `git tag -a v<version> -m "MyLoDB <version>"`, `git push origin v<version>`,
   `gh release create v<version> --draft --title "MyLoDB <version>" --notes-file <notes>`, `gh release upload ...`, `gh release edit v<version> --draft=false --latest`.
8. The build is unsigned. Signing and notarisation need an Apple Developer ID; until then the release notes and README explain **Open Anyway**.

`main` is protected: changes go through a pull request.
