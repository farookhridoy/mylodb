# phpMyAdmin-style administration, shell and preferences, and more database engines

Analysis of 2026-11-21 for phases 11 to 15 of `ROADMAP.md`. It starts from the phpMyAdmin screens you sent (home, Databases,
database Structure with the "With selected" menu, theme picker), your DBeaver and SQLyog references, and the list of engines you want.
Everything here is a design, not a claim that it exists; the status of each part is in `ROADMAP.md`.

---

## 1. phpMyAdmin screen by screen

### 1.1 Home dashboard (screen 1)

phpMyAdmin shows four panels: *General settings* (change password, server connection collation), *Appearance* (language, theme),
*Database server* (server, type, SSL, version, protocol, user, charset) and its own web-server / version panels (not relevant to a desktop app).
Its top row is the server-level tab bar: Databases, SQL, Status, User accounts, Export, Import, Settings, Replication, Variables, Charsets, Engines.

| phpMyAdmin tab | MyLoDB today | Plan |
|---|---|---|
| Databases | tree + single create / drop | 11b: Databases page |
| SQL | SQL console tabs ✅ | |
| Status | Server tools → Status ✅ | |
| User accounts | Users and privileges ✅ | |
| Export | Backup / Export tab ✅ | |
| Import | CSV import, Run SQL file, Restore ✅ | 11e: FK switch and preview everywhere |
| Settings | ❌ | 12: Preferences |
| Replication | binary logs and binlog events ✅ (partly); replica status ❌ | 11f |
| Variables | Server tools → Variables ✅ | |
| Charsets, Engines | ❌ as pages (the designer already reads both lists from the server) | 11f |

**Design (11a).** A `home` tab opens after connecting (a setting turns it off). Cards: *Connection* (account, host, port, through SSH or not,
SSL cipher, environment, read-only), *Server* (version and comment, uptime, protocol, max connections, `sql_mode`, time zone, charset),
*General settings* (server connection collation: `SET NAMES utf8mb4 COLLATE …`, remembered per connection and applied through the pool's `setup`
commands), *Appearance* (theme, a link to Preferences), *At a glance* (databases, total size, open connections). Buttons: Databases, SQL, Users,
Variables, Import, Export. Backend: `server_info` exists; one new read-only query for the counts. Test: dashboard shows the mocked server facts
and changing the collation sends the right `SET NAMES`.

### 1.2 Databases page (screen 2)

phpMyAdmin: a *Create database* box (name and collation), then a table with a checkbox per database, collation, and an action
(Check privileges), plus **Check all** and **Drop** for the selection.

**Design (11b).**
* A `databases` tab (server level). Columns: checkbox, name, collation, tables, size, link to its privileges. Names and collations load at
  once; table counts and sizes in a second query (grouped over `information_schema.TABLES`), because that query can be slow on a server with
  thousands of tables. The same cap and "type to narrow" rule as the dropdowns applies to huge lists.
* Search box, sortable columns, **Check all**, and a **Drop** button that opens the Preview dialog (11d) with the exact statements, for example
  `DROP DATABASE `a`;` for each selected database.
* System databases (`mysql`, `sys`, `performance_schema`, `information_schema`) cannot be selected.
* Read-only connections: Create and Drop are disabled (already the rule everywhere else).

### 1.3 Database Structure page (screens 3 and 4)

phpMyAdmin: server and database breadcrumb, database tabs (Structure, SQL, Search, Query, Export, Import, Operations, Privileges, Routines,
Events, Triggers), a "containing the word" filter, then the table list: checkbox, table, actions (favourite, Browse, Structure, Search, Insert,
Empty, Drop), rows, type (engine), collation, size, overhead, a totals row, **With selected** (menu), Print, Data dictionary, and *Create new
table* (name and number of columns).

**With selected** menu (screen 4): Copy table, Show create, Export, **Delete data or table**: Empty, Drop; **Table maintenance**: Analyze,
Check, Checksum, Optimize, Repair; **Prefix**: Add prefix to table, Replace table prefix, Copy table with prefix.

**Design (11c).**
* A `database` tab with sub-tabs like the table tab has today: **Structure** (this page), SQL (console in this database), Search (data search,
  later), Export, Import, Operations (11f), Privileges (the Users manager filtered to this database), Routines, Events, Triggers (lists with
  the definition menus that already exist).
* Table list from one query: name, type (table or view), engine, row estimate, collation, data + index size, `DATA_FREE` as overhead, comment.
  Checkboxes, filter box, totals row, sortable.
* Row actions reuse what exists: Browse = Data view, Structure = Properties view, Search = open with the column filter row on, Insert = new row,
  Empty = TRUNCATE, Drop = DROP, each through the Preview dialog.
* With selected, mapped to SQL (the dialog shows all of it before anything runs):

  | Action | SQL |
  |---|---|
  | Copy table | `CREATE TABLE `new` LIKE `t`; INSERT INTO `new` SELECT * FROM `t`;` (name asked; "structure only / with data") |
  | Show create | `SHOW CREATE TABLE` for each, opened in the console |
  | Export | opens the Export tab with these tables preselected |
  | Empty | `TRUNCATE TABLE `a`; TRUNCATE TABLE `b`;` |
  | Drop | `DROP TABLE `a`, `b`;` (one statement) |
  | Analyze / Check / Optimize / Repair | `ANALYZE TABLE …` etc. (exists as `maintain_tables`); results table |
  | Checksum | `CHECKSUM TABLE …` |
  | Add prefix | `RENAME TABLE `a` TO `p_a`, …` |
  | Replace prefix | `RENAME TABLE `old_a` TO `new_a`, …` (only tables that start with the old prefix) |
  | Copy with prefix | as Copy table, with `p_` added |
* *Create new table*: name and number of columns, then the existing table designer opens with that many empty columns.
* Print and Data dictionary: a printable page listing every table with its columns, types, keys and comments (no new data needed:
  `table_overview` already returns it).

### 1.4 "After Drop, show the descriptive command" (the Preview dialog, 11d)

Today a single drop asks for the table name, and nothing shows the SQL. phpMyAdmin shows the statement and asks "Do you really want to execute...".
MyLoDB will have **one** dialog for every UI operation that changes things:

1. The operation builds a **plan**: a list of SQL statements (the DDL helpers in `myq-schema` already produce them; today they are run at once).
2. The dialog shows the statements in a read-only code box, the number of objects, the estimated rows and size affected, the
   environment badge, and **Copy** / **Open in console**.
3. A switch **Foreign key checks: on / off**. Off wraps the plan in `SET FOREIGN_KEY_CHECKS=0; … SET FOREIGN_KEY_CHECKS=1;` and the wrapper is visible
   in the code box, so what you see is exactly what runs.
4. On production the typed word is required (as in the review dialog), and a read-only connection never gets here.
5. **Run** executes the whole plan on one connection, in order (so the foreign key switch applies to all of it), stops at the first error,
   writes the statements to the query history, refreshes the browser, and shows one line per statement.

Backend: `plan_*` commands return statements without running anything; one `run_plan(id, statements, options)` runs them (guarded for
read-only, like every writing command). Tests: a plan for a bulk drop equals the expected text; a plan run with checks off drops two tables that
reference each other, and with checks on fails with the foreign key error and leaves both tables.

### 1.5 Foreign key checks and previews for imports and runs (11e)

phpMyAdmin has "Enable foreign key checks" on the import page and the SQL tab. MyLoDB today: Restore and "Run SQL file" have a checkbox
(*Disable foreign key checks while running*, on by default); the console and CSV import do not.

* **Console**: Run becomes a split button: *Run* and *Run with foreign key checks off* (a toolbar toggle remembers the choice per tab). Implementation:
  the script already runs on one dedicated connection, so `SET FOREIGN_KEY_CHECKS=0` is sent first and `=1` last, and the Messages tab says so.
* **CSV import**: the same switch next to the import mode.
* **SQL file**: before running, a *Preview* step reads the file with the streaming reader and shows the statement count, the first 50
  statements, the statement kinds (how many DROP, DELETE, CREATE…), and the foreign key switch; then *Run*.
* Test: with checks off an `INSERT` that violates a foreign key succeeds; with checks on it fails and the Messages tab names the constraint.

### 1.6 Theme picker (screen 6)

phpMyAdmin offers four themes. MyLoDB has Light, Dark and System. A gallery of named UI themes (and matching editor colour schemes) is **nice to
have** and rides on the Preferences work in phase 12 (themes are sets of CSS variables, so adding one is a small file).

---

## 2. Top menu bar, toolbar and Preferences (phase 12)

### 2.1 Menu bar (DBeaver / SQLyog style)

macOS gets a native menu bar (Tauri 2 `menu` API); the same structure is drawn in the window for other platforms later.

| Menu | Items |
|---|---|
| File | New connection, New query, Open `.sql`, Save, Save as, Close tab, Preferences, Quit |
| Edit | Undo, Redo, Cut, Copy, Paste, Find, Replace, Select all |
| View | Object browser, History, Favorites, Result tabs, Theme (System / Light / Dark), Zoom in / out / reset, Full screen |
| Connection | Connect, Disconnect, Reconnect, Edit connection, Server home, Safety settings |
| Database | Create, Drop, Refresh, Backup, Restore, Import, Export |
| Table | Open data, Properties, Diagram, Design, Truncate, Drop, Generate SQL |
| Tools | Users, Server tools, Maintenance, Query builder (later), Compare (later) |
| Window | Next / previous tab, Close all |
| Help | Keyboard shortcuts, Documentation, About |

Menu entries call the same functions as the context menus and toolbar; each shows its shortcut. A toolbar under the menu keeps the current
icon strip (new, connect, run, cancel, format, explain, theme).

### 2.2 Preferences

* **Fonts per area**: application (menus, dialogs), sidebar (object browser), data grid, SQL editor, results and messages. Each has family, size,
  weight (regular, bold) and, for the editor, line height. A font family list of common system fonts plus a free-text box; a live preview line.
* **Editor colours**: preset schemes (One Dark, GitHub Light, Solarized, Dracula, Nord, …), multi-colour for keywords, functions, strings,
  numbers, comments, identifiers, operators; optionally per-token overrides. Switching is live (the editor theme is already a compartment).
* **UI**: theme, density (compact / comfortable), start with the home dashboard, restore tabs, confirm on exit.
* **Data**: page size default, null text, date format, the destructive-SQL review default for new connections.
* Storage: a `settings` row in the local SQLite store (not a browser-only setting), exported and imported with one file.
* Implementation: the values become CSS variables on the root (`--font-sidebar-size`, `--font-grid-family`, …) so every area styles itself; the
  grid's fixed row height follows the grid font size (row height = line height), which keeps the windowed grid correct.
* Tests: change the grid font size and the row height and window follow; the editor font changes without losing the undo history; settings survive a restart.

---

## 3. More database engines (phase 15)

You want MySQL, MariaDB, SQL Server, Oracle, SQLite and PostgreSQL.

### 3.1 Where we are

Everything below the UI is MySQL: `myq-driver` wraps `mysql_async`, and `myq-schema`, `myq-admin`, `myq-data`, `myq-transfer` and `myq-profile` build
MySQL-dialect SQL (backtick quoting, `LIMIT … OFFSET`, `information_schema` layouts, `SHOW …`, `KILL QUERY`, `SET SESSION TRANSACTION READ ONLY`).
So:

| Engine | Status | Notes |
|---|---|---|
| MySQL 8.x / 9.x | ✅ tested on 9.7 | 5.7 and 8.0 / 8.4 not yet run |
| **MariaDB** | 🟡 same wire protocol; **never tested** | likely differences: `EXPLAIN FORMAT`, `SHOW CREATE USER`, JSON type, `information_schema` columns, read-only session variable (`tx_read_only`), `max_statement_time`. Mandatory to verify (phase 14) |
| PostgreSQL | ❌ | |
| SQLite | ❌ | |
| SQL Server | ❌ | |
| Oracle | ❌ | |

### 3.2 Architecture

1. **`myq-core`**: a small crate with the engine contract: `Backend` (connect, close, run statements with cancel, stream rows), `Dialect` (quote
   identifier, literal, page SQL, keyset SQL, estimate rows, explain, kill), `Catalog` (databases, schemas, tables, columns, indexes, keys,
   routines, triggers, sizes, DDL), and `Capabilities` (has schemas, routines, events, users, tablespaces, read-only session, keyset ok).
2. **Per-engine crates** implement it: `myq-mysql` (what exists today, moved), `myq-postgres`, `myq-sqlite`, `myq-mssql`, `myq-oracle`.
3. The application layer and UI ask for **capabilities**, never for an engine name: a feature the engine lacks is hidden or disabled with a reason
   (SQLite has no users or events; Oracle and PostgreSQL have schemas; SQL Server has schemas and no `LIMIT`).
4. The connection dialog starts with the engine (default ports 3306 / 5432 / 1433 / 1521; SQLite is a file picker; Oracle takes a service name),
   and the URL box understands `mysql://`, `mariadb://`, `postgres://`, `sqlserver://`, `oracle://`, `sqlite:`.

### 3.3 Driver choice and cost

| Engine | Driver | Special points | Effort |
|---|---|---|---|
| Abstraction (`myq-core`, move MySQL behind it, make `ddl.rs` per dialect) | | the riskiest step: touches every crate; needs CI first | 3 to 4 wk |
| MariaDB | `mysql_async` (same) | version matrix and fixes | 1 wk (phase 14) |
| SQLite | `rusqlite` (already a dependency for the local store) | file based, no users, `PRAGMA query_only` for read-only, no row estimate (`COUNT(*)` is cheap enough on small files) | 2 wk |
| PostgreSQL | `tokio-postgres` + rustls | schemas, `pg_catalog`, `reltuples` estimates, `SET default_transaction_read_only`, `pg_cancel_backend`, `COPY` for fast import / export, `EXPLAIN (FORMAT JSON)` | 3 to 4 wk |
| SQL Server | `tiberius` | T-SQL (`TOP`, `OFFSET … FETCH`), `sys.*` views, `sys.dm_exec_*`, no built-in read-only session (use `ApplicationIntent=ReadOnly` on replicas or a read-only login) | 3 to 4 wk |
| Oracle | `oracle` crate (ODPI-C) | needs the **Oracle Instant Client**, which has its own licence and cannot be bundled: "bring your own", detected at runtime. Schemas are users, `ROWNUM` / `FETCH FIRST`, `ALL_*` views, `SET TRANSACTION READ ONLY` | 3 to 5 wk, only on demand |

What carries over unchanged: keyset paging (a universal technique; only the SQL spelling differs), the windowed grid, the export writers
(they work on text cells), the safety model (review of destructive SQL is dialect-light; read-only enforcement is per engine and weaker on SQL Server).

What must be redone per engine: DDL generation (table designer, preview), introspection queries, estimates and statistics, `EXPLAIN` parsing,
privileges, maintenance commands, backup format (a `.sql` dump is engine-specific; SQLite and PostgreSQL have their own dump conventions).

### 3.4 Testing

Real databases, as for MySQL today: SQLite files in a temp dir; PostgreSQL and SQL Server in containers on CI (SQL Server needs emulation on
Apple Silicon for local runs); Oracle only against a container image where the licence allows. Every engine gets the same contract tests
(introspect, page by keyset, export, cancel, read-only refusal) so a new engine is "make the contract pass".

### 3.5 Risks

* The abstraction touches everything: do it with the version matrix and CI already in place (phase 14), and keep MySQL's tests as the reference.
* Feature gating can make the UI feel uneven; capabilities must carry a human reason ("SQLite has no users").
* Oracle's client library is the one hard external dependency; treat Oracle as optional until someone needs it.

---

## 4. Acceptance checklist for phase 11 (what "done" means)

* Dashboard opens after connect; collation change is applied and remembered.
* Databases page: check all, drop two databases through the Preview dialog; system databases cannot be selected; read-only connections cannot drop.
* Structure page: filter, sort, totals; every **With selected** action produces the SQL in the table above; a bulk drop with foreign key checks
  off succeeds where checks on fails, and the Preview dialog shows the `SET FOREIGN_KEY_CHECKS` lines.
* Console run with checks off and on; CSV import with the switch; SQL file preview lists the first statements and kinds.
* Each of these has a UI test, and every new backend command has a test against the real server (including the read-only refusal).
