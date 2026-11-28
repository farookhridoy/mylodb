import { memo, useEffect, useMemo, useState } from "react";
import { quoteIdent } from "./quoting";
import type { Kind, Node } from "./api";
import Icon, { type IconName } from "./Icon";
import { columnMatches, formatBytes, type ColumnsOf } from "./browserMeta";
import type { IndexDetail, TableStat } from "./api";

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

const GROUP_ICON: Record<Kind, IconName> = {
  database: "database", table: "table", view: "view", procedure: "procedure", function: "function", trigger: "trigger", event: "event", column: "column",
};

function Chev({ open, onClick }: { open: boolean; onClick?: (e: React.MouseEvent) => void }) {
  return <span className={`chev ${open ? "open" : ""} ${onClick ? "clickable" : ""}`} onClick={onClick}><Icon name="chevron" size={10} /></span>;
}

/** What a click on a name in the tree refers to, so the console can paste it into the query. */
export type Pick =
  | { kind: "database"; db: string }
  | { kind: "table"; db: string; table: string }
  | { kind: "column"; db: string; table: string; column: string };

export interface TreeProps {
  /** The connection shown as the root node, e.g. `root@localhost`. Clicking it resets the browser. */
  rootLabel: string;
  onResetRoot(): void;
  /** The table highlighted in the tree. It does not change the filter. */
  selectedTable: string | null;
  onSelectTable(db: string, table: string): void;
  /** The table the filter is locked to (the box then searches its columns). */
  lockedTable: string | null;
  onLockTable(db: string, table: string): void;
  onUnlockTable(): void;
  /** Click on a database, table or column name: the console may paste it into the query. */
  onPick(p: Pick): void;
  /** Filter text for the locked table's columns. */
  columnQuery: string;
  stats: Record<string, Record<string, TableStat>>;
  /** Order of tables and views inside a database: by name (the server's order) or largest first. */
  sortBy?: "name" | "size";
  ensureStats(db: string): void;
  details: Record<string, ColumnsOf>;
  indexes: Record<string, IndexDetail[]>;
  /** Keys of the expanded nodes, restored per connection. */
  initialOpen: string[];
  onOpenChange(keys: string[]): void;
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
  onObjectMenu(e: React.MouseEvent, database: string, kind: Kind, name: string): void;
  /** Double-click on a routine, trigger or event: show its definition. */
  onOpenObject(database: string, kind: Kind, name: string): void;
  onDatabaseMenu(e: React.MouseEvent, database: string | null): void;
}

export default function ObjectTree(props: TreeProps) {
  const { index, matches, scope, selectedDb } = props;
  const [open, setOpen] = useState<Set<string>>(() => new Set(props.initialOpen));
  const [shown, setShown] = useState<Record<string, number>>({});
  // A filter opens the groups and tables that have hits. The arrows must still be able to close them, so closings are remembered
  // until the next search.
  const [autoClosed, setAutoClosed] = useState<Set<string>>(new Set());
  useEffect(() => { setAutoClosed(new Set()); }, [matches]);

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

  const update = (f: (o: Set<string>) => Set<string>) => setOpen((o) => { const n = f(o); props.onOpenChange([...n]); return n; });
  /** Expand nodes (never collapse): selecting or locking something shows it. The chevrons are what collapse. */
  const openKeys = (keys: string[]) => update((o) => (keys.every((k) => o.has(k)) ? o : new Set([...o, ...keys])));
  // a selected database shows its tables straight away
  useEffect(() => {
    if (selectedDb) openKeys([selectedDb, `${selectedDb}/table`]);
  }, [selectedDb]); // eslint-disable-line react-hooks/exhaustive-deps
  // a table locked from its menu (not only from its lock icon) is opened too
  useEffect(() => {
    if (!props.lockedTable || !selectedDb) return;
    const kind = index.find((n) => n.database === selectedDb && n.name === props.lockedTable && (n.kind === "table" || n.kind === "view"))?.kind;
    if (kind) openKeys([`${selectedDb}/${kind}`, `${selectedDb}/${kind}/${props.lockedTable}`]);
  }, [props.lockedTable]); // eslint-disable-line react-hooks/exhaustive-deps
  const toggle = (key: string) => update((o) => { const n = new Set(o); n.has(key) ? n.delete(key) : n.add(key); return n; });
  /** The arrow of a node that a filter may have opened: closes it (and keeps it closed) or opens it. */
  const toggleAuto = (key: string, openNow: boolean) => {
    if (openNow) { if (open.has(key)) toggle(key); setAutoClosed((a) => new Set(a).add(key)); }
    else { setAutoClosed((a) => { const n = new Set(a); n.delete(key); return n; }); if (!open.has(key)) toggle(key); }
  };
  const more = (key: string) => setShown((s) => ({ ...s, [key]: (s[key] ?? PAGE) + PAGE }));

  // With no scope the filter is on database names: hide the databases that do not match.
  // Selecting a database locks the browser to it (like SQLyog): only that database is listed until the lock is released.
  const locked = selectedDb !== null && dbs.some((d) => d.node.name === selectedDb);
  const visibleDbs = locked ? dbs.filter((d) => d.node.name === selectedDb) : matches && scope === null ? dbs.filter((d) => matches.has(d.pos)) : dbs;
  const dbLimit = shown["*dbs"] ?? PAGE * 2;

  if (!locked && matches && scope === null && visibleDbs.length === 0) return <div className="empty">No matching databases</div>;

  return (
    <ul className="tree" onContextMenu={(e) => e.target === e.currentTarget && props.onDatabaseMenu(e, null)}>
      <li className="root-node">
        <div className={`row-item root ${selectedDb === null ? "selected" : ""}`} title="Show all databases (clears the selection and the filter)"
             onClick={() => { update(() => new Set()); props.onResetRoot(); }}
             onContextMenu={(e) => props.onDatabaseMenu(e, null)}>
          <Chev open /><Icon name="server" className="ico-server" />
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
                 {...dragName(quoteIdent(name))}
                 onClick={() => { props.onSelectDb(name); props.ensureStats(name); openKeys([name, `${name}/table`]); props.onPick({ kind: "database", db: name }); }}
                 onContextMenu={(e) => { props.onSelectDb(name); props.onDatabaseMenu(e, name); }}>
              <Chev open={isOpen} onClick={(e) => { e.stopPropagation(); toggle(name); if (!isOpen) props.ensureStats(name); }} />
              <Icon name="database" className="ico-db" />
              <Highlight text={name} terms={scope === null ? props.terms : []} />
              <DbSize stats={props.stats[name]} />
              {scoped && <span className="badge" title="Matching objects">{[...matches!].filter((p) => index[p]?.database === name && index[p].kind !== "database").length}</span>}
            </div>
            {(isOpen || scoped) && (
              <DbBranch {...props} lockedTable={selectedDb === name ? props.lockedTable : null} db={name} entries={byDb.get(name) ?? []} scopedMatches={scoped ? matches : null} open={open} toggle={toggle} toggleAuto={toggleAuto} autoClosed={autoClosed} openKeys={openKeys} shown={shown} more={more} />
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
  lockedTable: string | null; db: string; entries: Entry[]; scopedMatches: Set<number> | null;
  open: Set<string>; toggle(k: string): void; toggleAuto(k: string, openNow: boolean): void; autoClosed: Set<string>; openKeys(k: string[]): void; shown: Record<string, number>; more(k: string): void;
}) {
  const { db, entries, scopedMatches, open, shown, lockedTable } = props;
  const dbStats = props.stats[db];
  useEffect(() => { props.ensureStats(db); }, [db]); // eslint-disable-line react-hooks/exhaustive-deps

  const { groups, columnsOf, tablesWithMatchingColumns } = useMemo(() => {
    const keep = (e: Entry) => (scopedMatches === null || scopedMatches.has(e.pos)) && (lockedTable === null || ((e.node.kind === "table" || e.node.kind === "view") && e.node.name === lockedTable));
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
    if (props.sortBy === "size" && dbStats) {
      const size = (e: Entry) => dbStats[e.node.name]?.bytes ?? -1;
      for (const g of groups) if (g.kind === "table" || g.kind === "view") g.items.sort((a, b) => size(b) - size(a) || a.pos - b.pos);
    }
    return { groups, columnsOf, tablesWithMatchingColumns };
  }, [entries, scopedMatches, lockedTable, props.sortBy, dbStats]);

  const filtering = scopedMatches !== null;
  const isOpen = (key: string) => open.has(key) || (filtering && !props.autoClosed.has(key));

  return (
    <ul>
      {groups.filter((g) => g.items.length > 0).map((g) => {
        const gk = `${db}/${g.kind}`;
        const limit = shown[gk] ?? PAGE;
        return (
          <li key={gk}>
            <div className="row-item group" onClick={() => props.toggleAuto(gk, isOpen(gk))}>
              <Chev open={isOpen(gk)} /><Icon name={GROUP_ICON[g.kind]} className="ico-group" />
              {g.label}<span className="badge">{g.items.length.toLocaleString()}</span>
            </div>
            {isOpen(gk) && (
              <ul>
                {g.items.slice(0, limit).map(({ node: n }) => {
                  const nk = `${gk}/${n.name}`;
                  const isTable = g.kind === "table" || g.kind === "view";
                  const cols = isTable ? columnsOf.get(n.name) : undefined;
                  const expanded = open.has(nk) || (filtering && tablesWithMatchingColumns.has(n.name) && !props.autoClosed.has(nk));
                  const isLocked = lockedTable === n.name;
                  return (
                    <li key={nk}>
                      <div className={`row-item leaf ${isTable && props.selectedTable === n.name && props.selectedDb === db ? "selected" : ""}`}
                           {...dragName(`${quoteIdent(db)}.${quoteIdent(n.name)}`)}
                           onClick={() => {
                             if (!isTable) return;
                             props.onSelectTable(db, n.name);
                             props.openKeys([nk]);
                             props.ensureDetails(db, n.name);
                             props.onPick({ kind: "table", db, table: n.name });
                           }}
                           onDoubleClick={() => { if (isTable) props.onOpenTable(db, n.name); else props.onOpenObject(db, g.kind, n.name); }}
                           onContextMenu={(e) => props.onObjectMenu(e, db, g.kind, n.name)}>
                        {isTable
                          ? <Chev open={expanded} onClick={(e) => { e.stopPropagation(); props.toggleAuto(nk, expanded); if (!expanded) props.ensureDetails(db, n.name); }} />
                          : <span className="chev" />}
                        <Icon name={GROUP_ICON[g.kind]} className={`ico-${g.kind}`} />
                        <span className="name"><Highlight text={n.name} terms={props.terms} /></span>
                        {n.parent && <span className="dim"> · {n.parent}</span>}
                        {isTable && <TableSize stat={dbStats?.[n.name]} />}
                        {isTable && (
                          <button className={`lock-btn ${isLocked ? "on" : ""}`} aria-pressed={isLocked}
                                  title={isLocked ? "Locked: the filter box searches this table's columns. Click to unlock." : "Lock the filter to this table (search its columns)"}
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    if (isLocked) props.onUnlockTable();
                                    else { props.onLockTable(db, n.name); props.openKeys([gk, nk]); props.ensureDetails(db, n.name); }
                                  }}><Icon name="lock" size={11} /></button>
                        )}
                      </div>
                      {isTable && expanded && (
                        <ColumnList
                          ensure={() => { if (open.has(nk)) props.ensureDetails(db, n.name); /* never for tables a filter expanded: that could be thousands of queries */ }}
                          detail={props.details[`${db}\0${n.name}`]}
                          indexes={props.indexes[`${db}\0${n.name}`]}
                          fallback={cols}
                          query={isLocked ? props.columnQuery : ""}
                          filtering={filtering}
                          matched={scopedMatches}
                          tableHasMatch={tablesWithMatchingColumns.has(n.name)}
                          terms={props.terms}
                          onPick={(column) => props.onPick({ kind: "column", db, table: n.name, column })}
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
function ColumnList({ onPick, ensure, detail, indexes, fallback, query, filtering, matched, tableHasMatch, terms }: {
  onPick(column: string): void; ensure(): void; detail: ColumnsOf | undefined; indexes: IndexDetail[] | undefined; fallback: Entry[] | undefined; query: string; filtering: boolean;
  matched: Set<number> | null; tableHasMatch: boolean; terms: string[];
}) {
  useEffect(() => { ensure(); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  // the indexes sit in a group that starts closed: a table can have dozens, and the columns are what people look for
  const [indexesOpen, setIndexesOpen] = useState(false);
  if (Array.isArray(detail)) {
    const shown = detail.filter((c) => columnMatches(query, c));
    return (
      <ul className="columns">
        {shown.slice(0, 500).map((c) => (
          <li key={c.name} className="row-item col" {...dragName(quoteIdent(c.name))} onClick={() => onPick(c.name)} title={`${c.name} ${c.column_type}${c.nullable ? "" : " NOT NULL"}${c.extra ? " " + c.extra : ""}`}>
            <span className="key">{c.key === "PRI" ? <Icon name="key" size={12} className="ico-pk" /> : c.key === "UNI" ? <Icon name="unique" size={11} /> : <Icon name="column" size={11} className="ico-col" />}</span>
            <span className="name"><Highlight text={c.name} terms={query ? query.split(/\s+/).map((t) => t.toLowerCase()) : terms} /></span>
            <span className="ctype">{c.column_type}</span>
          </li>
        ))}
        {shown.length === 0 && <li className="empty small">{query ? "No matching columns" : "No columns"}</li>}
        {shown.length > 500 && <li className="empty small">{shown.length - 500} more — type to narrow</li>}
        {!query && indexes && indexes.length > 0 && (
          <>
            <li className="row-item col idx-head" onClick={() => setIndexesOpen((o) => !o)} title={indexesOpen ? "Hide the indexes" : "Show the indexes"} aria-expanded={indexesOpen}>
              <Chev open={indexesOpen} onClick={(e) => { e.stopPropagation(); setIndexesOpen((o) => !o); }} />Indexes<span className="badge">{indexes.length}</span>
            </li>
            {indexesOpen && indexes.map((i) => (
              <li key={i.name} className="row-item col idx" title={`${i.unique ? "Unique " : ""}${i.kind} index ${i.name} on (${i.columns.join(", ")})`}>
                <span className="key">{i.name === "PRIMARY" ? <Icon name="key" size={12} className="ico-pk" /> : i.unique ? <Icon name="unique" size={11} /> : <Icon name="index" size={11} />}</span>
                <span className="name">{i.name}</span>
                <span className="ctype">({i.columns.join(", ")})</span>
              </li>
            ))}
          </>
        )}
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

/** Total size of a database's tables, once its sizes have been loaded. */
function DbSize({ stats }: { stats: Record<string, TableStat> | undefined }) {
  if (!stats) return null;
  const total = Object.values(stats).reduce((a, t) => a + (t.bytes ?? 0), 0);
  return <span className="size" title={`${total.toLocaleString()} bytes in ${Object.keys(stats).length} tables (estimate)`}>{formatBytes(total)}</span>;
}

/** Backtick-quote a MySQL identifier. */

/** Make a tree row draggable: dropping it on the editor inserts the (quoted) name where it lands. */
const dragName = (text: string) => ({
  draggable: true,
  onDragStart: (e: React.DragEvent) => { e.dataTransfer.setData("text/plain", text); e.dataTransfer.effectAllowed = "copy"; },
});
