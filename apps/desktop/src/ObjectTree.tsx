import { memo, useMemo, useState } from "react";
import type { Kind, Node } from "./api";
import { columnMatches, formatBytes, type ColumnsOf } from "./browserMeta";
import type { TableStat } from "./api";

/** Rows drawn per group before a "show more" row. Keeps a database with thousands of tables responsive. */
const PAGE = 300;

const GROUPS: { kind: Kind; label: string }[] = [
  { kind: "table", label: "Tables" },
  { kind: "view", label: "Views" },
  { kind: "procedure", label: "Stored Procs" },
  { kind: "function", label: "Functions" },
  { kind: "trigger", label: "Triggers" },
  { kind: "event", label: "Events" },
];

/** Highlight the case-insensitive substring terms of the query inside a name. */
function Highlight({ text, terms }: { text: string; terms: string[] }) {
  const lower = text.toLowerCase();
  const term = terms.find((t) => t && lower.includes(t));
  if (!term) return <>{text}</>;
  const i = lower.indexOf(term);
  return <>{text.slice(0, i)}<mark>{text.slice(i, i + term.length)}</mark>{text.slice(i + term.length)}</>;
}

interface Entry { pos: number; node: Node }

export interface TreeProps {
  /** The connection shown as the root node, e.g. `root@localhost`. Clicking it resets the browser. */
  rootLabel: string;
  onResetRoot(): void;
  selectedTable: string | null;
  onSelectTable(db: string, table: string): void;
  /** Filter text for the selected table's columns (when a table is selected). */
  columnQuery: string;
  stats: Record<string, Record<string, TableStat>>;
  ensureStats(db: string): void;
  details: Record<string, ColumnsOf>;
  ensureDetails(db: string, table: string): void;
  index: Node[];
  /** Index positions matching the filter, or null when no filter is active. */
  matches: Set<number> | null;
  /** The database the matches were computed for. With no scope the matches are database names only. */
  scope: string | null;
  terms: string[];
  selectedDb: string | null;
  onSelectDb(db: string): void;
  columnsLoaded: Set<string>;
  ensureColumns(db: string): Promise<unknown>;
  onOpenTable(database: string, table: string): void;
  onTableMenu(e: React.MouseEvent, database: string, table: string): void;
  onDatabaseMenu(e: React.MouseEvent, database: string | null): void;
}

export default function ObjectTree(props: TreeProps) {
  const { index, matches, scope, selectedDb } = props;
  const [open, setOpen] = useState<Set<string>>(new Set());
  const [shown, setShown] = useState<Record<string, number>>({});

  // One pass over the index per change of the index (not per keystroke).
  const { dbs, byDb } = useMemo(() => {
    const byDb = new Map<string, Entry[]>();
    const dbs: Entry[] = [];
    index.forEach((node, pos) => {
      let list = byDb.get(node.database);
      if (!list) byDb.set(node.database, (list = []));
      list.push({ pos, node });
      if (node.kind === "database") dbs.push({ pos, node });
    });
    return { dbs, byDb };
  }, [index]);

  const toggle = (key: string) => setOpen((o) => { const n = new Set(o); n.has(key) ? n.delete(key) : n.add(key); return n; });
  const more = (key: string) => setShown((s) => ({ ...s, [key]: (s[key] ?? PAGE) + PAGE }));

  // With no scope the filter is on database names: hide the databases that do not match.
  const visibleDbs = matches && scope === null ? dbs.filter((d) => matches.has(d.pos)) : dbs;
  const dbLimit = shown["*dbs"] ?? PAGE * 2;

  if (matches && scope === null && visibleDbs.length === 0) return <div className="empty">No matching databases</div>;

  return (
    <ul className="tree" onContextMenu={(e) => e.target === e.currentTarget && props.onDatabaseMenu(e, null)}>
      <li className="root-node">
        <div className={`row-item root ${selectedDb === null ? "selected" : ""}`} title="Show all databases (clears the selection and the filter)"
             onClick={() => { setOpen(new Set()); props.onResetRoot(); }}
             onContextMenu={(e) => props.onDatabaseMenu(e, null)}>
          <span className="chev">▾</span><span className="icon">🖥</span>
          <strong>{props.rootLabel}</strong>
          <span className="badge" title="Databases">{dbs.length.toLocaleString()}</span>
        </div>
      </li>
      {visibleDbs.slice(0, dbLimit).map(({ node }) => {
        const name = node.name;
        const isOpen = open.has(name);
        const scoped = matches !== null && scope === name;
        return (
          <li key={name} className="under-root">
            <div className={`row-item db ${selectedDb === name && props.selectedTable === null ? "selected" : ""} ${selectedDb === name ? "current" : ""}`}
                 onClick={() => { props.onSelectDb(name); props.ensureStats(name); toggle(name); }}
                 onContextMenu={(e) => { props.onSelectDb(name); props.onDatabaseMenu(e, name); }}>
              <span className="chev">{isOpen ? "▾" : "▸"}</span>
              <span className="icon">🛢</span>
              <Highlight text={name} terms={scope === null ? props.terms : []} />
              {scoped && <span className="badge" title="Matching objects">{[...matches!].filter((p) => index[p]?.database === name && index[p].kind !== "database").length}</span>}
            </div>
            {(isOpen || scoped) && (
              <DbBranch {...props} db={name} entries={byDb.get(name) ?? []} scopedMatches={scoped ? matches : null} open={open} toggle={toggle} shown={shown} more={more} />
            )}
          </li>
        );
      })}
      {visibleDbs.length > dbLimit && <li><div className="row-item more" onClick={() => more("*dbs")}>Show {Math.min(PAGE * 2, visibleDbs.length - dbLimit)} more of {visibleDbs.length - dbLimit} databases…</div></li>}
    </ul>
  );
}

/** One database's objects. Memoised: typing in the filter only re-renders the database that is being filtered. */
const DbBranch = memo(function DbBranch(props: TreeProps & {
  db: string; entries: Entry[]; scopedMatches: Set<number> | null;
  open: Set<string>; toggle(k: string): void; shown: Record<string, number>; more(k: string): void;
}) {
  const { db, entries, scopedMatches, open, shown } = props;
  const dbStats = props.stats[db];

  const { groups, columnsOf, tablesWithMatchingColumns } = useMemo(() => {
    const keep = (e: Entry) => scopedMatches === null || scopedMatches.has(e.pos);
    const groups = GROUPS.map((g) => ({ ...g, items: [] as Entry[] }));
    const columnsOf = new Map<string, Entry[]>();
    const tablesWithMatchingColumns = new Set<string>();
    for (const e of entries) {
      if (e.node.kind === "database") continue;
      if (e.node.kind === "column") {
        const t = e.node.parent ?? "";
        let list = columnsOf.get(t);
        if (!list) columnsOf.set(t, (list = []));
        list.push(e);
        if (scopedMatches?.has(e.pos)) tablesWithMatchingColumns.add(t);
        continue;
      }
      if (keep(e)) groups.find((g) => g.kind === e.node.kind)?.items.push(e);
    }
    // a table whose column matched (col:email) is listed even if its own name did not match
    if (scopedMatches && tablesWithMatchingColumns.size > 0) {
      const have = new Set(groups[0].items.map((e) => e.node.name));
      for (const e of entries) {
        if (e.node.kind === "table" && tablesWithMatchingColumns.has(e.node.name) && !have.has(e.node.name)) groups[0].items.push(e);
      }
      groups[0].items.sort((a, b) => a.pos - b.pos);
    }
    return { groups, columnsOf, tablesWithMatchingColumns };
  }, [entries, scopedMatches]);

  const filtering = scopedMatches !== null;
  const isOpen = (key: string) => filtering || open.has(key);
  const loaded = props.columnsLoaded.has(db);

  return (
    <ul>
      {groups.filter((g) => g.items.length > 0).map((g) => {
        const gk = `${db}/${g.kind}`;
        const limit = shown[gk] ?? PAGE;
        return (
          <li key={gk}>
            <div className="row-item group" onClick={() => props.toggle(gk)}>
              <span className="chev">{isOpen(gk) ? "▾" : "▸"}</span>
              {g.label}<span className="badge">{g.items.length.toLocaleString()}</span>
            </div>
            {isOpen(gk) && (
              <ul>
                {g.items.slice(0, limit).map(({ node: n }) => {
                  const nk = `${gk}/${n.name}`;
                  const isTable = g.kind === "table" || g.kind === "view";
                  const cols = isTable ? columnsOf.get(n.name) : undefined;
                  const expanded = open.has(nk) || (filtering && tablesWithMatchingColumns.has(n.name));
                  return (
                    <li key={nk}>
                      <div className={`row-item leaf ${isTable && props.selectedTable === n.name && props.selectedDb === db ? "selected" : ""}`}
                           onClick={() => { if (!isTable) return; const already = props.selectedTable === n.name && props.selectedDb === db; props.onSelectTable(db, n.name); props.ensureDetails(db, n.name); if (already || !open.has(nk)) props.toggle(nk); }}
                           onDoubleClick={() => isTable && props.onOpenTable(db, n.name)}
                           onContextMenu={(e) => isTable && props.onTableMenu(e, db, n.name)}>
                        <span className="chev">{isTable ? (expanded ? "▾" : "▸") : ""}</span>
                        <span className="name"><Highlight text={n.name} terms={props.terms} /></span>
                        {n.parent && <span className="dim"> · {n.parent}</span>}
                        {isTable && <TableSize stat={dbStats?.[n.name]} />}
                      </div>
                      {isTable && expanded && (
                        <ColumnList
                          detail={props.details[`${db}\0${n.name}`]}
                          fallback={cols}
                          query={props.selectedTable === n.name && props.selectedDb === db ? props.columnQuery : ""}
                          filtering={filtering}
                          matched={scopedMatches}
                          tableHasMatch={tablesWithMatchingColumns.has(n.name)}
                          terms={props.terms}
                        />
                      )}
                    </li>
                  );
                })}
                {g.items.length > limit && <li><div className="row-item more" onClick={() => props.more(gk)}>Show {Math.min(PAGE, g.items.length - limit)} more of {(g.items.length - limit).toLocaleString()}…</div></li>}
              </ul>
            )}
          </li>
        );
      })}
      {filtering && groups.every((g) => g.items.length === 0) && <li className="empty small">No objects match in {db}</li>}
    </ul>
  );
});

/** Size of a table in KB/MB/GB, with the row estimate as a tooltip. Views show nothing. */
function TableSize({ stat }: { stat: TableStat | undefined }) {
  if (!stat || stat.bytes === null) return null;
  return <span className="size" title={`${stat.bytes.toLocaleString()} bytes (data + indexes)${stat.rows !== null ? ` · about ${stat.rows.toLocaleString()} rows` : ""}\nAn estimate from the server's statistics.`}>{formatBytes(stat.bytes)}</span>;
}

/** The columns of a table with their types (`bigint unsigned`, `char(255)`), key and nullability. */
function ColumnList({ detail, fallback, query, filtering, matched, tableHasMatch, terms }: {
  detail: ColumnsOf | undefined; fallback: Entry[] | undefined; query: string; filtering: boolean;
  matched: Set<number> | null; tableHasMatch: boolean; terms: string[];
}) {
  if (Array.isArray(detail)) {
    const shown = detail.filter((c) => columnMatches(query, c));
    return (
      <ul className="columns">
        {shown.slice(0, 500).map((c) => (
          <li key={c.name} className="row-item col" title={`${c.name} ${c.column_type}${c.nullable ? "" : " NOT NULL"}${c.extra ? " " + c.extra : ""}`}>
            <span className="key">{c.key === "PRI" ? "🔑" : c.key === "UNI" ? "◆" : ""}</span>
            <span className="name"><Highlight text={c.name} terms={query ? query.split(/\s+/).map((t) => t.toLowerCase()) : terms} /></span>
            <span className="ctype">{c.column_type}</span>
          </li>
        ))}
        {shown.length === 0 && <li className="empty small">{query ? "No matching columns" : "No columns"}</li>}
        {shown.length > 500 && <li className="empty small">{shown.length - 500} more — type to narrow</li>}
      </ul>
    );
  }
  if (detail && typeof detail === "object" && "error" in detail) return <ul><li className="row-item col dim" title={detail.error}>couldn't load columns</li></ul>;
  // not loaded yet: show the names we already have (from the index) while the details arrive
  const names = (fallback ?? []).filter((c) => !filtering || matched?.has(c.pos) || !tableHasMatch).slice(0, 500);
  return (
    <ul>
      {names.length === 0 && <li className="row-item col dim">loading columns…</li>}
      {names.map((c) => <li key={c.node.name} className="row-item col"><span className="key" /><span className="name"><Highlight text={c.node.name} terms={terms} /></span></li>)}
    </ul>
  );
}
