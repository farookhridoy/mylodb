# SQLyog Functional UI Snapshot & Feature Reference

### Research notes for designing MyLoDB \| 20 November 2026

> **Scope note:** This is a functional reference based on SQLyog's
> published documentation and representative screenshots. SQLyog is a
> Windows application; menu names, toolbar icons, shortcuts and feature
> availability can vary by version and licence edition. This is not a
> pixel-perfect inventory of every release. Shortcuts below are SQLyog's
> documented Windows shortcuts and should not be copied directly as
> macOS shortcuts.

## 1. Main application layout

![SQLyog main interface showing Object Browser, SQL Window and Result
Window](https://assets.databasejournal.com/uploads/2021/12/SQLyog2.jpg)

The classic workspace is organised around three main areas:

-   **Object Browser (left):** server, databases, tables and other
    database objects in a navigable tree.
-   **SQL Window (upper/right):** one or more query-editor tabs, SQL
    text, autocomplete and script editing.
-   **Result Window (lower/right):** query results, table data, object
    information, history, messages and profiler output.

The title bar identifies the active connection/host. Query tabs can be
opened side-by-side with multiple connections. The status bar reports
general activity, execution/total time, row count, editor cursor
line/column and connection count. Query and connection tabs can be
rearranged by drag-and-drop.

## 2. Connection management

SQLyog connects to MySQL and MariaDB servers. The connection workflow
stores a connection profile and then opens a workspace for that server.

Typical connection options and related capabilities documented by SQLyog
include:

-   Direct TCP/IP connection using host, port, username and password.
-   SSH tunnelling and HTTP tunnelling for servers not directly
    reachable from the client.
-   SSL connection support.
-   LDAP/PAM-related authentication support in applicable
    editions/configurations.
-   Multiple simultaneous connections, each represented by a tab; switch
    between servers without closing the application.
-   Connection-specific Object Browser and query tabs.

**Operational flow:** create/select a saved connection → provide
credentials and tunnel/SSL settings if needed → connect → browse server
objects → open a query tab or table data view.

## 3. Object Browser and its context menus

![SQLyog Object Browser and table context
menu](https://s3.amazonaws.com/helpscout.net/docs/assets/546d870be4b07e8a335a0e7c/images/5714cc8fc697911a6f039fed/file-euLH3Y1vcW.png)

The Object Browser presents the connected server as a tree. Expanding a
database exposes tables, views, stored procedures, functions, triggers
and events. Expanding a table exposes columns and indexes. Metadata is
fetched as needed rather than requiring every object to be fully loaded
up front.

Right-clicking a node opens a context-sensitive menu. The exact entries
depend on the selected object, server capabilities and edition.
Documented operations include:

-   Refresh the browser; create a database.
-   Open table data; open an object in a new tab.
-   Create, alter, rename, truncate or drop tables and other supported
    objects.
-   Manage indexes, foreign keys and relationships.
-   Reorder columns and inspect table diagnostics.
-   Copy a table/database to another host or database.
-   Start data search, import, export or backup workflows.
-   View object information and generate schema documentation.

A browser filter helps locate objects in large schemas. Keyboard focus
can be moved to the browser and its filter.

## 4. SQL Window / query editor

The SQL Window is a multi-tab editor for ad-hoc SQL and larger scripts.

-   Create multiple query tabs and rename them.
-   Open SQL files in a new tab or the current tab; save scripts.
-   Syntax-aware editing, autocomplete and object/tag completion.
-   SQL formatting and configurable editor fonts/colours.
-   Find, replace, go to line, undo/redo, comment/uncomment and case
    conversion.
-   SQL templates and Favorites for reusable snippets.
-   Execute the current statement, selected SQL, or all statements in
    the editor.
-   Multiple statements separated by semicolons can produce separate
    result tabs.
-   Query execution feedback is shown in the Result Window and status
    bar.

**Execution choices:** execute current query; execute selected query;
execute all queries; execute and edit an updatable result set. SQLyog
also provides query profiling in supported editions/configurations.

## 5. Result Window and result tabs

![SQLyog Result Window with editable
grid](https://d33v4339jhl8k0.cloudfront.net/docs/assets/546d870be4b07e8a335a0e7c/images/5714c191c697911a6f039f92/file-NKAwvAKNLn.png)

The Result Window combines query output and supporting tabs. Documented
persistent tabs include **Data**, **Object** and **History**;
query-dependent tabs include **Result**, **Profiler** and **Message**.
Some releases also expose Table Data and Info as visible tabs.

Result presentation modes:

-   **Grid:** spreadsheet-like rows and columns; supports selecting
    cells/rows and editing eligible result sets.
-   **Form:** record-oriented view for inspecting/editing a row.
-   **Text:** text representation of result output.

Additional behaviour:

-   Separate result tabs for multiple result sets.
-   Configurable page size and automatic paging; SQLyog may add a LIMIT
    when paging a query that has no outer LIMIT.
-   Filtering/searching result data.
-   Copy a cell, selected rows or all rows to clipboard; configure
    delimiters, quoting/escaping, line endings and whether column names
    are included.
-   Export result sets.
-   BLOB viewer/preview for binary or large object fields.
-   Editable results only where the result set can be updated safely;
    not every query result is editable.

## 6. Editing table data

SQLyog offers direct data browsing and editing through table data and
eligible query result grids.

-   Browse table rows in a grid; page through large tables.
-   Insert, edit and delete rows using grid/form workflows.
-   Use an insert/update dialog for record-level editing.
-   Edit ENUM/SET values through appropriate grid controls.
-   View BLOB content separately.
-   Copy selected cells/rows or duplicate a row (while respecting
    auto-increment and other uniqueness constraints).
-   Refresh data and adjust visible columns/widths.

**Important design implication:** a GUI should clearly distinguish
read-only results from editable results, show pending changes, and
require explicit commit/save or rollback where applicable.

## 7. Table design and operations

Table design workflows cover:

-   Create table and define columns, data types, lengths, defaults,
    nullability, primary keys and auto-increment.
-   Alter an existing table and its columns.
-   Manage indexes and foreign keys.
-   Reorder columns.
-   Rename, truncate or drop a table.
-   Change storage engine where supported by the server.
-   Inspect table diagnostics and advanced table properties.
-   Preview generated CREATE/ALTER SQL in supported designer flows.

**Terminology:** truncate removes table rows while retaining the table
definition; drop removes the table object itself. These destructive
actions should be visually distinct.

![SQLyog table designer / advanced table
properties](https://cdn.appmus.com/images/763df00c22a7adc1e552656ebe6c6e49.jpg)

## 8. Database management

Database-level operations include creating, altering and dropping
databases; refreshing the Object Browser; copying databases between
hosts; generating schema documentation; importing external data; backing
up to SQL dump; restoring from SQL dump; and flushing supported server
resources such as hosts, logs, privileges and tables.

Some actions are server-wide or potentially disruptive. A modern
implementation should show the target server/database prominently and
confirm destructive or global operations.

## 9. Indexes, foreign keys and relationships

-   Create, alter and remove indexes.
-   Define primary, unique and ordinary indexes.
-   Define foreign keys and referential actions supported by the
    server/storage engine.
-   Inspect table relationships.
-   Use relationships to assist visual query construction and schema
    diagrams.

## 10. Other database objects

SQLyog's Object Browser and object editors cover:

-   Views.
-   Stored procedures.
-   Functions.
-   Triggers.
-   Events.

Users can browse, create and alter these objects where the server
version and privileges permit. The SQL editor remains available for
direct DDL when the GUI editor is insufficient.

## 11. Query Builder

![SQLyog Query Builder with joined
tables](https://media.trustradius.com/product-screenshots/Mw/zl/0ZLL8H6PUQTG.png)

The visual Query Builder has three principal regions:

1.  **Canvas:** tables/views are added and positioned; joins are shown
    as connecting lines.
2.  **Grid:** select columns, aliases, sort order, grouping, aggregate
    functions and criteria.
3.  **SQL preview/editor:** generated SQL is displayed for review; the
    query can be copied to a normal SQL editor.

Typical workflow:

-   Add tables/views by dragging them from the browser or selecting
    them.
-   Existing foreign-key relationships can be suggested as joins.
-   Create a manual join by connecting columns; right-click a join to
    change join type or remove it.
-   Select fields, aliases, filters, grouping, HAVING and ORDER BY.
-   Review generated SQL, then copy/run it in the normal editor.

Supported query construction includes common joins, WHERE/HAVING
criteria, sorting, aliases and aggregate/common functions. Complex SQL
should remain editable in the SQL Window rather than forcing every query
into a visual representation.

## 12. Schema Designer

![SQLyog Schema Designer relationship
canvas](https://dbmstools.com/storage/screenshots/sqlyog-qwajyjudi8ez3iuy.png)

The Schema Designer displays tables and relationships on a visual
canvas. Tables can be added, moved, resized and arranged. Per-table
controls expose table alteration, indexes, foreign keys and display
options such as names versus data types. The layout is stored separately
as an XML-based layout file.

**Critical behaviour from the documentation:** the designer operates
against the connected database; changes are applied to the live database
rather than being only an offline model. This is an important safety
consideration for MyLoDB: an offline draft/preview mode with generated
migration SQL would be a safer modern alternative.

## 13. Data Search

The Data Search tool searches data across selected database
objects/tables and helps locate values without manually opening each
table. It is available as a dedicated tool/tab and can also be launched
from object context menus. Exact options vary by version; use the
installed version's UI/help for the precise search scope and supported
data types.

## 14. Import external data

SQLyog provides import workflows for external sources and file-based
data. Documented capabilities include importing CSV and external ODBC
data sources in applicable editions. Import settings can include
field/column mapping and data-handling options. HTTP-tunnel connections
may have additional foreign-key-check considerations.

## 15. Export, backup and restore

Export and backup options include:

-   Export table data and query result sets.
-   CSV and other tabular formats; HTML/XML export options in documented
    releases.
-   SQL dump backup of databases/tables.
-   Restore from SQL dump.
-   Schema export/documentation, including HTML schema output.
-   Chunked export settings for large datasets.

A backup workflow should let the user select scope
(server/database/table), structure/data options, output path, encoding
and large-data behaviour. Restore should display the target connection
and database before execution.

## 16. Database copy, schema sync and data comparison

Power Tools / Ultimate-level capabilities include:

-   Copy database/table data between hosts or databases.
-   Compare and synchronise data between databases.
-   Compare and synchronise schemas.
-   Migration toolkit for moving data from external sources in supported
    configurations.

These are edition-dependent features. A sync tool typically compares
source and target, presents differences, generates a change plan and
applies selected changes. MyLoDB should make source/target direction and
destructive changes unmistakable.

## 17. SQL Scheduler and Reporting

The SQL Scheduler and Reporting tool can schedule SQL execution and
generate reports, with email/report delivery features documented for the
Power Tools/Ultimate offering. This is not a basic query-editor function
and should be treated as an advanced/edition-specific module.

## 18. Scheduled backups and Job Agent

SQLyog includes scheduled database backup workflows in supported
editions. The Job Agent supports running scheduled jobs from the command
line/agent environment. Exact deployment requirements, licensing and
command-line arguments depend on the SQLyog release; consult the
matching release documentation before implementing an equivalent.

## 19. User and privilege management

User Manager supports creating/editing users and managing privileges at
server, database, table and column levels, subject to server
capabilities and the connected account's permissions. The interface
should clearly show the account being modified and the scope of each
privilege.

## 20. Server administration tools

Documented server utilities include process-list viewing and killing
selected processes, viewing server status and variables, and flushing
supported resources (hosts, logs, privileges and tables). These are
privileged operations and should be guarded by clear confirmations and
permission checks.

## 21. Favorites, History, Info and Profiler

-   **Favorites:** save frequently used SQL snippets and retrieve them
    later.
-   **History:** inspect queries previously executed in the current
    connection/session context.
-   **Info:** view details for the selected connection, database, table
    or object; searchable, with tabular/HTML and plain-text presentation
    options documented.
-   **Profiler:** inspect query profiling information when enabled and
    supported.
-   **Messages:** review execution messages, warnings and errors.

## 22. Preferences and customisation

SQLyog preferences are grouped into tabs such as General, Power Tools,
Font & Editor, SQL Formatter and Others. Documented settings include:

-   Editor fonts/colours, word wrap, autocomplete and tag/template
    directories.
-   SQL formatter line-break and formatting behaviour.
-   Backticks and SQL editor focus behaviour.
-   Prompting for unsaved changes; restore previous session.
-   JSON formatting and object-name insertion behaviour.
-   Transactions for GUI batch operations.
-   Foreign-key checks for HTTP import.
-   Halt-on-error and warning behaviour.
-   Bulk INSERT size and export chunk size.
-   Result paging size, automatic refresh, column widths and truncated
    headers.
-   Warning before multi-row updates.
-   Theme choices in documented releases.

## 23. Shortcut reference (Windows)

These are SQLyog's published Windows shortcuts, not macOS mappings.

  --------------------------------------------------------------------------
  Area                    Shortcut                Action
  ----------------------- ----------------------- --------------------------
  Connection              Ctrl+M                  New connection

  Connection              Ctrl+N                  New connection using
                                                  current properties

  Connection              Ctrl+F4 / Ctrl+W        Disconnect current
                                                  connection

  Connection              Ctrl+Tab /              Next / previous connection
                          Ctrl+Shift+Tab          

  Connection              Ctrl+1...8 / Ctrl+9     Select connection tab

  Browser                 F5                      Refresh Object Browser

  Browser                 Ctrl+B / Ctrl+Shift+B   Focus browser / browser
                                                  filter

  Browser                 Shift+{-}               Collapse all

  SQL editor              Ctrl+T                  New query editor

  SQL editor              Ctrl+E                  Focus SQL Window

  SQL editor              Ctrl+O                  Open file in new tab

  SQL editor              Ctrl+Shift+O            Open file in current tab

  SQL editor              Ctrl+H / Ctrl+G         Replace / Go to

  SQL editor              Ctrl+Y / Ctrl+Z         Redo / Undo

  SQL editor              Ctrl+Shift+C /          Comment / uncomment
                          Ctrl+Shift+R            

  SQL editor              Ctrl+Shift+T            Templates

  Execute                 F9                      Execute current query

  Execute                 Ctrl+F9                 Execute selected query

  Execute                 F8                      Execute and edit result
                                                  set

  Execute                 F12                     Format current query

  Execute                 Ctrl+F12                Format selected query

  Execute                 Shift+F12               Format all queries

  Results                 Ctrl+R                  Focus Result Window

  Results                 Ctrl+L                  Toggle grid/text mode

  Results                 Ctrl+Alt+L              Grid/form mode

  Results                 Ctrl+Alt+C              Export table data

  Results                 Ctrl+Shift+E            Export result set

  Results                 Ctrl+Alt+E              Backup as SQL dump

  Database/table          Ctrl+D                  Create database

  Database/table          F6                      Alter
                                                  database/table/object

  Database/table          F2                      Rename
                                                  table/view/trigger/event

  Database/table          Shift+Delete            Truncate database/table
                                                  (context-dependent)

  Database/table          F7                      Manage indexes

  Database/table          F10                     Relationships / foreign
                                                  keys

  Database/table          Ctrl+Alt+R              Reorder columns

  Database/table          Ctrl+Alt+T              Table diagnostics

  Database/table          Ctrl+Alt+F              Flush dialog

  Power Tools             Ctrl+Alt+W              Database synchronisation

  Power Tools             Ctrl+Q                  Schema synchronisation

  Power Tools             Ctrl+Alt+O              Migration toolkit

  Power Tools             Ctrl+Alt+N              SQL Scheduler and
                                                  Reporting

  Power Tools             Ctrl+Alt+S              Scheduled Backup Wizard

  Other tabs              Ctrl+K                  Query Builder

  Other tabs              Ctrl+Alt+D              Schema Designer

  Other tabs              Ctrl+Shift+D            Data Search

  Other tabs              Ctrl+Shift+I            Info tab

  Other tabs              Ctrl+Shift+H            History tab

  Users                   Ctrl+U                  Add user

  Users                   Ctrl+Alt+U              Edit user

  Users                   Ctrl+Shift+W            Manage privileges

  Favorites               Ctrl+Shift+F            Add current SQL to
                                                  Favorites

  General                 F1 / Ctrl+S / Alt+F4    Help / Save / Exit
  --------------------------------------------------------------------------

## 24. Edition and availability cautions

Not every feature is available in every SQLyog edition. Core connection,
browsing, SQL editing and common object/data operations should be
distinguished from Power Tools/Ultimate capabilities such as advanced
schema/data sync, visual data compare, migration toolkit, scheduled
backups and scheduler/reporting. Confirm the exact edition/version
before treating any advanced feature as universally available.

## 25. MyLoDB product design checklist

  -----------------------------------------------------------------------
  SQLyog capability                   MyLoDB recommendation
  ----------------------------------- -----------------------------------
  Multi-connection tabs               Must-have; native macOS tab/window
                                      behaviour

  Object Browser                      Must-have; lazy metadata loading,
                                      search/filter, context menus

  SQL editor                          Must-have; multi-tab, autocomplete,
                                      formatter, templates, file tabs

  Query execution                     Must-have; current/selection/script
                                      execution, cancellation, clear
                                      status

  Results grid                        Must-have; virtualised rows,
                                      paging, sorting, filtering,
                                      copy/export

  Editable results                    Must-have with explicit
                                      read-only/editable state and
                                      transaction safety

  Table designer                      Must-have; preview generated DDL
                                      before applying

  Index/FK editor                     Must-have; relationship inspection

  Views/routines/triggers/events      Must-have for a complete
                                      MySQL/MariaDB manager

  Query Builder                       Valuable; visual joins plus SQL
                                      preview and round-trip to editor

  Schema Designer                     Valuable; start with read-only ERD,
                                      then safe migration-plan workflow

  Data Search                         Valuable; cross-table search with
                                      scope and type filters

  Import/export                       Must-have; CSV/JSON/SQL and
                                      streaming for large datasets

  Backup/restore                      Must-have; progress, cancellation,
                                      validation and restore safeguards

  Schema/data sync                    Advanced; diff-first, reviewable
                                      plan, explicit direction

  Scheduler/Job Agent                 Later; consider macOS background
                                      jobs and launch-at-login controls

  User/privilege manager              Valuable; scope-aware permission
                                      editor

  Process list/status/variables       Valuable admin tools

  Favorites/history/info/profiler     Must-have productivity layer

  Preferences                         Must-have; editor, result grid,
                                      connections, safety and appearance
  -----------------------------------------------------------------------

### Suggested MyLoDB navigation model

-   **File:** New Connection, New Query, Open, Save, Save As, Export,
    Close Tab, Quit.
-   **Edit:** Undo/Redo, Cut/Copy/Paste, Find/Replace, Select All,
    Preferences.
-   **View:** Object Browser, SQL Editor, Results, Info, History,
    Profiler, Console; layout presets.
-   **Connection:** Connect, Disconnect, Reconnect, Connection Manager,
    SSH/SSL settings, session variables.
-   **Database:** Create, Alter, Drop, Refresh, Backup, Restore, Import,
    Export, Compare/Sync.
-   **Table:** Browse Data, Structure, Create, Alter, Rename, Truncate,
    Drop, Indexes, Foreign Keys, Diagnostics.
-   **Tools:** Query Builder, Schema/ER Designer, Data Search, User
    Manager, Process List, Server Status, Scheduler.
-   **Window:** connection/query/result tab navigation and window
    management.
-   **Help:** Documentation, keyboard shortcuts, diagnostics, update
    checker, About.

This is a proposed MyLoDB information architecture, not a claim that
SQLyog uses these exact menu groupings in every release.

## 26. Reference links

1.  [SQLyog Knowledge Base --- documentation
    home](https://sqlyogkb.webyog.com/)
2.  [SQLyog User
    Interface](https://sqlyogkb.webyog.com/article/36-sqlyog-user-interface)
3.  [Object
    Browser](https://sqlyogkb.webyog.com/article/39-object-browser)
4.  [SQL Window](https://sqlyogkb.webyog.com/article/45-sql-window)
5.  [Result Tab](https://sqlyogkb.webyog.com/article/65-result-tab)
6.  [Query
    Builder](https://sqlyogkb.webyog.com/article/58-the-sqlyog-query-builder)
7.  [Schema
    Designer](https://sqlyogkb.webyog.com/article/166-the-sqlyog-schema-designer)
8.  [Preferences](https://sqlyogkb.webyog.com/article/131-preferences)
9.  [Keyboard
    Shortcuts](https://sqlyogkb.webyog.com/article/41-keyboard-shortcuts)
10. [SQLyog feature
    overview](https://sqlyogkb.webyog.com/article/23-what-is-sqlyog)
11. [Webyog documentation
    categories](https://webyog.com/sqlyog-documentation/article/164-object-browser/)

------------------------------------------------------------------------

**Use for MyLoDB planning:** Treat this as a functional coverage
checklist. Before matching exact menu names, button positions or
shortcut behaviour, compare against the specific SQLyog version/edition
you want MyLoDB to benchmark. The embedded screenshots are
representative historical UI examples; they are not a current macOS UI
reference.
