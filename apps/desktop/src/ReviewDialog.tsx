import { useState } from "react";
import Icon from "./Icon";
import type { Review } from "./api";
import { ENV_LABEL } from "./environment";

/**
 * "Review before running": the risky statements of a script, why they are risky, and how many rows the server expects them to touch.
 * On production the dialog asks for the word "production" before it enables the run button.
 */
export default function ReviewDialog({ review, onCancel, onRun }: { review: Review; onCancel(): void; onRun(): void }) {
  const production = review.environment === "production";
  const danger = review.items.some((i) => i.findings.some((f) => f.severity === "danger"));
  const [typed, setTyped] = useState("");
  const needsTyping = production && danger;
  const allowed = !needsTyping || typed.trim().toLowerCase() === "production";

  return (
    <div className="overlay">
      <div className="dialog review-dialog" role="alertdialog" aria-label="Review before running">
        <h3><Icon name="warn" /> Review before running <span className={`env-badge env-${review.environment}`}>{ENV_LABEL[review.environment]}</span></h3>
        <p className="dim">{review.items.length === 1 ? "This statement can destroy data." : `${review.items.length} statements can destroy data.`} Nothing has been sent to the server yet.</p>
        <ul className="risks">
          {review.items.map((it) => (
            <li key={it.index} className={it.findings.some((f) => f.severity === "danger") ? "danger" : "warning"}>
              <div className="risk-head">Statement {it.index + 1}{it.estimated_rows !== null && <span className="est" title="The optimizer's estimate from EXPLAIN, not a count"> · about {it.estimated_rows.toLocaleString()} row{it.estimated_rows === 1 ? "" : "s"}</span>}</div>
              <code className="risk-sql">{it.sql}</code>
              {it.findings.map((f) => <div key={f.reason} className="risk-why"><Icon name="warn" size={12} /> {f.reason}</div>)}
            </li>
          ))}
        </ul>
        {needsTyping && (
          <label><span>Type <b>production</b> to run it</span>
            <input autoFocus value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="production" aria-label="Type production to confirm" />
          </label>
        )}
        <div className="actions">
          <button autoFocus={!needsTyping} onClick={onCancel}>Cancel</button>
          <button className="danger" disabled={!allowed} onClick={onRun}>Run anyway</button>
        </div>
      </div>
    </div>
  );
}
