import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Select from "./Select";
import { open as pickPath, save as savePath } from "@tauri-apps/plugin-dialog";
import { capabilitiesApi, type Capabilities, fileApi, onMenuAction, api, describeError, editorApi, planApi, queryApi, schemaApi, serverApi, type PlanOp, type ConnectError, type Favorite, type FormatOptions, type Kind, type Environment, type Maintenance, type Review, type Node, type SavedConnection, type TableOp } from "./api";
import AnalysisPane, { type Analysis } from "./AnalysisPane";
import ConnectionDialog from "./ConnectionDialog";
import ContextMenu, { type MenuItem } from "./ContextMenu";
import DataGrid, { defaultView, type GridView } from "./DataGrid";
import DataSearchPage from "./DataSearchPage";
import IndexAdvisorPage from "./IndexAdvisorPage";
import SchemaComparePage from "./SchemaComparePage";
import AboutDialog from "./AboutDialog";
import Manual from "./Manual";
import Tour, { tourSeen } from "./Tour";
import { AlterDatabaseDialog, PromptDialog, TypeConfirmDialog } from "./Dialogs";
import PreviewDialog from "./PreviewDialog";
import DatabasesPage from "./DatabasesPage";
import StructurePage from "./StructurePage";
import HomePage from "./HomePage";
import CatalogPage from "./CatalogPage";
import PreferencesDialog from "./PreferencesDialog";
import TasksPanel from "./TasksPanel";
import CommandPalette, { type Command } from "./CommandPalette";
import { SNIPPETS } from "./snippets";
import { loadMySnippets, saveMySnippet, deleteMySnippet, type MySnippet } from "./mySnippets";
import { loadPrefs, type Prefs } from "./prefs";
import TableDesigner from "./TableDesigner";
import ServerTools from "./ServerTools";
import { useTheme, type ThemePref } from "./theme";
import TransferTools, { type TransferPreset } from "./TransferTools";
import UsersManager from "./UsersManager";
import Editor, { type EditorHandle } from "./Editor";
import ObjectTree from "./ObjectTree";
import { useBrowserMeta } from "./browserMeta";
import Icon from "./Icon";
import ReviewDialog from "./ReviewDialog";
import { ENV_COLOR, ENV_LABEL } from "./environment";
import FilterOptions, { DEFAULT_SCOPE, isDefaultScope, type FilterScope } from "./FilterOptions";
import TableProperties from "./TableProperties";
import TableDiagram from "./TableDiagram";
import Splitter, { useSize } from "./Splitter";
import { sampleQuery, type SampleKind } from "./sqlTemplates";
import type { Pick } from "./ObjectTree";
import type { IconName } from "./Icon";
import ResultsPane, { type RunState } from "./ResultsPane";
import { ConfirmDialog, FavoritesPanel, HistoryPanel, SaveFavoriteDialog } from "./SidePanels";

/** A SQL editor tab, or a table data tab (`kind: "table"`). Tabs saved before table tabs existed have no kind. */
interface QueryTab {
  id: string; title: string; sql: string; database: string | null;
  kind?: "sql" | "table" | "designer" | "users" | "server" | "transfer" | "databases" | "structure" | "home" | "catalog" | "compare" | "search";
  kind?: "sql" | "table" | "designer" | "users" | "server" | "transfer" | "databases" | "structure" | "home" | "catalog" | "compare" | "advisor";
  /// For `transfer` tabs: which tool to show and what it is about.
  transfer?: TransferPreset;
  /// For query tabs opened from or saved to a .sql file: its path.
  file?: string;
  /// For a new-table `designer` tab: the name and column count asked for on the Structure page.
  initialTable?: { name: string; columns: number };
  /// For `table` tabs: the table shown. For `designer` tabs: the table edited (`name: null` = a new table).
  table?: { db: string; name: string | null };
  grid?: GridView;
  /// For `table` tabs: which view is shown. Data is the default.
  view?: "properties" | "data" | "diagram";
}
const isTable = (t: QueryTab | undefined) => t?.kind === "table";
/** Tabs that have the SQL console: query tabs, and table tabs (console on top, the table's data below). */
const isSql = (t: QueryTab | undefined) => !t || !t.kind || t.kind === "sql" || t.kind === "table";
type Side = "objects" | "history" | "favorites";

const FILTER_SCOPE_KEY = "mylodb.filterScope";
const loadFilterScope = (): FilterScope => { try { return { ...DEFAULT_SCOPE, ...JSON.parse(localStorage.getItem(FILTER_SCOPE_KEY) ?? "{}") }; } catch { return DEFAULT_SCOPE; } };
const scopeKinds = (f: FilterScope): Kind[] => [
  ...(f.tables ? ["table" as Kind] : []), ...(f.views ? ["view" as Kind] : []), ...(f.routines ? ["procedure" as Kind, "function" as Kind] : []),
  ...(f.triggers ? ["trigger" as Kind] : []), ...(f.events ? ["event" as Kind] : []), ...(f.columns ? ["column" as Kind] : []),
];
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
  /// What the filter searches inside a locked database. Columns are off by default: a search for `supplier_` lists tables, not every column named so.
  const [filterScope, setFilterScopeState] = useState<FilterScope>(loadFilterScope);
  const setFilterScope = (f: FilterScope) => { setFilterScopeState(f); localStorage.setItem(FILTER_SCOPE_KEY, JSON.stringify(f)); };
  const [filterMenu, setFilterMenu] = useState(false);
  const [selectedTable, setSelectedTable] = useState<string | null>(null);
  /// The table the filter is locked to (the box searches its columns). Selecting a table does not lock it; the lock icon does.
  const [lockedTable, setLockedTable] = useState<string | null>(null);
  const [columnsLoaded, setColumnsLoaded] = useState<Set<string>>(new Set());
  const meta = useBrowserMeta(activeId);
  const sidebar = useSize("mylodb.sidebarWidth", 300, 200, () => Math.min(640, window.innerWidth - 360));
  const consoleSize = useSize("mylodb.editorHeight", 230, 70, () => window.innerHeight - 220);
  const activeIdRef = useRef(activeId);
  activeIdRef.current = activeId;
  // expanded nodes and selection are remembered per connection
  const [browserState, setBrowserState] = useState<{ open: string[] }>({ open: [] });
  const browserSaved = useRef<{ open: string[]; db: string | null; table: string | null }>({ open: [], db: null, table: null });
  const saveBrowser = (patch: Partial<{ open: string[]; db: string | null; table: string | null }>) => {
    browserSaved.current = { ...browserSaved.current, ...patch };
    if (activeIdRef.current !== null) localStorage.setItem(`mylodb.browser.${activeIdRef.current}`, JSON.stringify(browserSaved.current));
  };
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
  const connectionGroups = useMemo(() => [...new Set(connections.map((c) => c.group?.trim()).filter((g): g is string => !!g))].sort((a, b) => a.localeCompare(b)), [connections]);
  const [mySnippets, setMySnippets] = useState<MySnippet[]>(loadMySnippets);
  const [typeConfirm, setTypeConfirm] = useState<React.ComponentProps<typeof TypeConfirmDialog> | null>(null);
  const [restored, setRestored] = useState(false);

  const theme = useTheme();
  const [prefsOpen, setPrefsOpen] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  /// Connections that are open on the server side. Switching between them keeps each one's session (and its saved tabs).
  const [openIds, setOpenIds] = useState<number[]>([]);
  const [treeSort, setTreeSort] = useState<"name" | "size">(() => (localStorage.getItem("mylodb.treeSort") === "size" ? "size" : "name"));
  const [prefs, setPrefs] = useState<Prefs>(loadPrefs);
  const [livePrefs, setLivePrefs] = useState<Prefs>(prefs); // what the open Preferences dialog shows, so the editor previews it
  const editor = useRef<EditorHandle>(null);
  const filterSeq = useRef(0);
  const runIds = useRef<Record<string, string>>({});

  const [caps, setCaps] = useState<Capabilities | null>(null);
  const capOff = (k: "server_tools" | "users" | "dump_restore") => caps?.[k].available === false;
  const capTitle = (k: "server_tools" | "users" | "dump_restore", on: string) => (caps?.[k].available === false ? (caps[k].reason ?? "Not available for this connection") : on);
  const [aboutOpen, setAboutOpen] = useState(false);
  const [manualOpen, setManualOpen] = useState(false);
  // the first-run guide starts by itself once, shortly after the window is up
  const [tourOpen, setTourOpen] = useState(false);
  useEffect(() => { if (tourSeen()) return; const t = setTimeout(() => setTourOpen(true), 600); return () => clearTimeout(t); }, []);
  const [alterDb, setAlterDb] = useState<string | null>(null);
  const pendingSave = useRef<{ id: number; payload: string } | null>(null);
  const connectionsRef = useRef(connections);
  connectionsRef.current = connections;
  const refresh = useCallback(async () => setConnections(await api.listConnections()), []);
  useEffect(() => { refresh(); }, [refresh]);
  const active = connections.find((c) => c.id === activeId);
  // safety of the open connection
  const env: Environment = active?.environment ?? "development";
  const readOnly = !!active?.read_only;
  const bandColor = env === "development" ? active?.color : ENV_COLOR[env];
  const tab = tabs.find((t) => t.id === activeTab);
  if (tab && isSql(tab)) lastSqlTab.current = tab.id;
  const editorTab = tabs.find((t) => t.id === lastSqlTab.current && isSql(t)) ?? tabs.find((t) => isSql(t));
  const databases = useMemo(() => index.filter((n) => n.kind === "database").map((n) => n.name), [index]);

  // ---- connect / session restore ----
  const open = async (id: number, trust?: string) => {
    setStatus("Connecting…");
    try {
      const idx = await api.connect(id, trust);
      // what was expanded and selected last time on this connection (only what still exists on the server)
      let savedBrowser: { open: string[]; db: string | null; table: string | null } = { open: [], db: null, table: null };
      try { savedBrowser = { ...savedBrowser, ...JSON.parse(localStorage.getItem(`mylodb.browser.${id}`) ?? "{}") }; } catch { /* a damaged entry is ignored */ }
      const dbExists = (d: string | null) => !!d && idx.some((n) => n.kind === "database" && n.name === d);
      const restoredDb = dbExists(savedBrowser.db) ? savedBrowser.db : null;
      const restoredTable = restoredDb && idx.some((n) => n.database === restoredDb && (n.kind === "table" || n.kind === "view") && n.name === savedBrowser.table) ? savedBrowser.table : null;
      savedBrowser = { open: (savedBrowser.open ?? []).filter((k) => dbExists(k.split("/")[0])), db: restoredDb, table: restoredTable };
      browserSaved.current = savedBrowser;
      setBrowserState({ open: savedBrowser.open });
      setSelectedDb(restoredDb); setSelectedTable(restoredTable); setLockedTable(null); setQuery(""); meta.reset(); setColumnsLoaded(new Set()); setMatches(null); setMatchScope(null);
      colQueue.current = Promise.resolve();
      // what this connection's engine can do; a SQLite file has no server home, users, server tools or dump-based transfer
      const cp = await capabilitiesApi.get(id).catch(() => null);
      const serverKinds = cp && !cp.server_tools.available ? ["home", "users", "server", "transfer", "catalog", "designer"] : [];
      let restoredTabs: QueryTab[] = [];
      let restoredActive = "";
      try {
        const saved = await queryApi.sessionGet(id);
        if (saved) {
          const s = JSON.parse(saved) as { tabs: QueryTab[]; active: string };
          restoredTabs = s.tabs.filter((t) => typeof t.id === "string" && (t.kind === "users" || t.kind === "server" || t.kind === "home" || t.kind === "transfer" || (t.kind === "table" || t.kind === "designer" ? !!t.table : typeof t.sql === "string")))
            .filter((t) => !serverKinds.includes(t.kind ?? ""))
            .map((t) => (isTable(t) ? { ...t, grid: { ...defaultView(), ...t.grid } } : t));
          restoredActive = s.active;
        }
      } catch { /* a corrupt session is ignored; start fresh */ }
      if (restoredTabs.length === 0) {
        // a fresh connection opens on the server home, with a query tab beside it
        const home: QueryTab = { id: uid(), title: "Home", sql: "", database: null, kind: "home" };
        restoredTabs = serverKinds.length ? [newTab(1)] : [home, newTab(1)];
        restoredActive = restoredTabs[0].id;
      }
      setCaps(cp);
      setActiveId(id);
      setOpenIds((ids) => (ids.includes(id) ? ids : [...ids, id]));
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
    const payload = JSON.stringify({ tabs, active: activeTab });
    pendingSave.current = { id: activeId, payload };
    const t = setTimeout(() => {
      pendingSave.current = null;
      queryApi.sessionSet(activeId, payload).catch(() => {});
    }, 800);
    return () => clearTimeout(t);
  }, [tabs, activeTab, activeId, restored]);
  // Switching to another connection inside the delay must not drop the last edits of the one being left:
  // write them under the connection they belong to (unless it was deleted meanwhile).
  useEffect(() => () => {
    const p = pendingSave.current;
    if (p && connectionsRef.current.some((c) => c.id === p.id)) { pendingSave.current = null; queryApi.sessionSet(p.id, p.payload).catch(() => {}); }
  }, [activeId]);

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

  // autocomplete loads the columns it needs itself, into its own catalog (the filter index only grows through ensureColumns)

  // ---- selection: nothing → database (locked) → table (selected; the lock icon locks the filter to it) ----
  /// The console runs in the locked database: put it on the visible console tab.
  const useDatabaseInConsole = (db: string) => { if (editorTab && editorTab.database !== db) patchTab(editorTab.id, { database: db }); };
  const selectDb = (db: string) => {
    if (db !== selectedDb || lockedTable !== null) setQuery("");
    setSelectedDb(db); setSelectedTable(null); setLockedTable(null);
    saveBrowser({ db, table: null });
    useDatabaseInConsole(db);
  };
  const selectTable = (db: string, table: string) => {
    if (db !== selectedDb) { setQuery(""); setLockedTable(null); useDatabaseInConsole(db); }
    setSelectedDb(db); setSelectedTable(table);
    saveBrowser({ db, table });
  };
  const lockTable = (db: string, table: string) => {
    if (db !== selectedDb) useDatabaseInConsole(db);
    setSelectedDb(db); setSelectedTable(table); setLockedTable(table); setQuery("");
    saveBrowser({ db, table });
  };
  /// Clicking the connection node: back to the full list of databases.
  const resetBrowser = () => { setSelectedDb(null); setSelectedTable(null); setLockedTable(null); setQuery(""); setMatches(null); setMatchScope(null); saveBrowser({ db: null, table: null }); };

  /// Step out of the lock: locked table → database → all databases.
  const unlock = () => {
    if (lockedTable !== null) { setLockedTable(null); setQuery(""); }
    else if (selectedDb !== null) resetBrowser();
  };

  // ---- clicking a name in the tree pastes it into the query (only when the editor had the focus) ----
  const fromEditor = useRef(false);
  const lastPick = useRef({ text: "", at: 0 });
  const pickName = (p: Pick) => {
    if (!fromEditor.current || !editor.current || !isSql(tab)) return;
    const here = editorTab?.database;
    const text = p.kind === "database" ? quote(p.db)
      : p.kind === "table" ? (here === p.db ? quote(p.table) : `${quote(p.db)}.${quote(p.table)}`)
      : `${quote(p.table)}.${quote(p.column)}`;
    const now = Date.now();
    if (lastPick.current.text === text && now - lastPick.current.at < 500) return; // the second click of a double-click
    lastPick.current = { text, at: now };
    editor.current.insert(text);
  };

  // ---- sample queries (SQLyog's "Generate SQL"): the statement is put in the console and copied ----
  const putSql = (sql: string, db: string) => {
    if (editor.current && isSql(tab)) editor.current.insert(sql + "\n");
    else addTab(sql + "\n", db);
    navigator.clipboard?.writeText(sql).catch(() => {});
  };
  const sample = async (kind: SampleKind, db: string, table: string) => {
    if (activeId === null) return;
    try {
      const cached = meta.details[`${db}\0${table}`];
      const cols = Array.isArray(cached) ? cached : await api.columnDetails(activeId, db, table);
      putSql(sampleQuery(kind, db, table, cols, editorTab?.database !== db), db);
      setStatus(`${kind.toUpperCase()} sample for ${table} inserted and copied`);
    } catch (e) { setStatus(`Error: ${describeError(e)}`); }
  };
  const sampleItems = (db: string, table: string, kinds: SampleKind[] = ["select", "insert", "update", "delete"]): MenuItem => ({
    label: "Sample queries",
    children: kinds.map((k) => ({ label: k.toUpperCase(), onClick: () => sample(k, db, table) })),
  });
  // a restored (or newly selected) table needs its column details even if it was not clicked
  useEffect(() => {
    if (activeId !== null && selectedDb) { meta.ensureStats(selectedDb); if (selectedTable) meta.ensureDetails(selectedDb, selectedTable); }
  }, [activeId, selectedDb, selectedTable, meta.ensureStats, meta.ensureDetails]); // eslint-disable-line react-hooks/exhaustive-deps

  // ---- filter ----
  // Scoped to the selected database; with none selected it matches database names only.
  useEffect(() => {
    if (activeId === null) return;
    // with a table selected the box filters its columns in the browser; there is nothing to ask the backend
    if (lockedTable !== null || !query.trim()) { setMatches(null); setMatchScope(null); setFilterError(null); return; }
    const mine = ++filterSeq.current;
    const t = setTimeout(async () => {
      try {
        // a column filter needs the columns of the database it searches
        if (selectedDb && (filterScope.columns || /(^|\s)(col:|type:column)/i.test(query))) await ensureColumns(selectedDb);
        const hits = await api.filterIndex(activeId, query, selectedDb, selectedDb ? scopeKinds(filterScope) : null);
        if (mine !== filterSeq.current) return;
        setMatches(new Set(hits));
        setMatchScope(selectedDb);
        setFilterError(null);
      } catch (e) {
        if (mine === filterSeq.current) setFilterError(String(e));
      }
    }, 150);
    return () => clearTimeout(t);
  }, [query, activeId, selectedDb, lockedTable, ensureColumns, filterScope]);

  const terms = useMemo(
    () => query.split(/\s+/).filter((t) => t && !/[:*?/]/.test(t)).map((t) => t.toLowerCase()),
    [query],
  );

  // ---- tabs ----
  const nextTitleNumber = (ts: QueryTab[]) => Math.max(0, ...ts.map((x) => Number(x.title.replace(/\D/g, "")) || 0)) + 1;

  // State updaters must stay pure (React runs them twice in StrictMode), so ids are made outside them.
  const addTab = (sql = "", database: string | null = null, title?: string) => {
    const t = newTab(nextTitleNumber(tabs), sql, database ?? selectedDb);
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

  // Foreign key checks for the console's Run, per tab: null = leave the server's setting.
  const [fkByTab, setFkByTab] = useState<Record<string, boolean | null>>({});
  const fkRef = useRef(fkByTab);
  fkRef.current = fkByTab;

  // ---- .sql files and disconnect (File / Database menus) ----
  const baseName = (p: string) => p.split(/[\\/]/).pop() ?? p;
  const openSqlFile = async () => {
    const p = await pickPath({ multiple: false, filters: [{ name: "SQL", extensions: ["sql", "txt"] }] });
    if (typeof p !== "string") return;
    try {
      const text = await fileApi.read(p);
      const t = addTab(text, selectedDb, baseName(p));
      patchTab(t.id, { file: p });
      setStatus(`Opened ${p}`);
    } catch (e) { setStatus(`Error: ${describeError(e)}`); }
  };
  const saveSqlFile = async (asNew: boolean) => {
    if (!editorTab) return setStatus("Open a query tab first");
    let path = asNew ? undefined : editorTab.file;
    if (!path) {
      const chosen = await savePath({ defaultPath: `${editorTab.title.replace(/[^\w.-]+/g, "_")}.sql`, filters: [{ name: "SQL", extensions: ["sql"] }] });
      if (typeof chosen !== "string") return;
      path = chosen;
    }
    try {
      await fileApi.write(path, editorTab.sql);
      patchTab(editorTab.id, { file: path, title: baseName(path) });
      setStatus(`Saved ${path}`);
    } catch (e) { setStatus(`Error: ${describeError(e)}`); }
  };
  const closeConnection = async (id: number) => {
    try { await api.disconnect(id); } catch { /* the session is gone either way */ }
    const rest = openIds.filter((x) => x !== id);
    setOpenIds(rest);
    if (id !== activeId) return;
    if (rest.length) return void open(rest[rest.length - 1]);
    setActiveId(null); setIndex([]); setTabs([newTab(1)]); setSelectedDb(null); setSelectedTable(null); setStatus("Disconnected");
  };
  const disconnectActive = async () => {
    if (activeId === null) return setStatus("Not connected");
    await closeConnection(activeId);
  };

  // ---- native menu bar: the Rust side reports the item id, this decides what it means right now ----
  const menuHandler = useRef<(id: string) => void>(() => {});
  menuHandler.current = (id) => {
    const connected = activeId !== null;
    const need = (f: () => void) => { if (connected) f(); else setStatus("Connect first"); };
    // a feature the connected engine lacks says why instead of failing later
    const gate = (cap: "server_tools" | "users" | "dump_restore", f: () => void) => { const c = caps?.[cap]; if (c && !c.available) setStatus(c.reason ?? "Not available for this connection"); else f(); };
    switch (id) {
      case "preferences": return setPrefsOpen(true);
      case "new_query": return need(() => addTab("", tab?.database ?? null));
      case "open_file": return need(() => { openSqlFile(); });
      case "save_file": return saveSqlFile(false);
      case "save_file_as": return saveSqlFile(true);
      case "run_file": return need(() => openTransfer({ tab: "restore" }));
      case "run": return need(run);
      case "preview": return need(previewRun);
      case "explain": return need(() => { analyse("explain"); });
      case "format": return need(() => { formatSql(); });
      case "palette": return setPaletteOpen(true);
      case "about": return setAboutOpen(true);
      case "manual": return setManualOpen(true);
      case "tour": return setTourOpen(true);
      case "new_query_db": return need(() => addTab("", selectedDb));
      case "disconnect": return disconnectActive();
      case "close_tab": return tab ? closeTab(tab.id) : undefined;
      case "new_connection": return setDialog("new");
      case "edit_connection": return active ? setDialog(active) : setStatus("Pick a connection first");
      case "refresh_tree": return need(() => reloadIndex());
      case "focus_filter": return need(() => { setSide("objects"); setTimeout(() => filterInput.current?.focus(), 0); });
      case "theme_system": return theme.setPref("system");
      case "theme_light": return theme.setPref("light");
      case "theme_dark": return theme.setPref("dark");
      case "home": return need(() => gate("server_tools", openHome));
      case "databases": return need(openDatabases);
      case "search_data": return need(() => openSearch(null));
      case "compare": return need(() => (selectedDb ? openCompare(selectedDb) : setStatus("Select a database in the tree first")));
      case "advisor": return need(() => (selectedDb ? openAdvisor(selectedDb) : setStatus("Select a database in the tree first")));
      case "structure": return need(() => (selectedDb ? openStructure(selectedDb) : setStatus("Select a database in the tree first")));
      case "new_table": return need(() => (selectedDb ? openDesigner(selectedDb, null) : setStatus("Select a database in the tree first")));
      case "export": return need(() => openTransfer({ tab: "export" }));
      case "import": return need(() => openTransfer({ tab: "import" }));
      case "backup": return need(() => gate("dump_restore", () => openTransfer({ tab: "backup" })));
      case "users": return need(() => gate("users", openUsers));
      case "server_tools": return need(() => gate("server_tools", openServer));
      case "catalog": return need(() => gate("server_tools", openCatalog));
      case "about_shortcuts": return setStatus("⌘T new tab · ⌘W close tab · ⌘↵ run · ⇧⌘F format · ⇧⌘B search objects · F5 refresh · ⌘, preferences");
    }
  };
  useEffect(() => {
    let un: (() => void) | undefined; let gone = false;
    onMenuAction((id) => menuHandler.current(id)).then((f) => { if (gone) f(); else un = f; });
    return () => { gone = true; un?.(); };
  }, []);

  // ---- run / cancel ----
  const runSql = useCallback(async (tabId: string, sql: string, database: string | null) => {
    if (activeId === null || !sql.trim()) return;
    const runId = uid();
    runIds.current[tabId] = runId;
    const started = performance.now();
    setRuns((r) => ({ ...r, [tabId]: { running: true, results: null, error: null, totalMs: 0 } }));
    let state: RunState;
    try {
      const results = await queryApi.run(activeId, runId, database, sql, fkRef.current[tabId] ?? null);
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

  /// Destructive statements are shown for review first (when the connection asks for it). The review is advisory: if it cannot be
  /// made, the run goes ahead, and a read-only connection is read-only on the server anyway.
  const [pendingReview, setPendingReview] = useState<{ review: Review; go(): void } | null>(null);
  const runReviewed = useCallback(async (tabId: string, sql: string, database: string | null) => {
    if (activeId === null || !sql.trim()) return;
    if (!readOnly) {
      try {
        const review = await queryApi.review(activeId, database, sql);
        if (review.confirm && review.items.length > 0) { setPendingReview({ review, go: () => { setPendingReview(null); runSql(tabId, sql, database); } }); return; }
      } catch { /* advisory */ }
    }
    runSql(tabId, sql, database);
  }, [activeId, readOnly, runSql]);

  const run = useCallback(() => {
    if (!editorTab || !editor.current) return;
    runReviewed(editorTab.id, editor.current.runnableText(), editorTab.database);
  }, [editorTab, runReviewed]);

  // Preview: the exact script (selection or all) in the Preview SQL dialog, with the foreign key switch, before it runs.
  const [scriptPreview, setScriptPreview] = useState<{ sql: string; database: string | null } | null>(null);
  const previewRun = () => {
    if (!editorTab || !editor.current) return;
    const sql = editor.current.runnableText();
    if (!sql.trim()) return setStatus("Nothing to preview: the editor is empty");
    setScriptPreview({ sql, database: editorTab.database });
  };

  const cancel = () => {
    const id = editorTab && runIds.current[editorTab.id];
    if (id) queryApi.cancel(id).catch(() => {});
  };

  /** Open a table's data in a grid tab; an existing unfiltered tab for the same table is reused. */
  /// Open (or reuse) a table's tab, optionally on a given view (Properties / Data / Diagram).
  const openTable = (database: string, table: string, filters?: GridView["filters"], view?: QueryTab["view"]) => {
    if (!filters) {
      const existing = tabs.find((t) => isTable(t) && t.table?.db === database && t.table.name === table && Object.keys(t.grid?.filters ?? {}).length === 0);
      if (existing) { if (view) patchTab(existing.id, { view }); return setActiveTab(existing.id); }
    }
    const t: QueryTab = { id: uid(), title: table, sql: `SELECT * FROM ${quote(table)} LIMIT 1000;\n`, database, kind: "table", table: { db: database, name: table }, grid: { ...defaultView(), filters: filters ?? {} }, ...(view ? { view } : {}) };
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

  const openNewTable = (database: string, name: string, columns: number) => {
    const t: QueryTab = { id: uid(), title: name, sql: "", database, kind: "designer", table: { db: database, name: null }, initialTable: { name, columns } };
    setTabs([...tabs, t]);
    setActiveTab(t.id);
  };
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

  const [dbRefresh, setDbRefresh] = useState(0);
  const openDatabases = () => {
    const existing = tabs.find((t) => t.kind === "databases");
    if (existing) return setActiveTab(existing.id);
    const t: QueryTab = { id: uid(), title: "Databases", sql: "", database: null, kind: "databases" };
    setTabs([...tabs, t]);
    setActiveTab(t.id);
  };

  const openStructure = (db: string) => {
    const existing = tabs.find((t) => t.kind === "structure" && t.table?.db === db);
    if (existing) return setActiveTab(existing.id);
    const t: QueryTab = { id: uid(), title: `${db}: Structure`, sql: "", database: db, kind: "structure", table: { db, name: null } };
    setTabs([...tabs, t]);
    setActiveTab(t.id);
  };

  const openSearch = (db: string | null) => {
    const target = db ?? selectedDb ?? databases[0];
    if (!target) return setStatus("Connect and select a database first");
    const existing = tabs.find((t) => t.kind === "search");
    if (existing) return setActiveTab(existing.id);
    const t: QueryTab = { id: uid(), title: "Data search", sql: "", database: target, kind: "search", table: { db: target, name: null } };
  const openAdvisor = (db: string) => {
    const w = caps?.writes;
    if (w && !w.available) return setStatus(w.reason ?? "Not available for this connection");
    const existing = tabs.find((t) => t.kind === "advisor" && t.table?.db === db);
    if (existing) return setActiveTab(existing.id);
    const t: QueryTab = { id: uid(), title: `Indexes: ${db}`, sql: "", database: db, kind: "advisor", table: { db, name: null } };
    setTabs([...tabs, t]);
    setActiveTab(t.id);
  };

  const openCompare = (db: string) => {
    const w = caps?.writes;
    if (w && !w.available) return setStatus(w.reason ?? "Not available for this connection");
    const existing = tabs.find((t) => t.kind === "compare" && t.table?.db === db);
    if (existing) return setActiveTab(existing.id);
    const t: QueryTab = { id: uid(), title: `Compare: ${db}`, sql: "", database: db, kind: "compare", table: { db, name: null } };
    setTabs([...tabs, t]);
    setActiveTab(t.id);
  };

  const openCatalog = () => {
    const existing = tabs.find((t) => t.kind === "catalog");
    if (existing) return setActiveTab(existing.id);
    const t: QueryTab = { id: uid(), title: "Engines & charsets", sql: "", database: null, kind: "catalog" };
    setTabs([...tabs, t]);
    setActiveTab(t.id);
  };

  const openHome = () => {
    const existing = tabs.find((t) => t.kind === "home");
    if (existing) return setActiveTab(existing.id);
    const t: QueryTab = { id: uid(), title: "Home", sql: "", database: null, kind: "home" };
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
  // ---- Preview SQL, then run: every drop / empty / rename / copy goes through one dialog that shows the exact statements ----
  const [planDlg, setPlanDlg] = useState<{ title: string; summary?: string; statements: string[]; confirmLabel?: string; after?: () => void } | null>(null);
  const openPlan = async (op: PlanOp, o: { title: string; summary?: string; confirmLabel?: string; after?: () => void }) => {
    if (activeId === null) return;
    try { setPlanDlg({ ...o, statements: await planApi.build(activeId, op) }); }
    catch (e) { setStatus(`Not possible: ${describeError(e)}`); }
  };
  /// Close the tabs of tables that no longer exist, keeping the active tab valid.
  const forgetTables = (db: string, names: string[]) => setTabs((prev) => {
    const rest = prev.filter((t) => !((t.kind === "table" || t.kind === "designer") && t.table?.db === db && names.includes(t.table.name ?? "")));
    return rest.length ? rest : [newTab(1)];
  });
  useEffect(() => { if (tabs.length && !tabs.some((t) => t.id === activeTab)) setActiveTab(tabs[0].id); }, [tabs, activeTab]);
  const renameTabs = (db: string, from: string, to: string) => setTabs((prev) => prev.map((t) => ((t.kind === "table" || t.kind === "designer") && t.table?.db === db && t.table.name === from ? { ...t, title: to, table: { db, name: to } } : t)));

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

  const renameTable = (d: string, t: string) => setPrompt({ title: `Rename ${t}`, label: "New name", initial: t, confirmLabel: "Rename",
    onCancel: () => setPrompt(null), onSubmit: (to) => { setPrompt(null); if (to !== t) openPlan({ op: "rename_table", db: d, from: t, to }, { title: `Rename ${t}`, confirmLabel: "Rename", after: () => renameTabs(d, t, to) }); } });

  const maintainTable = async (op: Maintenance, d: string, t: string) => {
    if (activeId === null) return;
    const word = op.op.toUpperCase();
    setStatus(`${word} ${d}.${t}…`);
    try {
      const rows = await serverApi.maintain(activeId, d, [t], op);
      setStatus(`${word} ${d}.${t}: ${rows.map((r) => r.msg_text).filter(Boolean).join("; ") || "done"}`);
    } catch (e) { setStatus(`Error: ${describeError(e)}`); }
  };

  /// "Copy Advanced Info": the qualified name, size and every column, as text.
  const copyAdvanced = async (d: string, t: string) => {
    if (activeId === null) return;
    try {
      const o = await api.tableOverview(activeId, d, t);
      const size = (o.info.data_length ?? 0) + (o.info.index_length ?? 0);
      const text = [`${quote(d)}.${quote(t)}`, `Engine: ${o.info.engine ?? "-"}  Rows (est.): ${o.info.rows ?? "-"}  Size: ${size} bytes`, "Columns:",
        ...o.columns.map((c) => `  ${c.name}  ${c.column_type}${c.nullable ? "" : " NOT NULL"}${c.key ? "  " + c.key : ""}${c.extra ? "  " + c.extra : ""}`)].join("\n");
      await navigator.clipboard.writeText(text);
      setStatus(`Copied details of ${t}`);
    } catch (e) { setStatus(`Error: ${describeError(e)}`); }
  };

  /// "DDL" in Generate SQL: the table's CREATE statement, in the console.
  const putDdl = async (d: string, t: string) => {
    if (activeId === null) return;
    try { putSql((await api.objectDefinition(activeId, "table", d, t)) + ";", d); setStatus(`DDL of ${t} inserted and copied`); }
    catch (e) { setStatus(`Error: ${describeError(e)}`); }
  };

  const createItems = (d: string): MenuItem[] => [
    { label: "New table…", icon: "table", onClick: () => openDesigner(d, null) },
    ...(["view", "procedure", "function", "trigger", "event"] as const).map((k) => ({ label: `New ${k}…`, icon: k as IconName, onClick: () => addTab(TEMPLATES[k](d), d, `New ${k}`) })),
  ];

  const tableMenu = (e: React.MouseEvent, d: string, t: string) => {
    e.preventDefault();
    const locked = lockedTable === t && selectedDb === d;
    setTreeMenu({ x: e.clientX, y: e.clientY, items: [
      { label: "Create", icon: "plus", children: createItems(d) },
      { label: "View Table", icon: "properties", shortcut: "F4", onClick: () => openTable(d, t, undefined, "properties") },
      { label: "Design table…", icon: "edit", onClick: () => openDesigner(d, t) },
      { label: locked ? "Unlock the filter" : "Lock the filter to this table", icon: "filter", onClick: () => (locked ? unlock() : lockTable(d, t)) },
      { separator: true },
      { label: "View Diagram", icon: "diagram", onClick: () => openTable(d, t, undefined, "diagram") },
      { label: "View Data", icon: "grid", onClick: () => openTable(d, t, undefined, "data") },
      { separator: true },
      { label: "Export Data…", icon: "export", onClick: () => openTransfer({ tab: "export", db: d, table: t }) },
      { label: "Import Data…", icon: "import", disabled: readOnly, onClick: () => openTransfer({ tab: "import", db: d, table: t }) },
      { label: "Tools", icon: "tools", children: [
        { label: "Dump table…", icon: "export", onClick: () => openTransfer({ tab: "backup", db: d, table: t }) },
        { separator: true },
        { label: "Analyze", disabled: readOnly, onClick: () => maintainTable({ op: "analyze" }, d, t) },
        { label: "Check", disabled: readOnly, onClick: () => maintainTable({ op: "check", option: null }, d, t) },
        { label: "Optimize", disabled: readOnly, onClick: () => maintainTable({ op: "optimize" }, d, t) },
        { label: "Repair", disabled: readOnly, onClick: () => maintainTable({ op: "repair", quick: false, extended: false, use_frm: false }, d, t) },
        { separator: true },
        { label: "Truncate…", icon: "eraser", danger: true, disabled: readOnly, onClick: () => openPlan({ op: "empty_tables", db: d, objects: [[t, false]] }, { title: `Empty ${t}`, confirmLabel: "Truncate", summary: `All rows of ${d}.${t} will be deleted and cannot be restored.` }) },
        { separator: true },
        { label: "Databases", icon: "database", onClick: openDatabases },
        { label: "Server tools", icon: "pulse", onClick: openServer },
      ] },
      { separator: true },
      { label: "Generate SQL", icon: "sql", children: [
        ...(["select", "insert", "update", "delete", "upsert"] as const).map((k) => ({ label: k === "upsert" ? "UPSERT" : k.toUpperCase(), icon: "sql" as IconName, onClick: () => sample(k, d, t) })),
        { separator: true },
        { label: "DDL", icon: "sql", onClick: () => putDdl(d, t) },
      ] },
      { label: "Read data in SQL console", icon: "terminal", onClick: () => selectInEditor(d, t) },
      { separator: true },
      { label: "Copy name", icon: "copy", onClick: () => navigator.clipboard.writeText(t) },
      { label: "Copy Advanced Info", icon: "copy", onClick: () => copyAdvanced(d, t) },
      { label: "Duplicate…", icon: "copy", disabled: readOnly, onClick: () => setPrompt({ title: `Duplicate ${t}`, label: "New table name", initial: `${t}_copy`, confirmLabel: "Next", option: { label: "Copy the rows too", initial: false },
          onCancel: () => setPrompt(null), onSubmit: (to, withData) => { setPrompt(null); openPlan({ op: "copy_tables", db: d, copies: [[t, to]], with_data: withData }, { title: `Duplicate ${t}`, confirmLabel: "Duplicate" }); } }) },
      { label: "Rename…", icon: "edit", shortcut: "F2", disabled: readOnly, onClick: () => renameTable(d, t) },
      { label: "Drop…", icon: "trash", danger: true, disabled: readOnly, onClick: () => openPlan({ op: "drop_tables", db: d, objects: [[t, false]] }, { title: `Drop ${t}`, confirmLabel: "Drop table", summary: `The table ${d}.${t} and all its data will be permanently deleted.`, after: () => forgetTables(d, [t]) }) },
      { separator: true },
      { label: "Refresh", icon: "refresh", shortcut: "F5", onClick: () => reloadIndex() },
    ] });
  };

  // ---- views, routines, triggers, events ----
  const KIND_WORD: Record<string, string> = { view: "VIEW", procedure: "PROCEDURE", function: "FUNCTION", trigger: "TRIGGER", event: "EVENT" };
  const KIND_LABEL: Record<string, string> = { view: "view", procedure: "procedure", function: "function", trigger: "trigger", event: "event" };

  /// A script that recreates the object: a DROP followed by the server's own CREATE, with DELIMITER where the body needs it.
  const definitionScript = (kind: Kind, d: string, name: string, ddl: string) => {
    const word = KIND_WORD[kind];
    const qn = `${quote(d)}.${quote(name)}`;
    if (kind === "view") return `${ddl.replace(/^CREATE\b/i, "CREATE OR REPLACE")};\n`;
    return `DELIMITER $$\nDROP ${word} IF EXISTS ${qn}$$\n${ddl}$$\nDELIMITER ;\n`;
  };
  const showDefinition = async (d: string, kind: Kind, name: string) => {
    if (activeId === null) return;
    try {
      const ddl = await api.objectDefinition(activeId, kind, d, name);
      addTab(definitionScript(kind, d, name, ddl), d, name);
    } catch (e) { setStatus(`Error: ${describeError(e)}`); }
  };
  const objectMenu = (e: React.MouseEvent, d: string, kind: Kind, name: string) => {
    if (kind === "table") return tableMenu(e, d, name);
    e.preventDefault();
    if (!KIND_WORD[kind]) return;
    const items: MenuItem[] = [];
    if (kind === "view") items.push({ label: "View Data", icon: "grid", onClick: () => openTable(d, name, undefined, "data") }, { label: "View Properties", icon: "properties", onClick: () => openTable(d, name, undefined, "properties") }, { label: "Read data in SQL console", icon: "terminal", onClick: () => selectInEditor(d, name) }, { label: "Generate SELECT", icon: "sql", onClick: () => sample("select", d, name) }, { separator: true });
    items.push(
      { label: "Show / edit definition", icon: "sql", onClick: () => showDefinition(d, kind, name) },
      ...(kind === "procedure" || kind === "function" ? [{ label: "Rename…", icon: "edit", disabled: readOnly, onClick: () => setPrompt({ title: `Rename ${name}`, label: "New name", initial: name, confirmLabel: "Next", onCancel: () => setPrompt(null),
        onSubmit: async (to: string) => {
          setPrompt(null);
          if (to === name) return;
          try {
            const definition = await api.objectDefinition(activeId!, kind, d, name);
            openPlan({ op: "rename_routine", db: d, kind, from: name, to, definition }, { title: `Rename ${KIND_LABEL[kind]} ${name}`, confirmLabel: "Rename", summary: `Creates ${to} from the definition of ${name}, then drops ${name}. Grants on ${name} are not carried over.` });
          } catch (err) { setStatus(`Error: ${describeError(err)}`); }
        } }) }] : []),
      { separator: true },
      { label: "Drop…", icon: "trash", danger: true, disabled: readOnly, onClick: () => openPlan({ op: "drop_object", db: d, kind, name }, { title: `Drop ${KIND_LABEL[kind]} ${name}`, confirmLabel: `Drop ${KIND_LABEL[kind]}`, summary: `The ${KIND_LABEL[kind]} ${d}.${name} will be permanently deleted.`, after: () => { if (kind === "view") forgetTables(d, [name]); } }) },
      { separator: true },
      { label: "Copy name", icon: "copy", onClick: () => navigator.clipboard.writeText(name) },
      { separator: true },
      { label: "Refresh", icon: "refresh", shortcut: "F5", onClick: () => reloadIndex() },
    );
    setTreeMenu({ x: e.clientX, y: e.clientY, items });
  };
  const TEMPLATES: Record<string, (d: string) => string> = {
    view: (d) => `CREATE VIEW ${quote(d)}.\`new_view\` AS\nSELECT 1 AS id;\n`,
    procedure: (d) => `DELIMITER $$\nCREATE PROCEDURE ${quote(d)}.\`new_procedure\`(IN p_id INT)\nBEGIN\n  SELECT p_id;\nEND$$\nDELIMITER ;\n`,
    function: (d) => `DELIMITER $$\nCREATE FUNCTION ${quote(d)}.\`new_function\`(p_a INT) RETURNS INT\nDETERMINISTIC\nBEGIN\n  RETURN p_a + 1;\nEND$$\nDELIMITER ;\n`,
    trigger: (d) => `DELIMITER $$\nCREATE TRIGGER ${quote(d)}.\`new_trigger\`\nBEFORE INSERT ON ${quote(d)}.\`table_name\` FOR EACH ROW\nBEGIN\n  -- SET NEW.column = ...;\nEND$$\nDELIMITER ;\n`,
    event: (d) => `CREATE EVENT ${quote(d)}.\`new_event\`\nON SCHEDULE EVERY 1 DAY\nDO\n  SELECT 1;\n`,
  };

  const databaseMenu = (e: React.MouseEvent, d: string | null) => {
    e.preventDefault();
    const items: MenuItem[] = [];
    if (d) items.push(
      { label: "Create", icon: "plus", children: createItems(d) },
      { separator: true },
      { label: "Back up this database…", icon: "export", onClick: () => openTransfer({ tab: "backup", db: d }) },
      { label: "Import CSV…", icon: "import", disabled: readOnly, onClick: () => openTransfer({ tab: "import", db: d }) },
      { label: "New query console", icon: "terminal", onClick: () => addTab("", d) },
      { separator: true },
    );
    if (d) items.push({ label: "Structure (all tables)", icon: "table", onClick: () => openStructure(d) });
    if (d) items.push({ label: "Search data…", icon: "search", onClick: () => openSearch(d) });
    if (d) items.push({ label: "Index advisor…", icon: "key", disabled: readOnly, onClick: () => openAdvisor(d) });
    if (d) items.push({ label: "Compare schemas…", icon: "transfer", disabled: readOnly, onClick: () => openCompare(d) });
    items.push({ label: "Databases (list, bulk drop)", icon: "database", onClick: openDatabases });
    items.push({ label: "Create database…", icon: "database", disabled: readOnly, onClick: () => setPrompt({ title: "Create database", label: "Database name", confirmLabel: "Create",
      onCancel: () => setPrompt(null), onSubmit: async (name) => { setPrompt(null); try { await schemaApi.databaseOp(activeId!, { op: "create", name, charset: "utf8mb4", collation: null }); await reloadIndex(); } catch (err) { setStatus(`Error: ${err}`); } } }) });
    if (d) items.push({ label: "Alter database…", icon: "edit", disabled: readOnly, onClick: () => setAlterDb(d) });
    if (d) items.push({ label: "Drop database…", icon: "trash", danger: true, disabled: readOnly, onClick: () => openPlan({ op: "drop_databases", names: [d] }, { title: `Drop database ${d}`, confirmLabel: "Drop database", summary: `The database ${d} with all its tables and data will be permanently deleted.`, after: () => setTabs((prev) => { const rest = prev.filter((t) => t.table?.db !== d || t.kind === "sql"); return rest.length ? rest : [newTab(1)]; }) }) });
    items.push({ separator: true }, { label: "Refresh", icon: "refresh", shortcut: "F5", onClick: () => reloadIndex() });
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
      if (e.key === "F5" && activeId !== null) { e.preventDefault(); reloadIndex(); return; }
      const el = document.activeElement as HTMLElement | null;
      const typing = !!el && (/^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName) || el.isContentEditable || !!el.closest?.(".editor"));
      if (!typing && activeId !== null && selectedDb && selectedTable && (e.key === "F2" || e.key === "F4")) {
        e.preventDefault();
        if (e.key === "F4") openTable(selectedDb, selectedTable, undefined, "properties"); else renameTable(selectedDb, selectedTable);
        return;
      }
      if (e.metaKey && e.key.toLowerCase() === "k") { e.preventDefault(); setPaletteOpen(true); return; }
      if (e.metaKey && e.key === ",") { e.preventDefault(); setPrefsOpen(true); return; }
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
      <header className="topbar" role="toolbar" aria-label="Main toolbar">
        <button title={capTitle("server_tools", "Server home")} disabled={activeId === null || capOff("server_tools")} onClick={openHome}><Icon name="home" /> Home</button>
        <button title="New query tab (⌘T)" disabled={activeId === null} onClick={() => addTab("", selectedDb)}><Icon name="terminal" /> New query</button>
        <button title="List, create and drop databases" disabled={activeId === null} onClick={openDatabases}><Icon name="database" /> Databases</button>
        <span className="sep" />
        <button title={capTitle("dump_restore", "Backup, restore, import and export")} disabled={activeId === null || capOff("dump_restore")} onClick={() => openTransfer()}><Icon name="transfer" /> Transfer</button>
        <button title={capTitle("users", "Users and privileges")} disabled={activeId === null || capOff("users")} onClick={openUsers}><Icon name="user" /> Users</button>
        <button title={capTitle("server_tools", "Server: processes, variables, status, maintenance, logs")} disabled={activeId === null || capOff("server_tools")} onClick={openServer}><Icon name="pulse" /> Server</button>
        <span className="grow" />
        <button className="theme-toggle" title={`Theme: ${theme.pref} (click to change)`}
          onClick={() => { const order: ThemePref[] = ["system", "light", "dark"]; theme.setPref(order[(order.indexOf(theme.pref) + 1) % 3]); }}>
          <Icon name={theme.pref === "system" ? "auto" : theme.pref === "light" ? "sun" : "moon"} />
        </button>
        <button title="Preferences (⌘,)" aria-label="Preferences" onClick={() => setPrefsOpen(true)}><Icon name="settings" /></button>
      </header>
      {openIds.length > 0 && (
        <div className="conn-tabs" role="tablist" aria-label="Open connections">
          {openIds.map((id) => {
            const c = connections.find((x) => x.id === id);
            if (!c) return null;
            return (
              <div key={id} role="tab" aria-selected={id === activeId} className={`conn-tab env-${c.environment ?? "development"} ${id === activeId ? "active" : ""}`}>
                <button className="conn-tab-name" title={`${c.name}: switch to this connection`} onClick={() => id !== activeId && open(id)}><Icon name="server" size={12} /> {c.name}</button>
                <button className="conn-tab-close" title={`Disconnect ${c.name}`} aria-label={`Disconnect ${c.name}`} onClick={() => closeConnection(id)}><Icon name="close" size={10} /></button>
              </div>
            );
          })}
        </div>
      )}
      <div className="app-body">
      <aside className="sidebar" style={{ width: sidebar.size }} onMouseDownCapture={() => { fromEditor.current = !!(document.activeElement as HTMLElement | null)?.closest?.(".editor"); }}>
        <div className="conn-bar">
          <Select value={activeId ?? ""} onChange={(e) => e.target.value && open(Number(e.target.value))}>
            <option value="">Select connection…</option>
            {connections.filter((c) => !c.group?.trim()).map((c) => <option key={c.id} value={c.id!}>{c.name}</option>)}
            {connectionGroups.map((g) => (
              <optgroup key={g} label={g}>{connections.filter((c) => c.group?.trim() === g).map((c) => <option key={c.id} value={c.id!}>{c.name}</option>)}</optgroup>
            ))}
          </Select>
          <button title="New connection" onClick={() => setDialog("new")}><Icon name="plus" /></button>
          <button title="Edit connection" disabled={!active} onClick={() => active && setDialog(active)}><Icon name="edit" /></button>
          <button title="Delete connection" disabled={!active} onClick={() => setConfirmDelete(true)}><Icon name="trash" /></button>
        </div>
        {activeId !== null && (
          <div className={`env-bar env-${env}`} title={`${ENV_LABEL[env]} connection${readOnly ? ", read-only on the server" : ""}${active?.query_timeout_secs ? `, queries stop after ${active.query_timeout_secs} s` : ""}`}>
            <span className={`env-badge env-${env}`}>{ENV_LABEL[env]}</span>
            {readOnly && <span className="ro-badge"><Icon name="lock" size={11} /> Read-only</span>}
            {active?.query_timeout_secs ? <span className="dim small limit" title="Runs longer than this are stopped">{active.query_timeout_secs} s limit</span> : null}
          </div>
        )}
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
              <div className="filterbox" onClick={() => filterInput.current?.focus()}>
                {selectedDb && (
                  <span className="scope" title={lockedTable ? "Locked to this database" : "Locked to this database. The box searches its tables, and the console runs in it. ✕ releases the lock."}>
                    <Icon name="lock" /><span className="scope-name">{selectedDb}</span>
                    {!lockedTable && <button className="x" aria-label="Unlock database" onClick={(e) => { e.stopPropagation(); unlock(); }}><Icon name="close" /></button>}
                  </span>
                )}
                {lockedTable && (
                  <span className="scope" title="Locked to this table. The box searches its columns. ✕ releases the lock.">
                    <Icon name="table" /><span className="scope-name">{lockedTable}</span>
                    <button className="x" aria-label="Unlock table" onClick={(e) => { e.stopPropagation(); unlock(); }}><Icon name="close" /></button>
                  </span>
                )}
                <input
                  ref={filterInput}
                  placeholder={lockedTable ? "Filter columns" : selectedDb ? "Filter tables" : "Filter databases"}
                  title={lockedTable ? "Columns of the locked table: a name or a type, e.g.  email  ·  bigint  ·  char*  ·  pk" : "Filter (⇧⌘B). Click a database to lock it and filter its tables. Use the lock icon on a table to search its columns.\nord  ·  ord*  ·  /regex/  ·  type:view  ·  col:email"}
                  value={query}
                  disabled={activeId === null}
                  onChange={(e) => setQuery(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Escape") { if (query) setQuery(""); else unlock(); }
                    else if (e.key === "Backspace" && !query) unlock();
                  }}
                />
                {query && <button className="clear" aria-label="Clear filter" onClick={() => setQuery("")}><Icon name="close" /></button>}
              </div>
              <div className="popup-host">
                <button className={`icon-btn filter-opts ${isDefaultScope(filterScope) ? "" : "on"}`} disabled={activeId === null} aria-expanded={filterMenu}
                        title="Choose what the filter searches (tables, views, columns…)" onClick={() => setFilterMenu((v) => !v)}><Icon name="filter" /></button>
                {filterMenu && <FilterOptions scope={filterScope} onChange={setFilterScope} onClose={() => setFilterMenu(false)} />}
              </div>
              <button className={`icon-btn ${treeSort === "size" ? "on" : ""}`} disabled={activeId === null} aria-pressed={treeSort === "size"} title={treeSort === "size" ? "Tables sorted by size, largest first (click for name order)" : "Sort tables by size"}
                onClick={() => { const n = treeSort === "size" ? "name" : "size"; localStorage.setItem("mylodb.treeSort", n); setTreeSort(n); }}><Icon name="sort" /></button>
              <button className="refresh icon-btn" disabled={activeId === null} title="Refresh objects and rebuild autocomplete (F5)" onClick={() => reloadIndex()}><Icon name="refresh" /></button>
            </div>
            {filterError && <div className="error small">{filterError}</div>}
            <div className="tree-wrap">
              {activeId === null
                ? <div className="empty">Create or select a connection</div>
                : <ObjectTree key={activeId} sortBy={treeSort} index={index} matches={matches} scope={matchScope} terms={terms} selectedDb={selectedDb} onSelectDb={selectDb} selectedTable={selectedTable} onSelectTable={selectTable} lockedTable={lockedTable} onLockTable={lockTable} onUnlockTable={unlock} onPick={pickName} rootLabel={active ? `${active.user}@${active.host}` : ""} onResetRoot={resetBrowser} columnQuery={lockedTable ? query : ""} stats={meta.stats} ensureStats={meta.ensureStats} details={meta.details} ensureDetails={meta.ensureDetails}
                    columnsLoaded={columnsLoaded} ensureColumns={ensureColumns} onOpenTable={(d, t) => openTable(d, t)} onObjectMenu={objectMenu} onOpenObject={showDefinition} indexes={meta.indexes} initialOpen={browserState.open} onOpenChange={(keys) => saveBrowser({ open: keys })} onDatabaseMenu={databaseMenu} />}
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
      <Splitter axis="x" label="Resize the sidebar" onDrag={sidebar.adjust} onReset={sidebar.reset} />

      <main className="main">
        {activeId === null || !tab ? (
          <div className="placeholder"><h2>MyLoDB</h2><p>Create or select a connection to start.</p></div>
        ) : (
          <>
            <div className="query-tabs" style={{ borderBottomColor: bandColor }}>
              {tabs.map((t) => (
                <div key={t.id} className={`qtab ${t.id === activeTab ? "active" : ""}`} onClick={() => setActiveTab(t.id)}>
                  {runs[t.id]?.running ? <Icon name="clock" size={12} /> : isTable(t) ? <Icon name="table" size={12} /> : t.kind === "designer" ? <Icon name="edit" size={12} /> : t.kind === "users" ? <Icon name="user" size={12} /> : t.kind === "server" ? <Icon name="pulse" size={12} /> : t.kind === "transfer" ? <Icon name="transfer" size={12} /> : null}<span>{t.title}</span>
                  <button className="mini" onClick={(e) => { e.stopPropagation(); closeTab(t.id); }}><Icon name="close" size={10} /></button>
                </div>
              ))}
              <button className="mini plus" title="New query tab (⌘T)" onClick={() => addTab("", tab.database)}><Icon name="plus" size={12} /></button>
            </div>
            {isTable(tab) && tab.table?.name && (
              <div className="subtabs" role="tablist" aria-label="Table views">
                {([["properties", "Properties", "info"], ["data", "Data", "table"], ["diagram", "Diagram", "link"]] as const).map(([v, label, icon]) => (
                  <button key={v} role="tab" aria-selected={(tab.view ?? "data") === v} className={(tab.view ?? "data") === v ? "active" : ""} onClick={() => patchTab(tab.id, { view: v })}>
                    <Icon name={icon} size={13} /> {label}
                  </button>
                ))}
                <span className="grow" />
                <span className="dim crumb">{tab.table.db} › {tab.table.name}</span>
              </div>
            )}
            {isTable(tab) && tab.table?.name && tab.view === "properties" && (
              <TableProperties key={`${activeId}:${tab.id}:p`} connectionId={activeId} db={tab.table.db} table={tab.table.name}
                onOpenTable={(d, t) => openTable(d, t)} onDesign={() => openDesigner(tab.table!.db, tab.table!.name)}
                onOpenInConsole={(sql) => { patchTab(tab.id, { view: "data" }); setTimeout(() => editor.current?.insert(sql + "\n"), 0); }} />
            )}
            {isTable(tab) && tab.table?.name && tab.view === "diagram" && (
              <TableDiagram key={`${activeId}:${tab.id}:d`} connectionId={activeId} db={tab.table.db} table={tab.table.name} onOpenTable={(d, t) => openTable(d, t)} />
            )}
            {/* The editor stays mounted while a table tab is shown so each SQL tab keeps its undo history. */}
            {editorTab && (
              <div className="sql-pane" style={{ display: isSql(tab) && (!isTable(tab) || (tab.view ?? "data") === "data") ? "flex" : "none" }}>
                <div className="toolbar">
                  <button className="primary" disabled={running} onClick={run} title="Run selection or all (⌘↵)">▶ Run</button>
                  <button disabled={running} onClick={previewRun} title="Show exactly what will run, choose foreign key checks, then run"><Icon name="preview" /> Preview</button>
                  <button disabled={!running} onClick={cancel} title="Cancel running query">■ Cancel</button>
                  <Select aria-label="Foreign key checks" title="Foreign key checks while this tab runs" value={fkByTab[tab.id] === undefined || fkByTab[tab.id] === null ? "default" : fkByTab[tab.id] ? "on" : "off"}
                          onChange={(e) => setFkByTab({ ...fkByTab, [tab.id]: e.target.value === "default" ? null : e.target.value === "on" })}>
                    <option value="default">FK checks: server default</option>
                    <option value="on">FK checks: on</option>
                    <option value="off">FK checks: off</option>
                  </Select>
                  <button onClick={openFavDialog} title="Save to favorites"><Icon name="star" /></button>
                  <span className="sep" />
                  <button onClick={formatSql} title="Format the selection or the whole script (⇧⌘F)">Format</button>
                  <div className="popup-host">
                    <button className="mini" title="Formatting options" onClick={() => setFormatOpen(!formatOpen)}><Icon name="index" size={12} /></button>
                    {formatOpen && (
                      <div className="popup left" onMouseLeave={() => setFormatOpen(false)}>
                        <label>Indent
                          <Select value={format.indent.style === "tab" ? "tab" : String((format.indent as { width: number }).width)}
                            onChange={(e) => saveFormat({ ...format, indent: e.target.value === "tab" ? { style: "tab" } : { style: "spaces", width: Number(e.target.value) } })}>
                            <option value="2">2 spaces</option><option value="4">4 spaces</option><option value="8">8 spaces</option><option value="tab">Tab</option>
                          </Select></label>
                        <label>Keywords
                          <Select value={format.keyword_case} onChange={(e) => saveFormat({ ...format, keyword_case: e.target.value as FormatOptions["keyword_case"] })}>
                            <option value="upper">UPPER CASE</option><option value="lower">lower case</option><option value="preserve">As typed</option>
                          </Select></label>
                        <label>Blank lines between statements
                          <Select value={format.lines_between_statements} onChange={(e) => saveFormat({ ...format, lines_between_statements: Number(e.target.value) })}>
                            <option value="0">0</option><option value="1">1</option><option value="2">2</option>
                          </Select></label>
                      </div>
                    )}
                  </div>
                  <span className="sep" />
                  <button onClick={() => analyse("explain")} title="Explain the selected statement, or the one at the cursor">Explain</button>
                  <button onClick={() => analyse("analyze")} disabled={caps?.explain_analyze.available === false} title={caps?.explain_analyze.available === false ? (caps.explain_analyze.reason ?? "Not available on this server") : "EXPLAIN ANALYZE: runs the SELECT and shows real timings"}>Analyze</button>
                  <button onClick={() => analyse("profile")} title="Run the SELECT and show stage timings and the work it caused">Profile</button>
                  {formatError && <span className="error small">{formatError}</span>}
                  <span className="grow" />
                  <label className="dbsel">Database
                    <Select value={editorTab.database ?? ""} onChange={(e) => patchTab(editorTab.id, { database: e.target.value || null })}>
                      <option value="">(none)</option>
                      {databases.map((d) => <option key={d} value={d}>{d}</option>)}
                    </Select>
                  </label>
                </div>
                <div className="editor-wrap" style={{ height: consoleSize.size }}>
                  <Editor ref={editor} tabId={editorTab.id} initialDoc={editorTab.sql} onChange={(sql) => patchTab(editorTab.id, { sql })} onRun={run} onFormat={formatSql} connectionId={activeId} database={editorTab.database} dark={theme.resolved === "dark"} scheme={prefsOpen ? livePrefs.editorScheme : prefs.editorScheme} />
                </div>
                <Splitter axis="y" label="Resize the console" onDrag={consoleSize.adjust} onReset={consoleSize.reset} />
                <ResultsPane
                  key={editorTab.id}
                  run={runs[editorTab.id]}
                  analysis={analyses[editorTab.id]}
                  onProfile={() => analyse("profile")}
                  info={{ connectionId: activeId, who: active ? `${active.user}@${active.host}` : "", environment: env, readOnly, database: editorTab.database, selection: { db: selectedDb, table: selectedTable, index, stats: meta.stats },
                          ...(isTable(editorTab) && editorTab.table?.name ? { table: { db: editorTab.table.db, name: editorTab.table.name }, onOpenProperties: () => patchTab(editorTab.id, { view: "properties" }) } : {}) }}
                  history={<HistoryPanel connectionId={activeId} refreshKey={panelKey} onInsert={insertIntoEditor} onOpen={(sql) => addTab(sql, editorTab.database)} />}
                  data={isTable(editorTab) && editorTab.id === tab.id && (tab.view ?? "data") === "data" && editorTab.table?.name ? (
                    <DataGrid key={`${activeId}:${editorTab.id}`} connectionId={activeId} db={editorTab.table.db} table={editorTab.table.name}
                      initial={editorTab.grid ?? defaultView()} onViewChange={(grid) => patchTab(editorTab.id, { grid })}
                      readOnlyConnection={readOnly} confirmCommit={env === "production" ? "Production database:" : undefined}
                      onOpenTable={(d, t, f) => openTable(d, t, f)} />
                  ) : undefined}
                />
              </div>
            )}
            {tab.kind === "designer" && tab.table && (
              <TableDesigner key={`${activeId}:${tab.id}`} connectionId={activeId} db={tab.table.db} table={tab.table.name} initial={tab.initialTable} nodes={index} ensureColumns={ensureColumns}
                onOpenData={(d, t) => openTable(d, t)}
                readOnly={readOnly} onApplied={async (name) => { await reloadIndex(); patchTab(tab.id, { title: name, table: { db: tab.table!.db, name } }); }} />
            )}
            {tab.kind === "transfer" && <TransferTools key={`${activeId}:${tab.id}`} connectionId={activeId} nodes={index} ensureColumns={ensureColumns} preset={tab.transfer ?? {}} onSchemaChanged={() => reloadIndex()} />}
            {tab.kind === "home" && (
              <HomePage key={`${activeId}:${tab.id}`} connectionId={activeId} name={connections.find((c) => c.id === activeId)?.name ?? "Server"} readOnly={readOnly} environment={env}
                actions={[
                  { label: "Databases", icon: "database", run: openDatabases, hint: "List, create and drop databases" },
                  { label: "New query", icon: "terminal", run: () => addTab("", selectedDb) },
                  { label: "Server tools", icon: "pulse", run: openServer, hint: "Processes, variables, status, maintenance, logs" },
                  { label: "Engines, charsets, replication", icon: "info", run: openCatalog },
                  { label: "Users and privileges", icon: "user", run: openUsers },
                  { label: "Export", icon: "export", run: () => openTransfer({ tab: "export" }) },
                  { label: "Import", icon: "import", run: () => openTransfer({ tab: "import" }) },
                ]} />
            )}
            {tab.kind === "databases" && (
              <DatabasesPage key={`${activeId}:${tab.id}`} connectionId={activeId} readOnly={readOnly} refreshKey={dbRefresh}
                onOpenDatabase={(d) => { setSelectedDb(d); openStructure(d); }}
                onCreate={(name, collation) => openPlan({ op: "create_database", name, collation }, { title: `Create database ${name}`, confirmLabel: "Create", after: () => setDbRefresh((n) => n + 1) })}
                onDrop={(names, summary) => openPlan({ op: "drop_databases", names }, { title: `Drop ${names.length} database${names.length === 1 ? "" : "s"}`, confirmLabel: "Drop", summary,
                  after: () => { setDbRefresh((n) => n + 1); setTabs((prev) => { const rest = prev.filter((t) => !(t.table && names.includes(t.table.db))); return rest.length ? rest : [newTab(1)]; }); } })} />
            )}
            {tab.kind === "search" && tab.table && (
              <DataSearchPage key={`${activeId}:${tab.id}`} connectionId={activeId} database={tab.table.db} databases={databases}
                onOpen={(db, table, column, op, text) => openTable(db, table, { [column]: { op, text, text2: "" } })} />
            )}
            {tab.kind === "advisor" && tab.table && (
              <IndexAdvisorPage key={`${activeId}:${tab.id}`} connectionId={activeId} database={tab.table.db}
                onApply={(statements, summary) => setPlanDlg({ title: `Index changes: ${tab.table!.db}`, summary, statements, confirmLabel: "Apply changes", after: () => setDbRefresh((n) => n + 1) })} />
            )}
            {tab.kind === "compare" && tab.table && (
              <SchemaComparePage key={`${activeId}:${tab.id}`} connectionId={activeId} targetDb={tab.table.db} databases={databases}
                connections={openIds.map((i) => ({ id: i, name: connections.find((c) => c.id === i)?.name ?? `#${i}` }))}
                onApply={(statements, summary) => setPlanDlg({ title: `Compare schemas: ${tab.table!.db}`, summary, statements, confirmLabel: "Apply changes", after: () => setDbRefresh((n) => n + 1) })} />
            )}
            {tab.kind === "catalog" && <CatalogPage key={`${activeId}:${tab.id}`} connectionId={activeId} />}
            {tab.kind === "structure" && tab.table && (
              <StructurePage key={`${activeId}:${tab.id}`} connectionId={activeId} db={tab.table.db} readOnly={readOnly} refreshKey={dbRefresh}
                onBrowse={(t) => openTable(tab.table!.db, t)} onDesign={(t) => openDesigner(tab.table!.db, t)}
                onSearch={(t) => openTable(tab.table!.db, t)}
                onNewTable={(name, columns) => openNewTable(tab.table!.db, name, columns)}
                onExport={(tables) => openTransfer({ tab: "export", db: tab.table!.db, table: tables[0] })}
                onDatabaseGone={(d) => setTabs((prev) => { const rest = prev.filter((t) => t.table?.db !== d || t.kind === "sql"); return rest.length ? rest : [newTab(1)]; })}
                onPlan={(op, o) => openPlan(op, o)} />
            )}
            {tab.kind === "server" && <ServerTools key={`${activeId}:${tab.id}`} connectionId={activeId} nodes={index} />}
            {tab.kind === "users" && <UsersManager key={`${activeId}:${tab.id}`} connectionId={activeId} nodes={index} ensureColumns={ensureColumns} />}
          </>
        )}
      </main>
      </div>
      <footer className={`statusbar env-${env}`} style={{ borderTopColor: bandColor }}>
        <nav className="crumbs" aria-label="Location">
          {active ? (
            <>
              <button className="crumb" title="Server home" onClick={openHome}><Icon name="server" size={12} /> {active.name}</button>
              {selectedDb && <><span className="crumb-sep">›</span><button className="crumb" title="Tables of this database" onClick={() => openStructure(selectedDb)}><Icon name="database" size={12} /> {selectedDb}</button></>}
              {selectedDb && selectedTable && <><span className="crumb-sep">›</span><button className="crumb" title="Open this table" onClick={() => openTable(selectedDb, selectedTable)}><Icon name="table" size={12} /> {selectedTable}</button></>}
            </>
          ) : <span className="dim">No connection</span>}
        </nav>
        <span className="status-text">{status}</span>
        <TasksPanel />
        {activeId !== null && env !== "development" && <span className={`env-chip env-${env}`}>{ENV_LABEL[env]}{readOnly ? " · read-only" : ""}</span>}
      </footer>

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
                <h3><Icon name="warn" /> SSH host key changed</h3>
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
      {pendingReview && <ReviewDialog review={pendingReview.review} onCancel={() => setPendingReview(null)} onRun={pendingReview.go} />}
      {aboutOpen && <AboutDialog onClose={() => setAboutOpen(false)} onManual={() => { setAboutOpen(false); setManualOpen(true); }} />}
      {manualOpen && <Manual onClose={() => setManualOpen(false)} onTour={() => { setManualOpen(false); setTourOpen(true); }} />}
      {tourOpen && <Tour onClose={() => setTourOpen(false)} />}
      {prompt && <PromptDialog {...prompt} />}
      {alterDb && activeId !== null && (
        <AlterDatabaseDialog connectionId={activeId} database={alterDb} onCancel={() => setAlterDb(null)}
          onNext={(collation) => { const name = alterDb; setAlterDb(null); openPlan({ op: "alter_database", name, charset: null, collation }, { title: `Alter database ${name}`, confirmLabel: "Alter", summary: "Applies to tables created later. Existing tables keep their own collation.", after: () => setDbRefresh((n) => n + 1) }); }} />
      )}
      {paletteOpen && (
        <CommandPalette onClose={() => setPaletteOpen(false)} commands={[
          ...([["new_query", "New query tab"], ["home", "Server home"], ["databases", "Databases"], ["search_data", "Search data in a database"], ["compare", "Compare schemas of the selected database"], ["structure", "Tables of the selected database"], ["new_table", "New table"], ["export", "Export data"], ["import", "Import data"],
          ...([["new_query", "New query tab"], ["home", "Server home"], ["databases", "Databases"], ["advisor", "Index advisor for the selected database"], ["structure", "Tables of the selected database"], ["new_table", "New table"], ["export", "Export data"], ["import", "Import data"],
              ["backup", "Backup / restore"], ["users", "Users and privileges"], ["server_tools", "Server tools"], ["catalog", "Engines, charsets, replication"], ["refresh_tree", "Refresh object tree"],
              ["theme_dark", "Theme: dark"], ["theme_light", "Theme: light"], ["theme_system", "Theme: system"], ["preferences", "Preferences"], ["manual", "User manual"], ["tour", "Quick tour"], ["about", "About MyLoDB"], ["new_connection", "New connection"], ["edit_connection", "Edit connection"]] as const)
            .map(([id, label]): Command => ({ id, label, hint: "action", run: () => menuHandler.current(id) })),
          { id: "snippet-save", label: "Save editor text as my snippet…", hint: "selection, or the whole editor", run: () => {
            const text = editor.current?.runnableText().trim();
            if (!text) return setStatus("Nothing to save: the editor is empty");
            setPrompt({ title: "Save as snippet", label: "Snippet name", confirmLabel: "Save", onCancel: () => setPrompt(null),
              onSubmit: (name) => { setPrompt(null); setMySnippets((l) => saveMySnippet(l, name, text)); setStatus(`Snippet saved: ${name}`); } });
          } },
          ...mySnippets.flatMap((sn): Command[] => [
            { id: `ms:${sn.name}`, label: `My snippet: ${sn.name}`, hint: "insert into the editor", run: () => {
              if (activeId === null) return setStatus("Connect first");
              const t = editorTab; if (!t) return;
              setActiveTab(t.id); setTimeout(() => editor.current?.insert(sn.sql), 0);
            } },
            { id: `msd:${sn.name}`, label: `Delete my snippet: ${sn.name}`, hint: "removes it from the library", run: () => { setMySnippets((l) => deleteMySnippet(l, sn.name)); setStatus(`Snippet deleted: ${sn.name}`); } },
          ]),
          ...SNIPPETS.map((sn): Command => ({ id: `s:${sn.label}`, label: `Snippet: ${sn.label}`, hint: "insert into the editor", run: () => {
            if (activeId === null) return setStatus("Connect first");
            const t = editorTab; if (!t) return;
            setActiveTab(t.id); setTimeout(() => editor.current?.insert(sn.sql), 0);
          } })),
          ...index.filter((n) => n.kind === "table" || n.kind === "view").slice(0, 5000)
            .map((n): Command => ({ id: `t:${n.database}.${n.name}`, label: n.name, hint: `${n.kind} in ${n.database}`, run: () => openTable(n.database, n.name) })),
        ]} />
      )}
      {prefsOpen && <PreferencesDialog initial={prefs} theme={theme.pref} onTheme={theme.setPref} onChange={(p) => { setPrefs(p); setLivePrefs(p); }} onLive={setLivePrefs} onClose={() => { setLivePrefs(prefs); setPrefsOpen(false); }} />}
      {typeConfirm && <TypeConfirmDialog {...typeConfirm} />}
      {scriptPreview && activeId !== null && (
        <PreviewDialog title="Preview SQL" summary={scriptPreview.database ? `Runs in database ${scriptPreview.database}.` : undefined} statements={[scriptPreview.sql]} environment={env} confirmLabel="Run"
          onCancel={() => setScriptPreview(null)} onRun={(st) => queryApi.run(activeId, uid(), scriptPreview.database, st.join("\n"), null)}
          onDone={() => { reloadIndex(); setStatus("Preview run: done"); }} />
      )}
      {planDlg && activeId !== null && (
        <PreviewDialog title={planDlg.title} summary={planDlg.summary} statements={planDlg.statements} environment={env} confirmLabel={planDlg.confirmLabel}
          onCancel={() => setPlanDlg(null)} onRun={(st) => planApi.run(activeId, st)}
          onDone={() => { reloadIndex(); setDbRefresh((n) => n + 1); planDlg.after?.(); setStatus(`${planDlg.title}: done`); }}
          onOpenConsole={(sql) => addTab(sql, selectedDb)} />
      )}
      {treeMenu && <ContextMenu x={treeMenu.x} y={treeMenu.y} items={treeMenu.items} onClose={() => setTreeMenu(null)} />}
      {dialog && (
        <ConnectionDialog
          initial={dialog === "new" ? undefined : dialog}
          groups={connectionGroups}
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
