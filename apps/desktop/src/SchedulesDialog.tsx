import { useCallback, useEffect, useState } from "react";
import { open as pickPath } from "@tauri-apps/plugin-dialog";
import { plainError, scheduleApi, type Schedule, type ScheduleRule } from "./api";
import Icon from "./Icon";

const DAYS = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const two = (n: number) => String(n).padStart(2, "0");

const describe = (r: ScheduleRule) =>
  r.kind === "hours" ? `every ${r.hours} hour${r.hours === 1 ? "" : "s"}` : r.kind === "daily" ? `every day at ${two(r.hour)}:${two(r.minute)}` : `every ${DAYS[r.weekday]} at ${two(r.hour)}:${two(r.minute)}`;

function blank(connectionId: number, databases: string[]): Schedule {
  return { id: "", name: "", connection_id: connectionId, databases: databases.slice(0, 1), dir: "", rule: { kind: "daily", hour: 3, minute: 0 }, keep: 7, compress: true, data: true, enabled: true, created: "", last_run: null, log: [] };
}

/** The "when" part of the form: how often, on which day, at what time. */
function RuleFields({ rule, onChange }: { rule: ScheduleRule; onChange(r: ScheduleRule): void }) {
  return (
    <>
      <label>When
        <select value={rule.kind} aria-label="When" onChange={(e) => onChange(e.target.value === "hours" ? { kind: "hours", hours: 6 } : e.target.value === "daily" ? { kind: "daily", hour: 3, minute: 0 } : { kind: "weekly", weekday: 0, hour: 3, minute: 0 })}>
          <option value="daily">Every day</option><option value="weekly">Every week</option><option value="hours">Every few hours</option></select></label>
      {rule.kind === "hours" && <label>Every <input type="number" min={1} max={720} value={rule.hours} onChange={(e) => onChange({ kind: "hours", hours: Number(e.target.value) })} /> hours</label>}
      {rule.kind === "weekly" && <label>On <select value={rule.weekday} onChange={(e) => onChange({ ...rule, weekday: Number(e.target.value) })} aria-label="Day">{DAYS.map((d, i) => <option key={d} value={i}>{d}</option>)}</select></label>}
      {rule.kind !== "hours" && (
        <label>At <input type="time" value={`${two(rule.hour)}:${two(rule.minute)}`} onChange={(e) => { const [h, m] = e.target.value.split(":").map(Number); if (!Number.isNaN(h)) onChange({ ...rule, hour: h, minute: m }); }} /></label>
      )}
    </>
  );
}

/** Scheduled backups: a list of jobs, each with when it runs, where the file goes, how many to keep, and its log. They run while MyLoDB is open. */
export default function SchedulesDialog(props: { connectionId: number; connectionName: string; databases: string[]; onClose(): void }) {
  const [jobs, setJobs] = useState<Schedule[]>([]);
  const [edit, setEdit] = useState<Schedule | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const refresh = useCallback(() => scheduleApi.list().then(setJobs).catch((e) => setError(plainError(e))), []);
  useEffect(() => { refresh(); const t = setInterval(refresh, 5000); return () => clearInterval(t); }, [refresh]);
  useEffect(() => {
    const on = (e: KeyboardEvent) => { if (e.key === "Escape") { e.stopPropagation(); if (edit) setEdit(null); else props.onClose(); } };
    window.addEventListener("keydown", on, true);
    return () => window.removeEventListener("keydown", on, true);
  }, [props, edit]);

  const save = async () => {
    if (!edit) return;
    try { setJobs(await scheduleApi.save(edit)); setEdit(null); setError(null); } catch (e) { setError(plainError(e)); }
  };
  const run = async (j: Schedule) => {
    setBusy(j.id); setError(null);
    try { await scheduleApi.runNow(j.id); } catch (e) { setError(plainError(e)); }
    setBusy(null); refresh();
  };
  const remove = async (j: Schedule) => { try { setJobs(await scheduleApi.remove(j.id)); } catch (e) { setError(plainError(e)); } };
  const toggle = async (j: Schedule) => { try { setJobs(await scheduleApi.save({ ...j, enabled: !j.enabled })); } catch (e) { setError(plainError(e)); } };
  const chooseDir = async () => { const p = await pickPath({ directory: true, multiple: false }); if (typeof p === "string" && edit) setEdit({ ...edit, dir: p }); };
  const setRule = (r: ScheduleRule) => edit && setEdit({ ...edit, rule: r });

  return (
    <div className="overlay" onMouseDown={props.onClose}>
      <div className="dialog schedules-dialog" role="dialog" aria-label="Scheduled backups" onMouseDown={(e) => e.stopPropagation()}>
        <h3>Scheduled backups</h3>
        <p className="dim small">Jobs run while MyLoDB is open, one SQL dump each time, in the folder you choose. Older backups of a job beyond the limit are deleted. If the app was closed at the time, the missed run happens when you open it again.</p>
        {error && <div className="banner bad" role="alert">{error}</div>}
        {!edit ? (
          <>
            {jobs.length === 0 && <div className="empty">No scheduled backups yet.</div>}
            <ul className="sched-list">
              {jobs.map((j) => (
                <li key={j.id} className={j.enabled ? "" : "off"}>
                  <div className="sched-row">
                    <div className="sched-main">
                      <b>{j.name}</b> <span className="dim small">{j.databases.join(", ")} · {describe(j.rule)} · keep {j.keep}</span>
                      <div className="dim small">{j.running ? "Running now…" : j.enabled ? (j.next_run ? `Next: ${j.next_run}` : "") : "Paused"}{j.last_run ? ` · Last: ${j.last_run}` : ""}</div>
                      <div className="dim small sched-dir" title={j.dir}>{j.dir}</div>
                    </div>
                    <div className="sched-actions">
                      <button onClick={() => run(j)} disabled={busy === j.id || j.running} title="Run it now">{busy === j.id || j.running ? "Running…" : "Run now"}</button>
                      <button onClick={() => setEdit({ ...j })}>Edit</button>
                      <button onClick={() => toggle(j)}>{j.enabled ? "Pause" : "Resume"}</button>
                      <button onClick={() => setOpen(open === j.id ? null : j.id)} aria-expanded={open === j.id}>Log ({j.log.length})</button>
                      <button className="danger" onClick={() => remove(j)} title="Delete the job (the backup files stay)"><Icon name="trash" size={13} /></button>
                    </div>
                  </div>
                  {open === j.id && (
                    <ul className="sched-log">{j.log.length === 0 && <li className="dim">Nothing has run yet.</li>}
                      {j.log.map((l, i) => <li key={i} className={l.ok ? "ok" : "bad"}><span className="dim">{l.time}</span> {l.ok ? "✓" : "✗"} {l.message}</li>)}</ul>
                  )}
                </li>
              ))}
            </ul>
            <div className="actions">
              <button onClick={props.onClose}>Close</button>
              <button className="primary" onClick={() => setEdit(blank(props.connectionId, props.databases))}>New scheduled backup…</button>
            </div>
          </>
        ) : (
          <form className="sched-form" onSubmit={(e) => { e.preventDefault(); save(); }}>
            <label>Name<input value={edit.name} onChange={(e) => setEdit({ ...edit, name: e.target.value })} placeholder="Nightly shop backup" autoFocus /></label>
            <p className="dim small">Connection: <b>{edit.connection_id === props.connectionId ? props.connectionName : `#${edit.connection_id}`}</b></p>
            <fieldset><legend>Databases</legend>
              <div className="sched-dbs">{props.databases.map((d) => (
                <label key={d} className="check"><input type="checkbox" checked={edit.databases.includes(d)} onChange={(e) => setEdit({ ...edit, databases: e.target.checked ? [...edit.databases, d] : edit.databases.filter((x) => x !== d) })} /> {d}</label>
              ))}</div>
            </fieldset>
            <RuleFields rule={edit.rule} onChange={setRule} />
            <label>Folder
              <span className="sched-dirpick"><input value={edit.dir} onChange={(e) => setEdit({ ...edit, dir: e.target.value })} placeholder="/Users/you/Backups" spellCheck={false} /><button type="button" onClick={chooseDir}>Choose…</button></span></label>
            <label>Keep the last <input type="number" min={1} max={999} value={edit.keep} onChange={(e) => setEdit({ ...edit, keep: Number(e.target.value) })} /> backups</label>
            <label className="check"><input type="checkbox" checked={edit.compress} onChange={(e) => setEdit({ ...edit, compress: e.target.checked })} /> Compress into a .zip</label>
            <label className="check"><input type="checkbox" checked={edit.data} onChange={(e) => setEdit({ ...edit, data: e.target.checked })} /> Include the rows (not only the structure)</label>
            <div className="actions"><button type="button" onClick={() => setEdit(null)}>Cancel</button><button className="primary" type="submit">Save</button></div>
          </form>
        )}
      </div>
    </div>
  );
}
