import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Select from "./Select";
import Icon from "./Icon";
import {
  serverApi, type BinaryLog, type BinlogEvent, type CheckOption, type KillKind, type LogLine, type LogSettings, type Maintenance,
  type MaintenanceRow, type Node, type Process, type ServerInfo, type SlowQuery, type Variable, type VarScope,
} from "./api";
import { ConfirmDialog } from "./SidePanels";
import { usePoll } from "./usePoll";
import StatusCharts from "./StatusCharts";

type Tab = "overview" | "charts" | "processes" | "variables" | "status" | "maintenance" | "logs";

const fmtUptime = (s: number) => { const d = Math.floor(s / 86400), h = Math.floor((s % 86400) / 3600), m = Math.floor((s % 3600) / 60); return `${d ? d + "d " : ""}${h}h ${m}m`; };
const fmtBytes = (n: number) => (n >= 1 << 30 ? `${(n / (1 << 30)).toFixed(1)} GB` : n >= 1 << 20 ? `${(n / (1 << 20)).toFixed(1)} MB` : n >= 1024 ? `${(n / 1024).toFixed(1)} KB` : `${Math.round(n)} B`);

export default function ServerTools(props: { connectionId: number; nodes: Node[] }) {
  const [tab, setTab] = useState<Tab>("overview");
  const names: [Tab, string][] = [["overview", "Overview"], ["charts", "Charts"], ["processes", "Processes"], ["variables", "Variables"], ["status", "Status"], ["maintenance", "Maintenance"], ["logs", "Logs"]];
  return (
    <div className="server-tools">
      <div className="dz-tabs">{names.map(([t, label]) => <button key={t} className={tab === t ? "active" : ""} onClick={() => setTab(t)}>{label}</button>)}</div>
      <div className="st-body">
        {tab === "overview" && <Overview id={props.connectionId} />}
        {tab === "charts" && <StatusCharts id={props.connectionId} />}
        {tab === "processes" && <Processes id={props.connectionId} />}
        {tab === "variables" && <Variables id={props.connectionId} />}
        {tab === "status" && <Status id={props.connectionId} />}
        {tab === "maintenance" && <MaintenanceTab id={props.connectionId} nodes={props.nodes} />}
        {tab === "logs" && <Logs id={props.connectionId} />}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- overview

function Spark({ values: raw, color = "#4f8cff" }: { values: number[]; color?: string }) {
  const values = raw.filter(Number.isFinite);
  if (values.length < 2) return <svg className="spark" />;
  const max = Math.max(...values, 1);
  const pts = values.map((v, i) => `${(i / (values.length - 1)) * 100},${28 - (v / max) * 26}`).join(" ");
  return <svg className="spark" viewBox="0 0 100 30" preserveAspectRatio="none"><polyline points={pts} fill="none" stroke={color} strokeWidth="1.5" vectorEffect="non-scaling-stroke" /></svg>;
}

function Overview({ id }: { id: number }) {
  const [info, setInfo] = useState<ServerInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [kpi, setKpi] = useState<{ qps: number[]; conns: number[]; threads: number; running: number; hit: number | null; slow: number; aborted: number; inB: number; outB: number }>(
    { qps: [], conns: [], threads: 0, running: 0, hit: null, slow: 0, aborted: 0, inB: 0, outB: 0 });
  const last = useRef<{ t: number; s: Record<string, number> } | null>(null);

  useEffect(() => { serverApi.info(id).then(setInfo).catch((e) => setError(String(e))); }, [id]);
  usePoll(async () => {
    const rows = await serverApi.status(id, "global");
    const s: Record<string, number> = {};
    rows.forEach(([k, v]) => { const n = Number(v); if (!isNaN(n)) s[k] = n; });
    const now = performance.now();
    const prev = last.current;
    // two polls a few milliseconds apart would give a meaningless (or infinite) rate: skip the sample
    if (prev && now - prev.t < 300) return;
    last.current = { t: now, s };
    const per = (k: string) => (prev && s[k] !== undefined && prev.s[k] !== undefined ? Math.max(0, (s[k] - prev.s[k]) / ((now - prev.t) / 1000)) : 0);
    const reads = s.Innodb_buffer_pool_reads, reqs = s.Innodb_buffer_pool_read_requests;
    setKpi((k) => ({
      qps: prev ? [...k.qps, per("Questions")].slice(-60) : k.qps, conns: prev ? [...k.conns, s.Threads_connected ?? 0].slice(-60) : k.conns,
      threads: s.Threads_connected ?? 0, running: s.Threads_running ?? 0, slow: s.Slow_queries ?? 0, aborted: s.Aborted_connects ?? 0,
      hit: reqs ? (1 - reads / reqs) * 100 : null, inB: per("Bytes_received"), outB: per("Bytes_sent"),
    }));
  }, 2000, true);

  if (error) return <div className="error">{error}</div>;
  return (
    <div className="overview">
      {info && (
        <div className="cards">
          <div className="card"><span className="dim">Server</span><b>MySQL {info.version}</b><span className="dim small">{info.version_comment}</span></div>
          <div className="card"><span className="dim">Uptime</span><b>{fmtUptime(info.uptime_seconds)}</b></div>
          <div className="card"><span className="dim">Host</span><b>{info.hostname}:{info.port}</b></div>
          <div className="card"><span className="dim">Encryption</span><b>{info.ssl_cipher ?? "none"}</b></div>
          <div className="card wide"><span className="dim">Data directory</span><code>{info.datadir}</code></div>
          <div className="card"><span className="dim">Character set</span><b>{info.charset}</b><span className="dim small">{info.collation}</span></div>
          <div className="card"><span className="dim">Max connections</span><b>{info.max_connections}</b></div>
          <div className="card wide"><span className="dim">SQL mode</span><code>{info.sql_mode || "(empty)"}</code></div>
        </div>
      )}
      <h4>Live (every 2 s)</h4>
      <div className="cards">
        <div className="card"><span className="dim">Queries / s</span><b>{(kpi.qps.at(-1) ?? 0).toFixed(1)}</b><Spark values={kpi.qps} /></div>
        <div className="card"><span className="dim">Connections</span><b>{kpi.threads}</b><span className="dim small">{kpi.running} running</span><Spark values={kpi.conns} color="#4cc38a" /></div>
        <div className="card"><span className="dim">Network</span><b>↓ {fmtBytes(kpi.inB)}/s</b><span className="dim small">↑ {fmtBytes(kpi.outB)}/s</span></div>
        <div className="card"><span className="dim">Buffer pool hit rate</span><b>{kpi.hit === null ? "n/a" : `${kpi.hit.toFixed(2)}%`}</b></div>
        <div className="card"><span className="dim">Slow queries (total)</span><b>{kpi.slow.toLocaleString()}</b></div>
        <div className="card"><span className="dim">Aborted connects</span><b>{kpi.aborted.toLocaleString()}</b></div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- processes

function Processes({ id }: { id: number }) {
  const [rows, setRows] = useState<Process[]>([]);
  const [selfId, setSelfId] = useState(0);
  const [auto, setAuto] = useState(true);
  const [every, setEvery] = useState(2000);
  const [hideSleep, setHideSleep] = useState(true);
  const [filter, setFilter] = useState("");
  const [slowAfter, setSlowAfter] = useState(5);
  const [sel, setSel] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<{ p: Process; kind: KillKind } | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const load = useCallback(async () => {
    try { const r = await serverApi.processes(id); setRows(r.processes); setSelfId(r.self_id); setError(null); } catch (e) { setError(String(e)); }
  }, [id]);
  usePoll(load, every, auto);
  useEffect(() => { if (!auto) load(); }, [auto, load]);

  const shown = useMemo(() => {
    const f = filter.toLowerCase();
    return rows
      .filter((p) => p.id !== selfId)
      .filter((p) => !hideSleep || p.command !== "Sleep")
      .filter((p) => !f || [p.user, p.host, p.db, p.command, p.state, p.info].some((v) => (v ?? "").toLowerCase().includes(f)))
      .sort((a, b) => b.time - a.time);
  }, [rows, hideSleep, filter, selfId]);
  const current = rows.find((p) => p.id === sel);

  const doKill = async () => {
    if (!confirm) return;
    const { p, kind } = confirm;
    setConfirm(null);
    try { await serverApi.kill(id, p.id, kind); setNotice(`${kind === "query" ? "Query" : "Connection"} ${p.id} stopped`); await load(); } catch (e) { setError(String(e)); }
  };

  return (
    <div className="proc">
      <div className="dg-toolbar">
        <label className="check"><input type="checkbox" checked={auto} onChange={(e) => setAuto(e.target.checked)} /> Auto refresh</label>
        <Select value={every} onChange={(e) => setEvery(Number(e.target.value))}>{[1000, 2000, 5000, 10000].map((n) => <option key={n} value={n}>{n / 1000} s</option>)}</Select>
        <button onClick={load}>⟳</button>
        <label className="check"><input type="checkbox" checked={hideSleep} onChange={(e) => setHideSleep(e.target.checked)} /> Hide idle</label>
        <input placeholder="Filter user, db, SQL…" value={filter} onChange={(e) => setFilter(e.target.value)} />
        <label className="check">Slow after <input className="cell-input narrow" type="number" value={slowAfter} onChange={(e) => setSlowAfter(Number(e.target.value))} /> s</label>
        <span className="grow" /><span className="dim">{shown.length} process(es)</span>
      </div>
      {error && <div className="banner bad">{error}</div>}
      {notice && <div className="banner ok">{notice}</div>}
      <div className="st-split">
        <div className="dg-scroll">
          <table className="grid proc-grid">
            <thead><tr><th>Id</th><th>User</th><th>Host</th><th>DB</th><th>Command</th><th>Time</th><th>State</th><th>Info</th><th /></tr></thead>
            <tbody>
              {shown.map((p) => (
                <tr key={p.id} className={`${p.id === sel ? "row-selected" : ""} ${p.command !== "Sleep" && p.time >= slowAfter ? "slow" : ""}`} onClick={() => setSel(p.id)}>
                  <td>{p.id}</td><td>{p.user}</td><td>{p.host}</td><td>{p.db ?? ""}</td><td>{p.command}</td><td className="num">{p.time}s</td><td>{p.state ?? ""}</td>
                  <td className="info">{(p.info ?? "").replace(/\s+/g, " ").slice(0, 160)}</td>
                  <td className="kill">
                    <button className="mini" title="Stop the running statement, keep the connection" disabled={p.command === "Sleep"} onClick={(e) => { e.stopPropagation(); setConfirm({ p, kind: "query" }); }}>Stop query</button>
                    <button className="mini danger-text" title="Close the connection" onClick={(e) => { e.stopPropagation(); setConfirm({ p, kind: "connection" }); }}>Kill</button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {shown.length === 0 && <div className="empty">No matching processes</div>}
        </div>
        {current && (
          <aside className="dg-side">
            <div className="vp-head"><b>Process {current.id}</b><span className="grow" /><button className="mini" onClick={() => navigator.clipboard.writeText(current.info ?? "")}>Copy SQL</button></div>
            <div className="proc-detail">
              <div className="dim small">{current.user}@{current.host} · {current.db ?? "no database"} · {current.command} · {current.time}s · {current.state ?? ""}</div>
              <pre>{current.info ?? "(no statement)"}</pre>
            </div>
          </aside>
        )}
      </div>
      {confirm && (
        <ConfirmDialog
          message={confirm.kind === "query" ? `Stop the statement running on connection ${confirm.p.id} (${confirm.p.user})? The connection stays open.` : `Close connection ${confirm.p.id} (${confirm.p.user}@${confirm.p.host})? Any open transaction on it is rolled back.`}
          confirmLabel={confirm.kind === "query" ? "Stop query" : "Kill connection"} onCancel={() => setConfirm(null)} onConfirm={doKill} />
      )}
    </div>
  );
}

// ---------------------------------------------------------------- variables

function Variables({ id }: { id: number }) {
  const [scope, setScope] = useState<VarScope>("global");
  const [vars, setVars] = useState<Variable[]>([]);
  const [filter, setFilter] = useState("");
  const [changed, setChanged] = useState(false);
  const [edit, setEdit] = useState<{ name: string; value: string } | null>(null);
  const [confirm, setConfirm] = useState<{ name: string; value: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [warnings, setWarnings] = useState<string[]>([]);
  const load = useCallback(() => serverApi.variables(id, scope).then((v) => { setVars(v); setError(null); }).catch((e) => setError(String(e))), [id, scope]);
  useEffect(() => { load(); }, [load]);
  const shown = useMemo(() => vars.filter((v) => (!filter || v.name.toLowerCase().includes(filter.toLowerCase()) || v.value.toLowerCase().includes(filter.toLowerCase())) && (!changed || (v.source && !["COMPILED", "DYNAMIC"].includes(v.source) || v.source === "DYNAMIC"))), [vars, filter, changed]);

  const apply = async () => {
    if (!confirm) return;
    const { name, value } = confirm;
    setConfirm(null);
    try { setWarnings(await serverApi.setGlobal(id, name, value)); setEdit(null); await load(); } catch (e) { setError(String(e)); }
  };

  return (
    <div className="vars">
      <div className="dg-toolbar">
        <Select value={scope} onChange={(e) => setScope(e.target.value as VarScope)}><option value="global">Global</option><option value="session">This session (read-only)</option></Select>
        <input placeholder="Search name or value…" value={filter} onChange={(e) => setFilter(e.target.value)} />
        <label className="check" title="Variables whose value was set explicitly or changed at runtime"><input type="checkbox" checked={changed} onChange={(e) => setChanged(e.target.checked)} /> Only changed from default</label>
        <button onClick={load}>⟳</button><span className="grow" /><span className="dim">{shown.length} of {vars.length}</span>
      </div>
      {error && <div className="banner bad">{error}</div>}
      {warnings.length > 0 && <div className="banner">{warnings.map((w) => <div key={w}><Icon name="warn" size={12} /> {w}</div>)}</div>}
      <div className="dg-scroll">
        <table className="grid">
          <thead><tr><th>Variable</th><th>Value</th><th>Source</th><th /></tr></thead>
          <tbody>
            {shown.map((v) => (
              <tr key={v.name}>
                <td>{v.name}</td>
                <td className="val">{edit?.name === v.name ? (
                  <input className="cell-input" autoFocus value={edit.value} onChange={(e) => setEdit({ name: v.name, value: e.target.value })}
                    onKeyDown={(e) => { if (e.key === "Enter") setConfirm(edit); if (e.key === "Escape") setEdit(null); }} />
                ) : v.value}</td>
                <td className="dim">{v.source ?? ""}</td>
                <td>{scope === "global" && (edit?.name === v.name
                  ? <><button className="mini" onClick={() => setConfirm(edit)}>Apply…</button><button className="mini" onClick={() => setEdit(null)}><Icon name="close" size={10} /></button></>
                  : <button className="mini" title="Change this global variable" onClick={() => setEdit({ name: v.name, value: v.value })}><Icon name="edit" size={12} /></button>)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="dim small note">Changes use SET GLOBAL: they apply to new connections and last until the server restarts. Put permanent settings in the server's option file.</div>
      {confirm && <ConfirmDialog message={`Run  SET GLOBAL ${confirm.name} = ${confirm.value}  ?`} confirmLabel="Change" onCancel={() => setConfirm(null)} onConfirm={apply} />}
    </div>
  );
}

// ---------------------------------------------------------------- status

function Status({ id }: { id: number }) {
  const [scope, setScope] = useState<VarScope>("global");
  const [rows, setRows] = useState<[string, string][]>([]);
  const [prev, setPrev] = useState<{ t: number; m: Record<string, number> } | null>(null);
  const [rates, setRates] = useState<Record<string, number>>({});
  const [filter, setFilter] = useState("");
  const [auto, setAuto] = useState(true);
  const [moving, setMoving] = useState(false);
  const prevRef = useRef(prev);

  usePoll(async () => {
    const r = await serverApi.status(id, scope);
    const m: Record<string, number> = {};
    r.forEach(([k, v]) => { const n = Number(v); if (!isNaN(n)) m[k] = n; });
    const now = performance.now();
    if (prevRef.current && now - prevRef.current.t < 300) return;
    if (prevRef.current) {
      const dt = (now - prevRef.current.t) / 1000;
      const out: Record<string, number> = {};
      for (const k in m) if (prevRef.current.m[k] !== undefined) out[k] = (m[k] - prevRef.current.m[k]) / dt;
      setRates(out);
    }
    prevRef.current = { t: now, m };
    setPrev(prevRef.current);
    setRows(r);
  }, 2000, auto);
  useEffect(() => { prevRef.current = null; setRates({}); }, [scope]);

  const f = filter.toLowerCase();
  const shown = rows.filter(([k]) => (!f || k.toLowerCase().includes(f)) && (!moving || Math.abs(rates[k] ?? 0) > 0));
  return (
    <div className="vars">
      <div className="dg-toolbar">
        <Select value={scope} onChange={(e) => setScope(e.target.value as VarScope)}><option value="global">Global</option><option value="session">This session</option></Select>
        <input placeholder="Search…" value={filter} onChange={(e) => setFilter(e.target.value)} />
        <label className="check"><input type="checkbox" checked={auto} onChange={(e) => setAuto(e.target.checked)} /> Auto refresh (2 s)</label>
        <label className="check"><input type="checkbox" checked={moving} onChange={(e) => setMoving(e.target.checked)} /> Only changing</label>
        <span className="grow" /><span className="dim">{shown.length} of {rows.length}{prev ? "" : " …"}</span>
      </div>
      <div className="dg-scroll">
        <table className="grid">
          <thead><tr><th>Status</th><th>Value</th><th>Per second</th></tr></thead>
          <tbody>
            {shown.map(([k, v]) => {
              const r = rates[k];
              return <tr key={k}><td>{k}</td><td className="val">{v}</td><td className={`num ${r ? "moving" : "dim"}`}>{r === undefined || !r ? "" : r.toFixed(Math.abs(r) < 10 ? 2 : 0)}</td></tr>;
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- maintenance

function MaintenanceTab({ id, nodes }: { id: number; nodes: Node[] }) {
  const dbs = useMemo(() => nodes.filter((n) => n.kind === "database").map((n) => n.name), [nodes]);
  const [db, setDb] = useState(dbs[0] ?? "");
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [op, setOp] = useState<Maintenance["op"]>("check");
  const [checkOpt, setCheckOpt] = useState<CheckOption | "">("");
  const [quick, setQuick] = useState(false);
  const [extended, setExtended] = useState(false);
  const [useFrm, setUseFrm] = useState(false);
  const [rows, setRows] = useState<MaintenanceRow[] | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState(false);
  const tables = useMemo(() => nodes.filter((n) => n.database === db && n.kind === "table").map((n) => n.name), [nodes, db]);
  useEffect(() => { setPicked(new Set()); }, [db]);

  const operation = (): Maintenance =>
    op === "check" ? { op: "check", option: checkOpt || null } : op === "repair" ? { op: "repair", quick, extended, use_frm: useFrm } : ({ op } as Maintenance);
  const run = async () => {
    setConfirm(false); setRunning(true); setError(null);
    try { setRows(await serverApi.maintain(id, db, [...picked], operation())); } catch (e) { setError(String(e)); }
    setRunning(false);
  };
  const needsConfirm = op === "optimize" || op === "repair";

  return (
    <div className="maint">
      <div className="dg-toolbar">
        <label>Database <Select value={db} onChange={(e) => setDb(e.target.value)}>{dbs.map((d) => <option key={d}>{d}</option>)}</Select></label>
        <label>Operation <Select value={op} onChange={(e) => setOp(e.target.value as Maintenance["op"])}>
          <option value="check">CHECK TABLE</option><option value="analyze">ANALYZE TABLE</option><option value="optimize">OPTIMIZE TABLE</option><option value="repair">REPAIR TABLE</option></Select></label>
        {op === "check" && <Select value={checkOpt} onChange={(e) => setCheckOpt(e.target.value as CheckOption | "")}><option value="">(default)</option>{["quick", "fast", "changed", "medium", "extended"].map((o) => <option key={o} value={o}>{o.toUpperCase()}</option>)}</Select>}
        {op === "repair" && <>
          <label className="check"><input type="checkbox" checked={quick} onChange={(e) => setQuick(e.target.checked)} /> QUICK</label>
          <label className="check"><input type="checkbox" checked={extended} onChange={(e) => setExtended(e.target.checked)} /> EXTENDED</label>
          <label className="check"><input type="checkbox" checked={useFrm} onChange={(e) => setUseFrm(e.target.checked)} /> USE_FRM</label></>}
        <span className="grow" />
        <button className="primary" disabled={picked.size === 0 || running} onClick={() => (needsConfirm ? setConfirm(true) : run())}>{running ? "Running…" : `Run on ${picked.size} table(s)`}</button>
      </div>
      {error && <div className="banner bad">{error}</div>}
      <div className="st-split">
        <div className="dg-scroll tbl-pick">
          <label className="check all"><input type="checkbox" checked={tables.length > 0 && picked.size === tables.length} onChange={(e) => setPicked(e.target.checked ? new Set(tables) : new Set())} /> <b>All tables ({tables.length})</b></label>
          {tables.map((t) => (
            <label key={t} className="check"><input type="checkbox" checked={picked.has(t)} onChange={(e) => { const n = new Set(picked); e.target.checked ? n.add(t) : n.delete(t); setPicked(n); }} /> {t}</label>
          ))}
        </div>
        <div className="dg-scroll maint-result">
          {rows === null ? <div className="empty">Choose tables and an operation</div> : (
            <table className="grid">
              <thead><tr><th>Table</th><th>Operation</th><th>Type</th><th>Message</th></tr></thead>
              <tbody>{rows.map((r, i) => (
                <tr key={i} className={r.msg_type === "status" && r.msg_text === "OK" ? "ok" : /error/i.test(r.msg_type) ? "bad" : /warning|note/i.test(r.msg_type) ? "warn" : ""}>
                  <td>{r.table}</td><td>{r.op}</td><td>{r.msg_type}</td><td>{r.msg_text}</td></tr>))}</tbody>
            </table>
          )}
        </div>
      </div>
      {confirm && <ConfirmDialog message={op === "optimize" ? `OPTIMIZE rebuilds ${picked.size} table(s) and can lock them for a while on large tables. Continue?` : `REPAIR can drop rows it cannot recover${useFrm ? " and USE_FRM rebuilds the index from the table definition" : ""}. Make sure you have a backup. Continue?`}
        confirmLabel={op === "optimize" ? "Optimize" : "Repair"} onCancel={() => setConfirm(false)} onConfirm={run} />}
    </div>
  );
}

// ---------------------------------------------------------------- logs

function Logs({ id }: { id: number }) {
  const [sub, setSub] = useState<"error" | "slow" | "binary" | "settings">("error");
  const [settings, setSettings] = useState<LogSettings | null>(null);
  const reloadSettings = useCallback(() => serverApi.logSettings(id).then(setSettings).catch(() => {}), [id]);
  useEffect(() => { reloadSettings(); }, [reloadSettings]);
  return (
    <div className="logs">
      <div className="result-tabs sub">
        {([["error", "Error log"], ["slow", "Slow queries"], ["binary", "Binary logs"], ["settings", "Settings"]] as const).map(([t, l]) => <button key={t} className={sub === t ? "active" : ""} onClick={() => setSub(t)}>{l}</button>)}
      </div>
      {sub === "error" && <ErrorLog id={id} />}
      {sub === "slow" && <SlowLog id={id} />}
      {sub === "binary" && <BinaryLogs id={id} />}
      {sub === "settings" && <LogSettingsView id={id} s={settings} reload={reloadSettings} />}
    </div>
  );
}

function ErrorLog({ id }: { id: number }) {
  const [levels, setLevels] = useState<string[]>(["System", "Error", "Warning"]);
  const [search, setSearch] = useState("");
  const [lines, setLines] = useState<LogLine[] | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const load = useCallback(async () => {
    try { setLines(await serverApi.errorLog(id, 300, levels, search)); setNote(null); } catch (e) { setLines([]); setNote(String(e)); }
  }, [id, levels, search]);
  useEffect(() => { const t = setTimeout(load, 250); return () => clearTimeout(t); }, [load]);
  return (
    <div className="vars">
      <div className="dg-toolbar">
        {["System", "Error", "Warning", "Note"].map((l) => <label key={l} className="check"><input type="checkbox" checked={levels.includes(l)} onChange={(e) => setLevels(e.target.checked ? [...levels, l] : levels.filter((x) => x !== l))} /> {l}</label>)}
        <input placeholder="Search text…" value={search} onChange={(e) => setSearch(e.target.value)} /><button onClick={load}>⟳</button>
      </div>
      {note && <div className="banner">{note}</div>}
      <div className="dg-scroll">
        <table className="grid logtable"><thead><tr><th>Time</th><th>Level</th><th>Source</th><th>Message</th></tr></thead>
          <tbody>{(lines ?? []).map((l, i) => <tr key={i} className={`lvl-${l.level.toLowerCase()}`}><td>{l.time}</td><td>{l.level}</td><td>{l.source}</td><td className="msg">{l.text}</td></tr>)}</tbody></table>
        {lines?.length === 0 && !note && <div className="empty">No log entries match</div>}
      </div>
    </div>
  );
}

function SlowLog({ id }: { id: number }) {
  const [search, setSearch] = useState("");
  const [rows, setRows] = useState<SlowQuery[] | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const load = useCallback(async () => { try { setRows(await serverApi.slowLog(id, 200, search)); setNote(null); } catch (e) { setRows([]); setNote(String(e)); } }, [id, search]);
  useEffect(() => { const t = setTimeout(load, 250); return () => clearTimeout(t); }, [load]);
  return (
    <div className="vars">
      <div className="dg-toolbar"><input placeholder="Search SQL…" value={search} onChange={(e) => setSearch(e.target.value)} /><button onClick={load}>⟳</button></div>
      {note && <div className="banner">{note}</div>}
      <div className="dg-scroll">
        <table className="grid logtable"><thead><tr><th>Started</th><th>Time</th><th>Lock</th><th>Sent</th><th>Examined</th><th>DB</th><th>User</th><th>SQL</th></tr></thead>
          <tbody>{(rows ?? []).map((r, i) => <tr key={i}><td>{r.start_time}</td><td className="num">{r.query_time}</td><td className="num">{r.lock_time}</td><td className="num">{r.rows_sent}</td><td className="num">{r.rows_examined}</td><td>{r.db}</td><td>{r.user_host}</td><td className="msg">{r.sql_text}</td></tr>)}</tbody></table>
        {rows?.length === 0 && !note && <div className="empty">No slow queries recorded</div>}
      </div>
    </div>
  );
}

function BinaryLogs({ id }: { id: number }) {
  const [files, setFiles] = useState<BinaryLog[] | null>(null);
  const [file, setFile] = useState<string | null>(null);
  const [from, setFrom] = useState(4);
  const [events, setEvents] = useState<BinlogEvent[]>([]);
  const [note, setNote] = useState<string | null>(null);
  useEffect(() => { serverApi.binaryLogs(id).then((f) => { setFiles(f); setFile(f.at(-1)?.name ?? null); }).catch((e) => { setFiles([]); setNote(String(e)); }); }, [id]);
  useEffect(() => { if (file) serverApi.binlogEvents(id, file, from, 200).then((e) => { setEvents(e); setNote(null); }).catch((e) => setNote(String(e))); }, [id, file, from]);
  return (
    <div className="vars">
      {note && <div className="banner">{note}</div>}
      <div className="st-split">
        <div className="dg-scroll tbl-pick">
          {(files ?? []).map((f) => <div key={f.name} className={`dz-item ${f.name === file ? "active" : ""}`} onClick={() => { setFile(f.name); setFrom(4); }}>{f.name}<div className="dim small">{fmtBytes(f.size)}</div></div>)}
        </div>
        <div className="dg-scroll">
          <table className="grid logtable"><thead><tr><th>Pos</th><th>Event</th><th>End</th><th>Info</th></tr></thead>
            <tbody>{events.map((e) => <tr key={e.pos}><td className="num">{e.pos}</td><td>{e.event_type}</td><td className="num">{e.end_pos}</td><td className="msg">{e.info}</td></tr>)}</tbody></table>
          {events.length >= 200 && <button className="link" onClick={() => setFrom(events.at(-1)!.end_pos)}>Next 200 events →</button>}
        </div>
      </div>
    </div>
  );
}

function LogSettingsView({ id, s, reload }: { id: number; s: LogSettings | null; reload(): void }) {
  const [confirm, setConfirm] = useState<{ name: string; value: string } | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  if (!s) return <div className="empty">Loading…</div>;
  const row = (label: string, value: string, name?: string, toggleTo?: string) => (
    <tr><td>{label}</td><td className="val">{value}</td><td>{name && <button className="mini" onClick={() => setConfirm({ name, value: toggleTo! })}>{toggleTo === "ON" || toggleTo === "TABLE" ? "Enable" : toggleTo === "OFF" ? "Disable" : "Change"}…</button>}</td></tr>
  );
  return (
    <div className="vars">
      {msg && <div className="banner ok">{msg}</div>}
      <table className="grid">
        <tbody>
          {row("Slow query log", s.slow_query_log ? "ON" : "OFF", "slow_query_log", s.slow_query_log ? "OFF" : "ON")}
          {row("Long query time (s)", s.long_query_time)}
          {row("Log queries not using indexes", s.log_queries_not_using_indexes ? "ON" : "OFF", "log_queries_not_using_indexes", s.log_queries_not_using_indexes ? "OFF" : "ON")}
          {row("General log", s.general_log ? "ON" : "OFF", "general_log", s.general_log ? "OFF" : "ON")}
          {row("Log output", s.log_output, "log_output", s.log_output.toUpperCase().includes("TABLE") ? "FILE" : "TABLE")}
          {row("Binary logging", s.log_bin ? "ON" : "OFF")}
          {row("Error log file", s.log_error ?? "")}
          {row("Slow query log file", s.slow_query_log_file ?? "")}
          {row("General log file", s.general_log_file ?? "")}
          {row("Error log readable here", s.error_log_in_table ? "yes (performance_schema.error_log)" : "no (read the file on the server)")}
        </tbody>
      </table>
      <div className="dim small note">Slow queries appear in this app only when Log output includes TABLE. Settings are changed with SET GLOBAL and last until the server restarts.</div>
      {confirm && <ConfirmDialog message={`Run  SET GLOBAL ${confirm.name} = ${confirm.value}  ?`} confirmLabel="Change" onCancel={() => setConfirm(null)}
        onConfirm={async () => { const c = confirm; setConfirm(null); try { const w = await serverApi.setGlobal(id, c.name, c.value); setMsg(`${c.name} = ${c.value}${w.length ? " — " + w.join("; ") : ""}`); reload(); } catch (e) { setMsg(String(e)); } }} />}
    </div>
  );
}
