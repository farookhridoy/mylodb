# Server compatibility

Measured, not assumed. The whole Rust test suite (165 tests, many against a live server) passes on:

| Server | Result | How |
|---|---|---|
| MySQL 9.7.2 (local) | 165 / 165 | `cargo test --workspace` |
| MariaDB 13.0.2 (local, Homebrew, own data directory on port 3399) | 165 / 165 | `MYLODB_TEST_PORT=3399 cargo test --workspace` |
| MySQL 8.0, 8.4, 9, MariaDB 10.11, 11.4 | run by CI (`.github/workflows/ci.yml`), not yet run by hand | Docker services |
| MySQL 5.7 | CI, **non-blocking** (end of life) | not run by hand |

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
