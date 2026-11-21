import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Select from "./Select";
import Icon from "./Icon";
import { open as pickPath, save as savePath } from "@tauri-apps/plugin-dialog";
import {
  EXPORT_FORMATS, describeError, onJobProgress, transferApi, type SourcePreview, type BackupOptions, type BackupResult, type CsvFormat, type CsvPreview, type ExportFormat, type ImportMode,
  type ImportResult, type InferredColumn, type JobProgress, type Node, type RunResult,
} from "./api";
import { ConfirmDialog } from "./SidePanels";
import { formatBytes } from "./browserMeta";
import { newJobId, ProgressBar, useJob } from "./Jobs";

export type TransferTab = "backup" | "restore" | "import" | "export";
export interface TransferPreset { tab?: TransferTab; db?: string; table?: string }

const fmtBytes = (n: number) => (n >= 1 << 30 ? `${(n / (1 << 30)).toFixed(2)} GB` : n >= 1 << 20 ? `${(n / (1 << 20)).toFixed(1)} MB` : n >= 1024 ? `${(n / 1024).toFixed(1)} KB` : `${n} B`);
const fmtMs = (ms: number) => (ms >= 60000 ? `${(ms / 60000).toFixed(1)} min` : ms >= 1000 ? `${(ms / 1000).toFixed(1)} s` : `${ms} ms`);

export default function TransferTools(props: { connectionId: number; nodes: Node[]; preset: TransferPreset; onSchemaChanged(): void; ensureColumns(db: string): Promise<unknown> }) {
  const [tab, setTab] = useState<TransferTab>(props.preset.tab ?? "backup");
  useEffect(() => { if (props.preset.tab) setTab(props.preset.tab); }, [props.preset]);
  const names: [TransferTab, string][] = [["backup", "Backup"], ["restore", "Restore / run script"], ["import", "Import CSV"], ["export", "Export query"]];
  return (
    <div className="transfer">
      <div className="dz-tabs">{names.map(([t, l]) => <button key={t} className={tab === t ? "active" : ""} onClick={() => setTab(t)}>{l}</button>)}</div>
      <div className="st-body">
        {tab === "backup" && <Backup {...props} />}
        {tab === "restore" && <Restore {...props} />}
        {tab === "import" && <ImportCsv {...props} />}
        {tab === "export" && <ExportQuery {...props} />}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- backup

function Backup({ connectionId, nodes, preset }: { connectionId: number; nodes: Node[]; preset: TransferPreset }) {
  const dbs = useMemo(() => nodes.filter((n) => n.kind === "database").map((n) => n.name), [nodes]);
  const [picked, setPicked] = useState<Set<string>>(new Set(preset.db ? [preset.db] : []));
  const [only, setOnly] = useState<Record<string, Set<string>>>({});   // per database: selected tables (absent = all)
  const [expanded, setExpanded] = useState<string | null>(preset.db ?? null);
  const [o, setO] = useState({ structure: true, data: true, drop_if_exists: true, create_database: true, views: true, routines: true, triggers: true, events: true, snapshot: true, disable_fk_checks: true, strip_definer: true, file_per_table: false, compress: false, rows_per_insert: 100 });
  const [dir, setDir] = useState("");
  const [name, setName] = useState("backup");
  const job = useJob<BackupResult>();
  const tablesOf = (d: string) => nodes.filter((n) => n.database === d && (n.kind === "table" || n.kind === "view")).map((n) => n.name);
  const flag = (k: keyof typeof o, label: string, title?: string) => (
    <label className="check" title={title}><input type="checkbox" checked={o[k] as boolean} onChange={(e) => setO({ ...o, [k]: e.target.checked })} /> {label}</label>
  );

  useEffect(() => {
    if (preset.db) { setPicked(new Set([preset.db])); setName(preset.db); setExpanded(preset.db); }
  }, [preset]);

  const start = () => {
    const options: BackupOptions = {
      ...o,
      databases: [...picked].map((n) => ({ name: n, tables: only[n] ? [...only[n]] : null })),
    };
    job.run((id) => transferApi.backup(connectionId, id, options, dir, name || "backup"), `Backup ${name || "backup"}`);
  };
  const chooseDir = async () => { const p = await pickPath({ directory: true, multiple: false }); if (typeof p === "string") setDir(p); };
  const r = job.result;

  return (
    <div className="xfer-form">
      <div className="xfer-cols">
        <section>
          <h4>Databases</h4>
          <div className="db-pick">
            {dbs.map((d) => (
              <div key={d}>
                <label className="check">
                  <input type="checkbox" checked={picked.has(d)} onChange={(e) => { const n = new Set(picked); e.target.checked ? n.add(d) : n.delete(d); setPicked(n); if (e.target.checked && picked.size === 0) setName(d); }} /> {d}
                </label>
                {picked.has(d) && <button className="mini" onClick={() => setExpanded(expanded === d ? null : d)}>{only[d] ? `${only[d].size} table(s)` : "all tables"} ▾</button>}
                {expanded === d && picked.has(d) && (
                  <div className="tbl-sub">
                    <label className="check"><input type="checkbox" checked={!only[d]} onChange={(e) => { const m = { ...only }; if (e.target.checked) delete m[d]; else m[d] = new Set(tablesOf(d)); setOnly(m); }} /> <b>All tables and views</b></label>
                    {only[d] && tablesOf(d).map((t) => (
                      <label key={t} className="check"><input type="checkbox" checked={only[d].has(t)} onChange={(e) => { const s = new Set(only[d]); e.target.checked ? s.add(t) : s.delete(t); setOnly({ ...only, [d]: s }); }} /> {t}</label>
                    ))}
                  </div>
                )}
              </div>
            ))}
          </div>
        </section>
        <section>
          <h4>Contents</h4>
          {flag("structure", "Table structure")}
          {flag("data", "Table data")}
          {flag("views", "Views")}{flag("routines", "Stored procedures and functions")}{flag("triggers", "Triggers")}{flag("events", "Events")}
          <h4>Options</h4>
          {flag("drop_if_exists", "Add DROP … IF EXISTS before each object", "Restoring then replaces existing objects")}
          {flag("create_database", "Add CREATE DATABASE and USE", "Needed to restore into the same database name; turn off to restore into any database")}
          {flag("snapshot", "Consistent snapshot", "All rows are read at one point in time while the database is in use (InnoDB)")}
          {flag("disable_fk_checks", "Disable foreign key checks while restoring")}
          {flag("strip_definer", "Remove DEFINER clauses", "Views, routines and triggers restore under the restoring user")}
          <label>Rows per INSERT statement<input className="cell-input narrow" type="number" min={1} value={o.rows_per_insert} onChange={(e) => setO({ ...o, rows_per_insert: Math.max(1, Number(e.target.value)) })} /></label>
        </section>
        <section>
          <h4>Output</h4>
          {flag("file_per_table", "One file per table")}
          {flag("compress", "Compress into a .zip")}
          <label>Folder<div className="path-field"><input value={dir} placeholder="Choose a folder…" onChange={(e) => setDir(e.target.value)} /><button onClick={chooseDir}>Browse…</button></div></label>
          <label>File name<input value={name} onChange={(e) => setName(e.target.value)} /></label>
          <div className="dim small">A timestamp is added to the name, so a backup never overwrites an earlier one.</div>
          <button className="primary" disabled={job.running || picked.size === 0 || !dir || (!o.structure && !o.data)} onClick={start}>Start backup</button>
        </section>
      </div>
      <ProgressBar p={job.progress} running={job.running} onCancel={job.cancel} />
      {job.cancelled && <div className="banner">Backup cancelled; nothing was written.</div>}
      {job.error && <div className="banner bad">{job.error}</div>}
      {r && (
        <div className="banner ok result-card">
          <b>Backup complete</b> in {fmtMs(r.elapsed_ms)}: {r.databases} database(s), {r.tables} table(s), {r.rows.toLocaleString()} row(s), {fmtBytes(r.bytes)}, {r.files} file(s).
          <div><code>{r.path}</code> <button className="mini" onClick={() => navigator.clipboard.writeText(r.path)}>Copy path</button></div>
          {r.warnings.map((w) => <div key={w} className="dim"><Icon name="warn" size={12} /> {w}</div>)}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------- restore

function Restore({ connectionId, nodes, onSchemaChanged }: { connectionId: number; nodes: Node[]; onSchemaChanged(): void }) {
  const dbs = useMemo(() => nodes.filter((n) => n.kind === "database").map((n) => n.name), [nodes]);
  const [path, setPath] = useState("");
  const [db, setDb] = useState("");
  const [stop, setStop] = useState(false);
  const [fk, setFk] = useState(true);
  const [confirm, setConfirm] = useState(false);
  const job = useJob<RunResult>();
  const [preview, setPreview] = useState<SourcePreview | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const choose = async (directory: boolean) => {
    const p = await pickPath(directory ? { directory: true, multiple: false } : { multiple: false, filters: [{ name: "SQL or zip", extensions: ["sql", "zip"] }] });
    if (typeof p === "string") { setPath(p); setPreview(null); }
  };
  const showPreview = async () => {
    setPreviewing(true); setPreviewError(null);
    try { setPreview(await transferApi.previewSource(path)); } catch (e) { setPreview(null); setPreviewError(describeError(e)); }
    finally { setPreviewing(false); }
  };
  const go = () => {
    setConfirm(false);
    job.run(async (id) => { try { return await transferApi.runSource(connectionId, id, path, { stop_on_error: stop, disable_fk_checks: fk, database: db || null }); } finally { onSchemaChanged(); } }, `Run script ${path.split(/[\\/]/).pop()}`);
  };
  const r = job.result;
  return (
    <div className="xfer-form">
      <div className="xfer-cols">
        <section>
          <h4>Source</h4>
          <label>.sql file, .zip archive or folder of .sql files
            <div className="path-field"><input value={path} placeholder="Choose…" onChange={(e) => setPath(e.target.value)} />
              <button onClick={() => choose(false)}>File…</button><button onClick={() => choose(true)}>Folder…</button></div></label>
          <label>Run in database
            <Select value={db} onChange={(e) => setDb(e.target.value)}><option value="">(as written in the script)</option>{dbs.map((d) => <option key={d}>{d}</option>)}</Select></label>
          <label className="check"><input type="checkbox" checked={stop} onChange={(e) => setStop(e.target.checked)} /> Stop at the first error</label>
          <label className="check"><input type="checkbox" checked={fk} onChange={(e) => setFk(e.target.checked)} /> Disable foreign key checks while running</label>
          <div className="row-gap">
            <button disabled={!path || previewing} onClick={showPreview} title="Count and read the statements without running anything">Preview script</button>
            <button className="danger" disabled={!path || job.running} onClick={() => setConfirm(true)}>Run script…</button>
          </div>
          {previewError && <div className="banner bad">{previewError}</div>}
          {preview && (
            <div className="src-preview" aria-label="Script preview">
              <div><b>{preview.statements.toLocaleString()}</b> statements in {preview.files} file{preview.files === 1 ? "" : "s"} ({formatBytes(preview.bytes)})
                {preview.destructive > 0 && <span className="bad-text"> · {preview.destructive.toLocaleString()} drop, truncate or delete</span>}</div>
              <div className="dim small">{preview.kinds.map(([k, n]) => `${k} ${n.toLocaleString()}`).join(" · ")}</div>
              {preview.errors.map((e, i) => <div key={i} className="error small">{e}</div>)}
              <pre className="preview-sql">{preview.first.map((f) => `-- ${f.file}:${f.line}\n${f.text}`).join("\n\n")}</pre>
              <div className="dim small">{preview.statements > preview.first.length ? `The first ${preview.first.length} statements are shown. ` : ""}Foreign key checks will be {fk ? "off" : "on"} while it runs.</div>
            </div>
          )}
          <div className="dim small">The script is run statement by statement and may drop and recreate tables. It is streamed from disk, so very large files are fine.</div>
        </section>
      </div>
      <ProgressBar p={job.progress} running={job.running} onCancel={job.cancel} />
      {job.cancelled && <div className="banner">Cancelled. Statements that already ran were not undone.</div>}
      {job.error && <div className="banner bad">{job.error}</div>}
      {r && (
        <div className={`banner result-card ${r.failed ? "bad" : "ok"}`}>
          <b>{r.aborted ? "Stopped at an error" : r.failed ? "Finished with errors" : "Finished"}</b>: {r.statements.toLocaleString()} statement(s) from {r.files} file(s) in {fmtMs(r.elapsed_ms)}, {r.failed} failed.
        </div>
      )}
      {r && r.errors.length > 0 && (
        <div className="dg-scroll err-table">
          <table className="grid"><thead><tr><th>File</th><th>Line</th><th>Statement</th><th>Error</th></tr></thead>
            <tbody>{r.errors.map((e, i) => <tr key={i}><td>{e.file}</td><td className="num">{e.line}</td><td className="info"><code>{e.statement}</code></td><td>{e.message}</td></tr>)}</tbody></table>
          {r.failed > r.errors.length && <div className="dim small">Showing the first {r.errors.length} of {r.failed} errors.</div>}
        </div>
      )}
      {confirm && <ConfirmDialog message={`Run ${path.split("/").pop()} now? It can change or replace existing tables and data${db ? ` in ${db}` : ""}. Statements that run cannot be undone.`} confirmLabel="Run script" onCancel={() => setConfirm(false)} onConfirm={go} />}
    </div>
  );
}

// ---------------------------------------------------------------- import CSV

const ENCODINGS = ["utf-8", "windows-1252", "iso-8859-1", "iso-8859-15", "utf-16le", "utf-16be", "shift_jis", "gbk", "big5", "euc-kr", "windows-1251", "koi8-r"];
const DELIMS: [string, string][] = [[",", "Comma ,"], [";", "Semicolon ;"], ["\t", "Tab"], ["|", "Pipe |"]];

function ImportCsv({ connectionId, nodes, preset, onSchemaChanged, ensureColumns }: { connectionId: number; nodes: Node[]; preset: TransferPreset; onSchemaChanged(): void; ensureColumns(db: string): Promise<unknown> }) {
  const dbs = useMemo(() => nodes.filter((n) => n.kind === "database").map((n) => n.name), [nodes]);
  const [path, setPath] = useState("");
  const [fmt, setFmt] = useState<CsvFormat>({ delimiter: ",", quote: '"', has_header: true, encoding: "utf-8", null_text: null, trim: false });
  const [preview, setPreview] = useState<CsvPreview | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [db, setDb] = useState(preset.db ?? dbs[0] ?? "");
  const [table, setTable] = useState(preset.table ?? "");
  const [newTable, setNewTable] = useState(false);
  const [newName, setNewName] = useState("");
  const [inferred, setInferred] = useState<InferredColumn[]>([]);
  const [mapping, setMapping] = useState<(string | null)[]>([]);
  const [mode, setMode] = useState<ImportMode>("insert");
  const [stop, setStop] = useState(true);
  const [truncate, setTruncate] = useState(false);
  const [emptyNull, setEmptyNull] = useState(true);
  const [csvFk, setCsvFk] = useState(false); // true = load with foreign key checks off
  const [confirmTruncate, setConfirmTruncate] = useState(false);
  const job = useJob<ImportResult>();
  useEffect(() => { if (db) ensureColumns(db); }, [db]); // eslint-disable-line react-hooks/exhaustive-deps
  const tables = useMemo(() => nodes.filter((n) => n.database === db && n.kind === "table").map((n) => n.name), [nodes, db]);
  const tableCols = useMemo(() => nodes.filter((n) => n.database === db && n.kind === "column" && n.parent === table).map((n) => n.name), [nodes, db, table]);

  useEffect(() => { if (preset.db) setDb(preset.db); if (preset.table) { setTable(preset.table); setNewTable(false); } }, [preset]);

  const load = useCallback(async (p: string, f: CsvFormat) => {
    try {
      setErr(null);
      const pv = await transferApi.preview(p, f);
      setPreview(pv);
      return pv;
    } catch (e) { setPreview(null); setErr(String(e)); return null; }
  }, []);

  const choose = async () => {
    const p = await pickPath({ multiple: false, filters: [{ name: "CSV / text", extensions: ["csv", "tsv", "txt"] }] });
    if (typeof p !== "string") return;
    setPath(p);
    let f = fmt;
    try { f = { ...fmt, delimiter: await transferApi.sniff(p) }; setFmt(f); } catch { /* keep the default */ }
    await load(p, f);
    setNewName(p.split("/").pop()!.replace(/\.[^.]+$/, "").replace(/[^A-Za-z0-9_]+/g, "_"));
  };

  // refresh the preview when the format changes
  useEffect(() => { if (path) { const t = setTimeout(() => load(path, fmt), 250); return () => clearTimeout(t); } }, [path, fmt, load]);

  // new table: infer the columns; existing table: map by header name
  useEffect(() => {
    if (!preview) return;
    if (newTable) {
      transferApi.infer(path, fmt).then((cols) => { setInferred(cols); setMapping(cols.map((c) => c.name)); }).catch((e) => setErr(String(e)));
    } else {
      const by = new Map(tableCols.map((c) => [c.toLowerCase(), c]));
      setMapping(preview.headers.map((h) => by.get(h.toLowerCase()) ?? null));
    }
  }, [preview?.headers.join("\u0000"), newTable, table, db, tableCols.join(",")]); // eslint-disable-line react-hooks/exhaustive-deps

  const targetColumns = newTable ? inferred.map((c) => c.name) : tableCols;
  const mapped = mapping.filter(Boolean).length;
  const ready = !!preview && !job.running && mapped > 0 && (newTable ? !!newName : !!table);

  const start = () => {
    setConfirmTruncate(false);
    job.run(async (id) => {
      let target = table;
      if (newTable) {
        // the user may have edited names and types: create exactly what is shown
        await transferApi.createTable(connectionId, db, newName, inferred);
        onSchemaChanged();
        target = newName;
      }
      try {
        return await transferApi.importCsv(connectionId, id, path, fmt, {
          database: db, table: target, mapping, mode, batch_rows: 500, stop_on_error: stop, truncate_first: truncate && !newTable, empty_as_null: emptyNull, disable_fk_checks: csvFk,
        });
      } finally { onSchemaChanged(); }
    }, `Import ${path.split(/[\\/]/).pop()} into ${newTable ? newName : table}`);
  };
  const r = job.result;
  const set = (p: Partial<CsvFormat>) => setFmt({ ...fmt, ...p });

  return (
    <div className="xfer-form">
      <div className="xfer-cols">
        <section>
          <h4>File</h4>
          <label>CSV file<div className="path-field"><input value={path} readOnly placeholder="Choose a file…" /><button onClick={choose}>Browse…</button></div></label>
          <div className="row">
            <label>Delimiter<Select value={fmt.delimiter} onChange={(e) => set({ delimiter: e.target.value })}>{DELIMS.map(([v, l]) => <option key={l} value={v}>{l}</option>)}</Select></label>
            <label>Quote<Select value={fmt.quote} onChange={(e) => set({ quote: e.target.value })}><option value={'"'}>"</option><option value="'">'</option></Select></label>
          </div>
          <label>Encoding<Select value={fmt.encoding} onChange={(e) => set({ encoding: e.target.value })}>{ENCODINGS.map((x) => <option key={x}>{x}</option>)}</Select></label>
          <label className="check"><input type="checkbox" checked={fmt.has_header} onChange={(e) => set({ has_header: e.target.checked })} /> First row is a header</label>
          <label className="check"><input type="checkbox" checked={fmt.trim} onChange={(e) => set({ trim: e.target.checked })} /> Trim spaces around values</label>
          <label>Text meaning NULL<input className="cell-input" placeholder="e.g. NULL or \N" value={fmt.null_text ?? ""} onChange={(e) => set({ null_text: e.target.value || null })} /></label>
        </section>
        <section>
          <h4>Target</h4>
          <label>Database<Select value={db} onChange={(e) => { setDb(e.target.value); setTable(""); }}>{dbs.map((d) => <option key={d}>{d}</option>)}</Select></label>
          <label className="check"><input type="checkbox" checked={newTable} onChange={(e) => setNewTable(e.target.checked)} /> Create a new table from the file</label>
          {newTable
            ? <label>New table name<input value={newName} onChange={(e) => setNewName(e.target.value)} /></label>
            : <label>Table<Select value={table} onChange={(e) => setTable(e.target.value)}><option value="">(choose)</option>{tables.map((t) => <option key={t}>{t}</option>)}</Select></label>}
          {!newTable && (
            <>
              <label>If a row with the same key exists
                <Select value={mode} onChange={(e) => setMode(e.target.value as ImportMode)}>
                  <option value="insert">Report an error</option><option value="ignore">Skip the new row</option>
                  <option value="update">Update the existing row</option><option value="replace">Replace the existing row</option>
                </Select></label>
              <label className="check"><input type="checkbox" checked={truncate} onChange={(e) => setTruncate(e.target.checked)} /> Empty the table first (cannot be undone)</label>
            </>
          )}
          <label className="check"><input type="checkbox" checked={stop} onChange={(e) => setStop(e.target.checked)} /> Stop at the first bad row and undo everything</label>
          <label className="check"><input type="checkbox" checked={emptyNull} onChange={(e) => setEmptyNull(e.target.checked)} /> Empty values are NULL in number and date columns</label>
          <label className="check"><input type="checkbox" checked={csvFk} onChange={(e) => setCsvFk(e.target.checked)} /> Disable foreign key checks while importing</label>
          {csvFk && <div className="dim small">Rows that point at missing parents will be loaded. The setting is switched back on when the import ends.</div>}
          <button className="primary" disabled={!ready} onClick={() => (truncate && !newTable ? setConfirmTruncate(true) : start())}>Import {preview ? `(${mapped} column${mapped === 1 ? "" : "s"})` : ""}</button>
        </section>
      </div>
      {err && <div className="banner bad">{err}</div>}

      {preview && (
        <div className="map-table dg-scroll">
          <table className="grid">
            <thead>
              <tr><th className="rownum">Maps to</th>{preview.headers.map((h, i) => (
                <th key={i}>
                  <Select value={mapping[i] ?? ""} onChange={(e) => setMapping(mapping.map((m, k) => (k === i ? e.target.value || null : m)))}>
                    <option value="">(skip)</option>{targetColumns.map((c) => <option key={c} value={c}>{c}</option>)}
                  </Select>
                </th>
              ))}</tr>
              {newTable && <tr><th className="rownum">Type</th>{inferred.map((c, i) => (
                <th key={i}><input className="cell-input" value={c.column_type} onChange={(e) => setInferred(inferred.map((x, k) => (k === i ? { ...x, column_type: e.target.value } : x)))} /></th>
              ))}</tr>}
              {newTable && <tr><th className="rownum">Name</th>{inferred.map((c, i) => (
                <th key={i}><input className="cell-input" value={c.name} onChange={(e) => { const old = c.name; setInferred(inferred.map((x, k) => (k === i ? { ...x, name: e.target.value } : x))); setMapping(mapping.map((m) => (m === old ? e.target.value : m))); }} /></th>
              ))}</tr>}
              <tr><th className="rownum">CSV</th>{preview.headers.map((h, i) => <th key={i} className="csvh">{h}</th>)}</tr>
            </thead>
            <tbody>{preview.rows.map((r, i) => <tr key={i}><td className="rownum">{i + 1}</td>{preview.headers.map((_, c) => <td key={c}>{r[c] ?? ""}</td>)}</tr>)}</tbody>
          </table>
          {preview.more && <div className="dim small">Showing the first {preview.rows.length} rows.</div>}
        </div>
      )}

      <ProgressBar p={job.progress} running={job.running} onCancel={job.cancel} />
      {job.cancelled && <div className="banner">Import cancelled; everything was undone.</div>}
      {job.error && <div className="banner bad">{job.error}</div>}
      {r && (
        <div className={`banner result-card ${r.aborted ? "bad" : r.errors.length ? "" : "ok"}`}>
          <b>{r.aborted ? "Import stopped and undone" : "Import finished"}</b> in {fmtMs(r.elapsed_ms)}: {r.rows_read.toLocaleString()} row(s) read, {r.affected_rows.toLocaleString()} affected{r.errors.length ? `, ${r.errors.length} bad row(s)` : ""}.
          {r.warnings.map((w) => <div key={w} className="dim"><Icon name="warn" size={12} /> {w}</div>)}
          {r.errors.slice(0, 20).map((e, i) => <div key={i} className="dim">line {e.line}: {e.message}</div>)}
        </div>
      )}
      {confirmTruncate && <ConfirmDialog message={`Empty ${db}.${table} before importing? The existing rows cannot be restored.`} confirmLabel="Empty and import" onCancel={() => setConfirmTruncate(false)} onConfirm={start} />}
    </div>
  );
}

// ---------------------------------------------------------------- export query

function ExportQuery({ connectionId, nodes }: { connectionId: number; nodes: Node[] }) {
  const dbs = useMemo(() => nodes.filter((n) => n.kind === "database").map((n) => n.name), [nodes]);
  const [sql, setSql] = useState("SELECT * FROM ");
  const [db, setDb] = useState("");
  const [format, setFormat] = useState<ExportFormat>("csv");
  const [nullText, setNullText] = useState("");
  const [gzip, setGzip] = useState(false);
  const job = useJob<{ rows: number; warnings: string[]; path: string }>();
  const go = async () => {
    const f = EXPORT_FORMATS.find((x) => x.format === format)!;
    const zip = gzip && format !== "xlsx";
    const path = await savePath({ defaultPath: `query.${f.ext}${zip ? ".gz" : ""}` });
    if (!path) return;
    job.run(async (jobId) => ({ ...(await transferApi.exportQuery(connectionId, jobId, db || null, sql, format, nullText, zip, path)), path }), `Export query to ${path.split(/[\\/]/).pop()}`);
  };
  const r = job.result;
  return (
    <div className="xfer-form">
      <div className="xfer-cols">
        <section className="wide">
          <h4>Query</h4>
          <textarea className="sql-box" value={sql} onChange={(e) => setSql(e.target.value)} spellCheck={false} />
        </section>
        <section>
          <h4>Output</h4>
          <label>Database<Select value={db} onChange={(e) => setDb(e.target.value)}><option value="">(none)</option>{dbs.map((d) => <option key={d}>{d}</option>)}</Select></label>
          <label>Format<Select value={format} onChange={(e) => setFormat(e.target.value as ExportFormat)}>{EXPORT_FORMATS.map((f) => <option key={f.format} value={f.format}>{f.label}</option>)}</Select></label>
          {format === "csv" && <label>Text for NULL<input className="cell-input" value={nullText} placeholder="(empty)" onChange={(e) => setNullText(e.target.value)} /></label>}
          <label className="check"><input type="checkbox" checked={gzip && format !== "xlsx"} disabled={format === "xlsx"} onChange={(e) => setGzip(e.target.checked)} /> Compress with gzip (.gz)</label>
          <button className="primary" disabled={job.running || !sql.trim()} onClick={go}>Export…</button>
          <div className="dim small">Only a single SELECT is exported. Every row is streamed to the file, however many there are.</div>
        </section>
      </div>
      <ProgressBar p={job.progress} running={job.running} onCancel={job.cancel} />
      {job.cancelled && <div className="banner">Export cancelled. The partial file was removed.</div>}
      {job.error && <div className="banner bad">{job.error}</div>}
      {r && <div className="banner ok result-card"><b>Exported {r.rows.toLocaleString()} row(s)</b><div><code>{r.path}</code></div>{r.warnings.map((w) => <div key={w} className="dim"><Icon name="warn" size={12} /> {w}</div>)}</div>}
    </div>
  );
}
