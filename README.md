# MyLoDB

Open-source MySQL/MariaDB manager for macOS, written in Rust (Tauri + React).

Status: planning. See [docs/ANALYSIS.md](docs/ANALYSIS.md) for the feature analysis, architecture, and phased roadmap.

## Develop

```sh
cd apps/desktop
npm install
npm run tauri dev      # launches the app with hot reload
cargo test             # from the repo root: Rust unit + MySQL integration tests
npm run e2e            # UI test of the grid (needs `npm run dev` running and CHROME_PATH set)
```

Integration tests use a local MySQL (`MYLODB_TEST_HOST/PORT/USER/PASSWORD`) and create/drop only their own `mylodb_itest_*` database.

## Status

- Phase 1 (done): connection manager (passwords in macOS Keychain), object tree, and the left-menu filter (`ord*`, `/regex/`, `type:view`, `db:shop`, `col:email`).
- Phase 2 (done): SQL editor (CodeMirror, MySQL dialect), query tabs, run selection or all (⌘↵), cancel (`KILL QUERY`), multi-result tabs and messages, `DELIMITER` support, history with search, favorites with folders, and tab session restore. Double-click a table to open it.
- Phase 3 (done): table data grid — server-side paging, per-column filter row (contains, =, ≠, starts/ends with, <, ≤, >, ≥, between, in, regexp, NULL, empty), free-form WHERE, multi-column sort, staged inline edits (update/insert/delete in one transaction, rows keyed by primary/unique key, read-only otherwise), duplicate row, form view, value viewer (text/JSON/hex/image, upload/save BLOBs), foreign-key picker and ⌘-click navigation, copy rows as TSV/CSV/JSON/INSERT, export to CSV/JSON/SQL, "View queries".
- Next: Phase 4 — table designer, index/FK editors, user and privilege manager.
