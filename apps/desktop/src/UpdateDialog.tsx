import { useState } from "react";
import { relaunch } from "@tauri-apps/plugin-process";
import { type Update } from "@tauri-apps/plugin-updater";
import Icon from "./Icon";

/** The release notes of an update: the changelog section of the new version (`**Heading**` lines, `* ` bullets, `**bold**` and `code`). */
function Notes(props: { text: string }) {
  const inline = (s: string) =>
    s.split(/(\*\*[^*]+\*\*|`[^`]+`)/g).map((part, i) =>
      part.startsWith("**") ? <b key={i}>{part.slice(2, -2)}</b> : part.startsWith("`") ? <code key={i}>{part.slice(1, -1)}</code> : part);
  const lines = props.text.split("\n").map((l) => l.trimEnd()).filter((l) => l.trim() && !/^#{1,6}\s/.test(l));
  const out: React.ReactNode[] = [];
  let bullets: string[] = [];
  const flush = () => { if (bullets.length) out.push(<ul key={`u${out.length}`}>{bullets.map((b, i) => <li key={i}>{inline(b)}</li>)}</ul>); bullets = []; };
  for (const l of lines) {
    if (/^\s*[*-]\s+/.test(l)) bullets.push(l.replace(/^\s*[*-]\s+/, ""));
    else { flush(); out.push(/^\*\*[^*]+\*\*$/.test(l.trim()) ? <h4 key={`h${out.length}`}>{l.trim().slice(2, -2)}</h4> : <p key={`p${out.length}`}>{inline(l)}</p>); }
  }
  flush();
  return <div className="update-notes">{out}</div>;
}

type Phase = { kind: "ask" } | { kind: "download"; done: number; total: number | null } | { kind: "install" } | { kind: "error"; message: string };

const mb = (n: number) => `${(n / 1048576).toFixed(1)} MB`;

/** "A new version is available": what is new, then Update now (download, replace this copy, restart), Later, or Skip this version. */
export default function UpdateDialog(props: { update: Update; onLater(): void; onSkip(): void }) {
  const { update } = props;
  const [phase, setPhase] = useState<Phase>({ kind: "ask" });
  const busy = phase.kind === "download" || phase.kind === "install";

  const go = async () => {
    let done = 0;
    setPhase({ kind: "download", done: 0, total: null });
    try {
      await update.download((e) => {
        if (e.event === "Started") setPhase({ kind: "download", done: 0, total: e.data.contentLength ?? null });
        else if (e.event === "Progress") { done += e.data.chunkLength; setPhase((p) => (p.kind === "download" ? { ...p, done } : p)); }
      });
      setPhase({ kind: "install" });
      await update.install();
      await relaunch();
    } catch (e) {
      setPhase({ kind: "error", message: typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e) });
    }
  };

  return (
    <div className="overlay" onMouseDown={() => !busy && props.onLater()}>
      <div className="dialog update-dialog" role="dialog" aria-label="Update available" onMouseDown={(e) => e.stopPropagation()}>
        <h3><Icon name="info" size={13} /> MyLoDB {update.version} is available</h3>
        <div className="dim">You have version {update.currentVersion}.{update.date ? ` Released ${update.date.slice(0, 10)}.` : ""}</div>
        {update.body?.trim() ? <><div className="update-head">What is new</div><Notes text={update.body} /></> : <div className="dim">No release notes were published for this version.</div>}
        {phase.kind === "download" && (
          <div className="job-progress" role="progressbar" aria-label="Downloading the update">
            <div className="bar"><div className={phase.total ? "" : "indeterminate"} style={phase.total ? { width: `${Math.min(100, (phase.done / phase.total) * 100)}%` } : undefined} /></div>
            <div className="dim small">Downloading{phase.total ? ` ${mb(phase.done)} of ${mb(phase.total)}` : ` ${mb(phase.done)}`}…</div>
          </div>
        )}
        {phase.kind === "install" && <div className="dim">Installing, then MyLoDB restarts…</div>}
        {phase.kind === "error" && <div className="banner bad">The update failed: {phase.message}. Nothing was changed; you can try again or download it from the Releases page.</div>}
        <div className="actions">
          <button disabled={busy} onClick={props.onSkip} title="Do not offer this version again">Skip this version</button>
          <button disabled={busy} onClick={props.onLater}>Later</button>
          <button className="primary" disabled={busy} onClick={go}>{phase.kind === "error" ? "Try again" : "Update now"}</button>
        </div>
        <div className="dim small">Update now downloads the new version, replaces this copy of MyLoDB and restarts it. Your connections and settings are kept. Open tabs are restored.</div>
      </div>
    </div>
  );
}
