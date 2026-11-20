# MyLoDB

Open-source MySQL/MariaDB manager for macOS, written in Rust (Tauri + React).

Status: planning. See [docs/ANALYSIS.md](docs/ANALYSIS.md) for the feature analysis, architecture, and phased roadmap.

## Develop

```sh
cd apps/desktop
npm install
npm run tauri dev      # launches the app with hot reload
cargo test             # from the repo root: Rust unit + MySQL integration tests
```

Integration tests use a local MySQL (`MYLODB_TEST_HOST/PORT/USER/PASSWORD`) and create/drop only their own `mylodb_itest_*` database.

## Status

- Phase 1 (in progress): connection manager (passwords in macOS Keychain), object tree, and the left-menu filter (`ord*`, `/regex/`, `type:view`, `db:shop`, `col:email`).
