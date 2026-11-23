# MyLoDB roadmap: what is done, what must be done, what would be nice

Status of 2026-11-23 (version 0.4.0). This is the single page for "where are we": phases, what is done, what is left, and (sections 5 to 8) the
coverage of the SQLyog reference and the product plan. It replaces the former `PLAN_VS_CURRENT.md`. Other docs: `ANALYSIS.md` (original
scope, stack, architecture), `BENCHMARKS.md` (Phase 10 measurements), `PHPMYADMIN_AND_ENGINES.md` (analysis behind phases 11 to 15),
`SERVER_COMPATIBILITY.md` (MySQL / MariaDB results).

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
| 13 (earlier part) | **Workspace**: several connections open at once (tabs above the workspace, switching keeps sessions, closing disconnects one); **text result mode** (mysql-client style) and **Copy as** TSV / CSV / JSON / Markdown / INSERT; **SQL snippets** in ⌘K; tables **sorted by size** in the tree; open / save `.sql` files | ✅ |
| 12b | **Shell redesign (DBeaver style)**: top toolbar (Home, New query, Databases, Transfer, Users, Server, theme, preferences), breadcrumb footer (connection › database › table, clickable), **Preview** button beside Run, distinct icons; DBeaver-style native menus (File with Open / Save SQL file, Import, Export; Edit with Format; Search; SQL Editor; Database with Disconnect); Preferences as a filterable page tree | ✅ |
| 12 (all) | **Phase 12 complete**: editor colour schemes (Default, Multi-colour, Solarized, High contrast, previewed live) and a **command palette** (⌘K: every action and table). Plus the **native menu bar** (MyLoDB, File, Edit, View, Database, Tools, Window, Help) and **Preferences** (⌘,): theme, font family / size / bold per area (interface, sidebar, data grid, editor), editor line height, live preview, saved and clamped | ✅ |
| 11 (all) | **Phase 11 complete**: Operations (rename by copy, copy, collation), Data dictionary (print), Engines / Charsets / Replication page (`SHOW REPLICA STATUS` with `SLAVE` fallback) | ✅ |
| 13 (all but leftovers) | **Workspace parity complete**: several connections open at once, text results and Copy as, SQL snippets and a personal snippet library, open / save `.sql`, connection groups (folders in the picker), **drag a database, table or column name from the tree into the editor**, **Alter database** (charset / collation, previewed), **rename stored procedures and functions** (create under the new name, drop the old, previewed), **Info tab** facts for the object selected in the tree | ✅ |
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

### Mandatory (before a first release, or to replace phpMyAdmin / SQLyog). Only 8, 9 and 11 are still open

| # | Item | Phase | Effort |
|---|---|---|---|
| 1 | ~~Server home dashboard after connect~~ | 11a | ✅ done |
| 2 | ~~Databases page (sizes, check all, bulk drop, create with collation)~~ | 11b | ✅ done |
| 3 | ~~Database Structure page, With selected menu, create table~~ | 11c | ✅ done |
| 4 | ~~Preview SQL then run for every bulk or destructive operation, FK-check switch~~ | 11d | ✅ done |
| 5 | ~~FK-check switch for Run and CSV import, preview before running a SQL file~~ | 11e | ✅ done |
| 6 | ~~Top menu bar and toolbar~~ (native macOS menu; in-window menu for Windows / Linux is nice to have) | 12 | ✅ done |
| 7 | ~~Preferences (fonts per area, line height, editor colour schemes)~~ | 12 | ✅ done |
| 8 | MariaDB 13 and MySQL 9.7 pass by hand; **5.7, 8.0, 8.4, MariaDB 10.11 / 11.4 are in CI and still need a first green run on GitHub** | 14 | 🟡 2 d |
| 9 | ~~CI workflow and Tasks panel~~ written and tested locally; not yet seen running on GitHub | 14 | 🟡 push and check |
| 10 | ~~Open / save `.sql`, text results, copy options, snippets, several connections, connection groups, drag a name into the editor~~ | 13 | ✅ done |
| 11 | ❌ **Signing and notarisation** (needs an Apple Developer account), Intel build, Homebrew cask, user documentation | 18 | 1 to 2 wk |

### Nice to have

| Item | Phase |
|---|---|
| **PostgreSQL** and **SQLite** support (database abstraction, introspection, paging, export) | 15 |
| **SQL Server** support | 15 |
| Flush dialog, SQL-file drag-open, reorder columns by drag in the designer | 13 |
| Customisable shortcuts, a gallery of whole-UI themes, in-window menu bar for Windows / Linux | 12 / 13 |
| Server "Settings" page (phpMyAdmin tab we do not have). Engines, Charsets and Replication are done (Home > Engines, charsets, replication) | 11f |
| Table-level Operations page (move table to another database, table options), Replication writes | 11f |
| Schema compare and migration preview, data compare, copy a database or table between connections | 16 |
| Data Search across tables, query builder, whole-database ER diagram | 16 / 17 |
| Browser leftovers: sort by name / rows, collapse all, keyboard navigation, rename triggers and events (Info panel, alter database, rename procedures / functions, sort by size, drag into editor are done) | 13 |
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
| 13 ✅ | **Workspace parity** (done, small leftovers in the nice-to-have list) | `.sql` files, snippets, text results, connection tabs and groups, Info panel, alter database, drag into editor | 2 to 3 wk |
| 14 🟡 | **Quality gates** (built; first green CI run on GitHub still to confirm) | CI, Tasks panel, MySQL 5.7 / 8.0 / 8.4 / 9 and MariaDB matrix, network-drop tests | 1 to 2 wk |
| 15 🟡 | **Database engines** | done: `myq-core`, the `Backend` trait, SQLite read and write (SQL and grid edits; see ENGINE_ABSTRACTION.md). Open: SQLite table designer, PostgreSQL, SQL Server; Oracle only on demand | 8 to 12 wk |
| 16 🟡 | **Power tools** | done in 0.3.0: schema compare, data search, index advisor, server charts, JSON import. Open: data compare and sync, copying a database between connections, saved transfer configurations, XLSX / XML import, SQL inspections | 4 to 5 wk |
| 17 🟡 | **Visual tools** | done in 0.3.0: visual query builder, whole-database ER diagram (reverse engineering). Open: forward design, i.e. editing the diagram to generate DDL | 4 to 5 wk |
| 18 ❌ | **Beta release** | signing, Intel build, Homebrew cask, documentation | 1 to 2 wk |

Why this order: phase 11 is what you asked for next and what makes MyLoDB usable the way phpMyAdmin is. Phase 14 sits before the extra engines
on purpose: the version matrix finds MariaDB and older-MySQL differences while the codebase still has one engine, and the abstraction in
phase 15 is much safer with CI behind it. If a release matters sooner than new engines, do 14 and 18 next: phases 1 to 13 are finished, so the remaining release blockers are only the first green CI run, signing and notarisation, the Intel build, the Homebrew cask and the user documentation.

---

## 5. Object browser (left sidebar), against the original brief

| Requirement | Status |
|---|---|
| `user@host` is the root node; clicking it resets to the full database list | ✅ |
| Level 1: filter databases; level 2: click a database and the box locks to it (chip) and filters its tables; level 3: lock icon on a table and search its columns (name or type) | ✅ Lock released with ✕, Backspace or Esc |
| Filter options (tables only by default; views, routines, triggers, events, columns switchable, remembered); filtering happens in the backend index | ✅ |
| Column types like DBeaver, 🔑 primary, ◆ unique, table and database sizes, indexes under tables | ✅ |
| Views, procedures, functions, triggers, events: menus, definitions as runnable scripts, drop, templates; rename of procedures and functions | ✅ (rename of triggers and events open) |
| Alter database (charset / collation); Info panel for the selected object; drag a name into the editor; sort tables by size | ✅ |
| DBeaver-style context menus (icons, sections, submenus, shortcuts F2 / F4 / F5) | ✅ |
| No hang on big servers (names at connect, paged groups, per-database size query). 90,000 objects: connect 0.3 to 0.6 s, slowest keystroke about 0.4 s | ✅ |
| Table views: Properties, Data (console above grid) and a per-table Diagram | ✅ |
| Open: collapse all, keyboard navigation, sort by name / rows, copy a database to another host, schema documentation, flush dialog | ❌ |

MySQL 8 caches table statistics (`information_schema_stats_expiry`, 24 h), so sizes and row counts are estimates; `ANALYZE TABLE` refreshes them.

## 6. Coverage of the SQLyog reference

| SQLyog area | MyLoDB | Notes |
|---|---|---|
| Main layout, themes | ✅ | Light, dark, system; flat IDE look |
| Multiple connections at once | ✅ | Tabs; groups as folders |
| Connections: TCP, SSH, SSL, URL | ✅ | Keychain; fail-closed SSH host key. ❌ HTTP tunnel (not planned), LDAP / PAM |
| SQL editor: tabs, autocomplete, formatter, favorites, history, snippets, open / save `.sql` | ✅ | ❌ drag-open of files |
| Execute selection / all, cancel, multi-result tabs, text mode, Copy as | ✅ | ❌ "Execute and edit result set" |
| Table data editing (staged, SQL preview, FK picker, BLOB viewer) | ✅ | Beyond SQLyog |
| Table designer, rename, duplicate, truncate, drop | ✅ | ❌ reorder columns by drag |
| Views, routines, triggers, events | ✅ | Through the SQL editor; no form-based routine editor planned |
| Import CSV; export CSV, JSON, JSON Lines, SQL, XML, HTML, XLSX, gzip | ✅ | ❌ JSON / XML / XLSX import, ODBC |
| Backup and restore | ✅ | Progress, cancel, checksums |
| User and privilege manager | ✅ | Server, database, table, column scope |
| Process list, status, variables, maintenance, logs | ✅ | ❌ flush |
| Info tab, profiler, messages, history | ✅ | Profiler is `SHOW PROFILE` based; a `performance_schema` path is open |
| Preferences, command palette, native menus | ✅ | Customisable shortcuts open |
| Query Builder | ❌ | Phase 17 |
| Whole-database Schema Designer / ER | 🟡 | Per-table diagram only; Phase 17 |
| Data Search across tables | ❌ | Phase 16 |
| Database copy, schema sync, data compare | ❌ | Phase 16; `alter_table_sql` is the base to reuse |
| SQL Scheduler, scheduled backups, Job Agent | ❌ | Optional |

## 7. Coverage of the product plan

| Plan item | Status | Notes |
|---|---|---|
| Streaming, virtualized grid | ✅ rows / 🟡 columns | Windowed rows; no column virtualization or page cache yet |
| Smart pagination | ✅ | Keyset paging 350 to 1,100 times faster than OFFSET at 10M rows; estimated count with cancellable exact count |
| Large export | ✅ / 🟡 | Constant memory, JSON Lines, gzip, progress, cancel; no segmented export |
| Production safe mode | ✅ | See section 8. Open: backup before destructive operation, row limits, audit log |
| Staged editing with SQL preview, query cancel, secure connections | ✅ | |
| Autocomplete, formatter, EXPLAIN / ANALYZE | ✅ | |
| SQL inspections, index advisor, schema compare, data compare | ❌ | Phase 16 |
| JSON / BLOB / record views | 🟡 | JSON pretty-printed, not a searchable tree |
| Import / export wizard | 🟡 | No JSON / XML / XLSX import, no saved configurations |
| ER designer, query builder, charts, mock data, AI, MCP, plugins, job agent, CLI | ❌ | |
| Crate layout | 🟡 | No `myq-core`, `myq-stream`, `myq-safety`, `myq-agent`; `myq-transfer` is `myq-io`, `myq-profile` is `myq-profiler` |
| Long-running task model | ✅ | Backup, restore, import, exports: task ids, progress, cancel; Tasks panel in the footer |
| Tests | ✅ / 🟡 | About 170 Rust tests (MySQL 9.7 and MariaDB 13.0 by hand, a real `sshd`), 21 UI suites, 10M-row benchmarks, connection-loss test. Version matrix in CI not yet run on GitHub |
| CI | 🟡 | `.github/workflows/ci.yml` written, not yet seen running |
| Release | 🟡 | Unsigned arm64 `.dmg`. ❌ signing, notarisation, Intel build, Homebrew cask, user docs |

## 8. Production Safety (Phase 9), what it does and does not do

Per connection (Safety tab): environment (development / staging / production), read-only, query timeout, ask before destructive SQL.
* **Environment** shows as a badge, a colour band on the tab bar and a tinted status bar.
* **Read-only is enforced by the server** (`SET SESSION TRANSACTION READ ONLY`, re-applied when the pool recycles a connection; error 1792), and the app also refuses writes, session switches back to read-write and every writing backend command; the UI disables the write entries.
* **Review before running** (`review_sql`): DROP / TRUNCATE, DELETE or UPDATE without a real WHERE, ALTER … DROP, DROP USER, GRANT / REVOKE, SET GLOBAL, RENAME, KILL, REPLACE, with the optimizer's row estimate from `EXPLAIN`. On production, typing "production" is required.
* **Query timeout** stops a run with `KILL QUERY`; **confirm on commit** on production.

Limits, so nobody over-trusts it: read-only is a session setting (use a database account without write privileges for real protection); the review is advisory and does not understand dynamic SQL; the timeout is client-side per run; not built: backup before a destructive operation, row limits, sensitive-column controls, audit log beyond history.
