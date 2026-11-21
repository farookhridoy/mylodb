import { useState } from "react";
import Icon from "./Icon";
import { describeError, type Environment, type StatementResult } from "./api";
import { ENV_LABEL } from "./environment";

/** The plan as it will run: with checks off the foreign key switch is part of the text (the same shape as `myq_schema::plan::with_fk_checks`). */
export const withFkChecks = (statements: string[], checksOn: boolean) =>
  checksOn ? statements : ["SET FOREIGN_KEY_CHECKS = 0;", ...statements, "SET FOREIGN_KEY_CHECKS = 1;"];

/**
 * "Preview SQL, then run": every bulk or destructive operation of the UI ends here. It shows the exact statements (what you read is what runs),
 * a switch for foreign key checks, the environment, and on production asks for the word "production". Nothing is sent until Run.
 */
export default function PreviewDialog(props: {
  title: string;
  /** One line about the reach of the operation, e.g. "3 tables, about 1.2 million rows". */
  summary?: string;
  statements: string[];
  environment: Environment;
  /** Whether the switch is offered (not for operations where it makes no sense). */
  foreignKeySwitch?: boolean;
  confirmLabel?: string;
  onCancel(): void;
  onRun(statements: string[]): Promise<StatementResult[]>;
  /** Called once the run finished without error (refresh the browser, close tabs…). */
  onDone(): void;
  onOpenConsole?(sql: string): void;
}) {
  const [checksOn, setChecksOn] = useState(true);
  const [typed, setTyped] = useState("");
  const [running, setRunning] = useState(false);
  const [results, setResults] = useState<StatementResult[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const production = props.environment === "production";
  const final = withFkChecks(props.statements, checksOn || props.foreignKeySwitch === false);
  const text = final.join("\n");
  const allowed = !production || typed.trim().toLowerCase() === "production";
  const failed = results?.some((r) => r.error) ?? false;

  const run = async () => {
    setRunning(true); setError(null);
    try {
      const r = await props.onRun(final);
      setResults(r);
      if (!r.some((x) => x.error)) props.onDone();
    } catch (e) { setError(describeError(e)); } finally { setRunning(false); }
  };

  return (
    <div className="overlay">
      <div className="dialog preview-dialog" role="dialog" aria-label={props.title}>
        <h3>{props.title} <span className={`env-badge env-${props.environment}`}>{ENV_LABEL[props.environment]}</span></h3>
        {props.summary && <p className="dim">{props.summary}</p>}
        <div className="dim small">This is exactly what will run{results ? "" : ". Nothing has been sent to the server yet"}.</div>
        <pre className="preview-sql" aria-label="SQL to run">{text}</pre>
        {props.foreignKeySwitch !== false && !results && (
          <label className="check"><input type="checkbox" checked={checksOn} onChange={(e) => setChecksOn(e.target.checked)} /> Enable foreign key checks</label>
        )}
        {production && !results && (
          <label><span>Type <b>production</b> to run it</span>
            <input value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="production" aria-label="Type production to confirm" />
          </label>
        )}
        {results && (
          <ul className="plan-results">
            {results.map((r, i) => <li key={i} className={r.error ? "bad" : ""}><Icon name={r.error ? "warn" : "check"} size={12} /> <code>{r.sql.length > 90 ? r.sql.slice(0, 90) + "…" : r.sql}</code>{r.error ? <div className="error small">{r.error}</div> : <span className="dim"> · {r.elapsed_ms} ms</span>}
              {r.sets.map((set, k) => (
                <table key={k} className="info plan-set"><thead><tr>{set.columns.map((c, j) => <th key={j}>{c.name}</th>)}</tr></thead>
                  <tbody>{set.rows.slice(0, 50).map((row, ri) => <tr key={ri}>{row.map((v, j) => <td key={j} className="mono">{v ?? "NULL"}</td>)}</tr>)}</tbody></table>
              ))}</li>)}
          </ul>
        )}
        {error && <div className="banner bad">{error}</div>}
        <div className="actions">
          <button onClick={() => navigator.clipboard?.writeText(text).catch(() => {})} title="Copy the SQL"><Icon name="copy" /> Copy</button>
          {props.onOpenConsole && <button onClick={() => { props.onOpenConsole!(text); props.onCancel(); }}>Open in console</button>}
          <span className="grow" />
          {results ? (
            <button className="primary" onClick={props.onCancel}>{failed ? "Close" : "Done"}</button>
          ) : (
            <>
              <button autoFocus onClick={props.onCancel}>Cancel</button>
              <button className="danger" disabled={!allowed || running} onClick={run}>{running ? "Running…" : props.confirmLabel ?? "Run"}</button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
