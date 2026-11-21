# Server compatibility

Measured, not assumed. The whole Rust test suite (165 tests, many against a live server) passes on:

| Server | Result | How |
|---|---|---|
| MySQL 9.7.2 (local) | 165 / 165 | `cargo test --workspace` |
| MariaDB 13.0.2 (local, Homebrew, own data directory on port 3399) | 165 / 165 | `MYLODB_TEST_PORT=3399 cargo test --workspace` |
| MySQL 8.0, 8.4, 9 | **pass in CI** (first GitHub run, 2026-11-21) | Docker services, `.github/workflows/ci.yml` |
| MariaDB 10.11, 11.4 | **pass in CI** (since PR #1; two product bugs fixed, see below) | Docker services |
| MySQL 5.7 | **fails some tests in CI**; non-blocking (end of life) | Docker services |

The first MariaDB run failed **8 of 165** tests. Each one was triaged as a product bug or a wrong assumption in a test:

## Product bugs found and fixed (MariaDB)

| Area | What went wrong | Fix |
|---|---|---|
| Table designer DDL | `GENERATED ALWAYS AS (…) VIRTUAL NULL` is a syntax error on MariaDB | no `NULL` after a nullable generated column |
| Process list | `SHOW FULL PROCESSLIST` has an extra `Progress` column, so reading it as a fixed 8-column tuple panicked | rows are read by position |
| Column defaults | MariaDB writes "no default" as the text `NULL`, quotes string defaults and spells expressions in lower case; the overview and the data grid showed `NULL` | one parser, `myq_driver::parse_column_default`, used by the designer, overview and grid |
| Integer display width | `int(10) unsigned` appeared as a type parameter | default widths are not parameters |
| Users | account lock state lives in `mysql.global_priv` JSON; accounts always showed "unlocked" | read it from there |
| EXPLAIN ANALYZE | MariaDB spells it `ANALYZE` (use `ANALYZE FORMAT=JSON`) | fallback |
| Set variable | after a failed SET, `SHOW WARNINGS` on a later good SET still showed the old error | use the statement's own warning count |
| Backup | views were not put in dependency order (`VIEW_TABLE_USAGE` does not exist) and the failure was silent | read dependencies from the view definitions |

## Differences that are not bugs (tests now accept both)

* `ON UPDATE` defaults to `NO ACTION` on MySQL and `RESTRICT` on MariaDB (same behaviour).
* MariaDB can repair InnoDB tables; MySQL says it cannot.
* A non-numeric value for a numeric variable: MySQL warns and adjusts, MariaDB fails with error 1232.
* `DROP TABLE parent, child` in one statement: MySQL resolves the dependency, MariaDB refuses with error 1451 unless foreign key checks are off (the Preview dialog has that switch).
* Recursive CTE depth is `cte_max_recursion_depth` on MySQL and `max_recursive_iterations` on MariaDB (test fixtures only).
* MariaDB has no per-session `Innodb_rows_read`; Profile shows the handler counters, which tell the same story.

## Connection loss

`crates/myq-driver/tests/connection_loss.rs` kills a connection server-side in the middle of `SELECT SLEEP(20)`: the error reaches the caller within a second, the next statement does not run, the pool hands out a fresh connection, and read-only is still applied to it. Passes on both servers.

## Still not covered

A real network drop (packets disappearing, not a clean `KILL`), a server restart during a long export, TLS variants, MySQL 5.7 by hand, and the Windows / Linux builds.

## Found by the first GitHub CI run (2026-11-21) and fixed in PR #1

| Area | What went wrong | Fix |
|---|---|---|
| Collation lists (designer, Alter database) | MariaDB 10.10+ lists generic `uca1400_*` collations with a NULL character set and leaves `IS_DEFAULT` empty for most rows: the list panicked on 10.x, and on 11.4 / 13 the default collation of a character set was empty | read `COLLATION_CHARACTER_SET_APPLICABILITY` (charset-prefixed names) and `CHARACTER_SETS.DEFAULT_COLLATE_NAME` |
| Table overview | the table's character set was looked up in `COLLATIONS` and came back NULL for `uca1400_*` | derive it from the collation name (`myq_schema::ddl::charset_of_collation`) |
| Tests | `@@transaction_read_only` does not exist on MariaDB 10.x (`tx_read_only`); the backup test used the MySQL-only `utf8mb4_0900_ai_ci` | the tests ask the server (fallback variable, the server's own utf8mb4 default) |

Also noted: `SHOW CREATE TABLE` prints a column collation that is not its charset's default differently after a dump and restore
(`COLLATE x` before, `CHARACTER SET y COLLATE x` after). The data and the collation are the same; only the text differs, so the round-trip
test compares text only when the database uses the server's default collation.

## Still open: MySQL 5.7 (end of life, non-blocking in CI)

Fails on the first run (not triaged): `a_cancelled_backup_...`, `a_database_survives_backup_drop_and_restore`, `designer_and_privileges_end_to_end`,
`explain_and_profile_against_a_real_server`, `new_formats_and_query_export`, `overview_reports_keys_...`, `plans_do_what_the_preview_says`,
`restore_into_another_database_and_the_other_output_layouts`, `tls_modes_behave_as_documented`. Several of these use features 5.7 lacks
(window functions, EXPLAIN ANALYZE, roles, utf8mb4_0900), so some are expected; 5.7 is documented as unsupported until triaged.
