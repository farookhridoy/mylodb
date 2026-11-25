# Changelog

## 0.7.0 (2026-11-25)

**Added**
* **Flash messages.** The result of an action (for example *OPTIMIZE rmi_erp_db.users: Table does not support optimize, doing recreate + analyze instead; OK*) used to appear only as small text in the status bar. It now also pops up at the bottom right. Move the mouse over it for a copy button, or click × to close it. It fades after a few seconds (errors stay longer) and stays while the mouse is on it.
* **Smarter autocomplete (phase 20, first part).** After `JOIN` the tables linked by a foreign key to the tables already in the query come first. After `ON` you get the ready condition from the foreign key, such as `o.customer_id = c.id`. MySQL and MariaDB only.

## 0.6.0 (2026-11-25)

**Added**
* **MyLoDB for Windows** (Windows 10 and 11, 64-bit). A normal installer (`MyLoDB_<version>_x64-setup.exe`) that updates itself like the Mac app. Saved passwords go in the Windows Credential Manager (or the encrypted password file). Shortcuts use Ctrl where the Mac uses ⌘. The installer is not signed with a paid certificate yet, so Windows SmartScreen may ask you to click More info, then Run anyway.
* **Website**: a new look (warm paper and ink, serif headlines, one ochre accent) with a light and dark switch, a Windows download button, and the release notes in a fixed-height list that scrolls.
* **Release routine**: `scripts/after-release.sh` brings the Homebrew tap to each new version, and CI now also builds the app on Windows.

**Known limits**
* The Windows build is new and has had less testing than the Mac one. Report problems on GitHub.
## 0.5.1 (2026-11-24)

**Fixed**
* **Reloading the window no longer disconnects you.** The connection stayed open in the background, but a page reload (for example the Reload item in the right-click menu) forgot which one was on screen, so you had to pick the connection again. MyLoDB now attaches to the connections that are still open, on the one you were using, with your tabs and tree as before. Nothing reconnects or asks for a password.

## 0.5.0 (2026-11-24)

**Fixed**
* **The width of a resized pane is saved at once.** It was written in a delayed step, so a quit or reload right after a drag could keep an older width.

**Added**
* **Saved passwords can live in an encrypted file** instead of the macOS Keychain (Preferences, Security, Saved passwords). One master password per session replaces the Keychain prompt that came back after every update. The file is XChaCha20-Poly1305 with an Argon2id key; changing the password, moving back to the Keychain, and resetting after a forgotten password are all there. The Keychain stays the default.
* **One-command install** (`scripts/install.sh`): picks the build for your Mac, checks its SHA-256, installs into Applications and removes the download flag, so there is no "could not verify" step. Also `docs/INSTALL.md` with every way to install, the install box on the website, and a Homebrew cask file (`packaging/homebrew/mylodb.rb`) ready for a tap repository.

**Planned**
* The roadmap has phases 19 to 25 (install without a paid Apple account, smarter autocomplete, visual explain, scheduled jobs, SQLite completed, PostgreSQL, sync and teams).

## 0.4.4 (2026-11-23)

**Changed**
* **The manual now explains every menu** (File, Edit and Search, SQL Editor, Database, View and Help) and has new sections on filtering and sorting, long text/JSON/BLOB cells (including upload and save), exporting (all formats), importing, and backup and restore.
* **The Quick Tour grew from 6 to 11 steps**, covering the sidebar toggle, table data, import and export, backup, compare and tuning, and background jobs.

## 0.4.3 (2026-11-23)

**Fixed**
* **The user manual layout.** The section list sat above the text instead of beside it, so the page looked broken and the screenshots spilled outside the window. The list is on the left again and the text scrolls beside it.
* **Help → Keyboard Shortcuts did nothing visible** (it only wrote a line in the status bar). It now opens a window listing every shortcut by group, and ⌘/ opens it from anywhere.

**Changed**
* The manual's shortcut section uses the same list, which now also covers the object tree and the data grid.

## 0.4.2 (2026-11-23)

**Added**
* **Show or hide the sidebar.** A button before Home (also ⌘B, View → Toggle Sidebar, and ⌘K) hides the sidebar to give the data more room. The choice is remembered, and the tree keeps its state while hidden.
* The row-number column of Table Data is now titled **SL**. It resizes like the other columns, and clicking it sorts by the primary key: ascending, descending, then off, with an arrow.

**Changed**
* A table tab shows only **Table Data** until you run a query in it. Results, Profile, Messages, Info and History appear after the first Run (or Explain or Profile), because before that they had nothing to show.

## 0.4.1 (2026-11-23)

**Added**
* **Colour themes.** Preferences → Theme now offers Monokai, GitHub Dark, GitHub Light, Dracula, One Dark, Nord, Solarized Dark, Solarized Light and Tokyo Night, as cards with a swatch. The SQL editor takes each theme's syntax colours (a scheme picked under Syntax colours still overrides them). ⌘K lists every theme.
* The in-app manual (Help → User Manual) now shows its screenshots, and has a section on Preferences and themes.

**Fixed**
* **Switching between open connections no longer reconnects.** Clicking a connection tab or choosing an open connection in the picker keeps its session: no new pool or SSH tunnel, no wait, and no macOS keychain prompt each time. Editing a connection still reconnects so the new settings apply.
* Saved passwords are read from the keychain once per run and kept in memory, so even choosing "Allow" (not "Always Allow") asks at most once per connection per launch.
* The "Connecting to …" spinner appears only when connecting takes more than a moment.

**Changed**
* The project website is redesigned (live review-dialog demo, theme preview, release notes, downloads for Apple Silicon and Intel).

**Known limits**
* macOS asks for the keychain password once per saved connection after each new build, until the app is signed with an Apple Developer ID.
* SQLite: no table designer yet, no export from a table, no cancel of a running query. Unsigned and not notarised (updates are signed). MySQL 5.7 is unsupported.

## 0.4.0 (2026-11-23)

**Added**
* **Automatic updates.** MyLoDB checks for a new release after it starts and every six hours (Help → Check for Updates… checks at once; About switches the automatic check off). A dialog shows what changed, and **Update now** downloads the signed update, replaces the app and restarts it; **Later** and **Skip this version** are there too. Copies older than 0.4.0 cannot update themselves: install 0.4.0 by hand once.
* **Intel Macs.** Each release now also ships `MyLoDB_<version>_x64.dmg`, built by a release workflow that signs the updates and publishes `latest.json`.
* **SQLite writes.** A SQLite connection is now writable unless you tick **Open read-only** in the connection dialog (existing SQLite connections become writable; tick the box to keep them read-only). SQL in the editor can change the file, and grid edits are staged, previewed and applied in one transaction. Foreign keys are enforced; a table needs a primary key to be edited in the grid, and views stay read-only. Opening never creates a file.
* **Editor window for long values.** TEXT, MEDIUMTEXT, LONGTEXT and JSON cells in Table Data open in a large editor window (double-click, Enter or F2) with Apply (⌘↵), Set NULL, Copy, and for JSON a validity badge, Format and Minify. Long values are fetched in full; read-only connections get a viewer. In SQLite files a `text` column opens it only when the value is long or has line breaks.
* A centred "Connecting to …" spinner while a connection opens.

**Changed**
* A table's **Indexes** in the sidebar are a group that starts closed; click it to open.
* Data grid column headers show only the column name (the full type is still in the tooltip).

**Known limits**
* SQLite: no table designer yet, no export from a table, no cancel of a running query.
* Unsigned and not notarised (updates are signed). MySQL 5.7 is unsupported.

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
