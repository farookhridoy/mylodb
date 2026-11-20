# MyLoDB: the plan versus what exists

Compared on 2026-11-20 against `MyLoDB_Final_Product_and_Technical_Plan.md`, checked against the code (not from memory).
✅ done · 🟡 partly · ❌ not started

## Summary

The product is **broad**: connections, tree, editor, grid, designer, users, diagnostics, import/export and backup all work and are tested
against a real MySQL. But the plan says its two *defining* features are **Production Safety** and the **Large Data Engine**, and
**neither is built**. The grid renders every row of a page into the page, there is no read-only or production mode, and exports
cannot be cancelled from the grid. Those two should come before any further breadth (schema compare, ER diagrams, AI).

## P0: "must have" in the plan

| Plan item | Status | What exists / what is missing |
|---|---|---|
| Streaming, virtualized data grid | 🟡 | Server-side paging (up to 5,000 rows a page) with filters, sort and edit. **Every row of the page becomes DOM**, no row or column virtualization, no fetch-on-scroll, no bounded cache. 5,000 rows × 40 columns is already heavy. |
| Smart pagination | 🟡 | `LIMIT … OFFSET` only. No keyset / primary-key cursor, so deep pages get slower. Row count is an exact `COUNT(*)` in the background (the plan warns it can be expensive); should start from an estimate. |
| Large dataset export | 🟡 | ✅ Constant-memory writers for CSV, JSON, SQL, XML, HTML, XLSX. ❌ JSON Lines, GZIP, fetch-size option, rows/bytes/elapsed progress and **cancel for exports started from the grid or a query** (backup, restore and import have it), segmented export. |
| Production safe mode | ❌ | Nothing beyond a coloured tab. Missing: environment label and badge (prod/staging/dev), read-only mode enforced by the server (`SET SESSION TRANSACTION READ ONLY` + privileges), destructive-SQL review (`DROP`, `TRUNCATE`, `DELETE`/`UPDATE` without a real `WHERE`), query timeout (`max_execution_time`), confirm-before-commit on production. |
| Staged editing and SQL preview | ✅ | Edits are staged, "View queries" shows the exact SQL, one transaction, row-gone conflict detection. Gap: query results are read-only (plan wants them editable when keys are reliable). |
| Query cancellation | ✅ | `KILL QUERY`, connection stays usable. |
| Secure connection management | ✅ | Keychain, SSL modes with CA and client certificates, SSH tunnel with a fail-closed host-key policy, test-connection. |

## P1: "important"

| Plan item | Status | Notes |
|---|---|---|
| SQL autocomplete | ✅ | Alias-aware, db-qualified, quoting, loads columns lazily. |
| SQL inspections (unqualified columns, unused aliases, risky queries) | ❌ | |
| Formatter, EXPLAIN, EXPLAIN ANALYZE | ✅ | Plain-language hints and a JSON plan tree. |
| Profiler | 🟡 | Stage timings from `SHOW PROFILE` plus session counters. The plan says not to depend on `SHOW PROFILE`; add a `performance_schema` path. |
| JSON, BLOB and record views | 🟡 | ✅ record/form view, hex/image/text, upload and save. JSON is pretty-printed but not a tree with path search. |
| Schema comparison and migration preview | ❌ | The diff engine for one table (`alter_table_sql`) is the base to reuse. |
| Data comparison | ❌ | |
| Import/export wizard | 🟡 | ✅ CSV import (mapping, modes, new table from file), six export formats. ❌ JSON/XML/XLSX import, saved and re-runnable transfer configurations. |
| SQL history and Git | 🟡 | ✅ history, favorites, session restore. ❌ open/save `.sql` files, drag-open, local history and recovery, Git status/diff. |
| Diagnostics | 🟡 | ✅ process list, variables, status, CHECK/ANALYZE/OPTIMIZE/REPAIR, logs. ❌ redundant and unused index detection, index recommendations, column statistics. |

## P2 and visual tools: all not started

ER designer · visual query builder · charts and dashboards · mock data · AI assistant · MCP server · plugin API · job runner and
schedules (`myq-agent`) · CLI · two-way data sync · Git workflow.

## macOS experience

| Item | Status |
|---|---|
| Dark and light themes | ✅ (light added now; follows the system by default) |
| Keyboard | 🟡 ⌘↵ run, ⌘T/⌘W tabs, ⇧⌘F format, ⇧⌘B filter. No command palette (⌘K), no customisable shortcuts. |
| Object tree | ✅ filter with `type:` `db:` `col:`, regex, wildcards, highlight · ✅ **now scoped to the selected database and lazy** · 🟡 paged rather than virtualized · ❌ filter not remembered per connection |
| Connection groups and environments | ❌ (colour per connection only) |
| Multiple windows, split panes, scratchpad | ❌ |
| Native title bar / window feel | ❌ (standard window) |

## Architecture, tasks, testing, release

| Item | Status | Notes |
|---|---|---|
| Crate layout | 🟡 | Works, but differs from the plan: no `myq-core`, `myq-stream`, `myq-safety`, `myq-agent`. `myq-transfer` ≈ `myq-io`, `myq-profile` ≈ `myq-profiler`. |
| Long-running task model | 🟡 | Backup, restore and import have task ids, progress and cancel. Queries and grid exports do not, and there is **no Tasks panel** (type, connection, elapsed, output). |
| Tests | 🟡 | ✅ about 130 Rust tests incl. real MySQL 9.7 (DDL round trip, backup → drop → restore with checksums, tunnel against a real `sshd`) and about 100 UI checks. ❌ MySQL 5.7/8.0 and MariaDB matrix, 100k/1M/10M-row benchmarks, network-drop and restart tests. |
| CI | ❌ | No workflows. |
| Release | 🟡 | ✅ unsigned arm64 `.dmg`, Apache-2.0. ❌ signing and notarisation, Intel build, Homebrew cask, user documentation. |

## Found by this review and fixed in this change

* **The tree filter hung on big servers.** Four faults: connecting loaded every column of every database; filtering forced every
  database, group and table open (tens of thousands of rows); every keystroke rebuilt the whole tree; and the filter covered the
  whole server instead of the selected database. Now: names only at connect, columns loaded per database on demand, the filter is
  scoped to the selected database ("Filter tables in sakila", database names only when none is selected), groups are paged
  300 rows at a time. Measured on a fake 90,000-object server: connects in 0.6 s, about 600 rows on screen, slowest keystroke 427 ms
  in the browser.
* Light theme added; the connection dropdown no longer shrinks to two letters.

## Recommended order from here (rough, one developer)

1. **Production Safety**, ~2–3 weeks. Environment labels, server-enforced read-only, destructive-SQL review, timeouts, confirm-on-commit.
2. **Large Data Engine**, ~3–4 weeks. Virtualized grid, lazy fetch with a bounded cache, keyset pagination, estimated counts, exports as
   cancellable background tasks (progress, JSON Lines, GZIP); then benchmark on 1M and 10M rows.
3. **Task centre, CI, version matrix**, ~1–2 weeks. Tasks panel, GitHub Actions for macOS arm64/x64, MySQL 5.7/8.0 and MariaDB in containers.
4. **Command palette, connection groups, environments**, ~1 week.
5. **Schema compare and migration preview, then data compare**, ~4–5 weeks.
6. **SQL files and inspections, index analysis**, ~3 weeks.
7. **Beta release.** Signing and notarisation (needs an Apple Developer account), Intel build, Homebrew cask, documentation.
8. Later: ER designer, query builder, job agent, AI/MCP, plugin API.
