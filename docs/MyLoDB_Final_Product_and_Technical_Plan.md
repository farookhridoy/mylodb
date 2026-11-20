# MyLoDB --- Product Strategy & Technical Build Plan

**Product:** MyLoDB\
**Category:** macOS database management application\
**Initial database support:** MySQL 5.7/8/9 and MariaDB\
**Primary platforms:** macOS (Apple Silicon and Intel); Linux and
Windows later\
**Core technology:** Tauri 2, Rust, React, TypeScript\
**Positioning:** A fast, safe, developer-first database workspace with a
polished macOS experience.

------------------------------------------------------------------------

## 1. Product vision

MyLoDB aims to combine the strongest ideas from established database
tools while maintaining a focused MySQL/MariaDB-first product.

The product should bring together:

-   **DataGrip-inspired intelligence:** SQL autocomplete, inspections,
    refactoring and developer workflows.
-   **DBeaver-inspired data management:** import/export, background
    transfers, data comparison and enterprise-scale operations.
-   **TablePlus-inspired experience:** clean macOS interface,
    keyboard-first navigation and straightforward workflows.
-   **Beekeeper Studio-inspired usability:** approachable data editing,
    JSON inspection and extensible result presentation.

MyLoDB should not try to support every database engine immediately. It
should become exceptionally good at MySQL and MariaDB first.

### Four product promises

1.  **Fast** --- browse and export very large datasets without freezing
    the application or exhausting memory.
2.  **Safe** --- protect production databases and make destructive
    operations deliberate.
3.  **Native-feeling** --- deliver a polished, keyboard-first macOS
    experience.
4.  **Developer-first** --- provide strong SQL, schema, data and
    performance workflows.

------------------------------------------------------------------------

## 2. Scope and product decisions

  -----------------------------------------------------------------------
  Area                    Decision                Rationale
  ----------------------- ----------------------- -----------------------
  Product approach        Feature parity where    Learn from established
                          useful, original        workflows without
                          implementation and UI   copying code or assets

  Initial databases       MySQL 5.7/8/9 and       Keep the first release
                          MariaDB                 focused

  Initial platform        macOS arm64 and x64     Optimize for the
                                                  primary target

  Later platforms         Linux and Windows       Consider after the
                                                  macOS product is stable

  UI                      Tauri 2 + React +       Mature editor and grid
                          TypeScript              ecosystem with a Rust
                                                  core

  Database engine         Rust                    Async operations,
                                                  streaming and memory
                                                  control

  Credentials             macOS Keychain          Avoid plaintext secrets
                                                  in local storage

  Local application data  SQLite                  Connections, history,
                                                  favorites and session
                                                  state

  Licensing               Decide before the first Choose Apache-2.0 or
                          public release          GPL-3.0 based on
                                                  distribution goals
  -----------------------------------------------------------------------

Do not use SQLyog, DataGrip, DBeaver or other products' code, logos,
icons, screenshots or proprietary assets. Implement original code and
visual design.

------------------------------------------------------------------------

## 3. Recommended technology stack

### Desktop and frontend

-   Tauri 2
-   React 19
-   TypeScript
-   CodeMirror 6 for the SQL editor
-   TanStack Table and TanStack Virtual for data grids
-   React Flow for visual schema diagrams
-   Zustand for UI state
-   Tauri bundler for `.dmg` packaging
-   Code signing and notarization for distribution

### Rust backend

  Concern                    Recommended technology
  -------------------------- ---------------------------------------------
  Async runtime              Tokio
  MySQL driver               `mysql_async`
  SSH tunnel                 `russh`
  SQL parsing                `sqlparser-rs` with MySQL-specific handling
  Local store                `rusqlite`
  Secrets                    `keyring` / macOS Keychain integration
  CSV                        Streaming CSV writer
  XLSX                       `rust_xlsxwriter`
  JSON                       `serde_json`
  XML                        `quick-xml`
  Compression and archives   `zip` and suitable streaming compression
  Email automation           `lettre`

Validate crate maintenance, platform compatibility and licensing before
locking dependencies.

------------------------------------------------------------------------

## 4. Proposed architecture

``` text
apps/
  desktop/                 Tauri shell + React UI

crates/
  myq-core/                 Domain types, errors, connection models
  myq-driver/               MySQL driver, pooling, streaming, cancellation
  myq-tunnel/               SSH and TLS transport
  myq-schema/               Introspection, DDL generation, schema diff
  myq-sql/                  SQL parsing, formatting, autocomplete
  myq-data/                 Grid paging, editing, filtering, comparison
  myq-stream/               Bounded streaming and backpressure
  myq-io/                   Import, export, backup and restore
  myq-profiler/             EXPLAIN, query statistics and analysis
  myq-safety/               Risk analysis and production safeguards
  myq-store/                SQLite local application storage
  myq-agent/                Headless scheduled job runner
```

### Dependency rules

-   React UI calls Tauri commands.
-   Tauri commands call Rust domain crates.
-   Domain crates must not depend on the UI.
-   Long-running work runs as cancellable tasks.
-   Progress and status are emitted using task IDs.
-   Large result sets must not be transferred to the UI as one enormous
    payload.

### Long-running task model

Each background operation should have:

-   Stable task ID
-   Operation type and source connection
-   Progress state
-   Rows and bytes processed where measurable
-   Start time and elapsed time
-   Cancellation state
-   Error details
-   Output location when applicable

The UI should remain responsive while tasks run.

------------------------------------------------------------------------

## 5. Competitor analysis and lessons

This section is a product-planning comparison, not a claim that every
competitor feature has been independently benchmarked.

### 5.1 JetBrains DataGrip --- SQL intelligence

**Strengths to study**

-   Schema-aware SQL autocomplete
-   SQL inspections and error detection
-   SQL refactoring
-   Local History and recovery
-   Git integration for SQL files
-   Result-set comparison
-   Query result visualization
-   Metadata introspection and configurable fetching

**MyLoDB opportunities**

1.  Add SQL inspections before execution.
2.  Preserve local SQL file history and allow recovery.
3.  Support Git workflows for `.sql` files.
4.  Improve alias-aware, schema-aware autocomplete.
5.  Allow query results to be visualized as charts.
6.  Support local filtering of fetched result data.

### 5.2 DBeaver --- data management and transfer

**Strengths to study**

-   Data transfer wizard
-   Table-to-table and database-to-database transfer
-   Configurable fetch size
-   Segmented extraction
-   Background exports
-   Reusable transfer tasks
-   Data and column statistics
-   Mock data generation
-   Multiple data views

**MyLoDB opportunities**

1.  Build a robust background transfer engine.
2.  Save and rerun import/export configurations.
3.  Support chunked exports and migrations.
4.  Add column statistics and data profiling.
5.  Add realistic mock data generation.
6.  Support database-to-database migration workflows.

### 5.3 TablePlus --- macOS workflow and safety

**Strengths to study**

-   Clean, compact desktop interface
-   Keyboard-first workflows
-   Quick object navigation
-   Safe mode
-   Staged changes and commit/discard workflow
-   Quick filters
-   Multiple windows and panes
-   Database metrics and dashboards
-   AI/MCP-related workflows

**MyLoDB opportunities**

1.  Make the interface feel at home on macOS.
2.  Add a command palette and quick object navigation.
3.  Stage data edits before applying them.
4.  Show generated SQL before changes are committed.
5.  Make production safety visible at all times.
6.  Support multiple windows and split workspaces.

### 5.4 Beekeeper Studio --- approachable data editing

**Strengths to study**

-   Friendly SQL editor and data grid
-   Editable query results
-   Staged changes
-   JSON inspection
-   Custom result presentation
-   Shareable query workflows
-   Extensibility

**MyLoDB opportunities**

1.  Make query results editable when source-row identity is reliable.
2.  Add a JSON tree inspector.
3.  Support result formatting for links, images and indicators.
4.  Keep editing workflows simple and discoverable.

### Competitive positioning

  -------------------------------------------------------------------------------
  Capability     DataGrip          DBeaver          TablePlus      MyLoDB plan
  -------------- ----------------- ---------------- -------------- --------------
  SQL            Strong            Available        Available      Planned
  autocomplete                                                     

  SQL            Strong            Partial/basic    Basic          Add
  inspections                                                      

  Editable data  Yes               Yes              Yes            Planned
  grid                                                             

  Record/form    Yes               Yes              Yes            Planned
  view                                                             

  JSON/BLOB      Yes               Yes              Yes            Expand
  inspection                                                       

  ER diagrams    Yes               Yes              Yes            Planned

  Schema         Yes               Yes              Yes            Planned
  comparison                                                       

  Data           Yes               Yes              Varies         Planned
  comparison                                                       

  Advanced       Available         Strong           Available      Expand
  transfer                                                         

  Scheduled jobs Via               Available        Available      Planned
                 tools/workflows                                   

  Git SQL        Strong            Available        More limited   Add
  workflow                                                         

  Native Mac     IDE-style         Cross-platform   Strong         Core goal
  focus                                                            
  -------------------------------------------------------------------------------

Feature availability and implementation details vary by edition and
version. Verify exact behavior against current vendor documentation
before using this table for marketing claims.

------------------------------------------------------------------------

## 6. Large Data Engine --- highest engineering priority

The defining technical challenge is handling large tables and query
results without freezing the UI or exhausting Mac memory.

### 6.1 Smart data loading

When a user opens a table:

1.  Fetch an initial configurable batch, for example 500 rows.
2.  Render only visible rows and columns.
3.  Fetch additional batches as the user scrolls.
4.  Keep a bounded cache.
5.  Release old batches according to a cache policy.
6.  Allow cancellation of in-flight fetches.

Required capabilities:

-   Row virtualization
-   Column virtualization
-   Lazy loading
-   Configurable fetch size
-   Bounded memory use
-   Query cancellation
-   Background fetching
-   Clear loading and error states

**Important:** virtual scrolling reduces rendering work; it does not
automatically reduce database traffic or memory use. The Rust data layer
must also bound buffers and caches.

### 6.2 Pagination strategies

Support multiple pagination approaches and select an appropriate default
based on the query and available keys.

  -----------------------------------------------------------------------
  Strategy                            Best use
  ----------------------------------- -----------------------------------
  `LIMIT` + `OFFSET`                  Simple page navigation and
                                      small/medium datasets

  Keyset pagination                   Deep navigation through large
                                      indexed tables

  Primary-key cursor                  Sequential browsing

  Streaming result                    Continuous query consumption and
                                      exports

  Chunked extraction                  Large exports, backups and
                                      migrations
  -----------------------------------------------------------------------

Example offset pagination:

``` sql
SELECT *
FROM transactions
ORDER BY id
LIMIT 500 OFFSET 500000;
```

Example keyset pagination:

``` sql
SELECT *
FROM transactions
WHERE id > 500000
ORDER BY id
LIMIT 500;
```

Keyset pagination can be more efficient for deep navigation when the
cursor columns are indexed and ordering is stable.

For non-unique sorting columns, use a stable compound cursor, such as
`(created_at, id)`.

Expose the selected strategy and allow advanced users to override it. Do
not silently rewrite arbitrary SQL when the semantics cannot be
preserved.

### 6.3 Background export engine

Export must never require loading the entire result set into memory.

Recommended workflow:

1.  User chooses a table or enters a SQL query.
2.  User chooses output format.
3.  User selects columns, filters and options.
4.  Rust starts a background export task.
5.  Rows are fetched in bounded batches.
6.  Data is written incrementally to disk.
7.  UI reports progress and elapsed time.
8.  User can cancel the export without blocking other work.

Export formats:

-   CSV
-   JSON / JSON Lines
-   SQL
-   XLSX
-   XML
-   HTML

Configurable options:

  Option         Examples
  -------------- -----------------------------------
  Fetch size     1,000 / 5,000 / 10,000
  Export mode    Single query / segmented
  Compression    None / GZIP
  Encoding       UTF-8
  Output         CSV / JSON / SQL / XLSX
  Progress       Rows, bytes and elapsed time
  Cancellation   Stop task and safely close output

Do not claim an exact completion percentage unless total work is known.
A full `COUNT(*)` may itself be expensive.

Parallel export should be optional and bounded. It can increase server
load. Multiple independent queries may also fail to represent one
consistent point-in-time snapshot. Offer a consistent-snapshot mode
where the database and query pattern support it.

### 6.4 Smart data grid

Provide four complementary views:

  View              Purpose
  ----------------- ---------------------------------------
  Grid View         Spreadsheet-like browsing and editing
  Record View       Inspect one complete record
  JSON View         Expand and inspect nested JSON
  Text / Hex View   Inspect text and binary values

Additional features:

-   Freeze columns
-   Hide and reorder columns
-   Multi-column sorting
-   Advanced filter builder
-   Copy selected rows as SQL, JSON or CSV
-   Search loaded data locally
-   Search across the table on the server
-   Foreign-key navigation
-   Column statistics
-   Bulk editing with preview

Only allow editing when the application can reliably map a result row
back to its source record. Use primary keys or unique keys and original
values to generate safe updates. Clearly mark ambiguous or non-updatable
result sets as read-only.

### 6.5 Large BLOB and JSON handling

Avoid loading all binary values into the grid.

For BLOBs:

-   Show type and size
-   Load only on demand
-   Preview supported formats
-   Inspect hexadecimal content
-   Save to file

For JSON:

-   Pretty-print
-   Tree view with expand/collapse
-   JSON path search
-   Copy selected paths or values
-   Avoid expanding huge documents automatically

------------------------------------------------------------------------

## 7. Production safety --- signature feature

MyLoDB should make production status unmistakable and dangerous
operations deliberate.

### Connection-level safety

-   Read-only mode
-   Production/staging/development labels
-   Visible environment color and badge
-   Destructive-query warnings
-   Query execution timeout
-   Query cancellation
-   SQL execution history
-   Optional backup workflow before destructive operations

### Staged editing

Recommended workflow:

1.  User edits a row.
2.  MyLoDB records the proposed change locally.
3.  User reviews the generated SQL.
4.  User commits or discards the staged change.
5.  MyLoDB reports success or failure.

### Destructive SQL review

Detect and review operations such as:

-   `DROP`
-   `TRUNCATE`
-   `DELETE`
-   `UPDATE` without a sufficiently restrictive predicate
-   Schema changes with potentially destructive effects

Show the SQL, connection environment and available impact information
before execution.

**Security rule:** SQL parsing and UI warnings are not security
boundaries. Use database privileges and read-only transactions where
supported. Do not rely on a client-side safe-mode toggle as the only
protection.

------------------------------------------------------------------------

## 8. SQL developer workspace

### SQL editor

-   Syntax highlighting
-   Schema-aware autocomplete
-   Alias-aware completion
-   SQL formatter
-   Multiple query tabs
-   Query history
-   Favorites and snippets
-   Multi-statement execution
-   Query cancellation
-   Multiple result sets
-   SQL file tabs and drag-open

### Intelligence

-   SQL inspections
-   Potential error warnings
-   Unqualified-column warnings
-   Risky query warnings
-   Unused alias and reference suggestions
-   SQL refactoring where safe
-   Explain plan integration

### SQL file management

-   Open and save `.sql` files
-   Local History and recovery
-   Git status and diff
-   Search across SQL files
-   Project/folder organization
-   Reusable snippets

### Query performance

Use:

-   `EXPLAIN`
-   `EXPLAIN FORMAT=JSON`
-   `EXPLAIN ANALYZE` where supported
-   `performance_schema`
-   Server-side execution metrics

Do not depend on deprecated `SHOW PROFILE` as the primary profiler.

------------------------------------------------------------------------

## 9. Schema and database management

### Object browser

-   Databases
-   Tables
-   Views
-   Procedures
-   Functions
-   Triggers
-   Events
-   Columns
-   Indexes
-   Foreign keys
-   Users and privileges

Use lazy loading. Avoid fetching every object from a large server
immediately.

### Object tree filter

Build a high-quality filter at the top of the tree.

Requirements:

1.  Case-insensitive substring search.
2.  Prefix and wildcard matching.
3.  Optional regex mode.
4.  Type filters such as `type:table`.
5.  Database filters such as `db:shop`.
6.  Column filters such as `col:email`.
7.  Matching objects displayed with ancestors.
8.  Highlight matching text.
9.  Count matches by database.
10. Persist filters per connection.

For large servers:

-   Cache object names separately from full metadata.
-   Debounce user input.
-   Use an indexed local search structure.
-   Virtualize the tree.
-   Avoid a server request for every keystroke.

### Table designer

-   Create and alter tables
-   Duplicate tables
-   Add, edit and remove columns
-   Configure indexes and foreign keys
-   Configure charset and collation
-   Reorder columns
-   Preview generated DDL
-   Apply changes with confirmation

### Schema comparison and synchronization

-   Compare schemas
-   Show differences by object
-   Generate migration SQL
-   Allow selective application
-   Show destructive changes clearly
-   Provide a review step before execution

### ER designer

-   Reverse engineer schema
-   Visualize tables and relationships
-   Arrange entities
-   Export PNG/PDF
-   Generate DDL from supported diagram changes

### Diagnostics and optimization

-   Process list
-   Kill query
-   Server variables
-   Server status
-   Table diagnostics
-   `CHECK`, `OPTIMIZE`, `REPAIR`, `ANALYZE` where supported
-   Redundant index detection
-   Unused index analysis where privileges and server support allow
-   Index recommendations with explanations

Never automatically drop or modify indexes based solely on a
recommendation.

------------------------------------------------------------------------

## 10. Data comparison and synchronization

### Data comparison

-   Compare tables across connections
-   Compare selected records
-   Match using primary/unique keys
-   Show inserted, updated and deleted records
-   Generate selective SQL changes
-   Allow users to review before applying

### Data synchronization

Start with one-way synchronization.

Later add:

-   Chunked checksums
-   Conflict detection
-   Conflict resolution policies
-   Selective synchronization
-   Two-way synchronization
-   Progress and cancellation

Use stable keys and carefully define behavior for NULL values,
collations, timezones, generated columns and binary data.

------------------------------------------------------------------------

## 11. Import, backup and restore

### Import

-   CSV
-   SQL
-   JSON
-   XML
-   XLSX where practical

Provide:

-   Column mapping
-   Type preview
-   NULL and empty-string handling
-   Encoding selection
-   Delimiter selection
-   Duplicate handling
-   Transaction/batch options
-   Validation errors with row numbers

### Backup and restore

-   SQL dump
-   Compressed backup
-   Table-level backup
-   Database-level backup
-   Scheduled backup
-   Restore wizard
-   Backup verification
-   Progress and cancellation

Large backup operations should stream to disk. Avoid holding complete
dumps in memory.

------------------------------------------------------------------------

## 12. Automation and job runner

Create a separate headless job runner for scheduled work.

Possible jobs:

-   Scheduled backups
-   Scheduled exports
-   Data synchronization
-   Saved query reports
-   Health checks
-   Email reports

On macOS, use `launchd` for scheduling where appropriate.

Job files may use TOML or another documented format.

Each job should support:

-   Connection reference
-   Secure credential retrieval
-   Schedule
-   Operation parameters
-   Output location
-   Retry policy
-   Timeout
-   Logs
-   Notification configuration

Do not store database passwords in plain-text job files.

------------------------------------------------------------------------

## 13. AI and MCP integration

Treat AI features as optional and disabled by default.

Potential capabilities:

-   Explain SQL
-   Generate SQL from natural language
-   Explain query plans
-   Suggest indexes with supporting evidence
-   Summarize schema relationships
-   Generate test data
-   Explain errors
-   Create documentation for tables

### MCP server

An optional MCP server could allow coding agents to interact with
MyLoDB.

Safety requirements:

-   Explicit user enablement
-   Read-only default
-   Connection allowlist
-   Tool-level permissions
-   Query timeout and row limits
-   Sensitive-column controls
-   Audit log
-   Explicit confirmation for writes
-   No credential exposure to the model

AI-generated SQL must be reviewed before execution.

------------------------------------------------------------------------

## 14. Additional product features

  -----------------------------------------------------------------------
  Feature                 Description             Priority
  ----------------------- ----------------------- -----------------------
  Database Health         Connections, slow       High
  Dashboard               queries, table sizes    
                          and index usage         

  Query Performance       Explain plans and query High
  Analyzer                analysis                

  Production Guard        Read-only mode and      Critical
                          destructive-operation   
                          review                  

  Data Compare            Compare production and  High
                          staging records         

  Schema Migration        Generate and inspect    High
  Preview                 migration SQL           

  SQL Scratchpad          Temporary query         Medium
                          workspace               

  Mock Data Generator     Generate test records   Medium

  AI SQL Assistant        Explain, generate and   Medium
                          optimize SQL            

  MCP Server              Connect coding agents   Medium
                          safely                  

  Metrics Board           Custom query-powered    Medium
                          dashboards              

  Query Bookmarks         Organize frequently     High
                          used queries            

  Connection Groups       Organize environments   High
                          and projects            

  Command Palette         Fast keyboard           High
                          navigation              

  Multi-window workspace  Work across connections Medium

  CLI                     Run saved queries and   Medium
                          jobs from terminal      
  -----------------------------------------------------------------------

------------------------------------------------------------------------

## 15. Recommended macOS UX

The application should feel like a professional Mac application rather
than a web dashboard placed inside a desktop window.

### Interface principles

-   Compact, clean layout
-   Native-feeling title bar and window behavior
-   Dark and light themes
-   Keyboard-first navigation
-   Command palette (`⌘K`)
-   Clear connection/environment identity
-   Tabbed SQL workspaces
-   Split panes
-   Persistent session restoration
-   Searchable object tree
-   Customizable shortcuts
-   Accessible contrast and focus states

### Suggested main layout

``` text
┌─────────────────────────────────────────────────────────────┐
│ MyLoDB     Connections      Search       Command Palette    │
├───────────────┬─────────────────────────────────────────────┤
│ Connections   │ SQL Editor                                  │
│               │                                             │
│ Production    │ SELECT * FROM transactions;                 │
│ Staging       │                                             │
│ Development   ├─────────────────────────────────────────────┤
│               │ Results | Messages | Explain                 │
│ Object Tree   │                                             │
│               │ ID      Date          Amount      Status     │
│ Filter...     │ 1001    2026-01-01    5000        Paid       │
│               │ 1002    2026-01-02    2500        Pending    │
│ Tables        │                                             │
│ Views         │                                             │
│ Procedures    │                                             │
└───────────────┴─────────────────────────────────────────────┘
```

------------------------------------------------------------------------

## 16. Recommended development roadmap

Build the core performance and safety foundations early. Do not postpone
streaming architecture until the export phase.

  -----------------------------------------------------------------------
  Phase                   Name                    Main deliverables
  ----------------------- ----------------------- -----------------------
  0                       Foundation              Rust workspace, Tauri
                                                  shell, CI, design
                                                  tokens, license,
                                                  signing plan

  1                       Connect & Browse        Connection manager,
                                                  Keychain, TLS, SSH,
                                                  filterable object tree

  2                       SQL Workspace           Editor, execution,
                                                  cancellation, history,
                                                  favorites, session
                                                  restore

  3                       Data Engine             Virtual grid, lazy
                                                  loading, paging, record
                                                  view, JSON/BLOB
                                                  inspection

  4                       Production Safety       Read-only mode, staged
                                                  changes, SQL preview,
                                                  safeguards

  5                       Large Data              Streaming export,
                                                  chunked processing,
                                                  background tasks

  6                       Developer Intelligence  Autocomplete,
                                                  inspections, formatter,
                                                  EXPLAIN, profiler

  7                       Data Management         Import/export, backup,
                                                  restore, schema and
                                                  data comparison

  8                       Visual Tools            ER designer, query
                                                  builder, charts and
                                                  dashboards

  9                       Automation              Job runner, schedules,
                                                  reports and
                                                  notifications

  10                      Advanced                AI assistant, MCP, CLI,
                                                  plugin API
  -----------------------------------------------------------------------

### MVP release target

The first genuinely useful release should include:

-   Secure MySQL/MariaDB connections
-   Object browser and tree filtering
-   SQL editor and query execution
-   Query cancellation
-   Data grid with bounded paging
-   Basic inline editing
-   CSV export
-   Connection/session persistence
-   Production read-only mode

Do not wait for the ER designer, AI, plugin API or two-way
synchronization to release a useful beta.

------------------------------------------------------------------------

## 17. Priority matrix

### P0 --- Must have

-   Streaming and virtualized data grid
-   Smart pagination
-   Large dataset export
-   Production safe mode
-   Staged editing and SQL preview
-   Query cancellation
-   Secure connection management

### P1 --- Important

-   SQL autocomplete and inspections
-   Query profiler and EXPLAIN
-   JSON, BLOB and record views
-   Schema comparison
-   Data comparison
-   Import/export wizard
-   Git and SQL history
-   Database diagnostics

### P2 --- Later

-   AI assistant
-   MCP server
-   Dashboard builder
-   Mock data generation
-   Two-way synchronization
-   Plugin API
-   Advanced automation

------------------------------------------------------------------------

## 18. Testing and quality strategy

### Database compatibility

Test against real MySQL and MariaDB versions using isolated test
environments.

Maintain a capability matrix for:

-   MySQL 5.7
-   MySQL 8.x
-   MySQL 9.x
-   MariaDB supported versions

Test differences in:

-   System tables and metadata
-   Privilege management
-   EXPLAIN capabilities
-   JSON behavior
-   Index metadata
-   Character sets and collations
-   Transaction and DDL behavior

### Performance testing

Build repeatable test datasets with:

-   100,000 rows
-   1 million rows
-   10 million rows
-   Larger datasets where practical

Measure:

-   Initial table-open latency
-   Scroll responsiveness
-   Peak application memory
-   Rows fetched per request
-   Export throughput
-   Cancellation latency
-   UI responsiveness during background tasks

Use representative narrow and wide tables, large text fields, JSON
columns and BLOB columns.

### Correctness testing

-   Unit tests for schema diff and DDL generation
-   SQL formatter golden tests
-   Integration tests against MySQL and MariaDB
-   Transaction and rollback tests
-   NULL versus empty-string tests
-   Timezone and binary-value tests
-   Round-trip schema tests
-   Export/import integrity tests
-   UI smoke tests

### Reliability

-   Test network interruption
-   Test SSH tunnel failure
-   Test expired credentials
-   Test server restart during a task
-   Test cancellation during streaming
-   Test disk-full errors during export
-   Test application restart with active or incomplete jobs

------------------------------------------------------------------------

## 19. Risks and mitigations

  -----------------------------------------------------------------------
  Risk                                Mitigation
  ----------------------------------- -----------------------------------
  Scope becomes too large             Release a useful MVP before
                                      advanced visual and AI features

  Large datasets exhaust memory       Bounded buffers, streaming and
                                      disk-backed output

  Generated DDL loses data            SQL preview, destructive-change
                                      warnings and backup workflow

  Editable grid updates wrong records Require stable keys and use
                                      original values

  Server versions differ              Capability matrix and integration
                                      tests

  Credentials leak                    Keychain only; redact logs and job
                                      files

  Parallel export overloads server    Bounded concurrency and
                                      user-configurable limits

  Inconsistent exports                Offer snapshot-consistent modes
                                      where supported

  SQL safety bypass                   Database privileges and transaction
                                      controls, not UI warnings alone

  Licensing problems                  Review dependencies and license
                                      before release
  -----------------------------------------------------------------------

------------------------------------------------------------------------

## 20. Immediate next steps

1.  Confirm Tauri 2 + React + TypeScript + Rust.
2.  Decide Apache-2.0 versus GPL-3.0.
3.  Create the Cargo workspace and Tauri application.
4.  Establish a CI pipeline for macOS arm64 and x64.
5.  Implement secure MySQL/MariaDB connections.
6.  Build the filterable object browser.
7.  Implement the SQL execution and cancellation model.
8.  Build bounded streaming and virtualized data-grid foundations.
9.  Add production safety before enabling broad write workflows.
10. Benchmark with large, realistic datasets before expanding the
    feature set.

------------------------------------------------------------------------

## 21. Product definition

**MyLoDB is a MySQL/MariaDB-first database workspace for macOS.**

Its competitive advantage should be the combination of:

-   Large-data performance
-   Safe production operations
-   Excellent SQL developer tools
-   A polished macOS workflow
-   Powerful data import, export and comparison

The most important differentiator to prove early is the **Large Table
Explorer**: open a table with tens of millions of rows, browse using
smart pagination, filter efficiently, inspect individual records and
export the dataset without exhausting MacBook memory.

Build that exceptionally well, and MyLoDB will have a clear reason to
exist alongside established database clients.
