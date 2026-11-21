# MyLoDB

Open-source MySQL/MariaDB manager for macOS, written in Rust (Tauri + React).

See [docs/ROADMAP.md](docs/ROADMAP.md) for what is done and what is left, and [docs/ANALYSIS.md](docs/ANALYSIS.md) for the original analysis and architecture.

## Develop

```sh
cd apps/desktop
npm install
npm run tauri dev      # launches the app with hot reload
cargo test             # from the repo root: Rust unit + MySQL integration tests
npm run e2e            # UI tests of the grid, designer, users manager, editor, server tools, transfers, tree, object browser, objects, console, table views and theme (needs `npm run dev` running and CHROME_PATH set)
```

Integration tests use a local MySQL (`MYLODB_TEST_HOST/PORT/USER/PASSWORD`) and create/drop only their own `mylodb_itest_*` database.

## Status

- Phase 1 (done): connection manager (passwords in macOS Keychain), object tree, and the left-menu filter (`ord*`, `/regex/`, `type:view`, `db:shop`, `col:email`).
- Phase 2 (done): SQL editor (CodeMirror, MySQL dialect), query tabs, run selection or all (⌘↵), cancel (`KILL QUERY`), multi-result tabs and messages, `DELIMITER` support, history with search, favorites with folders, and tab session restore. Double-click a table to open it.
- Phase 3 (done): table data grid — server-side paging, per-column filter row (contains, =, ≠, starts/ends with, <, ≤, >, ≥, between, in, regexp, NULL, empty), free-form WHERE, multi-column sort, staged inline edits (update/insert/delete in one transaction, rows keyed by primary/unique key, read-only otherwise), duplicate row, form view, value viewer (text/JSON/hex/image, upload/save BLOBs), foreign-key picker and ⌘-click navigation, copy rows as TSV/CSV/JSON/INSERT, export to CSV/JSON/SQL, "View queries".
- Phase 4 (done): table designer (columns, types, defaults, generated columns, indexes, foreign keys, engine/charset/collation, live SQL preview, reads `information_schema`, diffs the original against your edit into a minimal ordered `ALTER TABLE` sequence, renames/moves keep data), table operations (duplicate, rename, truncate and drop with type-to-confirm), create/drop database, and a user and privilege manager (create/rename/lock/drop users, global, database, table and column privileges, shown as the exact `GRANT`/`REVOKE` diff with passwords masked).
- Phase 5 (done): context-aware autocomplete (tables, columns, aliases, `db.` qualifiers, functions, keywords; quoting of reserved/special names; works from the in-memory object index, so no server round trip), SQL formatter (indent, keyword case; selection or whole script; ⇧⌘F), Explain / Explain Analyze with plain-language hints and a collapsible JSON plan, and a query profiler (stage timings from `SHOW PROFILE` plus the rows read, sorts and temp tables the statement caused). Autocomplete rebuilds after DDL and from the tree's ⟳ button.
- Phase 6 (done): SSH tunnelling (password or private key; the host key is shown for confirmation on first use and a changed key blocks the connection), SSL modes (verify, verify CA only, skip verify) with CA and client certificate files, a *Test connection* button, and a Server tab: overview with live queries/s and connections, process list (stop query / kill connection), global variables (edit with confirmation, server warnings shown), status with per-second rates, CHECK / ANALYZE / OPTIMIZE / REPAIR for many tables at once, and logs (error log, slow queries, binary logs and events, log settings).
- Phase 7 (done): backup to SQL (structure, data, views, routines, triggers, events; one consistent snapshot; one file or a file per table; optional .zip; timestamped names; cancellable, partial output removed), restore from a .sql file, a .zip or a folder (streamed from disk, errors listed with line numbers), CSV import (delimiter detection, preview, encodings, column mapping, insert/skip/update/replace, all-or-nothing or continue-on-error, create a table from the file), and export of tables or any SELECT to CSV, JSON, SQL INSERT, XML, HTML or Excel.
- Tree and theme update: the object-tree filter is scoped to the selected database ("Filter tables in …") and database names only when none is selected; columns load per database on demand and groups are paged, so very large servers stay fast; ⇧⌘B focuses the filter. Light, dark and system themes.
- Object browser: the connection (`user@host`) is the root node; click it to reset. Filter databases, then click one to filter its tables, then click a table to filter its columns (by name or type). Columns show their type (`bigint unsigned`, `char(255)`) and tables their size.
- Remote servers: connect by host or paste a JDBC / mysql:// URL; "Show all databases" off lists only the default database (shared hosting). Result tabs: Results, Profile, Messages, Table Data, Info, History. All dropdowns are searchable.
- Large tables (Phase 10): keyset paging, instant estimated counts with a cancellable exact count, a windowed grid, exports as cancellable jobs with JSON Lines and gzip. Measured at 10 million rows: [docs/BENCHMARKS.md](docs/BENCHMARKS.md).
- Roadmap and what is still to do (mandatory / nice to have / optional): [docs/ROADMAP.md](docs/ROADMAP.md). phpMyAdmin parity, menu bar, preferences and more database engines: [docs/PHPMYADMIN_AND_ENGINES.md](docs/PHPMYADMIN_AND_ENGINES.md).
- Production safety (connection dialog → Safety): environment badge and colour, server-enforced read-only, review of destructive SQL with row estimates, query timeout, confirmation before saving grid changes on production.
- Filter options (filter icon): search tables only, or also views, routines, triggers, events and columns. DBeaver-style context menus with icons, sections, shortcuts and submenus.
- Table views: every table has Properties (settings, columns, constraints, foreign keys, references, triggers, indexes, partitions, statistics, DDL), Data (console + grid) and Diagram (the table and the tables it is related to).
- Workspace: a table opens as a SQL console (top) and its data grid (bottom); sidebar and console are resizable; arrows open and close nodes; clicking a table, column or database name pastes it into the query being written; the table menu has SELECT / INSERT / UPDATE / DELETE samples; the locked database is the console's database.
- Browser lock: clicking a database locks the filter and the tree to it (a chip in the filter box; ✕, Backspace or Esc releases it), clicking a table locks to its columns. Flat IDE-style look with one set of line icons.
- Phase 8: views, procedures, functions, triggers and events have menus (show / edit definition as a runnable script, drop, new-object templates); tables list their indexes; databases show their size; expansion and selection are remembered per connection; F5 refreshes.
- Next: see [docs/ROADMAP.md](docs/ROADMAP.md) for the phase list, the comparison with the product plan and the SQLyog reference (phases 1 to 13 are done; engines, power tools, visual tools and the beta release are open).
