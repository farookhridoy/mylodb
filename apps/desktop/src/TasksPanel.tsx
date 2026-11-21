import { useEffect, useState } from "react";
import Icon from "./Icon";
import { taskStore, useTasks, type Task } from "./Jobs";
import { transferApi } from "./api";

const ago = (t: Task) => {
  const sec = Math.max(0, Math.round(((t.endedAt ?? Date.now()) - t.startedAt) / 1000));
  return sec < 60 ? `${sec} s` : `${Math.floor(sec / 60)} min ${sec % 60} s`;
};

/** The footer's Tasks button and its list: every export, import, backup and script run, with progress, Cancel and the outcome. */
export default function TasksPanel() {
  const tasks = useTasks();
  const [open, setOpen] = useState(false);
  const [, tick] = useState(0);
  const running = tasks.filter((t) => t.state === "running").length;
  useEffect(() => { if (!running) return; const h = setInterval(() => tick((n) => n + 1), 1000); return () => clearInterval(h); }, [running]);
  return (
    <div className="tasks">
      <button className={`tasks-btn ${running ? "busy" : ""}`} aria-expanded={open} title="Long-running work: exports, imports, backups, scripts" onClick={() => setOpen((v) => !v)}>
        <Icon name="clock" size={12} /> Tasks{running ? ` (${running} running)` : tasks.length ? ` (${tasks.length})` : ""}
      </button>
      {open && (
        <div className="tasks-pop" role="dialog" aria-label="Tasks">
          <div className="tasks-head"><b>Tasks</b><span className="grow" />
            <button disabled={tasks.every((t) => t.state === "running")} onClick={() => taskStore.clearFinished()}>Clear finished</button>
            <button aria-label="Close tasks" onClick={() => setOpen(false)}><Icon name="close" size={11} /></button></div>
          {tasks.length === 0 && <div className="empty small">Nothing has run yet. Exports, imports, backups and scripts show up here.</div>}
          <ul>
            {tasks.map((t) => {
              const pct = t.progress?.total ? Math.min(100, (t.progress.done / t.progress.total) * 100) : null;
              return (
                <li key={t.id} className={`task ${t.state}`}>
                  <div className="task-line"><Icon name={t.state === "done" ? "check" : t.state === "running" ? "play" : "warn"} size={12} /><span className="task-label">{t.label}</span>
                    <span className="grow" /><span className="dim small">{t.state === "running" ? ago(t) : `${t.state} · ${ago(t)}`}</span>
                    {t.state === "running" && <button onClick={() => transferApi.cancel(t.id)}>Cancel</button>}</div>
                  {t.state === "running" && (<>
                    <div className="bar"><div style={{ width: pct === null ? "30%" : `${pct}%` }} className={pct === null ? "indeterminate" : ""} /></div>
                    <div className="dim small">{t.progress ? `${t.progress.phase} · ${t.progress.message}` : "Starting…"}</div></>)}
                  {t.detail && <div className="error small">{t.detail}</div>}
                </li>
              );
            })}
          </ul>
        </div>
      )}
    </div>
  );
}
