# MyLoDB

Open-source MySQL/MariaDB manager for macOS, written in Rust (Tauri + React).

Status: planning. See [docs/ANALYSIS.md](docs/ANALYSIS.md) for the feature analysis, architecture, and phased roadmap.

## Develop

```sh
cd apps/desktop
npm install
npm run tauri dev      # launches the app with hot reload
cargo test             # from the repo root: Rust unit + MySQL integration tests
npm run e2e            # UI tests of the grid, designer, users manager, editor and server tools (needs `npm run dev` running and CHROME_PATH set)
```

Integration tests use a local MySQL (`MYLODB_TEST_HOST/PORT/USER/PASSWORD`) and create/drop only their own `mylodb_itest_*` database.

## Status

- Phase 1 (done): connection manager (passwords in macOS Keychain), object tree, and the left-menu filter (`ord*`, `/regex/`, `type:view`, `db:shop`, `col:email`).
- Phase 2 (done): SQL editor (CodeMirror, MySQL dialect), query tabs, run selection or all (⌘↵), cancel (`KILL QUERY`), multi-result tabs and messages, `DELIMITER` support, history with search, favorites with folders, and tab session restore. Double-click a table to open it.
- Phase 3 (done): table data grid — server-side paging, per-column filter row (contains, =, ≠, starts/ends with, <, ≤, >, ≥, between, in, regexp, NULL, empty), free-form WHERE, multi-column sort, staged inline edits (update/insert/delete in one transaction, rows keyed by primary/unique key, read-only otherwise), duplicate row, form view, value viewer (text/JSON/hex/image, upload/save BLOBs), foreign-key picker and ⌘-click navigation, copy rows as TSV/CSV/JSON/INSERT, export to CSV/JSON/SQL, "View queries".
- Phase 4 (done): table designer (columns, types, defaults, generated columns, indexes, foreign keys, engine/charset/collation, live SQL preview, reads `information_schema`, diffs the original against your edit into a minimal ordered `ALTER TABLE` sequence, renames/moves keep data), table operations (duplicate, rename, truncate and drop with type-to-confirm), create/drop database, and a user and privilege manager (create/rename/lock/drop users, global, database, table and column privileges, shown as the exact `GRANT`/`REVOKE` diff with passwords masked).
- Phase 5 (done): context-aware autocomplete (tables, columns, aliases, `db.` qualifiers, functions, keywords; quoting of reserved/special names; works from the in-memory object index, so no server round trip), SQL formatter (indent, keyword case; selection or whole script; ⇧⌘F), Explain / Explain Analyze with plain-language hints and a collapsible JSON plan, and a query profiler (stage timings from `SHOW PROFILE` plus the rows read, sorts and temp tables the statement caused). Autocomplete rebuilds after DDL and from the tree's ⟳ button.
- Phase 6 (done): SSH tunnelling (password or private key; the host key is shown for confirmation on first use and a changed key blocks the connection), SSL modes (verify, verify CA only, skip verify) with CA and client certificate files, a *Test connection* button, and a Server tab: overview with live queries/s and connections, process list (stop query / kill connection), global variables (edit with confirmation, server warnings shown), status with per-second rates, CHECK / ANALYZE / OPTIMIZE / REPAIR for many tables at once, and logs (error log, slow queries, binary logs and events, log settings).
- Next: Phase 7 — import (CSV, SQL), XLSX/XML/HTML export, backup to a zip with file-per-table, restore.
