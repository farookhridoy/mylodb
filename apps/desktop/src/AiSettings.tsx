import { useEffect, useState } from "react";
import { aiApi, plainError } from "./api";
import { AI_PRESETS, loadAi, saveAi } from "./aiPrefs";

/** Preferences → AI assistant. Saved at once, like the password settings. */
export default function AiSettings() {
  const saved = loadAi();
  const [preset, setPreset] = useState(saved?.preset ?? "ollama");
  const p = AI_PRESETS.find((x) => x.id === preset)!;
  const [endpoint, setEndpoint] = useState(saved?.endpoint ?? p.endpoint);
  const [model, setModel] = useState(saved?.model ?? p.model);
  const [key, setKey] = useState("");
  const [hasKey, setHasKey] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  useEffect(() => { aiApi.hasKey().then(setHasKey).catch(() => {}); }, []);

  const pick = (id: string) => {
    const n = AI_PRESETS.find((x) => x.id === id)!;
    setPreset(id); setEndpoint(n.endpoint); setModel(n.model); setMsg(null);
  };
  const save = async () => {
    try {
      if (key.trim()) { await aiApi.setKey(key); setKey(""); setHasKey(true); }
      saveAi({ preset, endpoint: endpoint.trim(), model: model.trim() });
      setMsg({ ok: true, text: "Saved. Use Ask AI in the editor toolbar." });
    } catch (e) { setMsg({ ok: false, text: plainError(e) }); }
  };
  const forget = async () => {
    try { await aiApi.setKey(""); setHasKey(false); saveAi(null); setMsg({ ok: true, text: "The key and the settings are removed." }); }
    catch (e) { setMsg({ ok: false, text: plainError(e) }); }
  };

  return (
    <div className="prefs-page ai-page">
      <p>The AI assistant writes a query from a sentence you type. It is off until you choose a service here.</p>
      <label>Service
        <select value={preset} onChange={(e) => pick(e.target.value)}>{AI_PRESETS.map((x) => <option key={x.id} value={x.id}>{x.label}</option>)}</select>
      </label>
      <p className="dim small">{p.note}</p>
      <label>Address
        <input value={endpoint} onChange={(e) => setEndpoint(e.target.value)} placeholder="https://…/v1" spellCheck={false} />
      </label>
      <label>Model
        <input value={model} onChange={(e) => setModel(e.target.value)} spellCheck={false} />
      </label>
      {p.key && (
        <label>Key {hasKey && <span className="dim small">(a key is saved; type a new one to replace it)</span>}
          <input type="password" autoComplete="off" value={key} onChange={(e) => setKey(e.target.value)} placeholder={hasKey ? "••••••••" : "paste the key"} />
        </label>
      )}
      <div className="banner">What is sent: your sentence, and the <b>names</b> of the tables, columns and foreign keys of the selected database. Never rows or values. The answer is only put in the editor; it is not run. The key is kept with your other saved passwords.</div>
      {msg && <div className={`banner ${msg.ok ? "" : "bad"}`} role="status">{msg.text}</div>}
      <div className="actions">
        <button onClick={forget}>Remove settings and key</button>
        <button className="primary" disabled={!endpoint.trim() || !model.trim()} onClick={save}>Save</button>
      </div>
    </div>
  );
}
