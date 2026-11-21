# MyLoDB: plan versus what exists, and the next phases

Compared on 2026-11-20 against two documents in this folder, checked against the code (not from memory):

* `MyLoDB_Final_Product_and_Technical_Plan.md`, the product plan.
* `SQLyog_Functional_UI_Snapshot_with_Images.md`, the SQLyog functional reference (the "coverage checklist").

✅ done · 🟡 partly · ❌ not started

## 1. Summary

MyLoDB covers the **daily-driver core** of SQLyog: connections (SSH, SSL), object browser, SQL editor, editable grid, table designer, users,
server tools, backup, restore and import/export, all tested against a real MySQL. The gaps fall into three groups:

1. **The two features the plan calls defining are not built**: Production Safety and the Large Data Engine.
2. **SQLyog's "complete manager" surface is thin**: there is no `.sql` file handling, no Data Search, no Query Builder, no Schema Designer, no compare/sync, no Preferences.
3. **Distribution and quality gates**: no CI, no database-version matrix, no signing.

## 2. Object browser (the left sidebar), against your brief

What you asked for, and where it stands after this change:

| Requirement | Status |
|---|---|
| `root@localhost` is the root node of the tree; clicking it **resets** to the full database list | ✅ the connection is the root (`user@host`, with the database count). Click = no selection, filter cleared, everything collapsed. |
| **Level 1: filter databases** (nothing selected) | ✅ "Filter databases", names only. |
| **Level 2: click a database → the box locks to it and filters its tables** | ✅ Like SQLyog: the database becomes a chip in the filter box, only that database is listed, its tables open, and the box searches them. Clearing the text lists all its tables again. ✕ on the chip (or Backspace / Esc in the empty box) releases the lock. |
| **Level 3: lock a table and search its columns** | ✅ Clicking a table only selects it (the table search keeps its results). The **lock icon** on a table row locks the filter to it: a second chip, only that table shown, the box searches its columns. Click the icon again, or ✕, to unlock. | Matches column **name or type** (`email`, `bigint`, `char*`), entirely in the browser. |
| Columns show type like DBeaver (`bigint unsigned`, `char(255)`) | ✅ right-aligned declared type (`COLUMN_TYPE`), 🔑 for primary key, ◆ for unique. Nullability, `auto_increment` and the full definition are in the tooltip. |
| Table size in KB / MB / GB | ✅ data + index bytes, right-aligned on each table row; tooltip has exact bytes and the row estimate. Views show none. |
| No hang on big servers | ✅ names only at connect; per-database size query (one round trip); per-table column query; groups paged 300 rows at a time. 90,000-object fake server: connect 0.3–0.6 s, about 600 rows on screen, slowest keystroke about 350–430 ms. |

Added in Phase 8: an `Indexes` list under each table (name, columns, 🔑 primary, ◆ unique), total size on each database row, a context menu
and double-click for **views, procedures, functions, triggers and events** (show / edit definition, drop with typed confirmation), "New view /
procedure / function / trigger / event" templates in the database menu, expanded nodes and selection remembered per connection, F5 to refresh.

"Show / edit definition" opens a script you can run as it is: views as `CREATE OR REPLACE VIEW`, routines, triggers and events as
`DROP … IF EXISTS` + the server's own `CREATE` wrapped in `DELIMITER $$`. The tab is opened on the object's database, because the server
returns unqualified names. A real-MySQL test runs these scripts for every kind and checks the object exists afterwards.

Still open in the browser (carried over to Phase 11): sort by name / size / rows, an Info panel for the selected object, drag a table or column
into the editor, alter database (charset / collation), keyboard navigation, collapse all, and routine / trigger / event *rename*.

Note on sizes: MySQL 8 caches table statistics (`information_schema_stats_expiry`, 24 h by default), so sizes and row counts are estimates;
`ANALYZE TABLE` refreshes them. The tooltip says so.

## 3. Coverage of the SQLyog reference (section by section)

| SQLyog area | MyLoDB | Notes |
|---|---|---|
| Main layout: browser, SQL window, results | ✅ | Light, dark and system themes. |
| Multiple connections at once, one tab each | 🟡 | One connection is active at a time (switching disconnects). Plan: connection tabs. |
| Connection options: TCP, SSH, SSL | ✅ | Keychain, CA and client certificates, fail-closed SSH host-key policy. ❌ HTTP tunnel (not planned), ❌ LDAP/PAM. |
| Object browser lazy metadata | ✅ | Lazy by database and by table. |
| Browser context menus for tables | ✅ | Open, design, select, import CSV, duplicate, rename, truncate, drop, copy name. |
| Browser menus for views, procedures, functions, triggers, events | ✅ | Show / edit definition, drop, new-object templates. Edits are made in the SQL editor (the script is runnable). A form-based routine editor is not planned. |
| Browser: refresh, create database, drop database, backup | ✅ | ❌ alter database (charset), ❌ copy database to another host, ❌ schema documentation, ❌ flush dialog. |
| SQL editor: tabs, autocomplete, formatter, favorites, history | ✅ | |
| SQL editor: open / save `.sql` files, drag-open, templates | ❌ | |
| Execute current statement, selection, all; cancel | ✅ | ⌘↵ runs the selection or current statement; scripts run all. "Execute and edit result set" is ❌ (results are read-only). |
| Multiple statements → separate result tabs | ✅ | |
| Results: grid and form | ✅ | ❌ text mode, ❌ copy with delimiter / quoting options. |
| Result paging | 🟡 | Server paging; no virtualization yet (see Large Data Engine). |
| Table data editing, staged, SQL preview, FK picker, BLOB viewer | ✅ | Beyond SQLyog: staged changes with exact SQL preview. |
| Table designer: columns, indexes, FKs, preview DDL | ✅ | Beyond SQLyog: previews before applying. ❌ reorder columns by drag, ❌ engine-specific advanced properties. |
| Rename, truncate, drop table | ✅ | Typed confirmation for destructive ones. |
| Views, routines, triggers, events: browse, create, alter | ✅ | Closed in Phase 8, through the SQL editor. |
| Query Builder | ❌ | |
| Schema Designer / ER diagram | 🟡 | Per-table Diagram view (the table and its direct neighbours, read-only). A whole-database ER designer is still open. |
| Data Search across tables | ❌ | |
| Import: CSV | ✅ | ❌ ODBC (not planned). |
| Export: CSV, JSON, SQL, XML, HTML, XLSX | ✅ | Constant-memory writers. |
| Backup to SQL dump and restore | ✅ | Progress, cancel, checksums in tests. |
| Database copy, schema sync, data compare | ❌ | |
| SQL Scheduler, scheduled backups, Job Agent | ❌ | Later. |
| User and privilege manager | ✅ | Server, database, table and column scope. |
| Process list, status, variables, flush | 🟡 | ✅ process list (kill), status, variables, maintenance, logs. ❌ flush. |
| Favorites, History | ✅ | |
| Info tab (details of the selected object) | 🟡 | Tooltips show sizes, row estimates and column definitions; a real Info panel is open. |
| Profiler, messages | ✅ | `SHOW PROFILE` based. |
| Preferences dialog | ❌ | Only theme (toggle) and a few per-feature settings. |
| Keyboard shortcuts (mac-native) | 🟡 | ⌘↵, ⌘T, ⌘W, ⇧⌘F, ⇧⌘B. No command palette. |

## 4. Coverage of the product plan

### P0 "must have"

| Plan item | Status | What exists / what is missing |
|---|---|---|
| Streaming, virtualized data grid | 🟡 | Server-side paging (up to 5,000 rows a page) with filters, sort and edit. Every row of the page becomes DOM: no row or column virtualization, no fetch-on-scroll, no bounded cache. |
| Smart pagination | 🟡 | `LIMIT … OFFSET` only. No keyset / primary-key cursor, so deep pages slow down. Row count is an exact `COUNT(*)` in the background; should start from an estimate. |
| Large dataset export | 🟡 | ✅ Constant-memory writers, six formats. ❌ JSON Lines, GZIP, fetch-size option, rows/bytes/elapsed progress and **cancel for exports started from the grid or a query**, segmented export. |
| Production safe mode | ❌ | Missing: environment label and badge, read-only mode enforced by the server, destructive-SQL review (`DROP`, `TRUNCATE`, `DELETE`/`UPDATE` without a real `WHERE`), query timeout, confirm-before-commit. |
| Staged editing and SQL preview | ✅ | Gap: query results are read-only. |
| Query cancellation | ✅ | `KILL QUERY`. |
| Secure connection management | ✅ | |

### P1 and later

| Plan item | Status | Notes |
|---|---|---|
| SQL autocomplete | ✅ | Alias-aware, db-qualified, quoting, lazy columns. |
| SQL inspections | ❌ | |
| Formatter, EXPLAIN, EXPLAIN ANALYZE | ✅ | |
| Profiler | 🟡 | Add a `performance_schema` path (the plan says not to depend on `SHOW PROFILE`). |
| JSON, BLOB, record views | 🟡 | ✅ record view, hex/image/text. JSON is pretty-printed, not a searchable tree. |
| Schema compare and migration preview, data compare | ❌ | `alter_table_sql` (single-table diff) is the base to reuse. |
| Import/export wizard | 🟡 | ❌ JSON/XML/XLSX import, saved re-runnable configurations. |
| SQL history and Git | 🟡 | ✅ history, favorites, session restore. ❌ `.sql` files, local history recovery, Git. |
| Diagnostics | 🟡 | ❌ redundant and unused index detection, recommendations, column statistics. |
| ER designer, query builder, charts, mock data, AI, MCP, plugins, job agent, CLI, two-way sync | ❌ | |

### Architecture, tasks, tests, release

| Item | Status | Notes |
|---|---|---|
| Crate layout | 🟡 | No `myq-core`, `myq-stream`, `myq-safety`, `myq-agent`. `myq-transfer` ≈ `myq-io`, `myq-profile` ≈ `myq-profiler`. |
| Long-running task model | 🟡 | Backup, restore, import: task ids, progress, cancel. Queries and grid exports: no. **No Tasks panel.** |
| Tests | 🟡 | ✅ about 125 Rust tests incl. real MySQL 9.7 and a real `sshd`; about 110 UI checks. ❌ MySQL 5.7 / 8.0 / MariaDB matrix, 100k / 1M / 10M-row benchmarks, network-drop tests. |
| CI | ❌ | No workflows. |
| Release | 🟡 | ✅ unsigned arm64 `.dmg`. ❌ signing and notarisation, Intel build, Homebrew cask, user docs. |

## 5. Next phases

Phases 1–8 and the filter, theme and sidebar work are done. Rough effort for one developer; each phase ends with tests and a DMG.

| Phase | Theme | Scope | Effort |
|---|---|---|---|
| **8** ✅ | **Object browser completion** | Done: indexes under tables, database size, views / routines / triggers / events (definition, drop, templates), state remembered per connection, F5. Moved to Phase 11: sort, Info panel, drag into editor, alter database, collapse all, rename of routines. | done |
| **9** | **Production Safety** | Environment label and badge (prod / staging / dev); server-enforced read-only; destructive-SQL review; query timeout; confirm-on-commit for production. | 2–3 wk |
| **10** | **Large Data Engine** | Virtualized grid; lazy fetch with a bounded cache; keyset pagination; estimated counts; exports as cancellable tasks (progress, JSON Lines, GZIP); 1M and 10M-row benchmarks. | 3–4 wk |
| **11** | **SQL workspace parity** | Browser leftovers (sort, Info panel, drag into editor, alter database); open / save `.sql` (new tab or current), drag-open, templates; text result mode; copy with options; **Preferences** dialog; command palette (⌘K); connection tabs (several servers at once); connection groups and environments. | 2–3 wk |
| **12** | **Quality gates** | Tasks panel; GitHub Actions on macOS arm64 / x64; MySQL 5.7 / 8.0 / 9 and MariaDB in containers; network-drop and restart tests. | 1–2 wk |
| **13** | **Power tools** | Data Search; schema compare with migration preview; data compare; copy database / table between connections; schema documentation (HTML). | 4–5 wk |
| **14** | **Visual tools** | Query Builder (canvas, grid, SQL preview, round trip); ER / Schema Designer (read-only first, then migration-plan workflow, never live-edit). | 4–5 wk |
| **15** | **Beta release** | Signing and notarisation (needs an Apple Developer account), Intel build, Homebrew cask, user documentation. | 1–2 wk |
| Later | Automation and AI | Job agent, scheduler and reports, scheduled backups, SQL inspections, index advisor, MCP server, AI assistant, plugin API. | |

**Why this order.** Phase 8 is done. Phases 9 and 10 are the plan's
"defining" features and must land before the visual tools. Phase 11 removes daily friction. Phase 12 should run alongside 9–11 so the
version matrix catches MariaDB and 5.7 differences before the power tools depend on them.

## 6c. Filter options and menus

* **Filter icon**: choose what a search inside a locked database looks at: Tables, Views, Procedures and functions, Triggers, Events, Columns.
  Columns are **off** by default, so `supplier_` lists tables instead of every column named so; switch it on to search columns as well. The
  choice is remembered; the icon is highlighted when it differs from the default. A query that names kinds itself (`col:`, `type:`) still works.
  The restriction is applied in the backend (`filter_index` takes the kinds), so filter results stay positions in the one shared index.
* **Fixed: the arrow could not close a table opened by a search.** Search-opened groups and tables had no way to close. Arrows now close and
  reopen them, until the next search.
* **Context menus** follow the DBeaver layout: icons, grouped sections, shortcuts (F4, F2, F5 work), submenus that flip to stay on screen,
  dangerous entries in red. Table menu: Create ▸, View Table, Design table…, lock the filter, View Diagram, View Data, Export Data…, Import
  Data…, Tools ▸ (Dump table, Analyze, Check, Optimize, Repair, Truncate, Server tools), Generate SQL ▸ (SELECT, INSERT, UPDATE, DELETE,
  UPSERT, DDL), Read data in SQL console, Copy name, Copy Advanced Info, Duplicate, Rename, Drop, Refresh. Database, view and routine menus got
  the same treatment. Compare/Migrate is not offered because it does not exist yet.
* Checkboxes were text-field sized everywhere; fixed.

## 6b. Table views and the stacking bug

* **Three views per table** (the DBeaver idea): **Properties**, **Data** (console on top, grid below; the default) and **Diagram**. Each table tab
  remembers its own view.
* **Properties**: table name, engine, collation, charset, auto increment, comment, partitioned; sections Columns, Constraints, Foreign Keys
  (with ON DELETE / ON UPDATE), References (tables that point at this one), Triggers, Indexes, Partitions, Statistics and DDL (copy, or open in
  the console). Read-only; "Design table…" opens the designer, which previews the SQL before applying anything.
* **Diagram**: the table in the middle, tables that reference it on the left, tables it references on the right, with the relation drawn between
  the actual columns. Drag boxes, zoom, reset layout, double-click a box to open that table. Up to 12 neighbours per side.
* **Backend**: one `table_overview` call (information_schema only, tested on a real server with self-references, triggers, partitions, views).
* **Fixed: dialogs under other layers.** The modal overlay had no z-index, so anything with its own (the sidebar splitter, sticky grid headers,
  popups) was painted over the "New connection" dialog. Overlays are now above every in-page layer, with a test that fails without the fix.
* **Fixed: grid missing after returning to Data** from Properties/Diagram.

## 6a. Done after that: the SQLyog-style workspace

* **Table tab = console on top, data below** (like SQLyog). Opening a table gives a SQL console prefilled with `SELECT * FROM … LIMIT 1000;`
  and a **Table Data** tab with the editable grid. A query run in the console adds Result tabs beside it; Table Data stays one click away.
* **Resizable**: the sidebar (drag its edge, 200–640 px) and the console / results divider (drag, double-click resets). Both are remembered.
* **Arrows** (chevrons) open and close a database, the Tables list and a table's columns. Clicking the row selects; it no longer toggles.
* **Locked database = the console's database**: selecting a database sets it on the console, and new query tabs start in it.
* **Click a name to paste it into the query**: a database, table (`users`, or `db`.`users` outside the console's database) or column
  (`users`.`name`). Only while the editor had the focus, so browsing never changes a query.
* **Sample queries** in the table menu: SELECT, INSERT, UPDATE, DELETE with the real column names (auto-increment columns left out of INSERT,
  the primary key in WHERE). Inserted into the console and copied.
* **Fixed at the root**: autocomplete loaded columns into the same list the filter results index into, so the UI's copy could drift from the
  backend's. Autocomplete now keeps its columns in its own catalog (`completion_nodes`); the filter index only grows through `load_columns`.

## 6. Done after Phase 8: lock behaviour and a flatter look

* **Lock** (your report: searching databases, then clicking one, showed all databases again): selecting a database now locks the browser to it,
  selecting a table locks to the table. See section 2.
* **Double-click fix**: a table click is applied after 220 ms so that a double-click (open data) still hits the row; otherwise the first click
  locks the browser and the row moves away from the cursor.
* **Look**: the emoji icons are replaced by one set of flat line icons (`Icon.tsx`); neutral IDE-style palette (DataGrip-like dark and light),
  12 px type, 22 px rows, rectangular controls, underlined active tabs, plain counts instead of pill badges, borderless toolbar icons.

## 6b. Done in Phase 8

* Views, procedures, functions, triggers and events: menus, definitions as runnable scripts, drop, templates.
* Indexes under tables, database sizes, per-connection memory of expansion and selection, F5.
* New backend: `index_details`, `object_definition`, `drop_object_sql` (tested on a real server), `table_index_details`, `object_definition`, `drop_object` commands.

## 7. Done earlier

* Object browser: connection root node with reset, three-level filter (databases → tables → columns), column types, table sizes.
* New backend queries `table_stats` (one per database) and `column_details` (one per table), with a real-MySQL test.
* Earlier in the same effort: scoped, lazy, paged tree (no hang on big servers); light, dark and system themes.
