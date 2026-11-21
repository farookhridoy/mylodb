# MyLoDB roadmap: what is done, what must be done, what would be nice

Status of 2026-11-21. This is the single page for "where are we". The older `PLAN_VS_CURRENT.md` keeps the detailed
comparison with the product plan and the SQLyog reference; `BENCHMARKS.md` has the Phase 10 measurements;
`PHPMYADMIN_AND_ENGINES.md` has the analysis behind phases 11 to 15.

✅ done and tested · 🟡 partly · ❌ not started

Priority words used below:
* **Mandatory**: without it MyLoDB cannot replace phpMyAdmin / SQLyog for daily work, or cannot be released.
* **Nice to have**: clearly valuable, the product works without it.
* **Optional**: only if there is demand; may never be built.

---

## 1. Done

| Phase | What | Status |
|---|---|---|
| 1 to 7 | Connections (SSH, SSL, keychain), object browser and filter, SQL editor (autocomplete, formatter, EXPLAIN, profile), editable data grid with staged edits, table designer, users and privileges, server tools (processes, variables, status, maintenance, logs), backup / restore / CSV import / export, history and favorites | ✅ |
| 8 | Object browser completion: views, routines, triggers, events (definition, drop, templates), indexes, sizes, remembered state, F5 | ✅ |
| 9 | **Production safety**: environment badge and colour, server-enforced read-only, review of destructive SQL with row estimates, query timeout, confirm on commit | ✅ |
| 10 | **Large data engine**: windowed grid, keyset paging, estimated counts with cancellable exact count, exports as cancellable jobs with progress, JSON Lines, gzip. Measured at 1M and 10M rows (`BENCHMARKS.md`) | ✅ |
| 11b to 11e (part) | **Admin console, first half**: Preview SQL then run dialog for every drop, empty, rename, duplicate (FK-check switch inside, typed word on production); **Databases page** (sizes, table counts, sort, search, check all, bulk drop, create with collation); database **Structure page** (rows, engine, collation, size, overhead, filter, row actions, With selected: copy, show create, export, empty, drop, analyze, check, checksum, optimize, repair, add / replace prefix, copy with prefix, create new table); **FK checks on / off for the console's Run**; connection collation setting. Real-MySQL tests for the plan SQL and queries, UI suites for each page | ✅ |
| extra | Light / dark / system theme; flat IDE look with one icon set; searchable dropdowns (select2 style); DBeaver-style context menus; lock to a database or table in the browser; table Properties / Data / Diagram views; console above data on table tabs; result tabs (Results, Profile, Messages, Table Data, Info, History); connect by host or URL; "Show all databases"; resizable sidebar and console | ✅ |

Tests: 148 Rust tests (many against a real MySQL 9.7) and 16 UI suites. Unsigned arm64 `.dmg` builds and installs.

What Phase 10 did **not** cover (honest list): column virtualization for very wide tables, a multi-page cache or read-ahead, segmented exports,
MySQL 5.7 / 8.0 / 8.4 / MariaDB runs, network-drop tests, measuring the real Tauri window (only headless Chrome with a mock was measured).

---

## 2. Your phpMyAdmin / DBeaver / SQLyog list, item by item

| You asked for | Today | Plan |
|---|---|---|
| A **dashboard after connecting** (server info, collation, theme, like phpMyAdmin's home) | 🟡 Server tools has an Overview tab (version, host, charset, uptime, SSL cipher); nothing opens by itself after connect, no server connection collation setting | Phase 11a |
| **List all databases**, select all, **drop** the selected | ✅ Databases page (database menu > Databases) | done (11b) |
| **Go inside a database and see all tables**, choose some and drop them (the "Structure" tab: rows, engine, collation, size, overhead, Browse / Structure / Search / Insert / Empty / Drop, "With selected": copy, show create, export, empty, drop, analyze, check, checksum, optimize, repair, add / replace prefix, copy with prefix, "containing the word" filter, create table) | ✅ Structure page (database menu > Structure). Print / Data dictionary still to do | 11c done, print in 11f |
| After clicking Drop, **show the descriptive SQL** before it runs (`DROP TABLE a, b`) | ✅ one shared Preview SQL dialog for drops, empties, renames, copies, prefix operations | done (11d) |
| **Foreign key checks on / off** when importing and when running SQL | 🟡 console Run has an FK checks selector (default / on / off) per tab; Restore and Run SQL file already had one. **Still missing**: CSV import switch, preview before running a SQL file | 11e (rest) |
| **Preview the SQL** before the final run (phpMyAdmin style), run with and without FK checks | 🟡 grid changes have "View queries" and the designer previews DDL; the console reviews only risky statements; imports and SQL files cannot be previewed | Phase 11d / 11e |
| **Change the theme** | 🟡 Light / Dark / System. No gallery of named themes | Light and Dark done; named themes are nice to have (Phase 12) |
| **Preferences**: font size, family, bold, per area (sidebar, data grid, query), multi-colour query highlighting | 🟡 the editor is multi-colour (syntax highlighting), but there is no Preferences dialog and no per-area fonts | Phase 12 |
| A **navbar on top** like DBeaver or SQLyog (menus and a toolbar) | ❌ the sidebar has an icon strip; there is no menu bar (File, Edit, View, Connection, Database, Table, Tools, Window, Help) | Phase 12 |
| Connect to **MySQL, MariaDB, SQL Server, Oracle, SQLite, PostgreSQL** | 🟡 MySQL (tested on 9.7). MariaDB speaks the same protocol and probably works but has **never been tested**. The others need a database abstraction first | Phase 14 (MariaDB check), Phase 15 (engines) |

The analysis, screen mapping and design of these items are in `PHPMYADMIN_AND_ENGINES.md`.

---

## 3. What is left

### Mandatory (before a first release, or to replace phpMyAdmin / SQLyog)

| # | Item | Phase | Effort |
|---|---|---|---|
| 1 | Server **home dashboard** after connect (version, user, uptime, charset and connection collation, SSL, quick links, counts) | 11a | 3 d |
| 2 | **Databases page**: sorted list with sizes, table counts, collation; check all; **bulk drop**; create database with collation | 11b | 4 d |
| 3 | **Database Structure page**: table list (rows, engine, collation, size, overhead), "containing the word" filter, row actions, **With selected** menu (copy, show create, export, empty, drop, analyze, check, checksum, optimize, repair, prefix operations), create table | 11c | 1 to 1.5 wk |
| 4 | **Preview SQL, then run** for every bulk or destructive UI operation, with an on / off switch for foreign key checks inside the dialog | 11d | 4 d |
| 5 | FK-check switch for the **console's Run**, **CSV import** and a **preview before running a SQL file** | 11e | 3 d |
| 6 | **Top menu bar** (native macOS menu and in-window), toolbar | 12 | 1 wk |
| 7 | **Preferences**: fonts per area (sidebar, data grid, editor, results), size, family, weight, line height; editor colour theme; saved | 12 | 1 wk |
| 8 | **MariaDB and MySQL version matrix** (5.7, 8.0, 8.4, 9.x, MariaDB 10.11 and 11.x): find and fix what differs | 14 | 1 wk |
| 9 | **CI** (build and tests on every push), **Tasks panel** for long jobs | 14 | 1 wk |
| 10 | **Open / save `.sql` files**, templates, text result mode, copy options, several connections open at once (tabs) | 13 | 2 to 3 wk |
| 11 | **Signing and notarisation** (needs an Apple Developer account), Intel build, Homebrew cask, user documentation | 18 | 1 to 2 wk |

### Nice to have

| Item | Phase |
|---|---|
| **PostgreSQL** and **SQLite** support (database abstraction, introspection, paging, export) | 15 |
| **SQL Server** support | 15 |
| Command palette (Cmd+K), customisable shortcuts, a gallery of UI themes and editor colour themes | 12 / 13 |
| Charsets and Engines pages, Replication status, server "Settings" page (phpMyAdmin tabs we do not have) | 11f |
| Database Operations: rename (by copy), change charset and collation, copy database, Print and Data dictionary | 11f |
| Schema compare and migration preview, data compare, copy a database or table between connections | 16 |
| Data Search across tables, query builder, whole-database ER diagram | 16 / 17 |
| Browser leftovers: sort by name or size, drag a name into the editor, Info panel, alter database, rename routines | 13 |
| Column virtualization and a page cache for very wide or very deep grids; segmented exports | 10b |
| Charts for server status, an "Execute and edit result set" grid for single-table SELECTs | 16 |
| JSON / XML / XLSX import, saved and re-runnable transfer configurations | 16 |
| SQL inspections (unqualified columns, unused aliases), index advisor | 16 |
| Optional backup before a destructive operation, row limits, sensitive-column masking, audit log beyond history | 9b |

### Optional

| Item |
|---|
| **Oracle** support (needs the Oracle Instant Client, which cannot be bundled; likely "bring your own", like Ghidra in other tools) |
| Scheduler, job agent, scheduled backups, reports by email |
| AI assistant, MCP server, plugin API |
| Mock data generator, Git integration, two-way sync |
| UI translations (phpMyAdmin has a language picker) |
| Replication topology views, multi-window workspaces |

---

## 4. Proposed order

| Phase | Theme | What | Effort |
|---|---|---|---|
| 11 | **Admin console** (phpMyAdmin parity) | 11a dashboard · 11b Databases page and bulk drop · 11c Structure page and With selected · 11d Preview SQL then run · 11e FK-check switches and previews · 11f Operations, Charsets, Engines, Replication | 3 to 4 wk |
| 12 | **Shell and preferences** | top menu bar and toolbar, Preferences (fonts per area, themes), command palette | 2 to 3 wk |
| 13 | **Workspace parity** | `.sql` files, templates, text results, connection tabs and groups, browser leftovers | 2 to 3 wk |
| 14 | **Quality gates** | CI, Tasks panel, MySQL 5.7 / 8.0 / 8.4 / 9 and MariaDB matrix, network-drop tests | 1 to 2 wk |
| 15 | **Database engines** | driver abstraction; PostgreSQL, SQLite, SQL Server; Oracle only on demand | 8 to 12 wk |
| 16 | **Power tools** | compare and sync, Data Search, import formats, charts, inspections | 4 to 5 wk |
| 17 | **Visual tools** | query builder, whole-database ER designer | 4 to 5 wk |
| 18 | **Beta release** | signing, Intel build, Homebrew cask, documentation | 1 to 2 wk |

Why this order: phase 11 is what you asked for next and what makes MyLoDB usable the way phpMyAdmin is. Phase 14 sits before the extra engines
on purpose: the version matrix finds MariaDB and older-MySQL differences while the codebase still has one engine, and the abstraction in
phase 15 is much safer with CI behind it. If a release matters sooner than new engines, do 14 and 18 right after 12.
