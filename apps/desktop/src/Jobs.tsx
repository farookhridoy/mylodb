import { useCallback, useRef, useState } from "react";
import { onJobProgress, transferApi, type JobProgress } from "./api";

let jobSeq = 0;
export const newJobId = () => `job-${Date.now().toString(36)}-${jobSeq++}`;

/** Run one long operation: tracks its id, live progress, and lets the user cancel it. */
export function useJob<T>() {
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<JobProgress | null>(null);
  const [result, setResult] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [cancelled, setCancelled] = useState(false);
  const jobId = useRef<string | null>(null);

  const run = useCallback(async (fn: (jobId: string) => Promise<T>) => {
    const id = newJobId();
    jobId.current = id;
    setRunning(true); setProgress(null); setResult(null); setError(null); setCancelled(false);
    const stop = await onJobProgress(id, setProgress);
    try {
      setResult(await fn(id));
    } catch (e) {
      const msg = String(e);
      if (msg === "cancelled") setCancelled(true); else setError(msg);
    } finally {
      stop();
      setRunning(false);
      jobId.current = null;
    }
  }, []);
  const cancel = () => { if (jobId.current) transferApi.cancel(jobId.current); };
  return { running, progress, result, error, cancelled, run, cancel };
}

export function ProgressBar({ p, running, onCancel }: { p: JobProgress | null; running: boolean; onCancel(): void }) {
  if (!running) return null;
  const pct = p?.total ? Math.min(100, (p.done / p.total) * 100) : null;
  return (
    <div className="job-progress">
      <div className="bar"><div style={{ width: pct === null ? "30%" : `${pct}%` }} className={pct === null ? "indeterminate" : ""} /></div>
      <div className="job-line"><span>{p ? `${p.phase}${pct !== null ? ` · ${pct.toFixed(0)}%` : ""} · ${p.message}` : "Starting…"}</span><span className="grow" /><button onClick={onCancel}>Cancel</button></div>
    </div>
  );
}

