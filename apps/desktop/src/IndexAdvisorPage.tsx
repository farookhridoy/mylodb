import { Fragment, useEffect, useMemo, useState } from "react";
import { adviceApi, describeError, type IndexAdvice } from "./api";

const TITLE = { duplicate: "Duplicate index", redundant: "Redundant index", missing_foreign_key_index: "Foreign key without an index" } as const;

/** Index advisor: duplicates, redundant prefixes, foreign keys without an index, and indexes the server never used. Fixes go through Preview SQL. */
export default function IndexAdvisorPage(props: { connectionId: number; database: string; onApply(statements: string[], summary: string): void }) {
  const [advice, setAdvice] = useState<IndexAdvice | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(true);

  const load = () => {
    setBusy(true); setError(null);
    adviceApi.indexes(props.connectionId, props.database).then((a) => { setAdvice(a); setChosen(new Set(a.findings.filter((f) => f.kind !== "missing_foreign_key_index").map(key))); }).catch((e) => setError(describeError(e))).finally(() => setBusy(false));
  };
  useEffect(load, [props.connectionId, props.database]); // eslint-disable-line react-hooks/exhaustive-deps

  const script = useMemo(() => (advice ? advice.findings.filter((f) => chosen.has(key(f))).map((f) => `${f.sql};`) : []), [advice, chosen]);
  const toggle = (k: string) => { const n = new Set(chosen); n.has(k) ? n.delete(k) : n.add(k); setChosen(n); };

  return (
    <div className="admin-page">
      <h2>Index advisor <span className="dim">{props.database}</span></h2>
      <p className="dim small">Looks at the index definitions of every table. Dropping an index is safe for lookups when another index covers it, but it changes how the server plans queries: check slow queries afterwards. Nothing runs until you preview and apply.</p>
      <button disabled={busy} onClick={load}>{busy ? "Analysing…" : "Analyse again"}</button>
      {error && <div className="banner bad">{error}</div>}
      {advice && (
        <>
          <p aria-label="Summary">{advice.findings.length === 0 ? `No duplicate, redundant or missing indexes found in ${advice.tables} table${advice.tables === 1 ? "" : "s"}.` : `${advice.findings.length} finding${advice.findings.length === 1 ? "" : "s"} in ${advice.tables} table${advice.tables === 1 ? "" : "s"}.`}</p>
          {advice.findings.length > 0 && (
            <table className="info admin-table" aria-label="Findings">
              <thead><tr><th /><th>Table</th><th>Finding</th><th>Why</th></tr></thead>
              <tbody>
                {advice.findings.map((f) => (
                  <Fragment key={key(f)}>
                    <tr>
                      <td><input type="checkbox" aria-label={`Fix ${f.index} on ${f.table}`} checked={chosen.has(key(f))} onChange={() => toggle(key(f))} /></td>
                      <td className="mono">{f.table}</td>
                      <td>{TITLE[f.kind]}: <span className="mono">{f.index}</span></td>
                      <td>{f.detail}<div className="mono small dim">{f.sql}</div></td>
                    </tr>
                  </Fragment>
                ))}
              </tbody>
            </table>
          )}
          {advice.findings.length > 0 && (
            <div className="actions" style={{ justifyContent: "flex-start" }}>
              <button className="primary" disabled={script.length === 0} onClick={() => props.onApply(script, `${script.length} index change(s) in ${props.database}`)}>Preview and apply…</button>
              <span className="dim small">{script.length} statement(s) chosen. A missing foreign key index is not pre-selected: adding one costs write speed and space.</span>
            </div>
          )}
          <h3>Unused since the server started</h3>
          {advice.unused.length === 0 ? <p className="dim small">{advice.unused_note}</p> : (
            <>
              <p className="dim small">{advice.unused_note}</p>
              <table className="info admin-table" aria-label="Unused indexes">
                <thead><tr><th>Table</th><th>Index</th></tr></thead>
                <tbody>{advice.unused.map(([t, i]) => <tr key={`${t}.${i}`}><td className="mono">{t}</td><td className="mono">{i}</td></tr>)}</tbody>
              </table>
            </>
          )}
        </>
      )}
    </div>
  );
}

const key = (f: { table: string; index: string; kind: string }) => `${f.table}\0${f.index}\0${f.kind}`;
