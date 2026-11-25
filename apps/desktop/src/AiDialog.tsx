import { useEffect, useRef, useState } from "react";
import { aiApi, plainError } from "./api";
import { giveConsent, hasConsent, isLocal, loadAi } from "./aiPrefs";

/** "Ask AI": describe a query in plain words, review the SQL that comes back, then put it in the editor. It is never run from here. */
export default function AiDialog(props: { connectionId: number; database: string | null; onInsert(sql: string, replace: boolean): void; onSettings(): void; onClose(): void }) {
  const cfg = loadAi();
  const [request, setRequest] = useState("");
  const [sql, setSql] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [asked, setAsked] = useState(cfg ? hasConsent(cfg.endpoint) || isLocal(cfg.endpoint) : false);
  const box = useRef<HTMLTextAreaElement>(null);
  useEffect(() => { box.current?.focus(); }, [asked]);
  useEffect(() => {
    const on = (e: KeyboardEvent) => { if (e.key === "Escape" && !busy) { e.stopPropagation(); props.onClose(); } };
    window.addEventListener("keydown", on, true);
    return () => window.removeEventListener("keydown", on, true);
  }, [props, busy]);

  const write = async () => {
    if (!cfg || !request.trim() || busy) return;
    setBusy(true); setError(null); setSql("");
    try { setSql(await aiApi.write(props.connectionId, props.database, request, { endpoint: cfg.endpoint, model: cfg.model })); }
    catch (e) { setError(plainError(e)); }
    finally { setBusy(false); }
  };
  const copy = () => navigator.clipboard?.writeText(sql).catch(() => {});

  return (
    <div className="overlay" onMouseDown={() => !busy && props.onClose()}>
      <div className="dialog ai-dialog" role="dialog" aria-label="Ask AI to write a query" onMouseDown={(e) => e.stopPropagation()}>
        <h3>Ask AI to write a query</h3>
        {!cfg ? (
          <>
            <p className="dim">The assistant is off. Choose a service first: Ollama runs on your computer for free, and Groq, OpenRouter and Gemini have free tiers.</p>
            <div className="actions"><button onClick={props.onClose}>Close</button><button className="primary" onClick={props.onSettings}>Set up the assistant…</button></div>
          </>
        ) : !asked ? (
          <>
            <p>Your sentence and the <b>names</b> of the tables, columns and foreign keys of <b>{props.database ?? "the selected database"}</b> will be sent to <b>{new URL(cfg.endpoint).host}</b>. No rows or values are sent.</p>
            <div className="actions">
              <button onClick={props.onClose}>Cancel</button>
              <button className="primary" onClick={() => { giveConsent(cfg.endpoint); setAsked(true); }}>Allow and continue</button>
            </div>
          </>
        ) : (
          <>
            <p className="dim small">Database: <b>{props.database ?? "none selected"}</b> · Model: {cfg.model}</p>
            <textarea ref={box} value={request} rows={3} placeholder="For example: the 10 customers with the highest total orders this year" disabled={busy}
              onChange={(e) => setRequest(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) { e.preventDefault(); write(); } }} />
            {error && <div className="banner bad" role="alert">{error}</div>}
            {sql && <pre className="ai-sql" aria-label="Suggested query">{sql}</pre>}
            {sql && <p className="dim small">Check it before you run it: a model can get names or logic wrong.</p>}
            <div className="actions">
              <button onClick={props.onSettings}>Settings</button>
              <span className="grow" />
              {sql && <button onClick={copy}>Copy</button>}
              {sql && <button onClick={() => { props.onInsert(sql, true); props.onClose(); }}>Replace editor text</button>}
              {sql && <button className="primary" onClick={() => { props.onInsert(sql, false); props.onClose(); }}>Insert in editor</button>}
              {!sql && <button className="primary" disabled={busy || !request.trim()} onClick={write}>{busy ? "Writing…" : "Write query (⌘↵)"}</button>}
              {sql && <button disabled={busy} onClick={write}>{busy ? "Writing…" : "Try again"}</button>}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
