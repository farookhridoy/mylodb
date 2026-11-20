import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, describeError, editorApi, queryApi, schemaApi, type ConnectError, type Favorite, type FormatOptions, type Node, type SavedConnection, type TableOp } from "./api";
import AnalysisPane, { type Analysis } from "./AnalysisPane";
import ConnectionDialog from "./ConnectionDialog";
import ContextMenu, { type MenuItem } from "./ContextMenu";
import DataGrid, { defaultView, type GridView } from "./DataGrid";
import { PromptDialog, TypeConfirmDialog } from "./Dialogs";
import TableDesigner from "./TableDesigner";
import ServerTools from "./ServerTools";
import { useTheme, type ThemePref } from "./theme";
import TransferTools, { type TransferPreset } from "./TransferTools";
import UsersManager from "./UsersManager";
import Editor, { type EditorHandle } from "./Editor";
import ObjectTree from "./ObjectTree";
import { useBrowserMeta } from "./browserMeta";
import ResultsPane, { type RunState } from "./ResultsPane";
import { ConfirmDialog, FavoritesPanel, HistoryPanel, SaveFavoriteDialog } from "./SidePanels";

/** A SQL editor tab, or a table data tab (`kind: "table"`). Tabs saved before table tabs existed have no kind. */
interface QueryTab {
  id: string; title: string; sql: string; database: string | null;
  kind?: "sql" | "table" | "designer" | "users" | "server" | "transfer";
  /// For `transfer` tabs: which tool to show and what it is about.
  transfer?: TransferPreset;
  /// For `table` tabs: the table shown. For `designer` tabs: the table edited (`name: null` = a new table).
  table?: { db: string; name: string | null };
  grid?: GridView;
}
const isTable = (t: QueryTab | undefined) => t?.kind === "table";
/** Tabs that are not SQL editors (the editor stays mounted behind them). */
const isSql = (t: QueryTab | undefined) => !t || !t.kind || t.kind === "sql";
type Side = "objects" | "history" | "favorites";

const FORMAT_KEY = "mylodb.formatOptions";
const DEFAULT_FORMAT: FormatOptions = { indent: { style: "spaces", width: 2 }, keyword_case: "upper", lines_between_statements: 1 };
const loadFormat = (): FormatOptions => {
  try { return { ...DEFAULT_FORMAT, ...JSON.parse(localStorage.getItem(FORMAT_KEY) ?? "{}") }; } catch { return DEFAULT_FORMAT; }
};
/** Statements that change the schema: the object index (and so autocomplete) must be rebuilt after them. */
const isDdl = (sql: string) => /^\s*(?:\/\*[\s\S]*?\*\/\s*)*(create|alter|drop|rename|truncate)\b/i.test(sql);

let counter = 0;
const uid = () => `${Date.now().toString(36)}-${counter++}`;
const newTab = (n: number, sql = "", database: string | null = null): QueryTab => ({ id: uid(), title: `Query ${n}`, sql, database });
const quote = (s: string) => "`" + s.replace(/`/g, "``") + "`";

export default function App() {
  const [connections, setConnections] = useState<SavedConnection[]>([]);
  const [activeId, setActiveId] = useState<number | null>(null);
  const [index, setIndex] = useState<Node[]>([]);
  const [status, setStatus] = useState("Not connected");
  const [query, setQuery] = useState("");
  const [matches, setMatches] = useState<Set<number> | null>(null);
  /// The database the current matches were computed for (null: the matches are database names).
  const [matchScope, setMatchScope] = useState<string | null>(null);
  const [filterError, setFilterError] = useState<string | null>(null);
  /// The database selected in the tree: the filter searches inside it ("Filter tables in …").
  const [selectedDb, setSelectedDb] = useState<string | null>(null);
  /// The table selected in the tree (inside `selectedDb`): the filter then searches its columns.
  const [selectedTable, setSelectedTable] = useState<string | null>(null);
  const [columnsLoaded, setColumnsLoaded] = useState<Set<string>>(new Set());
  const meta = useBrowserMeta(activeId);
  const colQueue = useRef<Promise<unknown>>(Promise.resolve());
  const filterInput = useRef<HTMLInputElement>(null);
  const [dialog, setDialog] = useState<SavedConnection | "new" | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [side, setSide] = useState<Side>("objects");

  const [tabs, setTabs] = useState<QueryTab[]>([]);
  const [activeTab, setActiveTab] = useState<string>("");
  const [runs, setRuns] = useState<Record<string, RunState>>({});
  const [favDialog, setFavDialog] = useState<string | null>(null);
  const [favFolders, setFavFolders] = useState<string[]>([]);
  const [panelKey, setPanelKey] = useState(0);
  const [treeMenu, setTreeMenu] = useState<{ x: number; y: number; items: MenuItem[] } | null>(null);
  const lastSqlTab = useRef("");
  const [hostPrompt, setHostPrompt] = useState<{ id: number; host: string; fingerprint: string; changed: string | null } | null>(null);
  const [analyses, setAnalyses] = useState<Record<string, Analysis>>({});
  const [format, setFormat] = useState<FormatOptions>(loadFormat);
  const [formatOpen, setFormatOpen] = useState(false);
  const [formatError, setFormatError] = useState<string | null>(null);
  const [prompt, setPrompt] = useState<React.ComponentProps<typeof PromptDialog> | null>(null);
  const [typeConfirm, setTypeConfirm] = useState<React.ComponentProps<typeof TypeConfirmDialog> | null>(null);
  const [restored, setRestored] = useState(false);

  const theme = useTheme();
  const editor = useRef<EditorHandle>(null);
  const filterSeq = useRef(0);
  const runIds = useRef<Record<string, string>>({});

  const refresh = useCallback(async () => setConnections(await api.listConnections()), []);
  useEffect(() => { refresh(); }, [refresh]);
  const active = connections.find((c) => c.id === activeId);
  const tab = tabs.find((t) => t.id === activeTab);
  if (tab && isSql(tab)) lastSqlTab.current = tab.id;
  const editorTab = tabs.find((t) => t.id === lastSqlTab.current && isSql(t)) ?? tabs.find((t) => isSql(t));
  const databases = useMemo(() => index.filter((n) => n.kind === "database").map((n) => n.name), [index]);

  // ---- connect / session restore ----
  const open = async (id: number, trust?: string) => {
    setStatus("Connecting…");
    try {
      if (activeId !== null && activeId !== id) await api.disconnect(activeId);
      const idx = await api.connect(id, trust);
      setSelectedDb(null); setSelectedTable(null); setQuery(""); meta.reset(); setColumnsLoaded(new Set()); setMatches(null); setMatchScope(null);
      colQueue.current = Promise.resolve();
      let restoredTabs: QueryTab[] = [];
      let restoredActive = "";
      try {
        const saved = await queryApi.sessionGet(id);
        if (saved) {
          const s = JSON.parse(saved) as { tabs: QueryTab[]; active: string };
          restoredTabs = s.tabs.filter((t) => typeof t.id === "string" && (t.kind === "users" || t.kind === "server" || t.kind === "transfer" || (t.kind === "table" || t.kind === "designer" ? !!t.table : typeof t.sql === "string")))
            .map((t) => (isTable(t) ? { ...t, grid: { ...defaultView(), ...t.grid } } : t));
          restoredActive = s.active;
        }
      } catch { /* a corrupt session is ignored; start fresh */ }
      if (restoredTabs.length === 0) restoredTabs = [newTab(1)];
      setActiveId(id);
      setIndex(idx);
      setQuery("");
      setRuns({});
      setTabs(restoredTabs);
      setActiveTab(restoredTabs.some((t) => t.id === restoredActive) ? restoredActive : restoredTabs[0].id);
      setRestored(true);
      setStatus(`Connected · ${idx.filter((n) => n.kind === "database").length} databases`);
    } catch (e) {
      setActiveId(null); setIndex([]); setTabs([]); setRestored(false);
      const err = e as ConnectError;
      // an SSH server we have not seen before: ask the user before trusting its key
      if (typeof err === "object" && err?.kind === "unknown_host") {
        setStatus("Waiting for you to confirm the SSH host key");
        setHostPrompt({ id, host: err.host, fingerprint: err.fingerprint, changed: null });
      } else if (typeof err === "object" && err?.kind === "host_key_changed") {
        setStatus(`The SSH host key of ${err.host} changed`);
        setHostPrompt({ id, host: err.host, fingerprint: err.actual, changed: err.expected });
      } else setStatus(`Error: ${describeError(e)}`);
    }
  };

  // Autosave open tabs (debounced) so a crash or restart restores them.
  useEffect(() => {
    if (activeId === null || !restored) return;
    const t = setTimeout(() => {
      queryApi.sessionSet(activeId, JSON.stringify({ tabs, active: activeTab })).catch(() => {});
    }, 800);
    return () => clearTimeout(t);
  }, [tabs, activeTab, activeId, restored]);

  // ---- columns are loaded per database, on demand ----
  // Loads run one at a time, in order, so the positions the backend appended them at match this list.
  const ensureColumns = useCallback((db: string): Promise<unknown> => {
    if (activeId === null) return Promise.resolve();
    const id = activeId;
    const next = colQueue.current.then(async () => {
      let already = false;
      setColumnsLoaded((s) => { already = s.has(db); return s; });
      if (already) return;
      const r = await api.loadColumns(id, db);
      if (r.nodes.length === 0 && r.start === 0) return;
      setIndex((prev) => (prev.length === r.start ? [...prev, ...r.nodes] : prev));
      setColumnsLoaded((s) => new Set(s).add(db));
    }).catch(() => {});
    colQueue.current = next;
    return next;
  }, [activeId]);

  // autocomplete in the editor needs the columns of the database it is working in
  useEffect(() => { if (editorTab?.database) ensureColumns(editorTab.database); }, [editorTab?.database, ensureColumns]); // eslint-disable-line react-hooks/exhaustive-deps

  // ---- selection: nothing → database → table. Each level has its own filter, so the box starts empty at every level. ----
  const selectDb = (db: string) => {
    if (db === selectedDb && selectedTable === null) return;
    if (db !== selectedDb) setQuery("");
    setSelectedDb(db);
    if (selectedTable !== null) { setSelectedTable(null); setQuery(""); }
  };
  const selectTable = (db: string, table: string) => {
    if (db === selectedDb && table === selectedTable) return;
    setSelectedDb(db); setSelectedTable(table); setQuery("");
  };
  /// Clicking the connection node: back to the full list of databases.
  const resetBrowser = () => { setSelectedDb(null); setSelectedTable(null); setQuery(""); setMatches(null); setMatchScope(null); };

  // ---- filter ----
  // Scoped to the selected database; with none selected it matches database names only.
  useEffect(() => {
    if (activeId === null) return;
    // with a table selected the box filters its columns in the browser; there is nothing to ask the backend
    if (selectedTable !== null || !query.trim()) { setMatches(null); setMatchScope(null); setFilterError(null); return; }
    const mine = ++filterSeq.current;
    const t = setTimeout(async () => {
      try {
        // a column filter needs the columns of the database it searches
        if (selectedDb && /(^|\s)(col:|type:column)/i.test(query)) await ensureColumns(selectedDb);
        const hits = await api.filterIndex(activeId, query, selectedDb);
        if (mine !== filterSeq.current) return;
        setMatches(new Set(hits));
        setMatchScope(selectedDb);
        setFilterError(null);
      } catch (e) {
        if (mine === filterSeq.current) setFilterError(String(e));
      }
    }, 150);
    return () => clearTimeout(t);
  }, [query, activeId, selectedDb, selectedTable, ensureColumns]);

  const terms = useMemo(
    () => query.split(/\s+/).filter((t) => t && !/[:*?/]/.test(t)).map((t) => t.toLowerCase()),
    [query],
  );

  // ---- tabs ----
  const nextTitleNumber = (ts: QueryTab[]) => Math.max(0, ...ts.map((x) => Number(x.title.replace(/\D/g, "")) || 0)) + 1;

  // State updaters must stay pure (React runs them twice in StrictMode), so ids are made outside them.
  const addTab = (sql = "", database: string | null = null, title?: string) => {
    const t = newTab(nextTitleNumber(tabs), sql, database);
    if (title) t.title = title;
    setTabs([...tabs, t]);
    setActiveTab(t.id);
    return t;
  };

  const closeTab = (id: string) => {
    if (tabs.length === 1) {
      const t = newTab(1);
      setTabs([t]);
      setActiveTab(t.id);
    } else {
      const i = tabs.findIndex((t) => t.id === id);
      const rest = tabs.filter((t) => t.id !== id);
      setTabs(rest);
      if (id === activeTab) setActiveTab(rest[Math.min(i, rest.length - 1)].id);
    }
    setRuns((r) => { const { [id]: _drop, ...rest } = r; return rest; });
  };

  const patchTab = (id: string, patch: Partial<QueryTab>) =>
    setTabs((ts) => ts.map((t) => (t.id === id ? { ...t, ...patch } : t)));

  // ---- run / cancel ----
  const runSql = useCallback(async (tabId: string, sql: string, database: string | null) => {
    if (activeId === null || !sql.trim()) return;
    const runId = uid();
    runIds.current[tabId] = runId;
    const started = performance.now();
    setRuns((r) => ({ ...r, [tabId]: { running: true, results: null, error: null, totalMs: 0 } }));
    let state: RunState;
    try {
      const results = await queryApi.run(activeId, runId, database, sql);
      state = { running: false, results, error: null, totalMs: Math.round(performance.now() - started) };
    } catch (e) {
      state = { running: false, results: null, error: String(e), totalMs: Math.round(performance.now() - started) };
    }
    delete runIds.current[tabId];
    setRuns((r) => ({ ...r, [tabId]: state }));
    // keep the tree and autocomplete in step with what the script just changed
    if (state.results?.some((r) => !r.error && isDdl(r.sql))) schemaApi.refreshIndex(activeId).then(setIndex).catch(() => {});
    setPanelKey((k) => k + 1); // refresh history
  }, [activeId]);

  const run = useCallback(() => {
    if (!editorTab || !editor.current) return;
    runSql(editorTab.id, editor.current.runnableText(), editorTab.database);
  }, [editorTab, runSql]);

  const cancel = () => {
    const id = editorTab && runIds.current[editorTab.id];
    if (id) queryApi.cancel(id).catch(() => {});
  };

  /** Open a table's data in a grid tab; an existing unfiltered tab for the same table is reused. */
  const openTable = (database: string, table: string, filters?: GridView["filters"]) => {
    if (!filters) {
      const existing = tabs.find((t) => isTable(t) && t.table?.db === database && t.table.name === table && Object.keys(t.grid?.filters ?? {}).length === 0);
      if (existing) return setActiveTab(existing.id);
    }
    const t: QueryTab = { id: uid(), title: table, sql: "", database, kind: "table", table: { db: database, name: table }, grid: { ...defaultView(), filters: filters ?? {} } };
    setTabs([...tabs, t]);
    setActiveTab(t.id);
  };

  const selectInEditor = (database: string, table: string) => {
    const sql = `SELECT * FROM ${quote(database)}.${quote(table)} LIMIT 1000;`;
    const t = addTab(sql, database, table);
    runSql(t.id, sql, database);
  };

  // ---- schema tools ----
  const reloadIndex = async () => { if (activeId !== null) { meta.reset(); setIndex(await schemaApi.refreshIndex(activeId)); } };

  const openDesigner = (database: string, table: string | null) => {
    const existing = tabs.find((t) => t.kind === "designer" && t.table?.db === database && t.table.name === table && table !== null);
    if (existing) return setActiveTab(existing.id);
    const t: QueryTab = { id: uid(), title: table ?? "New table", sql: "", database, kind: "designer", table: { db: database, name: table } };
    setTabs([...tabs, t]);
    setActiveTab(t.id);
  };

  /// Open (or reuse) the Backup / Restore / Import / Export tab, optionally aimed at a database or table.
  const openTransfer = (preset: TransferPreset = {}) => {
    const existing = tabs.find((t) => t.kind === "transfer");
    if (existing) {
      setTabs(tabs.map((t) => (t.id === existing.id ? { ...t, transfer: preset } : t)));
      return setActiveTab(existing.id);
    }
    const t: QueryTab = { id: uid(), title: "Backup & import", sql: "", database: null, kind: "transfer", transfer: preset };
    setTabs([...tabs, t]);
    setActiveTab(t.id);
  };

  const openServer = () => {
    const existing = tabs.find((t) => t.kind === "server");
    if (existing) return setActiveTab(existing.id);
    const t: QueryTab = { id: uid(), title: "Server", sql: "", database: null, kind: "server" };
    setTabs([...tabs, t]);
    setActiveTab(t.id);
  };

  const openUsers = () => {
    const existing = tabs.find((t) => t.kind === "users");
    if (existing) return setActiveTab(existing.id);
    const t: QueryTab = { id: uid(), title: "Users", sql: "", database: null, kind: "users" };
    setTabs([...tabs, t]);
    setActiveTab(t.id);
  };

  /// Run a table operation, then refresh the tree and fix up any tabs that referred to the table.
  const runTableOp = async (database: string, table: string, operation: TableOp) => {
    if (activeId === null) return;
    try {
      await schemaApi.tableOp(activeId, database, table, operation);
      await reloadIndex();
      const refers = (t: QueryTab) => (t.kind === "table" || t.kind === "designer") && t.table?.db === database && t.table.name === table;
      if (operation.op === "drop") {
        const rest = tabs.filter((t) => !refers(t));
        const next = rest.length ? rest : [newTab(1)];
        setTabs(next);
        if (!next.some((t) => t.id === activeTab)) setActiveTab(next[0].id);
      } else if (operation.op === "rename") {
        setTabs(tabs.map((t) => (refers(t) ? { ...t, title: operation.to, table: { db: database, name: operation.to } } : t)));
      } else if (operation.op === "truncate") {
        setTabs([...tabs]); // data tabs reload when reopened; nothing to rewrite
      }
      setStatus(`${operation.op} ${database}.${table}: done`);
    } catch (e) { setStatus(`Error: ${e}`); }
  };

  const tableMenu = (e: React.MouseEvent, d: string, t: string) => {
    e.preventDefault();
    setTreeMenu({ x: e.clientX, y: e.clientY, items: [
      { label: "Open data", onClick: () => openTable(d, t) },
      { label: "Design table…", onClick: () => openDesigner(d, t) },
      { label: "Select in SQL editor", onClick: () => selectInEditor(d, t) },
      { label: "Import CSV into this table…", onClick: () => openTransfer({ tab: "import", db: d, table: t }) },
      { separator: true },
      { label: "Duplicate…", onClick: () => setPrompt({ title: `Duplicate ${t}`, label: "New table name", initial: `${t}_copy`, confirmLabel: "Duplicate", option: { label: "Copy the rows too", initial: false },
          onCancel: () => setPrompt(null), onSubmit: (to, withData) => { setPrompt(null); runTableOp(d, t, { op: "duplicate", to, with_data: withData }); } }) },
      { label: "Rename…", onClick: () => setPrompt({ title: `Rename ${t}`, label: "New name", initial: t, confirmLabel: "Rename",
          onCancel: () => setPrompt(null), onSubmit: (to) => { setPrompt(null); if (to !== t) runTableOp(d, t, { op: "rename", to }); } }) },
      { separator: true },
      { label: "Truncate…", onClick: () => setTypeConfirm({ title: `Truncate ${t}`, message: `All rows of ${d}.${t} will be deleted and cannot be restored.`, expect: t, confirmLabel: "Truncate",
          onCancel: () => setTypeConfirm(null), onConfirm: () => { setTypeConfirm(null); runTableOp(d, t, { op: "truncate" }); } }) },
      { label: "Drop…", onClick: () => setTypeConfirm({ title: `Drop ${t}`, message: `The table ${d}.${t} and all its data will be permanently deleted.`, expect: t, confirmLabel: "Drop table",
          onCancel: () => setTypeConfirm(null), onConfirm: () => { setTypeConfirm(null); runTableOp(d, t, { op: "drop" }); } }) },
      { separator: true },
      { label: "Copy name", onClick: () => navigator.clipboard.writeText(t) },
    ] });
  };

  const databaseMenu = (e: React.MouseEvent, d: string | null) => {
    e.preventDefault();
    const items: MenuItem[] = [];
    if (d) items.push({ label: "New table…", onClick: () => openDesigner(d, null) }, { label: "Back up this database…", onClick: () => openTransfer({ tab: "backup", db: d }) }, { label: "Import CSV…", onClick: () => openTransfer({ tab: "import", db: d }) }, { separator: true });
    items.push({ label: "Create database…", onClick: () => setPrompt({ title: "Create database", label: "Database name", confirmLabel: "Create",
      onCancel: () => setPrompt(null), onSubmit: async (name) => { setPrompt(null); try { await schemaApi.databaseOp(activeId!, { op: "create", name, charset: "utf8mb4", collation: null }); await reloadIndex(); } catch (err) { setStatus(`Error: ${err}`); } } }) });
    if (d) items.push({ label: "Drop database…", onClick: () => setTypeConfirm({ title: `Drop database ${d}`, message: `The database ${d} with all its tables and data will be permanently deleted.`, expect: d, confirmLabel: "Drop database",
      onCancel: () => setTypeConfirm(null), onConfirm: async () => { setTypeConfirm(null); try { await schemaApi.databaseOp(activeId!, { op: "drop", name: d }); await reloadIndex(); setTabs(tabs.filter((t) => t.table?.db !== d || t.kind === "sql")); } catch (err) { setStatus(`Error: ${err}`); } } }) });
    setTreeMenu({ x: e.clientX, y: e.clientY, items });
  };

  // ---- formatting and analysis ----
  const saveFormat = (f: FormatOptions) => { setFormat(f); localStorage.setItem(FORMAT_KEY, JSON.stringify(f)); };

  const formatSql = async () => {
    const ed = editor.current;
    if (!ed) return;
    try {
      ed.replaceRunnable(await editorApi.format(ed.runnableText(), format));
      setFormatError(null);
    } catch (e) { setFormatError(String(e)); setTimeout(() => setFormatError(null), 6000); }
  };

  const analyse = async (kind: "explain" | "analyze" | "profile") => {
    if (activeId === null || !editorTab || !editor.current) return;
    const tabId = editorTab.id;
    const database = editorTab.database;
    const sql = await editor.current.analysisTarget();
    const start = { kind, sql, running: true, error: null, data: null } as Analysis;
    setAnalyses((a) => ({ ...a, [tabId]: start }));
    try {
      const data = kind === "profile" ? await editorApi.profile(activeId, database, sql) : await editorApi.explain(activeId, database, sql, kind === "analyze");
      setAnalyses((a) => ({ ...a, [tabId]: { ...start, running: false, data } as Analysis }));
    } catch (e) {
      setAnalyses((a) => ({ ...a, [tabId]: { ...start, running: false, error: String(e) } as Analysis }));
    }
  };

  const running = editorTab ? runs[editorTab.id]?.running : false;
  const insertIntoEditor = (sql: string) => editor.current?.insert(sql);

  const openFavDialog = async () => {
    const text = editor.current?.runnableText().trim();
    if (!text) return;
    setFavFolders([...new Set((await queryApi.favoritesList()).map((f: Favorite) => f.folder).filter(Boolean))]);
    setFavDialog(text);
  };

  // Global shortcuts: ⌘T new tab, ⌘W close tab.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.metaKey || activeId === null) return;
      if (e.shiftKey && e.key.toLowerCase() === "b") { e.preventDefault(); setSide("objects"); const focus = () => filterInput.current?.focus(); focus(); setTimeout(focus, 0); /* the second call covers the objects tab not being mounted yet */ return; }
      if (e.key === "t") { e.preventDefault(); addTab("", tab?.database ?? null); }
      if (e.key === "w") { e.preventDefault(); if (tab) closeTab(tab.id); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="conn-bar">
          <select value={activeId ?? ""} onChange={(e) => e.target.value && open(Number(e.target.value))}>
            <option value="">Select connection…</option>
            {connections.map((c) => <option key={c.id} value={c.id!}>{c.name}</option>)}
          </select>
          <button title="New connection" onClick={() => setDialog("new")}>＋</button>
          <button title="Edit connection" disabled={!active} onClick={() => active && setDialog(active)}>✎</button>
          <button title="Delete connection" disabled={!active} onClick={() => setConfirmDelete(true)}>🗑</button>
          <button title="Users and privileges" disabled={activeId === null} onClick={openUsers}>👤</button>
 <button title="Backup, restore, import and export" disabled={activeId === null} onClick={() => openTransfer()}>⇅</button>
          <button className="theme-toggle" title={`Theme: ${theme.pref} (click to change)`}
            onClick={() => { const order: ThemePref[] = ["system", "light", "dark"]; theme.setPref(order[(order.indexOf(theme.pref) + 1) % 3]); }}>
            {theme.pref === "system" ? "◐" : theme.pref === "light" ? "☀" : "☾"}
          </button>
          <button title="Server: processes, variables, status, maintenance, logs" disabled={activeId === null} onClick={openServer}>🖥</button>
        </div>
        <div className="side-tabs">
          {(["objects", "history", "favorites"] as Side[]).map((s) => (
            <button key={s} className={side === s ? "active" : ""} onClick={() => setSide(s)}>
              {s === "objects" ? "Objects" : s === "history" ? "History" : "Favorites"}
            </button>
          ))}
        </div>
        {side === "objects" && (
          <>
            <div className="filter">
              <input
                ref={filterInput}
                placeholder={selectedTable ? `Filter columns in ${selectedTable}` : selectedDb ? `Filter tables in ${selectedDb}` : "Filter databases"}
                title={selectedTable ? "Columns of the selected table: a name or a type, e.g.  email  ·  bigint  ·  char*  ·  pk" : "Filter (⇧⌘B). Click a database to filter its tables, then a table to filter its columns.\nord  ·  ord*  ·  /regex/  ·  type:view  ·  col:email"}
                value={query}
                disabled={activeId === null}
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={(e) => e.key === "Escape" && setQuery("")}
              />
              {query && <button className="clear" onClick={() => setQuery("")}>✕</button>}
              <button className="refresh" disabled={activeId === null} title="Refresh objects and rebuild autocomplete" onClick={() => reloadIndex()}>⟳</button>
            </div>
            {filterError && <div className="error small">{filterError}</div>}
            <div className="tree-wrap">
              {activeId === null
                ? <div className="empty">Create or select a connection</div>
                : <ObjectTree index={index} matches={matches} scope={matchScope} terms={terms} selectedDb={selectedDb} onSelectDb={selectDb} selectedTable={selectedTable} onSelectTable={selectTable} rootLabel={active ? `${active.user}@${active.host}` : ""} onResetRoot={resetBrowser} columnQuery={selectedTable ? query : ""} stats={meta.stats} ensureStats={meta.ensureStats} details={meta.details} ensureDetails={meta.ensureDetails}
                    columnsLoaded={columnsLoaded} ensureColumns={ensureColumns} onOpenTable={(d, t) => openTable(d, t)} onTableMenu={tableMenu} onDatabaseMenu={databaseMenu} />}
            </div>
          </>
        )}
        {side === "history" && activeId !== null && (
          <HistoryPanel connectionId={activeId} refreshKey={panelKey} onInsert={insertIntoEditor} onOpen={(sql) => addTab(sql, tab?.database ?? null)} />
        )}
        {side === "favorites" && (
          <FavoritesPanel refreshKey={panelKey} onInsert={insertIntoEditor} onOpen={(sql) => addTab(sql, tab?.database ?? null)} />
        )}
      </aside>

      <main className="main">
        {activeId === null || !tab ? (
          <div className="placeholder"><h2>MyLoDB</h2><p>Create or select a connection to start.</p></div>
        ) : (
          <>
            <div className="query-tabs" style={{ borderBottomColor: active?.color }}>
              {tabs.map((t) => (
                <div key={t.id} className={`qtab ${t.id === activeTab ? "active" : ""}`} onClick={() => setActiveTab(t.id)}>
                  {isTable(t) ? "▦ " : t.kind === "designer" ? "✎ " : t.kind === "users" ? "👤 " : t.kind === "server" ? "🖥 " : t.kind === "transfer" ? "⇅ " : ""}{runs[t.id]?.running ? "⏳ " : ""}{t.title}
                  <button className="mini" onClick={(e) => { e.stopPropagation(); closeTab(t.id); }}>✕</button>
                </div>
              ))}
              <button className="mini plus" title="New query tab (⌘T)" onClick={() => addTab("", tab.database)}>＋</button>
            </div>
            {/* The editor stays mounted while a table tab is shown so each SQL tab keeps its undo history. */}
            {editorTab && (
              <div className="sql-pane" style={{ display: isSql(tab) ? "flex" : "none" }}>
                <div className="toolbar">
                  <button className="primary" disabled={running} onClick={run} title="Run selection or all (⌘↵)">▶ Run</button>
                  <button disabled={!running} onClick={cancel} title="Cancel running query">■ Cancel</button>
                  <button onClick={openFavDialog} title="Save to favorites">☆</button>
                  <span className="sep" />
                  <button onClick={formatSql} title="Format the selection or the whole script (⇧⌘F)">Format</button>
                  <div className="popup-host">
                    <button className="mini" title="Formatting options" onClick={() => setFormatOpen(!formatOpen)}>⚙</button>
                    {formatOpen && (
                      <div className="popup left" onMouseLeave={() => setFormatOpen(false)}>
                        <label>Indent
                          <select value={format.indent.style === "tab" ? "tab" : String((format.indent as { width: number }).width)}
                            onChange={(e) => saveFormat({ ...format, indent: e.target.value === "tab" ? { style: "tab" } : { style: "spaces", width: Number(e.target.value) } })}>
                            <option value="2">2 spaces</option><option value="4">4 spaces</option><option value="8">8 spaces</option><option value="tab">Tab</option>
                          </select></label>
                        <label>Keywords
                          <select value={format.keyword_case} onChange={(e) => saveFormat({ ...format, keyword_case: e.target.value as FormatOptions["keyword_case"] })}>
                            <option value="upper">UPPER CASE</option><option value="lower">lower case</option><option value="preserve">As typed</option>
                          </select></label>
                        <label>Blank lines between statements
                          <select value={format.lines_between_statements} onChange={(e) => saveFormat({ ...format, lines_between_statements: Number(e.target.value) })}>
                            <option value="0">0</option><option value="1">1</option><option value="2">2</option>
                          </select></label>
                      </div>
                    )}
                  </div>
                  <span className="sep" />
                  <button onClick={() => analyse("explain")} title="Explain the selected statement, or the one at the cursor">Explain</button>
                  <button onClick={() => analyse("analyze")} title="EXPLAIN ANALYZE: runs the SELECT and shows real timings">Analyze</button>
                  <button onClick={() => analyse("profile")} title="Run the SELECT and show stage timings and the work it caused">Profile</button>
                  {formatError && <span className="error small">{formatError}</span>}
                  <span className="grow" />
                  <label className="dbsel">Database
                    <select value={editorTab.database ?? ""} onChange={(e) => patchTab(editorTab.id, { database: e.target.value || null })}>
                      <option value="">(none)</option>
                      {databases.map((d) => <option key={d} value={d}>{d}</option>)}
                    </select>
                  </label>
                </div>
                <div className="editor-wrap">
                  <Editor ref={editor} tabId={editorTab.id} initialDoc={editorTab.sql} onChange={(sql) => patchTab(editorTab.id, { sql })} onRun={run} onFormat={formatSql} connectionId={activeId} database={editorTab.database} dark={theme.resolved === "dark"} />
                </div>
                <ResultsPane run={runs[editorTab.id]} analysis={analyses[editorTab.id]} />
              </div>
            )}
            {tab.kind === "designer" && tab.table && (
              <TableDesigner key={`${activeId}:${tab.id}`} connectionId={activeId} db={tab.table.db} table={tab.table.name} nodes={index} ensureColumns={ensureColumns}
                onOpenData={(d, t) => openTable(d, t)}
                onApplied={async (name) => { await reloadIndex(); patchTab(tab.id, { title: name, table: { db: tab.table!.db, name } }); }} />
            )}
            {tab.kind === "transfer" && <TransferTools key={`${activeId}:${tab.id}`} connectionId={activeId} nodes={index} ensureColumns={ensureColumns} preset={tab.transfer ?? {}} onSchemaChanged={() => reloadIndex()} />}
            {tab.kind === "server" && <ServerTools key={`${activeId}:${tab.id}`} connectionId={activeId} nodes={index} />}
            {tab.kind === "users" && <UsersManager key={`${activeId}:${tab.id}`} connectionId={activeId} nodes={index} ensureColumns={ensureColumns} />}
            {isTable(tab) && tab.table && tab.table.name && (
              <DataGrid key={`${activeId}:${tab.id}`} connectionId={activeId} db={tab.table.db} table={tab.table.name}
                initial={tab.grid ?? defaultView()} onViewChange={(grid) => patchTab(tab.id, { grid })}
                onOpenTable={(d, t, f) => openTable(d, t, f)} />
            )}
          </>
        )}
        <div className="statusbar" style={{ borderTopColor: active?.color }}>{status}</div>
      </main>

      {hostPrompt && (
        <div className="overlay">
          <div className="dialog">
            {hostPrompt.changed === null ? (
              <>
                <h3>Trust this SSH server?</h3>
                <p>The authenticity of <b>{hostPrompt.host}</b> cannot be established. Its host key fingerprint is:</p>
                <code className="preview">{hostPrompt.fingerprint}</code>
                <p className="dim small">Compare it with the fingerprint your server administrator gave you. If it does not match, do not continue.</p>
                <div className="actions">
                  <button onClick={() => { setHostPrompt(null); setStatus("Not connected"); }}>Cancel</button>
                  <button className="primary" onClick={() => { const h = hostPrompt; setHostPrompt(null); open(h.id, h.fingerprint); }}>Trust and connect</button>
                </div>
              </>
            ) : (
              <>
                <h3>⚠ SSH host key changed</h3>
                <p>The host key of <b>{hostPrompt.host}</b> is not the one saved before. Someone may be intercepting the connection, or the server was re-installed. The connection was <b>not</b> made.</p>
                <code className="preview">saved:   {hostPrompt.changed}{"\n"}now:     {hostPrompt.fingerprint}</code>
                <div className="actions">
                  <button onClick={() => { setHostPrompt(null); setStatus("Not connected"); }}>Close</button>
                  <button className="danger" onClick={async () => { const h = hostPrompt; await api.forgetSshHost(h.id); setHostPrompt(null); open(h.id); }}>I know why: forget the saved key</button>
                </div>
              </>
            )}
          </div>
        </div>
      )}
      {prompt && <PromptDialog {...prompt} />}
      {typeConfirm && <TypeConfirmDialog {...typeConfirm} />}
      {treeMenu && <ContextMenu x={treeMenu.x} y={treeMenu.y} items={treeMenu.items} onClose={() => setTreeMenu(null)} />}
      {dialog && (
        <ConnectionDialog
          initial={dialog === "new" ? undefined : dialog}
          onClose={() => setDialog(null)}
          onSaved={async (id) => { setDialog(null); await refresh(); open(id); }}
        />
      )}
      {confirmDelete && active?.id && (
        <ConfirmDialog
          message={`Delete connection "${active.name}" and its saved history and session?`}
          confirmLabel="Delete"
          onCancel={() => setConfirmDelete(false)}
          onConfirm={async () => {
            await api.deleteConnection(active.id!);
            setConfirmDelete(false); setActiveId(null); setIndex([]); setTabs([]); setRestored(false);
            setStatus("Not connected"); refresh();
          }}
        />
      )}
      {favDialog && (
        <SaveFavoriteDialog sql={favDialog} folders={favFolders} onClose={() => setFavDialog(null)}
          onSaved={() => { setFavDialog(null); setPanelKey((k) => k + 1); }} />
      )}
    </div>
  );
}
