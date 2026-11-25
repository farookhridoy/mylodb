import { useCallback, useEffect, useState } from "react";
import { auditApi, plainError, type AuditEntry, type AuditStatus } from "./api";

const mb = (n: number) => (n >= 1048576 ? `${(n / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1024))} KB`);

/** Settings → Audit log: switch it on, see where the file is, and read the latest entries. */
export default function AuditSettings() {
  const [st, setSt] = useState<AuditStatus | null>(null);
  const [rows, setRows] = useState<AuditEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(async () => {
    try { const s = await auditApi.status(); setSt(s); setRows(s.size > 0 ? await auditApi.tail(50) : []); setError(null); } catch (e) { setError(plainError(e)); }
  }, []);
  useEffect(() => { load(); }, [load]);
  const toggle = async (on: boolean) => { try { setSt(await auditApi.set(on)); await load(); } catch (e) { setError(plainError(e)); } };
  if (!st) return <div className="prefs-page dim">Loading…</div>;
  return (
    <div className="prefs-page audit-page">
      <p>The audit log is a file with one line per statement that <b>changed</b> something (data, structure, privileges, settings), written by this app: who, which connection and database, when, whether it worked and the SQL. Reads are not logged. It stays on this computer; nothing is sent anywhere.</p>
      <label className="check"><input type="checkbox" checked={st.enabled} onChange={(e) => toggle(e.target.checked)} /> Keep an audit log</label>
      <p className="dim small">File: <code>{st.path}</code> ({st.size ? mb(st.size) : "empty"}). A file over 10 MB is rotated; the newest three are kept. Each line is JSON, so any tool can read it.</p>
      {error && <div className="banner bad" role="alert">{error}</div>}
      <div className="audit-head"><b>Latest entries</b><button onClick={load} title="Reload">⟳</button></div>
      {rows.length === 0 ? <div className="empty">{st.enabled ? "Nothing has been logged yet." : "Turn the log on to start recording."}</div> : (
        <div className="audit-scroll">
          <table className="grid logtable audit-table"><thead><tr><th>Time</th><th>Connection</th><th>Database</th><th>OK</th><th>SQL</th></tr></thead>
            <tbody>{rows.map((r, i) => <tr key={i} className={r.ok ? "" : "lvl-error"}><td>{r.time}</td><td title={r.who}>{r.connection}</td><td>{r.database ?? ""}</td><td>{r.ok ? "✓" : "✗"}</td><td className="msg"><code>{r.sql.length > 300 ? r.sql.slice(0, 300) + "…" : r.sql}</code></td></tr>)}</tbody></table>
        </div>
      )}
    </div>
  );
}
