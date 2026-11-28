# Changelog

## Unreleased

PostgreSQL, part two.

**Added (PostgreSQL)**
* **Functions, procedures and triggers** in the browser, with *Show / edit definition* and *Drop*; views too. New objects start from PostgreSQL templates.
* **Table designer**: create and change tables with PostgreSQL's types, identity columns, generated columns, defaults, comments, primary and unique keys, indexes and foreign keys. Changes run in **one transaction**, so they apply completely or not at all. Indexes on expressions, partial indexes, deferrable constraints and checks are left unchanged, and the designer says so.
* **Explain** and **Analyze** with the diagram. Analyze runs SELECT statements only, in a transaction that is rolled back.
* **Backup and restore** (Transfer, and scheduled backups): a SQL script or a compressed archive of the whole database or chosen schemas. They use PostgreSQL's own `pg_dump`, `pg_restore` and `psql`, which you install yourself, at least as new as the server. A restore asks first (on production you type *production*) and by default is all or nothing.

**Known limits (PostgreSQL)**
* The designer cannot move columns, or change the expression of an existing generated column (PostgreSQL cannot).
* Not available: CSV import, users, server tools, the Structure and Databases pages, Profile, formatting, error underlining, data search, diagrams, query builder, compare and copy.

## 0.12.0 (2026-11-28)

PostgreSQL (phase 24 of the roadmap).

**Added**
* **PostgreSQL connections**, with SSH tunnels and the same SSL modes as MySQL. A connection is to one database; the browser lists its schemas, tables, views and columns. `postgres://` and `postgresql://` URLs fill the connection form.
* **SQL editor**: several statements, with dollar-quoted function bodies kept whole; Cancel and the query timeout stop a statement on the server; autocomplete quotes names the PostgreSQL way.
* **Data grid**: pages, filters, sorting, counts, long values and `bytea`, foreign-key lookup, and staged edits applied in one transaction. A table needs a primary key or a non-null unique key to be edited; views are read-only.
* **Export** a table (with its filters and sort) or a query to every format, from Transfer or as a scheduled export. SQL INSERT files are written for PostgreSQL.
* **Read-only connections** run every statement in a read-only transaction that is rolled back, so a statement cannot switch writing back on.
* Tested in CI on PostgreSQL 13, 16 and 18.

**Fixed**
* **Encrypted MySQL and MariaDB connections could crash** instead of connecting (confirmed with the settings of 0.11.0; earlier versions are probably affected too). Connections with SSL switched on work again.

**Fixed and improved (table designer)**
* The column buttons and column titles now stay in place while the columns scroll.
* A **Refresh** button reads the table again from the server; with unapplied changes it asks before dropping them.
* Ticking **Auto inc** or **PK** puts an auto-increment column first in the primary key, where MySQL needs it. Before, the key could end up in an order that left *Apply changes* disabled. The error now names the key and says how to fix it, and the disabled button shows why.

**Known issue (MySQL and MariaDB)**
* SSL mode *Verify CA* also checks the host name, so it refuses a certificate that does not name the host you connect to (such as the one MySQL generates for itself). The cause is in the MySQL driver library. Use *Required* with a certificate that names the host, or *Skip verify*.

**Not available for PostgreSQL yet**
* Backup, restore and import, the table designer, users, server tools, the Structure and Databases pages, Explain and Profile views, formatting and error underlining, data search, ER diagram, query builder, schema compare, data compare and copying a database. Their buttons and menu entries are disabled, hidden or say so.
* Functions, procedures and triggers are not listed in the browser (run SQL against the catalog instead).
* The editor colours SQL as MySQL does. Grid pages use `OFFSET`, so pages far into a very large table are slower.

## 0.11.0 (2026-11-26)

Finishes phases 20 to 23 and 25 of the roadmap.

**Added**
* **Scheduled imports** (MySQL and MariaDB). A job can load a CSV, TSV, JSON or JSON Lines file into a table, with columns matched by name, or run a `.sql` script or zip. A bad row in a data file stops the import and nothing changes; *Replace the table's rows* works in one transaction and is refused where foreign keys point at the table. An unchanged file is not imported again unless you say so.
* **Jobs run while MyLoDB is closed** (File → Scheduled Jobs…, off until you switch it on; macOS and Windows). The system's scheduler checks every 5 minutes while you are logged in. If passwords are in the encrypted file, jobs that need one wait until you unlock it in the app.
* **Where is it used?** Right-click a table → *Where is it used…*: the views, routines, triggers, events and foreign keys that mention it, grouped by kind, optionally narrowed to a column (MySQL, MariaDB and SQLite). The rename dialog now says how many other objects still use the old name, because a rename does not change them.
* **Scheduled exports.** A scheduled job can now export the result of a SELECT (CSV, JSON, JSON Lines, SQL INSERT, XML, HTML or Excel) into a folder and keep the last N files, besides making backups. Jobs also work on SQLite files: a backup is a consistent copy of the file.
* **SQLite table designer.** *New table…* and *Design table…* work for a file. Columns and indexes are added, renamed and dropped in place; any other change rebuilds the table and copies its rows, all in one transaction that is undone if it fails. The SQL tab shows exactly what will run.
* **SQLite export.** *Transfer* is available for a file with the Export tab (a table with its filters and sort, or a SELECT) in every format.
* **Cancel a running SQLite query.** The Cancel button now stops a query on a file; the statements after it are not started.

* **Compare data and make the target match** (Database menu, or right-click a table): two tables on any two open MySQL or MariaDB connections are matched on the primary key (or chosen columns). The result shows identical, different, source-only and target-only rows with samples and the exact SQL; applying runs in one transaction and asks first (a production target asks for the table name). Deleting target-only rows is a separate tick.
* **Copy a database to another connection** (right-click a database): tables, views and rows, under the same or a new name; same-named tables in the target are replaced after you confirm.
* **Audit log** (Settings → Audit log): a file with one line per statement that changed something, including syncs and copies. Off until you turn it on; stays on your computer; rotates at 10 MB.
* **Renaming a table updates its views.** The rename plan now also contains the `CREATE OR REPLACE VIEW` statements that point the views at the new name, so you can read them before anything runs.
* **SQLite generated columns** in the designer (and they are no longer lost when the table is rebuilt: the designer used to read the table without them).

**Fixed**
* **Closing a tunnel now closes its open connections.** Dropping an SSH tunnel stopped it listening but left the connections already open; they are closed with it. The tunnel test that failed at random in CI now waits for the close instead of guessing a delay.

**Changed**
* Menu entries that only make sense for MySQL and MariaDB (restore, CSV import) now say so instead of opening a tab that cannot work for a file.

**Known limits**
* Jobs with MyLoDB closed need the computer on and you logged in. A failure then shows only in the job's log. Switch the option off before removing MyLoDB.
* A scheduled SQL script is not one transaction: what ran before an error stays.
* Data compare reads both tables into memory (up to a million rows each) and works for MySQL and MariaDB only. Copying a database goes through a dump, so a view that names another database keeps pointing at it.
* After a rename, routines, triggers and events that use the old name are listed but not rewritten.

## 0.10.0 (2026-11-25)

Phases 20 to 23 and a slice of 25 of the roadmap, plus fixes to the Ask AI panel.

**Added**
* **Explain as a diagram.** *Explain* and *Analyze* open a **Diagram**: the plan as a tree, each step with its rows and a bar for its cost (or, after Analyze, its measured time). The slowest step is marked, a full table scan is flagged, branches fold, and a line tells you how much faster or slower the run was than the one before.
* **Top statements** (Server Tools → Logs): the statements that cost the server most, from the Performance Schema, sortable by total time, average time, times run or rows examined, with runs that used no index flagged.
* **Scheduled backups** (File → Scheduled Backups…): a job backs up chosen databases every day, every week or every few hours into a folder, keeps the last N and deletes older backups of that job only. *Run now* tests a job; each job has a log. Jobs run while MyLoDB is open, and a run missed while it was closed happens when you open it again. A failed background backup is announced as a flash message.
* **Share connections and snippets** (File → Export / Import Connections and Snippets…): one file for a colleague. Passwords are never written. Importing skips what you already have and renames name clashes.
* **SQLite upkeep**: right-click the database for *Check integrity*, *Check foreign keys*, *Compact the file (VACUUM)*, *Update statistics* and *Rebuild indexes*. The result is a flash message.
* **Snippets with tab stops**: a snippet selects its first name to replace, a name used twice changes everywhere, and Tab goes to the next.
* **Autocomplete knows `WITH` lists and derived tables**: the columns a `WITH x AS (SELECT …)` or `(SELECT …) alias` makes are offered for `x.` and `alias.`.
* The Quick Tour has a step for Ask AI.

**Fixed**
* **Ask AI panel layout.**
* **Ask AI panel layout.** The message box no longer runs past the panel's edge, the Send button is always visible, and a long model name or a long word wraps instead of widening the panel.
* **Answers are formatted.** Bold text, `code`, bullet and numbered lists now show properly instead of raw `**` and backticks.
* **The query editor toolbar is compact.** Run keeps its name; Preview, Cancel, Format, Explain, Analyze and Profile are icons with the name on hover. In a narrow window the toolbar goes onto a second line instead of its Database list spilling over the neighbouring panel.

**Not done yet**
* Scheduled exports and imports, and running jobs with the app closed; a table designer for SQLite; rename-everywhere; PostgreSQL; data compare and sync.

## 0.9.0 (2026-11-25)

**Changed**
* **Ask AI is now a panel.** A new **Ask AI** button sits in the toolbar right after Server (and there is an Ask AI menu). It opens a chat panel on the right, like the assistant panels in code editors: ask for a query, get the SQL in a block with *Insert in editor*, *Replace editor text* and *Copy*, and ask follow-ups ("only for 2024"). The answer is never run for you. The conversation stays when you close and reopen the panel.
* **Connect a service from the panel's gear.** Pick Ollama (on your computer, free), OpenRouter, Groq or Gemini (free tiers), or Claude (API key). *Get a key in your browser* opens the provider's key page; paste the key and press *Connect*. The key is checked, and the models the service offers fill a dropdown: for OpenRouter only the **free** models (with a switch for paid ones), each with its context size.
* **Automatic model switching.** If the chosen model's context window is too small for the question, or it is busy, rate-limited or out of its free quota, the next model of the list answers instead, and the panel says which one and why. It can be switched off.
* **Claude**: added as a service with an API key. Signing in with a Claude or ChatGPT *subscription* is not possible for other apps; the API is billed separately.
* The **Preferences** button is now **Settings** (gear), and the Settings window has an Ask AI page too.

**Known limits**
* The assistant is tested against a stand-in service only, not yet against the real Ollama, OpenRouter, Groq, Gemini or Claude.

## 0.8.0 (2026-11-25)

**Added**
* **Ask AI.** Describe a query in plain words and a model writes the SQL. It works with any service that speaks the common chat protocol: **Ollama** on your own computer (free and private), or the free tiers of **Groq**, **OpenRouter** and **Google Gemini** (each needs a key from that service). Set it up under Preferences, AI assistant. It is off until you do. Only your sentence and the *names* of the tables, columns and foreign keys of the selected database are sent, never rows or values, and only after you allow it once per service. The SQL is shown for review and put in the editor only when you choose; it is never run for you. The key is stored with your other saved passwords.
* **Errors underlined as you type** (phase 20): a bracket or quote that never closes, a table that is not in the database, a column that is not in its table (`alias.column`). Hover for the reason.
* **Tab takes the highlighted suggestion** and stays in the editor. Without a suggestion it indents (it used to move focus to another control).

**Known limits**
* The assistant has been tested against a stand-in service only, not yet against the real Ollama, Groq, OpenRouter or Gemini. Free-tier model names change; the model name is editable.
* Underlining checks `alias.column` and table names only, not bare column names.

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
