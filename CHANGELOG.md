# Changelog

## Unreleased

**Added**
* **SQLite writes.** A SQLite connection is now writable unless you tick **Open read-only** in the connection dialog (existing SQLite connections become writable; tick the box to keep them read-only). SQL in the editor can change the file, and grid edits are staged, previewed and applied in one transaction. Foreign keys are enforced; a table needs a primary key to be edited in the grid, and views stay read-only. Opening never creates a file.
* A centred "Connecting to …" spinner while a connection opens.

**Changed**
* Data grid column headers show only the column name (the full type is still in the tooltip).

**Known limits**
* SQLite: no table designer yet, no export from a table, no cancel of a running query.

## 0.3.0 (2026-11-21)

**Added**
* **Server charts** (Server → Charts): six live charts of the status counters (rates per second and connection counts), pause, interval and clear.
* **JSON import**: `.json`, `.jsonl` and `.ndjson` files in the CSV import (converted once to a temporary CSV; nulls, nested values and mixed keys handled).
* **Data search** (database context menu, Search menu): find a value in any table of a database, with per-column counts, cancel and progress; click a column to open the table filtered to it.
* **Index advisor** (database context menu, Database menu): duplicate, redundant and missing foreign key indexes found from the definitions, unused indexes from the server, fixes through Preview SQL.
* **ER diagram** of a whole database (database context menu, SQL Editor menu): tables by foreign-key depth, drag, zoom, filter, open a table, copy as Mermaid.
* **Visual query builder** (database context menu, SQL Editor menu): tables, joins from foreign keys, columns with functions, conditions, order and limit, with the SQL built by the backend and opened or run in a query tab.
* **Compare schemas** (database context menu): make a database's tables match another database's, on the same or another open connection. Differences are listed per table with their SQL; drops are never pre-selected; Preview SQL shows the exact script.

**Fixed**
* Server charts draw across the whole width from the first samples and show a sensible scale for a flat zero series.

**Internal**
* A check (`npm run check:e2e-list`) fails when a UI suite is not part of `npm run e2e`; two suites were not running in CI before.

**Known limits**
* Not done from the roadmap's Phase 16: comparing the *data* of two tables, copying a database between connections, saved and re-runnable transfer configurations, XLSX and XML import, SQL inspections (the index advisor is the first of these). The ER diagram and the server charts need a MySQL or MariaDB connection (not a SQLite file) and were only checked on small demo data.
* SQLite is still read-only. Unsigned and not notarised; Apple Silicon only. MySQL 5.7 is unsupported.

## 0.2.0 (2026-11-21)

**Added**
* **SQLite files, read-only.** Choose "SQLite file" in the connection dialog: browse tables, views, triggers, columns and indexes, run SQL, and use the data grid (paging, filters, sort, exact counts). Features a file cannot have (users, events, routines, server tools, dump-based backup) are disabled with the reason in their tooltip. The file is opened read-only and is never created or changed.
* The Analyze button is disabled with the server's reason on servers that cannot `EXPLAIN ANALYZE` (MySQL before 8.0.18).

**Fixed**
* MariaDB 10.10 and later: the collation list (table designer, Alter database) panicked or showed no default collation, and a table's character set was lost in its properties. MariaDB 10.11 and 11.4 now pass the whole integration suite in CI.
* Edits made within a second of switching connections are saved for the connection you left (already in 0.1.0 notes: kept here for upgraders).

**Changed (internal)**
* Groundwork for more engines: one shared place for SQL quoting (NUL and Ctrl-Z in string literals are now escaped everywhere), an engine-neutral `Backend` interface, and the app reading through it.
* The test suites run on Linux CI as well as macOS.

**Known limits**
* SQLite is read-only: no editing, no export from a table, no cancel of a running query.
* Unsigned and not notarised; Apple Silicon only; MySQL, MariaDB and SQLite only. MySQL 5.7 still fails some integration tests and is unsupported.

## 0.1.0 (2026-11-21)

First public release. macOS, Apple Silicon, unsigned.

**Added**
* Connection manager with Keychain passwords, SSH tunnels, SSL modes, connect by URL, connection groups, several connections open at once.
* Object browser with a fast filter, sizes, facts about the selected object in the Info tab, drag a name into the editor, alter database, rename procedures and functions.
* SQL editor with autocomplete, formatter, Explain and profiler, history, favorites, personal snippets, `.sql` files, text results and Copy as.
* Editable data grid with staged edits, filter row, foreign-key picker, BLOB viewer and exports; keyset paging and a windowed grid for very large tables.
* Table designer with a live `ALTER TABLE` preview, users and privileges, server tools, backup, restore and CSV import.
* Admin console: Databases page, Structure page with "With selected", Preview SQL before every destructive operation, FK-check switches, Engines / charsets / replication.
* Production safety: environment colours, server-enforced read-only, query time limit, review of destructive SQL with row estimates.
* Preferences (fonts per area, editor colour schemes), command palette, native macOS menus.
* **Help menu:** built-in User Manual, a first-run Quick Tour, and About (author links).
* Documentation: manual, benchmarks, roadmap, engine abstraction design.

**Fixed (found while preparing this release)**
* Edits made within a second of switching connections were not saved for the connection you left.
* On MariaDB 10.10 and later the list of collations (designer, Alter database) panicked on `uca1400_*` rows with no character set.
* The desktop app no longer imports the MySQL driver directly (groundwork for more database engines).

**Known limits**
* Not signed or notarised: macOS asks you to confirm the first launch.
* Apple Silicon only; no Intel, Windows or Linux build yet.
* MySQL and MariaDB only. Tested and passing in CI on MySQL 8.0, 8.4 and 9 and locally on MySQL 9.7 and MariaDB 13. MariaDB 10.11 / 11.4 were fixed and pass in CI after this release (see `main`); MySQL 5.7 still fails some integration tests ([details](docs/SERVER_COMPATIBILITY.md)) and is unsupported.
