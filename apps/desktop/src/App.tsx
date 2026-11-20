import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, queryApi, type Favorite, type Node, type SavedConnection } from "./api";
import ConnectionDialog from "./ConnectionDialog";
import ContextMenu, { type MenuItem } from "./ContextMenu";
import DataGrid, { defaultView, type GridView } from "./DataGrid";
import Editor, { type EditorHandle } from "./Editor";
import ObjectTree from "./ObjectTree";
import ResultsPane, { type RunState } from "./ResultsPane";
import { ConfirmDialog, FavoritesPanel, HistoryPanel, SaveFavoriteDialog } from "./SidePanels";

/** A SQL editor tab, or a table data tab (`kind: "table"`). Tabs saved before table tabs existed have no kind. */
interface QueryTab {
  id: string; title: string; sql: string; database: string | null;
  kind?: "sql" | "table";
  table?: { db: string; name: string };
  grid?: GridView;
}
const isTable = (t: QueryTab | undefined) => t?.kind === "table";
type Side = "objects" | "history" | "favorites";

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
  const [filterError, setFilterError] = useState<string | null>(null);
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
  const [restored, setRestored] = useState(false);

  const editor = useRef<EditorHandle>(null);
  const filterSeq = useRef(0);
  const runIds = useRef<Record<string, string>>({});

  const refresh = useCallback(async () => setConnections(await api.listConnections()), []);
  useEffect(() => { refresh(); }, [refresh]);
  const active = connections.find((c) => c.id === activeId);
  const tab = tabs.find((t) => t.id === activeTab);
  if (tab && !isTable(tab)) lastSqlTab.current = tab.id;
  const editorTab = tabs.find((t) => t.id === lastSqlTab.current && !isTable(t)) ?? tabs.find((t) => !isTable(t));
  const databases = useMemo(() => index.filter((n) => n.kind === "database").map((n) => n.name), [index]);

  // ---- connect / session restore ----
  const open = async (id: number) => {
    setStatus("Connecting…");
    try {
      if (activeId !== null && activeId !== id) await api.disconnect(activeId);
      const idx = await api.connect(id);
      let restoredTabs: QueryTab[] = [];
      let restoredActive = "";
      try {
        const saved = await queryApi.sessionGet(id);
        if (saved) {
          const s = JSON.parse(saved) as { tabs: QueryTab[]; active: string };
          restoredTabs = s.tabs.filter((t) => typeof t.id === "string" && (isTable(t) ? !!t.table : typeof t.sql === "string"))
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
      setStatus(`Error: ${e}`);
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

  // ---- filter ----
  useEffect(() => {
    if (activeId === null) return;
    if (!query.trim()) { setMatches(null); setFilterError(null); return; }
    const mine = ++filterSeq.current;
    const t = setTimeout(async () => {
      try {
        const hits = await api.filterIndex(activeId, query);
        if (mine !== filterSeq.current) return;
        setMatches(new Set(hits));
        setFilterError(null);
      } catch (e) {
        if (mine === filterSeq.current) setFilterError(String(e));
      }
    }, 80);
    return () => clearTimeout(t);
  }, [query, activeId]);

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
                placeholder="Filter objects…  ord*  /regex/  type:view  db:shop  col:email"
                value={query}
                disabled={activeId === null}
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={(e) => e.key === "Escape" && setQuery("")}
              />
              {query && <button className="clear" onClick={() => setQuery("")}>✕</button>}
            </div>
            {filterError && <div className="error small">{filterError}</div>}
            <div className="tree-wrap">
              {activeId === null
                ? <div className="empty">Create or select a connection</div>
                : <ObjectTree index={index} matches={matches} terms={terms} onOpenTable={(d, t) => openTable(d, t)} onTableMenu={(e, d, t) => {
                    e.preventDefault();
                    setTreeMenu({ x: e.clientX, y: e.clientY, items: [
                      { label: "Open data", onClick: () => openTable(d, t) },
                      { label: "Select in SQL editor", onClick: () => selectInEditor(d, t) },
                      { separator: true },
                      { label: "Copy name", onClick: () => navigator.clipboard.writeText(t) },
                    ] });
                  }} />}
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
                  {isTable(t) ? "▦ " : ""}{runs[t.id]?.running ? "⏳ " : ""}{t.title}
                  <button className="mini" onClick={(e) => { e.stopPropagation(); closeTab(t.id); }}>✕</button>
                </div>
              ))}
              <button className="mini plus" title="New query tab (⌘T)" onClick={() => addTab("", tab.database)}>＋</button>
            </div>
            {/* The editor stays mounted while a table tab is shown so each SQL tab keeps its undo history. */}
            {editorTab && (
              <div className="sql-pane" style={{ display: isTable(tab) ? "none" : "flex" }}>
                <div className="toolbar">
                  <button className="primary" disabled={running} onClick={run} title="Run selection or all (⌘↵)">▶ Run</button>
                  <button disabled={!running} onClick={cancel} title="Cancel running query">■ Cancel</button>
                  <button onClick={openFavDialog} title="Save to favorites">☆</button>
                  <span className="grow" />
                  <label className="dbsel">Database
                    <select value={editorTab.database ?? ""} onChange={(e) => patchTab(editorTab.id, { database: e.target.value || null })}>
                      <option value="">(none)</option>
                      {databases.map((d) => <option key={d} value={d}>{d}</option>)}
                    </select>
                  </label>
                </div>
                <div className="editor-wrap">
                  <Editor ref={editor} tabId={editorTab.id} initialDoc={editorTab.sql} onChange={(sql) => patchTab(editorTab.id, { sql })} onRun={run} />
                </div>
                <ResultsPane run={runs[editorTab.id]} />
              </div>
            )}
            {isTable(tab) && tab.table && (
              <DataGrid key={`${activeId}:${tab.id}`} connectionId={activeId} db={tab.table.db} table={tab.table.name}
                initial={tab.grid ?? defaultView()} onViewChange={(grid) => patchTab(tab.id, { grid })}
                onOpenTable={(d, t, f) => openTable(d, t, f)} />
            )}
          </>
        )}
        <div className="statusbar" style={{ borderTopColor: active?.color }}>{status}</div>
      </main>

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
