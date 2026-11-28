import { useEffect, useMemo, useState } from "react";
import { open as pickPath } from "@tauri-apps/plugin-dialog";
import Select from "./Select";
import { ConfirmDialog } from "./SidePanels";
import { formatBytes } from "./browserMeta";
import { ProgressBar, useJob } from "./Jobs";
import { pgTransferApi, type Node, type PgDumpOptions, type PgDumpResult, type PgRestoreResult } from "./api";

const seconds = (ms: number) => (ms >= 60000 ? `${(ms / 60000).toFixed(1)} min` : `${(ms / 1000).toFixed(1)} s`);
const TOOLS = "Uses PostgreSQL's own programs (pg_dump, pg_restore, psql) installed on this computer, at least as new as the server.";

/** Back up a PostgreSQL database: everything, or chosen schemas, as a SQL script or an archive. */
export function PgBackup({ connectionId, nodes, schema }: { connectionId: number; nodes: Node[]; schema?: string }) {
  const schemas = useMemo(() => nodes.filter((n) => n.kind === "database").map((n) => n.name), [nodes]);
  const [all, setAll] = useState(!schema);
  const [picked, setPicked] = useState<Set<string>>(new Set(schema ? [schema] : []));
  const [o, setO] = useState<Omit<PgDumpOptions, "schemas">>({ format: "sql", content: "all", clean: false, no_owner: false });
  const [dir, setDir] = useState("");
  const [name, setName] = useState(schema ?? "backup");
  const job = useJob<PgDumpResult>();
  useEffect(() => { if (schema) { setAll(false); setPicked(new Set([schema])); setName(schema); } }, [schema]);
  const chooseDir = async () => { const p = await pickPath({ directory: true, multiple: false }); if (typeof p === "string") setDir(p); };
  const start = () => job.run((id) => pgTransferApi.backup(connectionId, id, { ...o, schemas: all ? [] : [...picked], clean: o.format === "sql" && o.content !== "data" && o.clean }, dir, name || "backup"), `Backup ${name || "backup"}`);
  const r = job.result;
  return (
    <div className="xfer-form">
      <div className="xfer-cols">
        <section>
          <h4>Schemas</h4>
          <label className="check"><input type="checkbox" checked={all} onChange={(e) => setAll(e.target.checked)} /> <b>The whole database</b></label>
          {!all && <div className="db-pick">{schemas.map((s) => (
            <label key={s} className="check"><input type="checkbox" checked={picked.has(s)} onChange={(e) => { const n = new Set(picked); if (e.target.checked) n.add(s); else n.delete(s); setPicked(n); }} /> {s}</label>
          ))}</div>}
        </section>
        <section>
          <h4>Contents</h4>
          <label>Format<Select aria-label="Format" value={o.format} onChange={(e) => setO({ ...o, format: e.target.value as PgDumpOptions["format"] })}>
            <option value="sql">SQL script (.sql)</option><option value="custom">Compressed archive (.dump)</option></Select></label>
          <label>Include<Select aria-label="Include" value={o.content} onChange={(e) => setO({ ...o, content: e.target.value as PgDumpOptions["content"] })}>
            <option value="all">Structure and data</option><option value="structure">Structure only</option><option value="data">Data only</option></Select></label>
          {o.format === "sql" ? (
            <>
              <label className="check"><input type="checkbox" disabled={o.content === "data"} checked={o.clean && o.content !== "data"} onChange={(e) => setO({ ...o, clean: e.target.checked })} /> Drop each object before creating it</label>
              <label className="check"><input type="checkbox" checked={o.no_owner} onChange={(e) => setO({ ...o, no_owner: e.target.checked })} /> Leave out owners and privileges</label>
            </>
          ) : <div className="dim small">What to drop and who owns the objects is chosen when the archive is restored.</div>}
        </section>
        <section>
          <h4>Output</h4>
          <label>Folder<div className="path-field"><input value={dir} placeholder="Choose a folder…" onChange={(e) => setDir(e.target.value)} /><button onClick={chooseDir}>Browse…</button></div></label>
          <label>File name<input value={name} onChange={(e) => setName(e.target.value)} /></label>
          <div className="dim small">A timestamp is added to the name, so a backup never overwrites an earlier one. {TOOLS}</div>
          <button className="primary" disabled={job.running || !dir || (!all && picked.size === 0)} onClick={start}>Start backup</button>
        </section>
      </div>
      <ProgressBar p={job.progress} running={job.running} onCancel={job.cancel} />
      {job.cancelled && <div className="banner">Backup cancelled; nothing was written.</div>}
      {job.error && <div className="banner bad pg-output">{job.error}</div>}
      {r && <div className="banner ok result-card"><b>Backup complete</b> in {seconds(r.elapsed_ms)} ({formatBytes(r.bytes)}, {r.tool}).
        <div><code>{r.path}</code> <button className="mini" onClick={() => navigator.clipboard.writeText(r.path)}>Copy path</button></div></div>}
    </div>
  );
}

/** Restore a backup into the connected database. */
export function PgRestore({ connectionId, onSchemaChanged, production }: { connectionId: number; onSchemaChanged(): void; production: boolean }) {
  const [path, setPath] = useState("");
  const [o, setO] = useState({ clean: false, single_transaction: true, no_owner: false });
  const [confirm, setConfirm] = useState(false);
  const [typed, setTyped] = useState("");
  const job = useJob<PgRestoreResult>();
  const archive = !/\.sql$/i.test(path.trim());
  const choose = async () => { const p = await pickPath({ multiple: false, filters: [{ name: "PostgreSQL backup", extensions: ["sql", "dump", "backup", "pgdump"] }] }); if (typeof p === "string") setPath(p); };
  const go = () => {
    setConfirm(false); setTyped("");
    job.run(async (id) => { try { return await pgTransferApi.restore(connectionId, id, path.trim(), o); } finally { onSchemaChanged(); } }, `Restore ${path.split(/[\\/]/).pop()}`);
  };
  const r = job.result;
  return (
    <div className="xfer-form">
      <div className="xfer-cols">
        <section>
          <h4>Source</h4>
          <label>Backup file (.sql script or archive)<div className="path-field"><input aria-label="Backup file" value={path} placeholder="Choose…" onChange={(e) => setPath(e.target.value)} /><button onClick={choose}>File…</button></div></label>
          <label className="check"><input type="checkbox" checked={o.single_transaction} onChange={(e) => setO({ ...o, single_transaction: e.target.checked })} /> All or nothing (one transaction)</label>
          {archive && <label className="check"><input type="checkbox" checked={o.clean} onChange={(e) => setO({ ...o, clean: e.target.checked })} /> Drop each object before restoring it</label>}
          {archive && <label className="check"><input type="checkbox" checked={o.no_owner} onChange={(e) => setO({ ...o, no_owner: e.target.checked })} /> Leave out owners and privileges</label>}
          {production && <label>This is a production connection. Type <b>production</b> to allow the restore.<input aria-label="Type production" value={typed} onChange={(e) => setTyped(e.target.value)} /></label>}
          <button className="danger" disabled={!path.trim() || job.running || (production && typed !== "production")} onClick={() => setConfirm(true)}>Restore…</button>
          <div className="dim small">Restoring runs everything in the file against the connected database. A script can also run commands on this computer (psql's <code>\!</code>), so only restore files you trust. It stops at the first error. {TOOLS}</div>
        </section>
      </div>
      <ProgressBar p={job.progress} running={job.running} onCancel={job.cancel} />
      {job.cancelled && <div className="banner">Cancelled.{o.single_transaction ? " Nothing was changed." : " What had already been restored was not undone."}</div>}
      {job.error && <div className="banner bad pg-output">{job.error}</div>}
      {r && <div className="banner ok result-card"><b>Restore complete</b> in {seconds(r.elapsed_ms)} ({r.tool}).{r.warnings.map((w, i) => <div key={i} className="dim small">{w}</div>)}</div>}
      {confirm && <ConfirmDialog message={`Restore ${path.split(/[\\/]/).pop()} into the connected database now? It can create, replace and delete tables and data.`} confirmLabel="Restore" onCancel={() => setConfirm(false)} onConfirm={go} />}
    </div>
  );
}
