import { useEffect, useState } from "react";
import { syncApi, plainError, adminApi, type SavedConnection } from "./api";
import { ProgressBar, useJob } from "./Jobs";

/** Copy one database into another connection (or another name on the same one). Existing tables of the same name are replaced. */
export default function CopyDatabaseDialog(props: { connections: SavedConnection[]; sourceId: number; sourceDb: string; onClose(): void; onDone(msg: string): void }) {
  const others = props.connections;
  const [target, setTarget] = useState<number>(others.find((c) => c.id !== props.sourceId)?.id ?? props.sourceId);
  const [name, setName] = useState(props.sourceDb);
  const [withData, setWithData] = useState(true);
  const [existing, setExisting] = useState<string[]>([]);
  const [agree, setAgree] = useState(false);
  const job = useJob<Awaited<ReturnType<typeof syncApi.copyDatabase>>>();
  const same = target === props.sourceId && name.trim().toLowerCase() === props.sourceDb.toLowerCase();

  useEffect(() => {
    setAgree(false);
    if (!name.trim()) { setExisting([]); return; }
    adminApi.tables(target, name.trim()).then((t) => setExisting(t.map((x) => x.name))).catch(() => setExisting([]));
  }, [target, name]);
  useEffect(() => {
    const on = (e: KeyboardEvent) => { if (e.key === "Escape" && !job.running) { e.stopPropagation(); props.onClose(); } };
    window.addEventListener("keydown", on, true);
    return () => window.removeEventListener("keydown", on, true);
  }, [props, job.running]);
  useEffect(() => { if (job.result) props.onDone(`Copied ${props.sourceDb} to ${name.trim()}: ${job.result.tables} table(s), ${job.result.rows.toLocaleString()} rows.`); }, [job.result]); // eslint-disable-line react-hooks/exhaustive-deps

  const start = () => job.run((id) => syncApi.copyDatabase(id, props.sourceId, props.sourceDb, target, name.trim(), withData), `Copy ${props.sourceDb}`);
  const canStart = !!name.trim() && !same && !job.running && (existing.length === 0 || agree);
  return (
    <div className="overlay" onMouseDown={() => !job.running && props.onClose()}>
      <div className="dialog copydb-dialog" role="dialog" aria-label="Copy database" onMouseDown={(e) => e.stopPropagation()}>
        <h3>Copy {props.sourceDb} to another connection</h3>
        <p className="dim small">MySQL and MariaDB. The tables, views and routines are dumped and run on the target. Nothing in the source changes.</p>
        <label>Into connection<select value={target} onChange={(e) => setTarget(Number(e.target.value))} aria-label="Target connection" disabled={job.running}>{others.map((c) => <option key={c.id} value={c.id!}>{c.name}</option>)}</select></label>
        <label>Database name<input value={name} onChange={(e) => setName(e.target.value)} disabled={job.running} aria-label="Target database name" spellCheck={false} /></label>
        <label className="check"><input type="checkbox" checked={withData} onChange={(e) => setWithData(e.target.checked)} disabled={job.running} /> Include the rows (not only the structure)</label>
        {same && <div className="banner bad">That is the source itself. Choose another connection or another name.</div>}
        {existing.length > 0 && !same && (
          <div className="banner">
            <b>{existing.length} table(s) already exist there and will be replaced:</b> {existing.slice(0, 12).join(", ")}{existing.length > 12 ? "…" : ""}
            <label className="check"><input type="checkbox" checked={agree} onChange={(e) => setAgree(e.target.checked)} disabled={job.running} /> Replace them</label>
          </div>
        )}
        <ProgressBar p={job.progress} running={job.running} onCancel={job.cancel} />
        {job.error && <div className="banner bad" role="alert">{plainError(job.error)}</div>}
        {job.cancelled && <div className="banner">Cancelled. The target may hold part of the copy.</div>}
        {job.result && <div className="banner ok" role="status">Done: {job.result.tables} table(s), {job.result.rows.toLocaleString()} rows in {(job.result.elapsed_ms / 1000).toFixed(1)} s.{job.result.warnings.length ? ` ${job.result.warnings.join(" ")}` : ""}</div>}
        <div className="actions"><button onClick={props.onClose} disabled={job.running}>{job.result ? "Close" : "Cancel"}</button>{!job.result && <button className="primary" disabled={!canStart} onClick={start}>Copy</button>}</div>
      </div>
    </div>
  );
}
