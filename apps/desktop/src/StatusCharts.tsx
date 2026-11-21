import { useMemo, useRef, useState } from "react";
import { serverApi } from "./api";
import { usePoll } from "./usePoll";

type Kind = "rate" | "gauge";
interface SeriesDef { label: string; key: string; kind: Kind; color: string }
interface ChartDef { id: string; title: string; unit: "" | "bytes" | "%"; series: SeriesDef[] }

const COLORS = ["#4f8cff", "#4cc38a", "#e0a03b", "#d9534f", "#9b6cf0"];
const s = (label: string, key: string, kind: Kind, i: number): SeriesDef => ({ label, key, kind, color: COLORS[i] });

/** What is drawn: counters become per-second rates between two samples, gauges are plotted as they are. */
export const CHARTS: ChartDef[] = [
  { id: "qps", title: "Queries per second", unit: "", series: [s("Questions", "Questions", "rate", 0)] },
  { id: "statements", title: "Statements per second", unit: "", series: [s("SELECT", "Com_select", "rate", 0), s("INSERT", "Com_insert", "rate", 1), s("UPDATE", "Com_update", "rate", 2), s("DELETE", "Com_delete", "rate", 3)] },
  { id: "conns", title: "Connections", unit: "", series: [s("Connected", "Threads_connected", "gauge", 0), s("Running", "Threads_running", "gauge", 1)] },
  { id: "net", title: "Network per second", unit: "bytes", series: [s("Received", "Bytes_received", "rate", 0), s("Sent", "Bytes_sent", "rate", 1)] },
  { id: "rows", title: "InnoDB rows per second", unit: "", series: [s("Read", "Innodb_rows_read", "rate", 0), s("Inserted", "Innodb_rows_inserted", "rate", 1), s("Updated", "Innodb_rows_updated", "rate", 2), s("Deleted", "Innodb_rows_deleted", "rate", 3)] },
  { id: "slow", title: "Slow queries per second", unit: "", series: [s("Slow", "Slow_queries", "rate", 3)] },
];

/** The per-second rate of a counter between two samples; null when it cannot be known (first sample, no time passed, or the counter went back after a restart). */
export function rateBetween(prev: number | undefined, now: number | undefined, seconds: number): number | null {
  if (prev === undefined || now === undefined || !(seconds > 0.2) || now < prev) return null;
  return (now - prev) / seconds;
}

const fmt = (v: number, unit: ChartDef["unit"]) => {
  if (unit === "bytes") return v >= 1 << 20 ? `${(v / (1 << 20)).toFixed(1)} MB` : v >= 1024 ? `${(v / 1024).toFixed(1)} KB` : `${Math.round(v)} B`;
  return v >= 1000 ? `${(v / 1000).toFixed(1)}k` : v >= 10 || Number.isInteger(v) ? `${Math.round(v)}` : v.toFixed(1);
};

const POINTS = 150; // 5 minutes at 2 s

function Chart({ def, data, seconds }: { def: ChartDef; data: Record<string, (number | null)[]>; seconds: number }) {
  const present = def.series.filter((x) => data[x.key]?.some((v) => v !== null));
  const [hover, setHover] = useState<number | null>(null);
  const W = 560, H = 130, L = 46, R = 8, T = 8, B = 18;
  const all = present.flatMap((x) => data[x.key].filter((v): v is number => v !== null));
  const max = Math.max(1e-9, ...all) * 1.1;
  const x = (i: number, len: number) => L + ((POINTS - len + i) / (POINTS - 1)) * (W - L - R);
  const y = (v: number) => T + (1 - v / max) * (H - T - B);
  return (
    <div className="chart" data-chart={def.id}>
      <h4>{def.title}</h4>
      {present.length === 0 ? <div className="dim small">This server does not report {def.series.map((x) => x.key).join(", ")}.</div> : (
        <>
          <svg viewBox={`0 0 ${W} ${H}`} width="100%" onMouseLeave={() => setHover(null)}
               onMouseMove={(e) => { const r = e.currentTarget.getBoundingClientRect(); const rel = ((e.clientX - r.left) / r.width) * W; const len = Math.max(...present.map((p) => data[p.key].length)); const i = Math.round(((rel - L) / (W - L - R)) * (POINTS - 1)) - (POINTS - len); setHover(i >= 0 && i < len ? i : null); }}>
            {[0, 0.5, 1].map((f) => <g key={f}><line x1={L} x2={W - R} y1={y(max * f / 1.1)} y2={y(max * f / 1.1)} className="chart-grid" /><text x={L - 4} y={y(max * f / 1.1) + 3} textAnchor="end" className="chart-axis">{fmt(max / 1.1 * f, def.unit)}</text></g>)}
            <text x={L} y={H - 3} className="chart-axis">−{Math.round(POINTS * seconds / 60 * 10) / 10} min</text>
            <text x={W - R} y={H - 3} textAnchor="end" className="chart-axis">now</text>
            {present.map((p) => {
              const vals = data[p.key];
              const pts = vals.map((v, i) => (v === null ? null : `${x(i, vals.length)},${y(v)}`)).filter(Boolean).join(" ");
              return <polyline key={p.key} points={pts} fill="none" stroke={p.color} strokeWidth="1.6" data-series={p.key} />;
            })}
            {hover !== null && <line x1={x(hover, Math.max(...present.map((p) => data[p.key].length)))} x2={x(hover, Math.max(...present.map((p) => data[p.key].length)))} y1={T} y2={H - B} className="chart-cursor" />}
          </svg>
          <div className="chart-legend">
            {present.map((p) => {
              const vals = data[p.key];
              const v = hover !== null ? vals[hover - (Math.max(...present.map((q) => data[q.key].length)) - vals.length)] : vals[vals.length - 1];
              return <span key={p.key}><i style={{ background: p.color }} /> {p.label} <b data-legend={p.key}>{v === null || v === undefined ? "–" : fmt(v, def.unit)}</b></span>;
            })}
          </div>
        </>
      )}
    </div>
  );
}

/** Server charts: the status counters sampled every few seconds and drawn as lines. Pause, change the interval, or clear the history. */
export default function StatusCharts({ id }: { id: number }) {
  const [every, setEvery] = useState(2000);
  const [paused, setPaused] = useState(false);
  const [data, setData] = useState<Record<string, (number | null)[]>>({});
  const [error, setError] = useState<string | null>(null);
  const last = useRef<{ t: number; s: Record<string, number> } | null>(null);

  usePoll(async () => {
    let rows: [string, string][];
    try { rows = await serverApi.status(id, "global"); setError(null); } catch (e) { setError(String(e)); return; }
    const cur: Record<string, number> = {};
    rows.forEach(([k, v]) => { const n = Number(v); if (!Number.isNaN(n)) cur[k] = n; });
    const now = performance.now();
    const prev = last.current;
    if (prev && now - prev.t < 200) return; // two polls too close together give a meaningless rate
    last.current = { t: now, s: cur };
    if (!prev) return;
    const seconds = (now - prev.t) / 1000;
    setData((d) => {
      const next: Record<string, (number | null)[]> = { ...d };
      for (const c of CHARTS) for (const sr of c.series) {
        if (cur[sr.key] === undefined) continue;
        const v = sr.kind === "gauge" ? cur[sr.key] : rateBetween(prev.s[sr.key], cur[sr.key], seconds);
        next[sr.key] = [...(d[sr.key] ?? []), v].slice(-POINTS);
      }
      return next;
    });
  }, every, !paused);

  const samples = useMemo(() => Math.max(0, ...Object.values(data).map((v) => v.length)), [data]);
  return (
    <div className="charts">
      <div className="chart-bar">
        <button onClick={() => setPaused((p) => !p)}>{paused ? "Resume" : "Pause"}</button>
        <button onClick={() => { setData({}); last.current = null; }}>Clear</button>
        <label>Every <select aria-label="Sampling interval" value={every} onChange={(e) => { setEvery(Number(e.target.value)); setData({}); last.current = null; }}>{[1000, 2000, 5000, 10000].map((m) => <option key={m} value={m}>{m / 1000} s</option>)}</select></label>
        <span className="dim small" aria-label="Samples">{paused ? "paused · " : ""}{samples} sample{samples === 1 ? "" : "s"} · rates are per second between samples</span>
      </div>
      {error && <div className="error">{error}</div>}
      {samples === 0 && !error && <div className="dim">Collecting the first samples…</div>}
      <div className="chart-grid-layout">{CHARTS.map((c) => <Chart key={c.id} def={c} data={data} seconds={every / 1000} />)}</div>
    </div>
  );
}
