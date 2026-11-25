import { useState, type ReactNode } from "react";
import { ExtLink } from "./AboutDialog";
import { SHORTCUT_GROUPS } from "./shortcuts";
import shot_tour from "../../../docs/images/tour.png";
import shot_connDialog from "../../../docs/images/connection-dialog.png";
import shot_connSafety from "../../../docs/images/connection-safety.png";
import shot_sqliteDialog from "../../../docs/images/sqlite-dialog.png";
import shot_browserInfo from "../../../docs/images/browser-info.png";
import shot_query from "../../../docs/images/query.png";
import shot_palette from "../../../docs/images/command-palette.png";
import shot_tableData from "../../../docs/images/table-data.png";
import shot_databases from "../../../docs/images/databases.png";
import shot_structure from "../../../docs/images/structure.png";
import shot_erDiagram from "../../../docs/images/er-diagram.png";
import shot_queryBuilder from "../../../docs/images/query-builder.png";
import shot_preferences from "../../../docs/images/preferences.png";
import shot_darkTheme from "../../../docs/images/dark-theme.png";

interface Section { id: string; title: string; body: ReactNode }

/** A screenshot of the real interface (the same files the written manual uses). */
const Shot = ({ src, alt }: { src: string; alt: string }) => (
  <figure className="manual-shot"><img src={src} alt={alt} loading="lazy" /><figcaption>{alt}</figcaption></figure>
);

const K = ({ children }: { children: ReactNode }) => <kbd>{children}</kbd>;

const SECTIONS: Section[] = [
  { id: "start", title: "Getting started", body: (
    <>
      <p>MyLoDB connects to MySQL and MariaDB servers. Everything you do goes through a <b>connection</b>, so the first step is to add one.</p>
      <ol>
        <li>Click <b>+</b> at the top of the sidebar (or <b>Database → New Database Connection…</b>).</li>
        <li>Enter a name, the host (or paste a <code>mysql://</code> or JDBC URL), the user and the password. Click <b>Test</b>.</li>
        <li>Choose the <b>Environment</b> (Development, Staging or Production) on the Safety tab. Production connections get a red band and extra confirmations.</li>
        <li>Save. The connection opens on the server <b>Home</b> page with a query tab beside it.</li>
      </ol>
      <p>Passwords are stored in the macOS Keychain, never in the app's own database or in logs.</p>
      <Shot src={shot_connDialog} alt="The new connection dialog" />
      <Shot src={shot_connSafety} alt="The Safety tab: environment, read-only, time limit" />
      <Shot src={shot_tour} alt="The quick tour" />
    </>) },
  { id: "updates", title: "Updates", body: (
    <>
      <p>MyLoDB checks for a new release a few seconds after it starts and every six hours. <b>Help → Check for Updates…</b> checks at once; <b>Help → About</b> switches the automatic check off.</p>
      <ul>
        <li>The dialog shows <b>what is new</b> in the version offered.</li>
        <li><b>Update now</b> downloads the update (with progress), replaces this copy of MyLoDB and restarts it. Connections, settings and open tabs are kept.</li>
        <li><b>Later</b> asks again at the next check. <b>Skip this version</b> stops offering that version on its own; a manual check still offers it.</li>
        <li>Updates are signed and checked before they are installed. MyLoDB contacts only GitHub for this and sends nothing about you or your databases.</li>
      </ul>
    </>) },
  { id: "sqlite", title: "SQLite files", body: (
    <>
      <p>MyLoDB opens SQLite files too. In the connection dialog choose <b>Database type → SQLite file</b>, pick the file and press <b>Test connection</b>.</p>
      <ul>
        <li>The file is <b>writable</b> unless you tick <b>Open read-only</b> in the dialog (then it is opened read-only and nothing can change it). Opening never creates a file.</li>
        <li>Browse tables, views, triggers, columns and indexes, run SQL, and page, sort, filter and count a table in the grid. Counts are exact; <code>REGEXP</code> is not available.</li>
        <li>Edit rows in the grid (staged, previewed as SQL, applied in one transaction; foreign keys are enforced) and run <code>INSERT</code>, <code>UPDATE</code>, <code>DELETE</code> and DDL in the editor. A table needs a primary key to be edited in the grid; views are read-only.</li>
        <li>Users, server tools, events, routines and dump-based backup do not exist for a file: those buttons are disabled and their tooltips say why.</li>
        <li>Not yet: the table designer, export from a table, and cancelling a running query.</li>
      </ul>
      <Shot src={shot_sqliteDialog} alt="A SQLite connection in the connection dialog" />
    </>) },
  { id: "browse", title: "Browsing objects", body: (
    <>
      <p>The sidebar tree shows databases, tables, views, routines, triggers and events. Click a name to filter the tree to it; click a table to see its columns.</p>
      <ul>
        <li><b>Filter box</b> (<K>⇧⌘B</K>): type part of a name. Use <code>/regex/</code>, <code>type:view</code>, <code>db:shop</code> or <code>col:email</code>.</li>
        <li><b>Double-click</b> a table to open it. Right-click for the menu: data, properties, SELECT/INSERT/UPDATE/DELETE samples, rename, duplicate, drop.</li>
        <li><b>Drag</b> a database, table or column name into the editor to insert it, quoted.</li>
        <li>The <b>Info</b> tab under the editor shows facts about what is selected in the tree.</li>
        <li><K>F5</K> refreshes the tree.</li>
      </ul>
      <Shot src={shot_browserInfo} alt="The Info tab shows facts about the object selected in the tree" />
    </>) },
  { id: "query", title: "Running SQL", body: (
    <>
      <p>Open a script with <K>⌘T</K> (or File → New SQL Script). Press <K>⌘↵</K> to run the selection, or the whole script if nothing is selected.</p>
      <ul>
        <li><b>Preview</b> shows the statements and a risk review before anything runs. Destructive statements (DROP, TRUNCATE, DELETE or UPDATE without WHERE) always ask first.</li>
        <li><b>Cancel</b> stops a running statement. A per-connection time limit can stop long runs automatically.</li>
        <li>Results appear as tabs: Results, Profile, Messages, Table Data, Info, History. Switch to <b>text results</b> or use <b>Copy as</b> TSV, CSV, JSON, Markdown or INSERT.</li>
        <li><b>Autocomplete</b> knows your tables, columns, aliases, functions and keywords. After <code>JOIN</code> it lists first the tables linked by a foreign key to the ones already in the query. After <code>ON</code> it offers the ready condition from the foreign key (for example <code>o.customer_id = c.id</code>).</li>
        <li><b>Tab</b> takes the highlighted suggestion and never leaves the editor (without a suggestion it indents; <K>⇧Tab</K> outdents).</li>
        <li><b>Underlines</b>: a red or yellow wavy line marks a bracket or quote that never closes, a table that is not in the database and a column that is not in its table (<code>alias.column</code>). Hover for the reason. It stays quiet when it cannot be sure.</li>
        <li><b>Ask AI</b> (editor toolbar, or ⌘K → <i>Ask AI to write a query</i>): describe what you want in plain words and a model writes the SQL. You review it and choose <b>Insert in editor</b>; it is never run for you. Set it up once under <b>Preferences → AI assistant</b>: <b>Ollama</b> runs on your computer (free, private), and <b>Groq</b>, <b>OpenRouter</b> and <b>Gemini</b> have free tiers (they need a key from the service). Only your sentence and the <i>names</i> of tables, columns and foreign keys of the selected database are sent, never rows or values, and only after you allow it once for that service. A model can be wrong, so read the query before running it.</li>
        <li><b>Messages</b>: what an action did (done, failed, copied) pops up at the bottom right and also shows in the status bar. Move the mouse over it for a copy button, or click × to close it; errors stay longer, and it stays while the mouse is on it.</li>
        <li><K>⇧⌘F</K> formats SQL. <b>Explain</b> and the profiler show how a query ran.</li>
        <li><K>⌘K</K> opens the command palette: every action, every table, SQL snippets, and your own saved snippets (<i>Save editor text as my snippet…</i>).</li>
        <li>Open and save <code>.sql</code> files from the File menu. History and Favorites live in the sidebar tabs.</li>
      </ul>
      <Shot src={shot_query} alt="A query and its results" />
      <Shot src={shot_palette} alt="The command palette (⌘K)" />
    </>) },
  { id: "m-file", title: "File menu", body: (
    <>
      <p>Everything that opens, saves or moves files. Most items need an open connection.</p>
      <table className="info kv"><tbody>
        {([
          ["New SQL Script", "Opens an empty editor tab on the selected database. Use it for any query you want to keep apart from your other work."],
          ["Open SQL File…", "Loads a .sql file into a new editor tab so you can read and edit it before running anything."],
          ["Save / Save As…", "Writes the editor text to a .sql file. Save asks for a name only the first time."],
          ["Close Tab", "Closes the current tab. Unsaved grid edits ask first."],
          ["Refresh", "Reloads the object tree from the server. Use it after another tool changed the schema."],
          ["Import Data…", "Opens Transfer on the Import CSV tab: load a CSV, TSV, TXT, JSON or JSON Lines file into a table. See Importing data."],
          ["Export Data…", "Opens Transfer on the Export query tab: write the result of any SELECT to a file. See Exporting data."],
          ["Backup / Restore…", "Opens Transfer on the Backup tab: a consistent SQL dump of whole databases. See Backup and restore."],
          ["Run SQL File…", "Opens the Restore tab to run a big .sql file, a zip or a folder straight from disk. The editor is not used, so size does not matter."],
        ] as [string, string][]).map(([k, v]) => <tr key={k}><th>{k}</th><td>{v}</td></tr>)}
      </tbody></table>
    </>) },
  { id: "m-edit", title: "Edit and Search menus", body: (
    <>
      <p><b>Edit</b> has the usual Undo, Redo, Cut, Copy, Paste and Select All, which work in the editor and in text boxes, and <b>Format SQL</b>, which lays the script out with one clause per line.</p>
      <p><b>Search</b> has three tools, each for a different question:</p>
      <ul>
        <li><b>Search Objects (DB Metadata)</b> (<K>⇧⌘B</K>) puts the cursor in the sidebar filter. Use it when you know part of a table, view, routine or column name. The funnel icon chooses what is searched (tables, views, procedures and functions, triggers, events, columns).</li>
        <li><b>Search Data in Database…</b> answers "which table holds this value?". Pick a database, type the text or number and press Search. It scans every table and lists the tables, columns and row counts that match, with progress and a Cancel button. Matching follows each column's collation, so it is usually not case-sensitive. Tick <i>Whole value</i> to match exactly instead of "contains", and <i>Also numbers, dates…</i> to include non-text columns (slower).</li>
        <li><b>Command Palette…</b> (<K>⌘K</K>) runs any action by name, jumps to a table, inserts a snippet or a theme.</li>
      </ul>
    </>) },
  { id: "m-sql", title: "SQL Editor menu", body: (
    <>
      <table className="info kv"><tbody>
        {([
          ["New SQL Script / New Script in Selected Database", "A fresh editor tab, with or without the database already chosen."],
          ["Visual Query Builder…", "Build a SELECT by picking tables, joins, columns and conditions. The SQL is written for you and can be sent to the editor."],
          ["ER Diagram of Database…", "Draws the tables of a database and the foreign keys between them. Drag boxes to arrange them."],
          ["Execute", "Runs the selection, or the whole script when nothing is selected (⌘↵)."],
          ["Preview Before Running…", "Shows each statement and a risk review (DROP, TRUNCATE, DELETE or UPDATE without WHERE) before anything is sent."],
          ["Explain Plan", "Asks the server how it would run the query: which indexes, how many rows. Use it to find why a query is slow."],
          ["Format SQL", "Tidies the script (⇧⌘F)."],
        ] as [string, string][]).map(([k, v]) => <tr key={k}><th>{k}</th><td>{v}</td></tr>)}
      </tbody></table>
      <p>With a statement in the editor, the Results pane shows <b>Results</b>, <b>Profile</b> (time per stage), <b>Messages</b> (rows affected, warnings, errors), <b>Info</b> and <b>History</b>. These tabs appear after the first run; a table tab shows only <b>Table Data</b> until you run something in its console.</p>
    </>) },
  { id: "m-db", title: "Database menu", body: (
    <>
      <table className="info kv"><tbody>
        {([
          ["New Database Connection… / Edit Connection…", "Add a server or change a saved one: host, user, SSH tunnel, SSL, environment, read-only and time limit."],
          ["Disconnect", "Closes the connection and its tabs."],
          ["Server Home", "Version, uptime, connections, character set and quick links for the server."],
          ["Databases…", "Every database with size, table count and collation. Create, drop several at once, check all."],
          ["Tables of Selected Database", "The Structure page: every table with rows, engine, size and overhead, and a With selected menu for bulk actions."],
          ["New Table…", "Opens the table designer on an empty table."],
          ["Compare Schemas…", "Makes the tables of a target database match a source database, which may be on another connection. Click Compare to list the differences (new, changed or dropped tables), tick the ones you want, read the SQL with Show SQL, then Preview and apply. Only tables are compared, and tables that exist only in the target are listed as drops, so read the list."],
          ["Index Advisor…", "Reads every index in a database and reports duplicate, redundant and missing indexes (a foreign key with no index), plus indexes never used since the server started. Tick findings and Preview and apply to drop or add them. A missing foreign key index is not pre-ticked because it costs write speed."],
          ["Tools: Users and Privileges", "List users, edit privileges and see the exact GRANT and REVOKE before it runs. Passwords are masked."],
          ["Tools: Server Tools", "Overview, Charts, Processes (kill a query), Variables, Status, Maintenance (analyze, check, optimize, repair) and Logs (error log, slow queries, binary logs, settings)."],
          ["Tools: Engines, Charsets, Replication", "Read-only lists of storage engines, character sets and collations, and the replication state."],
        ] as [string, string][]).map(([k, v]) => <tr key={k}><th>{k}</th><td>{v}</td></tr>)}
      </tbody></table>
      <p>A SQLite file has no users, server tools or dump backup, so those items are disabled with a tooltip that says why.</p>
    </>) },
  { id: "m-view", title: "View, Window and Help menus", body: (
    <>
      <ul>
        <li><b>Toggle Sidebar</b> (<K>⌘B</K>) hides or shows the object tree, to give a wide table more room. The same button sits before the Home tab.</li>
        <li><b>Theme: System, Light, Dark</b> switch the look at once. Named colour themes are in Preferences.</li>
        <li><b>Window</b> is the standard macOS menu: minimize, zoom, full screen, close.</li>
        <li><b>Help</b>: User Manual (this window), Quick Tour, Keyboard Shortcuts (<K>⌘/</K>), Check for Updates… and About.</li>
      </ul>
    </>) },
  { id: "grid", title: "Editing table data", body: (
    <>
      <p>A table opens as a console (top) and a data grid (bottom). The grid pages through the data on the server, so very large tables stay fast; row counts are estimates until you ask for an exact count.</p>
      <ul>
        <li>Double-click a cell to edit. Edits are <b>staged</b>; nothing is written until you save, and the changes go in one transaction.</li>
        <li>Rows are matched by primary or unique key. A table without one is read-only.</li>
        <li>Use the filter row, a free-form WHERE, and multi-column sort. Foreign-key cells open a picker; <K>⌘</K>-click jumps to the referenced row.</li>
        <li>Export the table or any SELECT to CSV, JSON, JSON Lines, SQL, XML, HTML or Excel, as a cancellable background job.</li>
      </ul>
      <Shot src={shot_tableData} alt="A table: console above, data grid below" />
    </>) },
  { id: "filter", title: "Filtering and sorting data", body: (
    <>
      <p>All of this happens on the server, so it works the same on a table of ten rows and ten million.</p>
      <ul>
        <li><b>Column filters</b>: press the filter button in the grid toolbar to show a filter row. Each column has an operator and a value. Operators: contains, doesn't contain, =, ≠, starts with, ends with, &gt;, ≥, &lt;, ≤, between (two values), in (a, b, …), regexp, is NULL, is not NULL, is empty. Filters on several columns combine with AND. The × beside the toolbar button clears them all.</li>
        <li><b>WHERE box</b>: type your own condition, for example <code>status = 'paid' AND total &gt; 100</code>. It is added to the column filters.</li>
        <li><b>Sort</b>: click a column header to sort up, again for down, again to clear. <K>Shift</K>-click adds a second sort column. The <b>SL</b> column header sorts by the primary key.</li>
        <li><b>Paging</b>: first, previous, next and last page, and a rows-per-page box. <b>Count</b> replaces the estimated total with an exact one; Stop cancels a slow count.</li>
        <li><b>Columns</b>: drag a header edge to resize, including the SL column.</li>
        <li><b>SQL</b> button: shows the exact queries the grid is running, so you can copy them to the editor.</li>
        <li><b>SQLite files</b> support all of the above except <code>regexp</code>.</li>
      </ul>
    </>) },
  { id: "cells", title: "Long text, JSON, files and images", body: (
    <>
      <ul>
        <li><b>TEXT, MEDIUMTEXT, LONGTEXT and JSON</b> cells open in an editor window instead of a one-line box. It has Apply (<K>⌘↵</K>), Set NULL and Copy. For JSON it shows whether the text is valid and has <b>Format</b> (indent) and <b>Minify</b> (one line).</li>
        <li><b>Value panel</b>: select a cell to see its full value in Text, JSON, Hex or Image view. Long values are loaded in full, not cut to the preview.</li>
        <li><b>BLOB and binary cells</b>: <b>Upload…</b> puts a file from your disk into the cell, and <b>Save…</b> writes the cell back out to a file. Images show in the Image view. Uploads are staged like any other edit and are written when you press Submit.</li>
        <li><b>Set NULL</b> (∅) clears a nullable cell. <b>Foreign key</b> cells have a … button that opens a picker of the referenced rows.</li>
      </ul>
    </>) },
  { id: "export", title: "Exporting data", body: (
    <>
      <p>There are four ways to get data out. Pick by what you want.</p>
      <ul>
        <li><b>Copy as…</b> (above a result): copies the rows you see to the clipboard as TSV, CSV, JSON, a Markdown table or INSERT statements. Quick and small; for a few hundred rows.</li>
        <li><b>Export Data…</b> (File menu, or Export in a table's right-click menu): writes the result of any SELECT to a file, streamed from the server so a big result does not fill memory. Formats: <b>CSV</b>, <b>JSON</b> (an array of objects), <b>JSON Lines</b> (one object per line, easy to stream), <b>SQL INSERT</b>, <b>XML</b>, <b>HTML table</b> and <b>Excel (XLSX)</b>. For CSV you choose the text written for NULL. Tick <i>Compress with gzip</i> to write a .gz (not for Excel). It runs as a background job with progress and Cancel; a cancelled export leaves no partial file.</li>
        <li><b>Dump table…</b> and <b>Back up this database…</b> (right-click): a SQL file that can recreate the table or database. See Backup and restore.</li>
        <li><b>With selected → Export</b> on the Structure page exports several tables at once.</li>
      </ul>
      <p>The Tasks button (top right) lists running and finished background jobs: exports, imports, backups and scripts.</p>
    </>) },
  { id: "import", title: "Importing data", body: (
    <>
      <p><b>File → Import Data…</b> loads a file into a table. Choose a CSV, TSV, TXT, JSON or JSON Lines file, then:</p>
      <ol>
        <li><b>Describe the file</b>: delimiter, quote character, encoding, whether the first row is a header, trim spaces, and which text means NULL (for example <code>NULL</code> or <code>\N</code>). The preview shows the first rows as MyLoDB reads them.</li>
        <li><b>Choose the target</b>: an existing table, or <i>Create a new table from the file</i> and give it a name. Columns are mapped by name; change a mapping in the preview if names differ.</li>
        <li><b>Decide on duplicates</b> (existing table): report an error, skip the new row, update the existing row, or replace it. Rows are matched by primary or unique key. <i>Empty the table first</i> deletes all current rows and asks you to confirm.</li>
        <li><b>Safety options</b>: stop at the first bad row and undo everything (or continue and report the bad rows), treat empty values as NULL in number and date columns, and disable foreign key checks while importing.</li>
        <li>Press <b>Import</b>. It runs as a background job with progress and ends with a count of rows added, skipped and failed.</li>
      </ol>
      <p>To run a <code>.sql</code> file instead, use <b>Run SQL File…</b> (Restore tab).</p>
    </>) },
  { id: "design", title: "Table designer and admin pages", body: (
    <>
      <p>The table designer edits columns, indexes, foreign keys and options and shows the exact <code>ALTER TABLE</code> before it runs. Renames keep data.</p>
      <ul>
        <li><b>Databases</b> page: sizes, table counts, collation, check all, bulk drop, create.</li>
        <li><b>Structure</b> page of a database: every table with rows, engine, size and overhead, and a <b>With selected</b> menu (copy, export, empty, drop, analyze, check, optimize, repair, prefixes).</li>
        <li>Every drop, empty, rename and copy shows <b>Preview SQL</b> first, with a switch for foreign key checks.</li>
        <li><b>Users and Privileges</b> shows the exact GRANT and REVOKE changes with passwords masked. <b>Server Tools</b> has processes, variables, status, maintenance and logs.</li>
        <li><b>Alter database</b> (tree menu) changes the default collation. <b>Rename…</b> on a procedure or function creates it under the new name and then drops the old one.</li>
      </ul>
      <Shot src={shot_databases} alt="The Databases page" />
      <Shot src={shot_structure} alt="The Structure page" />
      <Shot src={shot_erDiagram} alt="ER diagram of a database" />
      <Shot src={shot_queryBuilder} alt="The visual query builder" />
    </>) },
  { id: "passwords", title: "Saved passwords", body: (
    <>
      <p>Passwords for saved connections (database, SSH password, SSH key passphrase) are never stored in MyLoDB's own database. Where they go is your choice, under <b>Preferences → Security → Saved passwords</b>.</p>
      <ul>
        <li><b>macOS Keychain</b> (the default). Convenient. Because MyLoDB is not signed with a paid Apple certificate, macOS asks for your login password again for each saved connection after an update. Choose <b>Always Allow</b> to quiet it for that version.</li>
        <li><b>Encrypted file</b>. Choose a master password (at least 8 characters). Every saved password moves out of the Keychain into one encrypted file in MyLoDB's data folder, and the Keychain copies are removed. You type the master password once per session and there are no Keychain prompts. The file uses XChaCha20-Poly1305 with a key made from your password by Argon2id, and it never leaves your Mac.</li>
      </ul>
      <p>With the encrypted file, MyLoDB asks for the master password when it starts, or when you open a connection while it is still locked. Press Esc to skip; the question comes back when a connection needs a password.</p>
      <ul>
        <li><b>Change the master password</b> needs the current one. <b>Move passwords back to the Keychain</b> needs the file unlocked and deletes the file.</li>
        <li><b>If you forget the master password, nobody can open the file</b>, including us. In the unlock window choose <i>I forgot the master password</i> to delete it: your connections stay and you type each password again.</li>
        <li>Anyone who can read the file and guess a weak master password could open it, so pick a long one. Back it up with your other private files if you like; it is useless without the password.</li>
      </ul>
    </>) },
  { id: "safety", title: "Staying safe on production", body: (
    <>
      <ul>
        <li>The connection's <b>Environment</b> sets the colour band and badge. Production asks you to type a name before destructive operations.</li>
        <li><b>Read-only</b> is enforced by the server for the session, not just by the app.</li>
        <li>A <b>query time limit</b> stops runaway statements.</li>
        <li>Saving grid changes on production asks for confirmation.</li>
        <li>SSH tunnels check the host key and block a changed key. SSL modes include verify, verify CA and skip verify.</li>
      </ul>
    </>) },
  { id: "transfer", title: "Backup and restore", body: (
    <>
      <p>Open <b>File → Backup / Restore…</b>. A backup is a plain SQL file you can read, version and restore with MyLoDB or the <code>mysql</code> client.</p>
      <ul>
        <li><b>Backup</b>: tick the databases, and optionally only some tables of each. Choose the contents (structure, data, views, procedures and functions, triggers, events) and the options: add <code>DROP … IF EXISTS</code> so a restore replaces objects, add <code>CREATE DATABASE</code> and <code>USE</code>, take a <b>consistent snapshot</b> (every table read at one moment, InnoDB), disable foreign key checks while restoring, remove <code>DEFINER</code> clauses so views and routines restore under the restoring user, and rows per INSERT statement. Output: a folder, a file name, one file or one file per table, optionally one .zip. A timestamp is added, so a backup never overwrites an earlier one. Cancel removes the partial output.</li>
        <li><b>Restore / run script</b>: choose a <code>.sql</code> file, a zip or a folder of .sql files and the database to run in. <b>Preview script</b> counts and shows the first statements without running anything. Then <b>Run script…</b>, which asks to confirm. Options: stop at the first error, and disable foreign key checks. The file is streamed from disk, so a multi-gigabyte dump works.</li>
        <li>Before you restore over data you care about, take a fresh backup first.</li>
      </ul>
    </>) },
  { id: "perf", title: "Performance", body: (
    <>
      <p>Measured on a local MySQL 9.7 with a table of 10 million rows (about 1 GB, larger than the server's buffer pool):</p>
      <table className="info kv"><tbody>
        {[["First page (500 rows)", "6 ms"], ["Jump to the middle or the end (keyset paging)", "2 to 3 ms (OFFSET paging: 1.2 s and 2.3 s)"], ["Estimated row count", "under 1 ms (exact COUNT(*): 0.8 s)"],
          ["Export to CSV", "9.4 s, about 1 million rows per second"], ["Export to JSON", "15 s"], ["Memory during a 1.4 GB export", "5 to 11 MB"]]
          .map(([k, v]) => <tr key={k}><th>{k}</th><td>{v}</td></tr>)}
      </tbody></table>
      <p>The grid draws only the rows you can see, so a page of 5,000 rows draws about 30. Row counts are estimates until you press <b>Count</b>; filtered estimates can be far off, which is why the grid marks them with ≈. Keyset paging needs a primary key made of integer, text or date columns; other tables fall back to OFFSET automatically.</p>
      <p className="dim small">Single runs on one machine; the full method, the limits and what is not covered are in docs/BENCHMARKS.md in the repository.</p>
    </>) },
  { id: "themes", title: "Preferences and themes", body: (
    <>
      <p>Open <b>Preferences</b> with <K>⌘,</K>. Changes apply as you make them; Cancel puts the old values back.</p>
      <ul>
        <li><b>Theme</b>: System, Light, Dark, or a colour theme: Monokai, GitHub Dark and Light, Dracula, One Dark, Nord, Solarized Dark and Light, Tokyo Night. The SQL editor takes the theme's syntax colours.</li>
        <li><b>Fonts</b> for the interface, sidebar, data grid and query editor, each with its own family, size and weight.</li>
        <li><b>Syntax colours</b>: keep the theme's own, or force Multi-colour, Solarized or High contrast in the editor.</li>
        <li><b>Saved passwords</b>: keep them in the macOS Keychain or in an encrypted file behind a master password. See Saved passwords.</li>
      </ul>
      <Shot src={shot_preferences} alt="Preferences" />
      <Shot src={shot_darkTheme} alt="The dark theme" />
    </>) },
  { id: "keys", title: "Keyboard shortcuts", body: (
    <>
      <p>The same list opens anywhere with <K>⌘/</K> (Help → Keyboard Shortcuts).</p>
      {SHORTCUT_GROUPS.map((g) => (
        <div key={g.title}>
          <h3>{g.title}</h3>
          <table className="info kv"><tbody>
            {g.keys.map(([k, v]) => <tr key={k + v}><th><K>{k}</K></th><td>{v}</td></tr>)}
          </tbody></table>
        </div>
      ))}
    </>) },
  { id: "help", title: "Troubleshooting", body: (
    <>
      <ul>
        <li><b>Windows: "Windows protected your PC"</b> on first run: the installer is not signed with a paid certificate. Click <b>More info</b>, then <b>Run anyway</b>. Shortcuts shown with ⌘ use <b>Ctrl</b> on Windows.</li>
        <li><b>"Apple could not verify MyLoDB"</b> on first launch: the app is not signed with a paid certificate. Click <b>Done</b>, then <b>System Settings → Privacy &amp; Security → Open Anyway</b>. Or run <code>xattr -dr com.apple.quarantine /Applications/MyLoDB.app</code>.</li>
        <li><b>Cannot connect</b>: check host and port, the user's allowed host, and the SSL mode. <b>Test</b> in the connection dialog shows the exact server message.</li>
        <li><b>Only one database is listed</b>: the connection has <i>Show all databases</i> off (shared hosting). Turn it on in the connection dialog.</li>
        <li><b>A table is read-only</b> in the grid: it has no primary or unique key.</li>
        <li>Open <b>Help → Quick Tour</b> to see the first-run guide again.</li>
      </ul>
      <p>Found a bug? <ExtLink href="https://github.com/farookhridoy/mylodb/issues">Open an issue</ExtLink>. Please do not include passwords or customer data.</p>
    </>) },
];

/** The in-app user manual: a section list on the left, the text on the right. */
export default function Manual(props: { onClose(): void; onTour(): void }) {
  const [id, setId] = useState(SECTIONS[0].id);
  const section = SECTIONS.find((s) => s.id === id) ?? SECTIONS[0];
  return (
    <div className="overlay" onMouseDown={props.onClose}>
      <div className="dialog manual" role="dialog" aria-label="User manual" onMouseDown={(e) => e.stopPropagation()} onKeyDown={(e) => e.key === "Escape" && props.onClose()}>
        <nav aria-label="Manual sections">
          <h3>User manual</h3>
          {SECTIONS.map((s) => <button key={s.id} className={s.id === id ? "active" : ""} onClick={() => setId(s.id)}>{s.title}</button>)}
          <button className="tour-link" onClick={props.onTour}>▶ Quick tour</button>
        </nav>
        <article>
          <h2>{section.title}</h2>
          {section.body}
        </article>
        <button className="manual-close" aria-label="Close manual" onClick={props.onClose}>✕</button>
      </div>
    </div>
  );
}
