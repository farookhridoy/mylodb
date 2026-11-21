import { useCallback, useEffect, useState } from "react";
import Icon from "./Icon";
import { queryApi, type Favorite, type HistoryEntry } from "./api";

export function HistoryPanel(props: { connectionId: number; refreshKey: number; onInsert(sql: string): void; onOpen(sql: string): void }) {
  const [search, setSearch] = useState("");
  const [rows, setRows] = useState<HistoryEntry[]>([]);
  useEffect(() => {
    let live = true;
    queryApi.historyList(props.connectionId, search).then((r) => live && setRows(r));
    return () => { live = false; };
  }, [props.connectionId, search, props.refreshKey]);

  return (
    <div className="panel">
      <div className="panel-bar">
        <input placeholder="Search history…" value={search} onChange={(e) => setSearch(e.target.value)} />
        <button title="Clear history" onClick={async () => { await queryApi.historyClear(props.connectionId); setRows([]); }}><Icon name="trash" /></button>
      </div>
      <ul className="list">
        {rows.map((h) => (
          <li key={h.id} className={h.ok ? "" : "bad"} title="Click: insert · Double-click: open in new tab"
              onClick={() => props.onInsert(h.sql)} onDoubleClick={() => props.onOpen(h.sql)}>
            <code>{h.sql.length > 200 ? h.sql.slice(0, 200) + "…" : h.sql}</code>
            <span className="dim">{h.ran_at} · {h.duration_ms} ms</span>
          </li>
        ))}
        {rows.length === 0 && <div className="empty">No history yet</div>}
      </ul>
    </div>
  );
}

export function FavoritesPanel(props: { refreshKey: number; onInsert(sql: string): void; onOpen(sql: string): void }) {
  const [favs, setFavs] = useState<Favorite[]>([]);
  const load = useCallback(() => queryApi.favoritesList().then(setFavs), []);
  useEffect(() => { load(); }, [load, props.refreshKey]);
  const folders = [...new Set(favs.map((f) => f.folder))];

  return (
    <div className="panel">
      <ul className="list">
        {folders.map((folder) => (
          <li key={folder} className="folder">
            <div className="dim"><Icon name="folder" size={12} /> {folder || "Unfiled"}</div>
            <ul>
              {favs.filter((f) => f.folder === folder).map((f) => (
                <li key={f.id} title={f.sql} onClick={() => props.onInsert(f.sql)} onDoubleClick={() => props.onOpen(f.sql)}>
                  <Icon name="star" size={12} className="ico-fav" /> {f.name}
                  <button className="mini" title="Delete" onClick={async (e) => { e.stopPropagation(); await queryApi.favoriteDelete(f.id!); load(); }}><Icon name="close" size={10} /></button>
                </li>
              ))}
            </ul>
          </li>
        ))}
        {favs.length === 0 && <div className="empty">Save a query with the star in the toolbar</div>}
      </ul>
    </div>
  );
}

export function SaveFavoriteDialog(props: { sql: string; folders: string[]; onClose(): void; onSaved(): void }) {
  const [name, setName] = useState("");
  const [folder, setFolder] = useState("");
  const [error, setError] = useState<string | null>(null);
  return (
    <div className="overlay">
      <div className="dialog">
        <h3>Save to favorites</h3>
        <label>Name<input autoFocus value={name} onChange={(e) => setName(e.target.value)} /></label>
        <label>Folder
          <input list="fav-folders" value={folder} onChange={(e) => setFolder(e.target.value)} placeholder="(none)" />
          <datalist id="fav-folders">{props.folders.map((f) => <option key={f} value={f} />)}</datalist>
        </label>
        <code className="preview">{props.sql.slice(0, 300)}</code>
        {error && <div className="error">{error}</div>}
        <div className="actions">
          <button onClick={props.onClose}>Cancel</button>
          <button className="primary" disabled={!name.trim()} onClick={async () => {
            try { await queryApi.favoriteSave({ id: null, folder: folder.trim(), name: name.trim(), sql: props.sql }); props.onSaved(); }
            catch (e) { setError(String(e)); }
          }}>Save</button>
        </div>
      </div>
    </div>
  );
}

export function ConfirmDialog(props: { message: string; confirmLabel: string; onCancel(): void; onConfirm(): void }) {
  return (
    <div className="overlay">
      <div className="dialog">
        <p>{props.message}</p>
        <div className="actions">
          <button onClick={props.onCancel}>Cancel</button>
          <button className="primary" onClick={props.onConfirm}>{props.confirmLabel}</button>
        </div>
      </div>
    </div>
  );
}
