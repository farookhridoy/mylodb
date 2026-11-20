# MyLoDB — Analysis & Build Report

Open-source, native-feeling MySQL/MariaDB manager for macOS (Apple Silicon + Intel), written in Rust.
Goal: functional parity with the SQLyog feature set the author listed, with an original UI/brand and
original code (no SQLyog assets, icons, or code are copied; only *features* are matched).

## 1. Scope decisions

| Question | Decision | Why |
|---|---|---|
| "Exact copy"? | Feature parity + SQLyog-style layout (left object tree, tabbed right pane, result tabs under editor). Own name, icons, code. | Features/workflow aren't copyrightable; assets and code are. Keeps the project safely open source. |
| Databases | MySQL 5.7/8/9 + MariaDB first. | SQLyog scope. Postgres/SQLite are out of scope until v2. |
| Platforms | macOS first (arm64 + x64, universal binary). Linux/Windows come "free" later. | The gap you want to fill. |
| License | Apache-2.0 or GPL-3.0 (decide before first public commit). | GPL protects against closed forks; Apache maximizes adoption. |

## 2. Tech stack (recommendation)

**Tauri 2 (Rust core) + React/TypeScript UI**, with all database logic in pure-Rust crates.

Why not a pure-Rust GUI (egui/iced/Slint/GPUI)? SQLyog's UI is dense: virtualized editable grids with
100k+ rows, a Monaco-class SQL editor, a drag-and-drop query builder, an ER canvas, a filterable tree.
These exist as mature web components (CodeMirror 6/Monaco, TanStack Table/AG Grid, React Flow) and would
take months to rebuild natively. Tauri keeps the app small (~10 MB), uses macOS WKWebView, and the
Rust side stays the product's real engine.

| Concern | Choice |
|---|---|
| MySQL driver | `mysql_async` (binary protocol, streaming rows, SSL, compression) |
| Async runtime | `tokio` |
| SSH tunnel | `russh` (+ `russh-keys`) |
| SQL parsing / formatting | `sqlparser-rs` (MySQL dialect) + custom formatter |
| Local app store (connections, history, favorites, session) | `rusqlite` (SQLite) |
| Secrets | macOS Keychain via `keyring` crate |
| Scheduler / job agent | separate `mysqlyog-agent` binary + `launchd` plist; XML/TOML job files |
| Export formats | `csv`, `rust_xlsxwriter`, `serde_json`, `quick-xml`, `zip` |
| Frontend | React 19, TypeScript, CodeMirror 6, TanStack Table + virtualizer, React Flow, Zustand |
| Packaging | Tauri bundler → signed/notarized `.dmg`, Homebrew cask |

## 3. Architecture

```
apps/desktop            Tauri shell + React UI (thin; calls commands)
crates/
  myq-core              domain types, error algebra, Result, connection model
  myq-driver            mysql_async wrapper: pooling, streaming, cancel (KILL QUERY)
  myq-tunnel            SSH / HTTP-tunnel / SSL transports
  myq-schema            introspection (information_schema), DDL generation, diff engine
  myq-sql               parse, format, autocomplete index, query builder model
  myq-data              grid paging, edit-commit, FK lookup, search, compare, sync
  myq-io                import/export/backup (CSV, SQL, XLSX, JSON, XML, HTML)
  myq-store             SQLite: connections, history, favorites, session restore
  myq-agent             headless job runner (backup, sync, report, email)
```

Dependency rule: UI → Tauri commands → `myq-*` crates → `myq-core`. No crate depends on the UI.
Every long operation is a cancellable task that streams progress events.

## 4. Feature analysis (your list → difficulty → crate)

Effort: S ≤ 1 wk, M 1–3 wk, L 3–6 wk, XL 6+ wk (one developer).

| # | Feature | Crate | Effort | Notes / risk |
|---|---|---|---|---|
| 1.1 | Object browser tree **+ left-menu filter** | schema, UI | M | See §5. Lazy-load per node; filter must not fetch everything. |
| 1.2 | Table/schema visual editors (create/alter/dup/truncate/drop/reorder) | schema | L | Generate `ALTER` from a model diff; show SQL preview before apply. Column reorder = `MODIFY … AFTER`. |
| 1.3 | User & privilege manager | schema | M | `mysql.user`, `GRANT/REVOKE`; differences MySQL 8 vs MariaDB. |
| 1.4 | Multi-tab/multi-connection, color tabs, session restore | store, UI | M | Autosave tab state every few seconds to SQLite. |
| 1.5 | UTF-8 / charset | driver | S | Default `utf8mb4`; per-connection override. |
| 2.1 | Autocomplete | sql | L | Cache schema index per connection; alias resolution via parser. |
| 2.2 | SQL formatter | sql | M | Configurable indent/keyword case. |
| 2.3 | Visual query builder | sql, UI | XL | Model → SQL and (stretch) SQL → model. |
| 2.4 | Profiler & EXPLAIN | driver, UI | M | `SHOW PROFILE` is deprecated; use `performance_schema` + `EXPLAIN FORMAT=JSON/ANALYZE`. |
| 2.5 | History & favorites | store | S | |
| 3.1 | Grid + form view, inline editing, paging | data, UI | L | Edit only when a PK/unique key exists; else read-only with warning. |
| 3.2 | FK lookup dropdown | data | S | |
| 3.3 | BLOB/image/hex viewer | UI | M | |
| 3.4 | Global data search | data | M | Text/number/regex across tables; run in parallel with limits. |
| 4.1 | Visual schema designer (ER) | schema, UI | XL | Reverse-engineer + forward-engineer + export PNG/PDF. |
| 4.2 | Schema optimizer / redundant index finder | schema | M | `PROCEDURE ANALYSE` is gone in 8; sample-based type inference instead. Redundant = prefix-duplicate indexes; unused via `sys.schema_unused_indexes`. |
| 5.1 | Schema sync | schema | L | Diff engine is shared with 1.2. |
| 5.2 | Data compare | data | L | PK-ordered merge walk; selective apply. |
| 5.3 | Two-way data sync (checksums) | data | XL | Chunked checksum, conflict policy. Ship one-way first. |
| 5.4 | ODBC import wizard | io | XL | `odbc-api` + unixODBC on macOS; drivers are user-supplied. Defer. |
| 6.1 | Job agent + scheduled backups | agent | L | `launchd` on macOS. |
| 6.2 | Scheduled reports by email | agent | M | `lettre` SMTP. |
| 6.3 | Import/export multi-format | io | M | Streaming; never load whole table in memory. |
| 7.1 | SSH tunnel / SSL / HTTP tunnel | tunnel | L | SSH + SSL are mandatory. HTTP tunnel needs a PHP script; low priority and a security surface — v2. |
| 7.2 | Diagnostics (CHECK/OPTIMIZE/REPAIR/ANALYZE, processlist, variables, logs) | schema, UI | M | |

**Extra features worth adding (beyond SQLyog):** dark mode, command palette (⌘K), query cancel button,
result-set diffing, saved-connection folders, `.sql` file tabs with drag-open, read-only "safe mode" for
production connections with confirm-on-write, CLI (`myq query`), MCP/AI-assisted SQL (optional, off by default).

## 5. The left-menu filter (your priority)

SQLyog's object-browser filter is a high-value feature. Design:

1. **Filter box at the top of the tree** — instant, case-insensitive; supports `substring`, `prefix*`,
   `/regex/`, and `type:table`, `db:shop`, `col:email` prefixes.
2. **Scope toggles** — Databases / Tables / Views / Procedures / Functions / Triggers / Events / Columns.
3. **Matching strategy** — client-side over a cached node index (names only, fetched with one
   `information_schema` query per database on first expand/"index all" action), so typing never hits the server.
4. **Tree behavior** — matches shown with ancestors auto-expanded; non-matches collapsed/hidden; match text
   highlighted; count badge per database; `Esc` clears; filter state persists per connection.
5. **Large servers** — 10k+ tables: virtualized tree, debounce 80 ms, indexed in a Rust task and
   returned as a flat match list.
6. **Saved filters** (stretch) — e.g. "legacy_*" pinned to a connection.

## 6. Phased roadmap

| Phase | Name | Deliverable | Est. |
|---|---|---|---|
| 0 | Foundation | Install Rust toolchain, Cargo workspace, Tauri shell, CI, signing plan, license, design tokens | 1 wk |
| 1 | Connect & browse (MVP-0) | Connection manager (Keychain), SSL, object tree **with filter**, multi-tab | 3 wk |
| 2 | Query | SQL editor, run/cancel, multi-result tabs, history, favorites, charset, session restore | 3 wk |
| 3 | Data grid | Paged/streaming grid, inline edit, form view, FK lookup, BLOB viewer, export CSV/JSON/SQL | 4 wk |
| 4 | Schema tools | Table designer (create/alter/dup/truncate/drop), index/FK editors, SQL preview, user & privilege manager | 5 wk |
| 5 | Editor intelligence | Autocomplete, formatter, EXPLAIN/profiler | 4 wk |
| 6 | Connectivity & ops | SSH tunnel, diagnostics (processlist/kill, variables, logs, CHECK/OPTIMIZE) | 3 wk |
| 7 | Import/export & backup | Import CSV/SQL, XLSX/XML/HTML export, backup to zip, file-per-table | 3 wk |
| 8 | **Public beta 0.1** | Signed `.dmg`, Homebrew cask, docs site | 1 wk |
| 9 | Search & optimize | Global data search, redundant index analyzer, type recommender | 3 wk |
| 10 | Visual tools | Query builder, ER designer (reverse + forward) | 8 wk |
| 11 | Sync | Schema sync, data compare, one-way then two-way data sync | 8 wk |
| 12 | Automation | `myq-agent`, launchd schedules, email reports, XML/TOML jobs | 4 wk |
| 13 | Extras | ODBC import, HTTP tunnel, plugin API | open |

Roughly 6–7 months for a solo developer to 0.1 + automation; ~12 months for full list. MVP usable daily at end of Phase 3 (~10 wk).

## 7. Risks

- **Scope**: the list is a multi-year commercial product. Mitigation: ship at Phase 3 and iterate.
- **Safe DDL**: generated `ALTER`s can lose data. Always show SQL preview; confirm destructive ops; offer backup first.
- **Editable grid correctness**: updates must be keyed by primary key, typed per column, and handle NULL vs empty string, binary, and timezones.
- **Server variance**: MySQL 5.7/8/9, MariaDB, Aurora, TiDB differ in privileges, `information_schema`, and profiling. Maintain a capability matrix and test against each in CI (Docker).
- **Credentials**: Keychain only; never plaintext in SQLite or logs.
- **Legal**: don't use the SQLyog name, logo, icons, or screenshots. "MyLoDB" is distinct from "SQLyog"; still run a trademark and domain search before launch.

## 8. Testing strategy

- Unit tests per crate (schema diff, DDL generation, formatter golden files).
- Integration tests against real MySQL 8 / MariaDB 11 containers (never the user's dev DB).
- Round-trip tests: introspect → generate DDL → apply to empty DB → introspect again → equal.
- UI smoke tests with Playwright against the Tauri webview.

## 9. Immediate next steps

1. Install Rust: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh` (not yet installed on this machine) and `xcode-select --install` if missing.
2. Confirm decisions: Tauri+React stack, license, and whether to rename.
3. Start Phase 0 → Phase 1 (connection manager + filterable tree).
