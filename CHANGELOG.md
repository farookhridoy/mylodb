# Changelog

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
