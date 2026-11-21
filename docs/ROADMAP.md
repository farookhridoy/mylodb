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
| 14 (part) | **Quality gates**: the whole Rust suite now passes on **MySQL 9.7 and MariaDB 13.0 (166 tests each)**; running it on MariaDB found and fixed 8 real bugs (`docs/SERVER_COMPATIBILITY.md`); **GitHub Actions CI** (Rust on MySQL 8.0 / 8.4 / 9 and MariaDB 10.11 / 11.4, 5.7 non-blocking; UI suites; macOS desktop build check); **Tasks panel** in the footer (progress, Cancel, outcome); connection-loss test | ✅ (CI not yet seen running on GitHub) |
| 13 (part) | **Workspace**: several connections open at once (tabs above the workspace, switching keeps sessions, closing disconnects one); **text result mode** (mysql-client style) and **Copy as** TSV / CSV / JSON / Markdown / INSERT; **SQL snippets** in ⌘K; tables **sorted by size** in the tree; open / save `.sql` files | ✅ |
| 12b | **Shell redesign (DBeaver style)**: top toolbar (Home, New query, Databases, Transfer, Users, Server, theme, preferences), breadcrumb footer (connection › database › table, clickable), **Preview** button beside Run, distinct icons; DBeaver-style native menus (File with Open / Save SQL file, Import, Export; Edit with Format; Search; SQL Editor; Database with Disconnect); Preferences as a filterable page tree | ✅ |
| 12 (all) | **Phase 12 complete**: editor colour schemes (Default, Multi-colour, Solarized, High contrast, previewed live) and a **command palette** (⌘K: every action and table). Plus the **native menu bar** (MyLoDB, File, Edit, View, Database, Tools, Window, Help) and **Preferences** (⌘,): theme, font family / size / bold per area (interface, sidebar, data grid, editor), editor line height, live preview, saved and clamped | ✅ |
| 11 (all) | **Phase 11 complete**: Operations (rename by copy, copy, collation), Data dictionary (print), Engines / Charsets / Replication page (`SHOW REPLICA STATUS` with `SLAVE` fallback) | ✅ |
| extra | Light / dark / system theme; flat IDE look with one icon set; searchable dropdowns (select2 style); DBeaver-style context menus; lock to a database or table in the browser; table Properties / Data / Diagram views; console above data on table tabs; result tabs (Results, Profile, Messages, Table Data, Info, History); connect by host or URL; "Show all databases"; resizable sidebar and console | ✅ |

Tests: about 170 Rust tests (many against a real MySQL 9.7) and 21 UI suites. Unsigned arm64 `.dmg` builds and installs.

What Phase 10 did **not** cover (honest list): column virtualization for very wide tables, a multi-page cache or read-ahead, segmented exports,
MySQL 5.7 / 8.0 / 8.4 / MariaDB runs, network-drop tests, measuring the real Tauri window (only headless Chrome with a mock was measured).

---

## 2. Your phpMyAdmin / DBeaver / SQLyog list, item by item

| You asked for | Today | Plan |
|---|---|---|
| A **dashboard after connecting** (server info, collation, theme, like phpMyAdmin's home) | ✅ Home page opens on a fresh connect (version, account, uptime, SSL, collation, links); connection collation is a connection setting | done (11a) |
| **List all databases**, select all, **drop** the selected | ✅ Databases page (database menu > Databases) | done (11b) |
| **Go inside a database and see all tables**, choose some and drop them (the "Structure" tab: rows, engine, collation, size, overhead, Browse / Structure / Search / Insert / Empty / Drop, "With selected": copy, show create, export, empty, drop, analyze, check, checksum, optimize, repair, add / replace prefix, copy with prefix, "containing the word" filter, create table) | ✅ Structure page (database menu > Structure), with Operations (rename, copy, collation) and a printable Data dictionary | done (11c, 11f) |
| After clicking Drop, **show the descriptive SQL** before it runs (`DROP TABLE a, b`) | ✅ one shared Preview SQL dialog for drops, empties, renames, copies, prefix operations | done (11d) |
| **Foreign key checks on / off** when importing and when running SQL | ✅ FK checks selector on the console's Run (per tab), CSV import switch, Restore and Run SQL file switch; Preview script shows counts and the first statements before a SQL file runs | done (11e) |
| **Preview the SQL** before the final run (phpMyAdmin style), run with and without FK checks | ✅ drops, renames, copies and bulk operations preview their SQL; SQL files can be previewed | done (11d / 11e) |
| **Change the theme** | 🟡 Light / Dark / System, also from Preferences and View menu. No gallery of named themes | nice to have (12) |
| **Preferences**: font size, family, bold, per area (sidebar, data grid, query), multi-colour query highlighting | ✅ Preferences with fonts per area and four editor colour schemes | done (12) |
| A **navbar on top** like DBeaver or SQLyog (menus and a toolbar) | ✅ native menu bar (File, Edit, View, Database, Tools, Window, Help). In-window menu bar for Windows / Linux is not built | done on macOS (12) |
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
| 8 | ~~MariaDB and MySQL matrix~~ MariaDB 13 and MySQL 9.7 done by hand; 5.7, 8.0, 8.4, MariaDB 10.11 / 11.4 are in CI and still need a first green run | 14 | 2 d |
| 9 | ~~CI and Tasks panel~~ written and tested locally; CI has not run on GitHub yet | 14 | done |
| 10 | ~~Open / save `.sql` files, text results, copy options, snippets, several connections~~ done. Done since: saved snippet library of your own (⌘K: save editor text, insert, delete). Connection groups (folders in the picker) done. Left: drag a name into the editor | 13 | 1 wk |
| 11 | **Signing and notarisation** (needs an Apple Developer account), Intel build, Homebrew cask, user documentation | 18 | 1 to 2 wk |

### Nice to have

| Item | Phase |
|---|---|
| **PostgreSQL** and **SQLite** support (database abstraction, introspection, paging, export) | 15 |
| **SQL Server** support | 15 |
| Customisable shortcuts, a gallery of whole-UI themes, in-window menu bar for Windows / Linux | 12 / 13 |
| Server "Settings" page (phpMyAdmin tab we do not have). Engines, Charsets and Replication are done (Home > Engines, charsets, replication) | 11f |
| Table-level Operations page (move table to another database, table options), Replication writes | 11f |
| Schema compare and migration preview, data compare, copy a database or table between connections | 16 |
| Data Search across tables, query builder, whole-database ER diagram | 16 / 17 |
| Browser leftovers: sort by name or size, (Info panel, alter database and rename routines done) | 13 |
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
| 11 ✅ | **Admin console** (phpMyAdmin parity), finished | 11a dashboard · 11b Databases page and bulk drop · 11c Structure page and With selected · 11d Preview SQL then run · 11e FK-check switches and previews · 11f Operations, Charsets, Engines, Replication | 3 to 4 wk |
| 12 ✅ | **Shell and preferences** (done) | top menu bar and toolbar, Preferences (fonts per area, themes), command palette | 2 to 3 wk |
| 13 | **Workspace parity** | `.sql` files, templates, text results, connection tabs and groups, browser leftovers | 2 to 3 wk |
| 14 ✅ | **Quality gates** (done, see above) | CI, Tasks panel, MySQL 5.7 / 8.0 / 8.4 / 9 and MariaDB matrix, network-drop tests | 1 to 2 wk |
| 15 | **Database engines** | driver abstraction; PostgreSQL, SQLite, SQL Server; Oracle only on demand | 8 to 12 wk |
| 16 | **Power tools** | compare and sync, Data Search, import formats, charts, inspections | 4 to 5 wk |
| 17 | **Visual tools** | query builder, whole-database ER designer | 4 to 5 wk |
| 18 | **Beta release** | signing, Intel build, Homebrew cask, documentation | 1 to 2 wk |

Why this order: phase 11 is what you asked for next and what makes MyLoDB usable the way phpMyAdmin is. Phase 14 sits before the extra engines
on purpose: the version matrix finds MariaDB and older-MySQL differences while the codebase still has one engine, and the abstraction in
phase 15 is much safer with CI behind it. If a release matters sooner than new engines, do 14 and 18 right after 12.
