import { useState, type ReactNode } from "react";
import { ExtLink } from "./AboutDialog";

interface Section { id: string; title: string; body: ReactNode }

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
    </>) },
  { id: "query", title: "Running SQL", body: (
    <>
      <p>Open a script with <K>⌘T</K> (or File → New SQL Script). Press <K>⌘↵</K> to run the selection, or the whole script if nothing is selected.</p>
      <ul>
        <li><b>Preview</b> shows the statements and a risk review before anything runs. Destructive statements (DROP, TRUNCATE, DELETE or UPDATE without WHERE) always ask first.</li>
        <li><b>Cancel</b> stops a running statement. A per-connection time limit can stop long runs automatically.</li>
        <li>Results appear as tabs: Results, Profile, Messages, Table Data, Info, History. Switch to <b>text results</b> or use <b>Copy as</b> TSV, CSV, JSON, Markdown or INSERT.</li>
        <li><K>⇧⌘F</K> formats SQL. <b>Explain</b> and the profiler show how a query ran.</li>
        <li><K>⌘K</K> opens the command palette: every action, every table, SQL snippets, and your own saved snippets (<i>Save editor text as my snippet…</i>).</li>
        <li>Open and save <code>.sql</code> files from the File menu. History and Favorites live in the sidebar tabs.</li>
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
  { id: "transfer", title: "Backup, restore and import", body: (
    <>
      <p>Use <b>File → Backup / Restore…</b> and <b>Import / Export Data…</b>.</p>
      <ul>
        <li><b>Backup</b> writes one consistent snapshot as SQL: structure, data, views, routines, triggers and events; one file or a file per table; optional zip. Partial output is removed if you cancel.</li>
        <li><b>Restore</b> runs a <code>.sql</code> file, a zip or a folder, streamed from disk. Use <b>Preview script</b> first to see the counts and the first statements.</li>
        <li><b>CSV import</b> detects the delimiter, previews rows, maps columns and can insert, skip, update or replace; all-or-nothing or continue on error.</li>
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
  { id: "keys", title: "Keyboard shortcuts", body: (
    <table className="info kv"><tbody>
      {[["⌘T", "New SQL script"], ["⌘W", "Close tab"], ["⌘↵", "Run selection or script"], ["⇧⌘F", "Format SQL"], ["⇧⌘B", "Search objects"], ["⌘K", "Command palette"], ["⌘,", "Preferences"], ["F5", "Refresh the object tree"]]
        .map(([k, v]) => <tr key={k}><th><K>{k}</K></th><td>{v}</td></tr>)}
    </tbody></table>) },
  { id: "help", title: "Troubleshooting", body: (
    <>
      <ul>
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
