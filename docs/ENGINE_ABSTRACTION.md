# Phase 15 design: the engine abstraction

Status of 2026-11-21. Steps 1 to 3 (sections 5 to 7) are done; the rest is design only. It narrows section 3.2 of `PHPMYADMIN_AND_ENGINES.md` into steps that can each ship and be tested.

## 1. What is coupled to MySQL today (measured by grep, not guessed)

| Crate | Lines | `mysql_async` mentions | `information_schema` | `SHOW …` | Notes |
|---|---|---|---|---|---|
| `myq-driver` | 1,018 | 18 | 30 | 1 | connection, pool, introspection, overview, scripts, values. The seam goes here. |
| `myq-schema` | 1,422 | 0 | 1 | 2 | pure SQL builders (`ddl.rs`, `plan.rs`, `filter.rs`); backtick quoting in 34 places via `ddl::q`. Pure, so per-dialect variants are easy to test. |
| `myq-sql` | 2,020 | 0 | 0 | 6 | splitter, formatter, risk review, completion. Parses MySQL syntax; completion keywords are MySQL's. |
| `myq-data` | 1,632 | 7 | 5 | 0 | paging, keyset, edit, export. Builds `LIMIT` and backtick SQL. |
| `myq-admin` | 1,143 | 11 | 6 | 13 | users, server variables, processes: the most MySQL-specific. |
| `myq-profile` | 323 | 2 | 0 | 6 | `EXPLAIN`, `SHOW PROFILE`. |
| `myq-transfer` | 1,912 | 13 | 10 | 8 | dump format, CSV import, SQL file runner. |
| `myq-store` | 734 | 0 | 0 | 0 | local SQLite store; unaffected. |
| `src-tauri/lib.rs` | 1,656 | 41 via `myq_driver::mysql_async` | | | commands reach into `mysql_async` types directly (`Queryable`, `Row`, `Value`). **This is the leak to close first.** |

Reading: about 6,500 of 10,400 lines touch the dialect or the driver. The pure builders in `myq-schema` and `myq-sql` are the cheapest to make
dialect-aware; `lib.rs` talking to `mysql_async` directly is the real obstacle, because every command would need rewriting at once.

## 2. Strategy: strangler, not big bang

Do not move MySQL behind a trait in one change (the roadmap's "riskiest step"). Instead:

1. **Close the leak.** Commands in `lib.rs` stop importing `mysql_async` and call functions in `myq-driver` (or the crate that owns the feature).
   Pure refactor, MySQL only, the whole existing suite (170 Rust tests, 21+ UI suites) stays the safety net. After this step `mysql_async` appears
   only inside `myq-*` crates.
2. **Introduce `myq-core` with `Capabilities` and `Dialect` only** (no `Backend` yet): quote identifier, string literal, page SQL, keyset SQL,
   a capability list with a human reason per missing feature. MySQL implements them by delegating to today's functions. `ddl::q` and the
   `LIMIT` builders take a `&dyn Dialect`. Tests: golden SQL per dialect, MySQL output unchanged byte for byte.
3. **`Backend` trait for the read path first**: connect, list databases/tables/columns/indexes, run a statement, stream rows, cancel.
   MySQL wraps the existing code. The UI reads `Capabilities` to hide what an engine lacks.
4. **First new engine: SQLite** (vertical slice, read path only). Reasons: no server, no container in CI, `rusqlite` is already a dependency,
   and it forces every capability gate (no users, no events, no schemas, no estimate, `PRAGMA query_only` as read-only) to exist and work.
   Slice: open a file, browse tables and columns, run SQL, paged grid, export. Writes (edit, designer) follow once the read path is solid.
5. **PostgreSQL** (`tokio-postgres`), then **SQL Server** (`tiberius`) reuse the same seam. Oracle only on demand (Instant Client is bring-your-own).

## 3. Rules that keep it safe

* MySQL behaviour must not change: every step keeps the existing tests green and adds golden tests that pin today's SQL.
* The UI and application layer ask for **capabilities**, never an engine name; a missing feature says why ("SQLite has no users").
* Identity of an object is `(engine, connection, schema-or-database, name)`; do not assume `database.table` is the whole path (PostgreSQL has
  database > schema > table; SQLite has attached databases, `main` by default).
* Read-only sessions are enforced by the server where possible (`SET SESSION TRANSACTION READ ONLY`, `PRAGMA query_only`,
  `default_transaction_read_only`); never by the client alone.
* Credentials stay in the Keychain; a SQLite connection has none, only a path.
* A dump is engine-specific: do not promise cross-engine restore.

## 4. Decisions for the owner

1. **Order of engines.** Recommended: SQLite, then PostgreSQL, then SQL Server. (SQLite first because it is the cheapest way to prove the seam.)
2. **Scope of the first SQLite release.** Recommended: read-only browsing and querying first; editing and the table designer in a second step.
3. **Where `Dialect` lives.** Recommended: a new `myq-core` crate that `myq-schema`, `myq-data` and the engines depend on, so `myq-schema` stays pure.

## 5. Step 1: close the leak (done)

`apps/desktop/src-tauri/src` no longer mentions `mysql_async`. The six places that did (connection test, foreign-key-check switch,
server home facts, SHOW grids, server options, restore of the FK switch) moved into `myq-driver::session`
(`probe`, `set_foreign_key_checks`, `restore_foreign_key_checks`, `session_facts`, `query_grid`, `server_options`).
Guard: `crates/myq-driver/tests/engine_seam.rs` fails if the app imports the driver again.
Real-MySQL test: `crates/myq-driver/tests/session.rs`. Full Rust workspace suite passes on MySQL 9.7. Not rerun: MariaDB 13 and the UI suites.
`myq_driver::Conn` is still a MySQL type that the app passes around (`conn_for`); step 3 replaces it with the `Backend` handle.

## 6. Step 2: `myq-core` with `Dialect` and `Capabilities` (done)

New crate `crates/myq-core` (no dependencies on a driver, no I/O):

* `Dialect` (`quote_ident`, `quote_string`, `limit_offset`) with one implementation, `MySql`. It is now the **only** place that quotes identifiers and
  strings. Before, seven private copies existed (`myq-schema::ddl`, `myq-data`, `myq-admin` twice, `myq-transfer` twice, `myq-sql`); they
  now re-export `myq_core::{q, lit}`. Output is byte-for-byte the same for ordinary text; the one deliberate change is that NUL and Ctrl-Z in a
  string literal are now escaped everywhere (only the backup writer did that before). The data grid's `LIMIT … OFFSET` goes through the dialect.
* `Capabilities` (`schemas`, `routines`, `triggers`, `events`, `users`, `replication`, `explain_analyze`, `read_only_session`, `row_estimates`), each a
  `Cap { available, reason }`, built from `SELECT VERSION()` (`Capabilities::mysql_family`). Exposed as the `capabilities` command;
  the UI uses it first for the **Analyze** button, which is now disabled with the server's reason on MySQL older than 8.0.18.
* Tests: golden quoting and paging strings, flavor and version parsing, a real-server check on MySQL 9.7 and MariaDB 13, a UI test for the disabled button.

## 7. Step 3: the `Backend` trait, read path (done)

`myq-core::Backend` is what the application asks an engine for: `dialect`, `capabilities`, `databases`, `tables`, `columns`, `indexes`, `run` (a script, one result per
statement; statement errors live in the results). The engine-neutral shapes it returns (`DatabaseRow`, `TableRow`, `ColumnDetail`, `IndexDetail`,
`ColumnInfo`, `ResultSet`, `StatementResult`) moved from `myq-driver` to `myq-core`; `myq-driver` re-exports them, so no caller changed.
`myq_driver::MySqlBackend` implements the trait by delegating to the functions that already existed.

Design choices: native `async fn` in the trait with `Send` futures and **enum dispatch** in the application (no `dyn`, no `async-trait` dependency);
`Dialect` is `Send + Sync`.

Proof: `crates/myq-driver/tests/backend.rs` drives a real server through a function that is generic over `Backend` and knows nothing about MySQL.
That function is the contract a SQLite or PostgreSQL backend has to pass. Passes on MySQL 9.7 and MariaDB 13.

Not done yet (step 3b / 4): the application (`src-tauri`) still holds a `mysql_async::Pool` per session and calls `myq_driver` functions directly; switching
its read commands to an `enum Engine { MySql(MySqlBackend), … }` is the next change, then the first second engine (SQLite) implements `Backend`.
Writes (grid edits, table designer, admin pages, transfer) stay MySQL-only until the read path works for SQLite.

Next: step 4, after the three decisions in section 4.
