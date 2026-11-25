import { useEffect, useRef, useState } from "react";
import { aiApi, plainError, type AiTurn } from "./api";
import { giveConsent, hasConsent, isLocal, loadAi, toSettings } from "./aiPrefs";
import AiSetup from "./AiSetup";
import ChatText from "./ChatText";
import Icon from "./Icon";

interface Msg { role: "user" | "assistant" | "error"; text: string; sql?: string | null; model?: string; note?: string | null }

// the conversation survives closing and opening the panel (it is only cleared by "New chat")
let conversation: Msg[] = [];

const EXAMPLES = ["the 10 customers with the highest total orders this year", "orders without any items", "how many rows does each table have"];

/** The Ask AI panel on the right: a chat that writes SQL. The answer is only inserted into the editor when the user chooses. */
export default function AiPanel(props: { connectionId: number; database: string | null; onInsert(sql: string, replace: boolean): void; onClose(): void }) {
  const [msgs, setMsgs] = useState<Msg[]>(conversation);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const [view, setView] = useState<"chat" | "setup">(() => (loadAi() ? "chat" : "setup"));
  const [cfgTick, setCfgTick] = useState(0);
  const cfg = loadAi();
  void cfgTick;
  const [allowed, setAllowed] = useState(false);
  const end = useRef<HTMLDivElement>(null);
  const box = useRef<HTMLTextAreaElement>(null);
  useEffect(() => { conversation = msgs; end.current?.scrollIntoView({ block: "end" }); }, [msgs]);
  useEffect(() => { if (view === "chat") box.current?.focus(); }, [view]);

  const needsConsent = !!cfg && !isLocal(cfg.endpoint) && !hasConsent(cfg.endpoint) && !allowed;

  const send = async (request = text) => {
    if (!cfg || !request.trim() || busy || needsConsent) return;
    const history: AiTurn[] = msgs.filter((m) => m.role !== "error").map((m) => ({ role: m.role as "user" | "assistant", content: m.text }));
    setMsgs((l) => [...l, { role: "user", text: request.trim() }]); setText(""); setBusy(true);
    try {
      const a = await aiApi.chat(props.connectionId, props.database, history, request.trim(), toSettings(cfg));
      setMsgs((l) => [...l, { role: "assistant", text: a.text, sql: a.sql, model: a.model, note: a.note }]);
    } catch (e) { setMsgs((l) => [...l, { role: "error", text: plainError(e) }]); }
    finally { setBusy(false); }
  };
  const newChat = () => { conversation = []; setMsgs([]); };

  // an answer is shown as its explanation with the code block pulled out into a block with buttons
  const explanation = (m: Msg) => (m.sql ? m.text.replace(/```[\s\S]*?```/g, "").trim() : m.text);

  return (
    <aside className="ai-panel" aria-label="Ask AI">
      <header className="ai-head">
        <Icon name="sparkle" />
        <b>Ask AI</b>
        <span className="grow" />
        {view === "chat" && msgs.length > 0 && <button className="icon-btn" title="New chat" aria-label="New chat" onClick={newChat}><Icon name="plus" /></button>}
        <button className={`icon-btn${view === "setup" ? " on" : ""}`} title="AI settings" aria-label="AI settings" aria-pressed={view === "setup"} onClick={() => setView(view === "setup" ? "chat" : "setup")}><Icon name="settings" /></button>
        <button className="icon-btn" title="Close" aria-label="Close Ask AI" onClick={props.onClose}><Icon name="close" /></button>
      </header>

      {view === "setup" ? (
        <div className="ai-body">
          <AiSetup onReady={() => setCfgTick((n) => n + 1)} />
          <div className="actions"><button className="primary" disabled={!loadAi()} onClick={() => { setCfgTick((n) => n + 1); setView("chat"); }}>Done</button></div>
        </div>
      ) : (
        <>
          <div className="ai-body ai-chat" aria-live="polite">
            {msgs.length === 0 && (
              <div className="ai-empty">
                <p><b>Describe the query you need.</b></p>
                <p className="dim small">Database: <b>{props.database ?? "none selected"}</b>. I only see table and column names, never your data.</p>
                {EXAMPLES.map((x) => <button key={x} className="ai-example" onClick={() => send(x)} disabled={!cfg || busy || needsConsent}>{x}</button>)}
              </div>
            )}
            {msgs.map((m, i) => (
              <div key={i} className={`ai-msg ${m.role}`}>
                {m.role === "user" && <div className="ai-bubble">{m.text}</div>}
                {m.role === "error" && <div className="banner bad" role="alert">{m.text}</div>}
                {m.role === "assistant" && (
                  <>
                    {m.note && <div className="ai-note">{m.note}</div>}
                    {explanation(m) && <ChatText text={explanation(m)} />}
                    {m.sql && (
                      <div className="ai-code">
                        <pre aria-label="Suggested query">{m.sql}</pre>
                        <div className="ai-code-actions">
                          <button className="primary" onClick={() => props.onInsert(m.sql!, false)}>Insert in editor</button>
                          <button onClick={() => props.onInsert(m.sql!, true)}>Replace editor text</button>
                          <button onClick={() => navigator.clipboard?.writeText(m.sql!).catch(() => {})}>Copy</button>
                        </div>
                      </div>
                    )}
                    {m.model && <div className="ai-model dim small">{m.model}</div>}
                  </>
                )}
              </div>
            ))}
            {busy && <div className="ai-msg assistant"><div className="ai-thinking dim">Writing…</div></div>}
            <div ref={end} />
          </div>
          {needsConsent && cfg && (
            <div className="banner ai-consent">
              Your questions and the <b>names</b> of the tables, columns and foreign keys of <b>{props.database ?? "the selected database"}</b> will be sent to <b>{new URL(cfg.endpoint).host}</b>. No rows or values are sent.
              <button className="primary" onClick={() => { giveConsent(cfg.endpoint); setAllowed(true); }}>Allow</button>
            </div>
          )}
          <div className="ai-composer">
            <textarea ref={box} rows={3} value={text} disabled={!cfg || busy} placeholder={cfg ? "Ask for a query… (Enter to send, Shift+Enter for a new line)" : "Connect a service in the settings first"}
              onChange={(e) => setText(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) { e.preventDefault(); send(); } }} />
            <div className="ai-composer-foot">
              {cfg ? (
                <select className="ai-model-pick" aria-label="Model" value={cfg.model} onChange={(e) => { const next = { ...cfg, model: e.target.value, context: cfg.models.find((m) => m.id === e.target.value)?.context ?? null }; localStorage.setItem("mylodb.ai", JSON.stringify(next)); setCfgTick((n) => n + 1); }}>
                  {(cfg.models.some((m) => m.id === cfg.model) ? cfg.models : [{ id: cfg.model, name: cfg.model, context: null, free: false }, ...cfg.models]).map((m) => <option key={m.id} value={m.id}>{m.id}</option>)}
                </select>
              ) : <button onClick={() => setView("setup")}>Set up…</button>}
              <span className="grow" />
              <button className="primary" disabled={!cfg || busy || !text.trim() || needsConsent} onClick={() => send()}>{busy ? "…" : "Send"}</button>
            </div>
          </div>
        </>
      )}
    </aside>
  );
}
