import { useCallback, useRef, useState, useSyncExternalStore } from "react";
import { onJobProgress, transferApi, type JobProgress } from "./api";

// ---- the Tasks list: every long operation of the window (export, import, backup, run script), running or finished ----
export interface Task {
  id: string; label: string; state: "running" | "done" | "failed" | "cancelled";
  progress: JobProgress | null; startedAt: number; endedAt: number | null; detail: string | null;
}
let tasks: Task[] = [];
const listeners = new Set<() => void>();
const emit = () => { tasks = [...tasks]; listeners.forEach((l) => l()); };
const patchTask = (id: string, patch: Partial<Task>) => { const t = tasks.find((x) => x.id === id); if (t) { Object.assign(t, patch); emit(); } };
export const taskStore = {
  subscribe: (l: () => void) => { listeners.add(l); return () => { listeners.delete(l); }; },
  snapshot: () => tasks,
  add: (t: Task) => { tasks = [t, ...tasks].slice(0, 50); listeners.forEach((l) => l()); },
  clearFinished: () => { tasks = tasks.filter((t) => t.state === "running"); listeners.forEach((l) => l()); },
};
export const useTasks = () => useSyncExternalStore(taskStore.subscribe, taskStore.snapshot);

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

  const run = useCallback(async (fn: (jobId: string) => Promise<T>, label = "Task") => {
    const id = newJobId();
    taskStore.add({ id, label, state: "running", progress: null, startedAt: Date.now(), endedAt: null, detail: null });
    jobId.current = id;
    setRunning(true); setProgress(null); setResult(null); setError(null); setCancelled(false);
    const stop = await onJobProgress(id, (p) => { setProgress(p); patchTask(id, { progress: p }); });
    try {
      const r = await fn(id);
      setResult(r);
      patchTask(id, { state: "done", endedAt: Date.now() });
    } catch (e) {
      const msg = String(e);
      if (msg === "cancelled") { setCancelled(true); patchTask(id, { state: "cancelled", endedAt: Date.now() }); }
      else { setError(msg); patchTask(id, { state: "failed", endedAt: Date.now(), detail: msg }); }
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

