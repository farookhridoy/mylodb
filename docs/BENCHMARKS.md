# Large-table benchmarks (Phase 10)

Measured on 2026-11-21 against a local MySQL 9.7.2 (DBngin, `innodb_buffer_pool_size` = 128 MB, so the 10M-row table, about 1 GB, does **not**
fit in memory). Table `bench_N`: `id BIGINT UNSIGNED AUTO_INCREMENT PRIMARY KEY, customer_id INT, status VARCHAR(12), amount DECIMAL(12,2),
note VARCHAR(100), created_at DATETIME`, one secondary index on `created_at`, N = 1,000,000 and 10,000,000 rows.

Reproduce: `MYLODB_BENCH_ROWS=10000000 cargo test --release -p myq-data --test large -- --ignored --nocapture`
(it builds its own database `mylodb_bench_data` once; drop it afterwards). Always use `--release`: the first run of this benchmark was done
in a debug build by mistake and showed an export speed of 74,000 rows/s, which is not what the app does (see "Lesson" below).

"Before" is the previous commit (`a137696`) built in a separate worktree and run through the same harness. "After" is this change.
Times are single runs on an otherwise idle machine, warm cache for the repeated reads; the two CSV lines were run twice to show the noise (under 1%).

## Paging, counting

| Operation | 1M rows before | 1M rows after | 10M rows before | 10M rows after |
|---|---:|---:|---:|---:|
| First page (500 rows) | 2.0 ms | 2.1 ms | 3.2 ms | 5.9 ms |
| Page at 50 % by `OFFSET` | 117 ms | 124 ms | 1,261 ms | 1,190 ms |
| Page at 50 % by **keyset** (seek on the primary key) | n/a | **2.1 ms** | n/a | **3.3 ms** |
| Last page by `OFFSET` | 219 ms | 234 ms | 2,367 ms | 2,336 ms |
| Last page by **keyset `End`** | n/a | **2.0 ms** | n/a | **2.0 ms** |
| Exact `COUNT(*)` | 75 ms | 75 ms | 808 ms | 820 ms |
| **Estimated count** (table statistics) | n/a | **0.8 ms** | n/a | **0.9 ms** |
| Estimated count with a filter (`EXPLAIN`) | n/a | 0.3 ms | n/a | 0.2 ms |

* `OFFSET` cost grows with the table (1.2 s to reach the middle of 10M rows, 2.3 s for the end). Keyset paging costs the same at any depth:
  about **350 to 1,100 times faster** at 10M rows.
* The estimate for the whole table was 9,955,106 for 10,000,000 rows (0.4 % off) and 995,784 for 1,000,000 (0.4 % off).
* **Filtered estimates can be far off.** `status = 'paid'` (an unindexed column, really 25 % of the rows, 2.5M) was estimated at 9.7M. That is why
  the grid shows it as "≈", counts exactly by itself only below 200,000 estimated rows, and otherwise offers a **Count** button (cancellable).

## Export (all rows to a file)

| Format | 1M before | 1M after | 10M before | 10M after | Output at 10M |
|---|---:|---:|---:|---:|---:|
| CSV | 1.22 s (818k rows/s) | **0.92 s (1.09M rows/s)** | 12.2 s | **9.4 s (1.06M rows/s)** | 638 MB |
| JSON (array) | 2.87 s | **1.51 s** | 29.5 s | **15.0 s** | 1.39 GB |
| JSON Lines | n/a | 1.46 s | n/a | 15.0 s | 1.26 GB |
| CSV + gzip | n/a | 1.27 s | n/a | 12.8 s (784k rows/s) | **204 MB** |

* Memory stays flat: resident size 5 to 11 MB during a 1.4 GB export, before and after.
* Ceiling: just reading every row from the server without any formatting takes 5.2 s for 10M rows (1.9M rows/s); reading and converting
  every cell to text takes 8.7 s. CSV is therefore within about 8 % of "read and convert".
* What changed: each row is built in one reusable buffer and written once (before: two `Vec<String>` and a `join` per row), the JSON writer
  does the same, and gzip gets 256 KB chunks from a buffer instead of one tiny write per row (that fix doubled its speed).
* JSON Lines, gzip, progress every 1,000 rows and cancellation are new.

## Grid in the window

Headless Chrome against the mock bridge (`e2e/large.mjs`): a page of **5,000 rows draws 32 rows** (fixed 22 px row height, 12 rows of overscan),
the scroll height still covers all 5,000, and Cmd+End / Cmd+Home / PageDown scroll the active row into view.
The real Tauri window was **not** measured.

## Lesson: debug builds

`cargo test` builds without optimisation. In a debug build the same export ran at 74,000 rows/s and the formatting looked like the bottleneck.
A release build ran 14 times faster, the writers were never the problem (12M rows/s in a micro-benchmark), and the gzip slowness was the
only real finding. Benchmark with `--release`, and measure before optimising.

## What is not covered

* Wide tables: only rows are windowed, not columns (a table with hundreds of columns still draws every column of the visible rows).
* Memory: the grid holds one page (at most 5,000 rows); there is no multi-page cache or read-ahead.
* Segmented exports (split into files of N rows), exports of an editable grid's unsaved state.
* MySQL 5.7 / 8.0 / 8.4 and MariaDB runs, network drops during an export or a count, a 100k-row case.
* Keyset paging needs a primary key whose columns are all integer, string (`char`, `varchar`) or date/datetime/timestamp (composite keys of
  those work). Keys with a decimal, float, binary or other column type, tables without a primary key, views and any user sort fall back to
  `OFFSET` (the grid does this by itself).
