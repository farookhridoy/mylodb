import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, type Node, type SavedConnection } from "./api";
import ConnectionDialog from "./ConnectionDialog";
import ObjectTree from "./ObjectTree";

export default function App() {
  const [connections, setConnections] = useState<SavedConnection[]>([]);
  const [activeId, setActiveId] = useState<number | null>(null);
  const [index, setIndex] = useState<Node[]>([]);
  const [status, setStatus] = useState<string>("Not connected");
  const [query, setQuery] = useState("");
  const [matches, setMatches] = useState<Set<number> | null>(null);
  const [filterError, setFilterError] = useState<string | null>(null);
  const [dialog, setDialog] = useState<SavedConnection | "new" | null>(null);
  const seq = useRef(0);

  const refresh = useCallback(async () => setConnections(await api.listConnections()), []);
  useEffect(() => { refresh(); }, [refresh]);

  const active = connections.find((c) => c.id === activeId);

  const open = async (id: number) => {
    setStatus("Connecting…");
    try {
      if (activeId !== null && activeId !== id) await api.disconnect(activeId);
      const idx = await api.connect(id);
      setActiveId(id);
      setIndex(idx);
      setQuery("");
      setStatus(`Connected · ${idx.filter((n) => n.kind === "database").length} databases`);
    } catch (e) {
      setActiveId(null);
      setIndex([]);
      setStatus(`Error: ${e}`);
    }
  };

  // Debounced filter, evaluated in Rust. Stale responses are dropped via the sequence number.
  useEffect(() => {
    if (activeId === null) return;
    if (!query.trim()) { setMatches(null); setFilterError(null); return; }
    const mine = ++seq.current;
    const t = setTimeout(async () => {
      try {
        const hits = await api.filterIndex(activeId, query);
        if (mine !== seq.current) return;
        // A match keeps its database and (for children) shows under it.
        setMatches(new Set(hits));
        setFilterError(null);
      } catch (e) {
        if (mine === seq.current) setFilterError(String(e));
      }
    }, 80);
    return () => clearTimeout(t);
  }, [query, activeId]);

  const terms = useMemo(
    () => query.split(/\s+/).filter((t) => t && !/[:*?/]/.test(t)).map((t) => t.toLowerCase()),
    [query],
  );

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
          <button
            title="Delete connection"
            disabled={!active}
            onClick={async () => {
              if (active?.id && confirm(`Delete connection "${active.name}"?`)) {
                await api.deleteConnection(active.id);
                setActiveId(null); setIndex([]); setStatus("Not connected");
                refresh();
              }
            }}
          >🗑</button>
        </div>
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
            : <ObjectTree index={index} matches={matches} terms={terms} />}
        </div>
      </aside>
      <main className="main">
        <div className="placeholder">
          <h2>MyLoDB</h2>
          <p>Query tabs, data grid and designers arrive in the next phases.</p>
        </div>
        <div className="statusbar" style={{ borderTopColor: active?.color }}>{status}</div>
      </main>
      {dialog && (
        <ConnectionDialog
          initial={dialog === "new" ? undefined : dialog}
          onClose={() => setDialog(null)}
          onSaved={async (id) => { setDialog(null); await refresh(); open(id); }}
        />
      )}
    </div>
  );
}
