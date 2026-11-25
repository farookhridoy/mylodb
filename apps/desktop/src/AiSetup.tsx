import { useCallback, useEffect, useState } from "react";
import { aiApi, openUrl, plainError, type AiModel } from "./api";
import { AI_PRESETS, contextLabel, loadAi, saveAi, type AiConfig } from "./aiPrefs";

/** Choose the AI service, connect it (key), and pick a model. Used in the Ask AI panel and in Settings. Everything is saved as it changes. */
export default function AiSetup({ onReady }: { onReady?(): void }) {
  const saved = loadAi();
  const [preset, setPreset] = useState(saved?.preset ?? "openrouter");
  const p = AI_PRESETS.find((x) => x.id === preset)!;
  const same = saved?.preset === preset ? saved : null;
  const [endpoint, setEndpoint] = useState(same?.endpoint ?? p.endpoint);
  const [model, setModel] = useState(same?.model ?? "");
  const [auto, setAuto] = useState(saved?.auto ?? true);
  const [models, setModels] = useState<AiModel[]>(same?.models ?? []);
  const [hasKey, setHasKey] = useState(false);
  const [key, setKey] = useState("");
  const [all, setAll] = useState(false);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);

  const load = useCallback(async (pr: string, ep: string, everything: boolean) => {
    setBusy(true); setMsg(null);
    try {
      const list = await aiApi.models(pr, ep, everything);
      setModels(list);
      setModel((cur) => (list.some((m) => m.id === cur) ? cur : list[0]?.id ?? ""));
      if (!list.length) setMsg({ ok: false, text: pr === "ollama" ? "No models are installed yet. Run: ollama pull qwen2.5-coder:7b" : "The service has no models for this key." });
      return true;
    } catch (e) { setModels([]); setMsg({ ok: false, text: plainError(e) }); return false; }
    finally { setBusy(false); }
  }, []);

  // switching service: show what is saved for it, and read its models when it needs no key or the key is already there
  useEffect(() => {
    let live = true;
    const n = AI_PRESETS.find((x) => x.id === preset)!;
    const s = loadAi();
    const mine = s?.preset === preset ? s : null;
    setEndpoint(mine?.endpoint ?? n.endpoint); setModel(mine?.model ?? ""); setModels(mine?.models ?? []); setKey(""); setMsg(null);
    aiApi.hasKey(preset).then((h) => { if (!live) return; setHasKey(h); if ((!n.key || h) && (mine?.endpoint ?? n.endpoint)) load(preset, mine?.endpoint ?? n.endpoint, false); }).catch(() => {});
    return () => { live = false; };
  }, [preset, load]);

  // everything chosen here is saved at once
  useEffect(() => {
    if (!model || !endpoint.trim()) return;
    const cfg: AiConfig = { preset, endpoint: endpoint.trim(), model, context: models.find((m) => m.id === model)?.context ?? null, auto, models };
    saveAi(cfg);
  }, [preset, endpoint, model, auto, models]);

  const connect = async () => {
    if (!key.trim()) return;
    setBusy(true); setMsg(null);
    try {
      await aiApi.setKey(preset, key);
      const ok = await load(preset, endpoint.trim(), all);
      if (ok) { setHasKey(true); setKey(""); setMsg({ ok: true, text: "Connected." }); onReady?.(); }
      else { await aiApi.setKey(preset, ""); setHasKey(false); }
    } catch (e) { setMsg({ ok: false, text: plainError(e) }); }
    finally { setBusy(false); }
  };
  const disconnect = async () => {
    try { await aiApi.setKey(preset, ""); setHasKey(false); setModels([]); setModel(""); if (loadAi()?.preset === preset) saveAi(null); setMsg({ ok: true, text: "Disconnected. The key is removed." }); }
    catch (e) { setMsg({ ok: false, text: plainError(e) }); }
  };
  const needsConnect = p.key && !hasKey;
  const chosen = models.find((m) => m.id === model);

  return (
    <div className="ai-setup">
      <label>Service
        <select value={preset} onChange={(e) => setPreset(e.target.value)} disabled={busy}>{AI_PRESETS.map((x) => <option key={x.id} value={x.id}>{x.label}</option>)}</select>
      </label>
      <p className="dim small">{p.note}</p>
      {preset === "custom" && (
        <label>Address<input value={endpoint} onChange={(e) => setEndpoint(e.target.value)} placeholder="https://…/v1" spellCheck={false} /></label>
      )}

      {needsConnect ? (
        <div className="ai-connect">
          {p.keyUrl && <button type="button" onClick={() => openUrl(p.keyUrl!).catch(() => {})}>Get a key in your browser</button>}
          <label>Paste the key
            <input type="password" autoComplete="off" value={key} onChange={(e) => setKey(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") connect(); }} placeholder="paste the key here" />
          </label>
          <button className="primary" disabled={busy || !key.trim() || (preset === "custom" && !endpoint.trim())} onClick={connect}>{busy ? "Connecting…" : "Connect"}</button>
        </div>
      ) : (
        <div className="ai-connected">
          {p.key && <div className="ai-status"><span className="dot ok" /> Connected <button type="button" className="link" onClick={disconnect}>Disconnect</button></div>}
          {!p.key && <div className="ai-status"><button type="button" className="link" onClick={() => load(preset, endpoint, all)}>Refresh the model list</button></div>}
          <label>Model {models.length > 0 && <span className="dim small">({models.length}{preset === "openrouter" && !all ? " free" : ""})</span>}
            <select value={model} onChange={(e) => setModel(e.target.value)} disabled={busy || !models.length} aria-label="Model">
              {!models.length && <option value="">{busy ? "Loading models…" : "No models"}</option>}
              {models.map((m) => <option key={m.id} value={m.id}>{m.id}{m.free && preset === "openrouter" ? " · free" : ""}{m.context ? ` · ${contextLabel(m.context)}` : ""}</option>)}
            </select>
          </label>
          {preset === "openrouter" && (
            <label className="check"><input type="checkbox" checked={all} onChange={(e) => { setAll(e.target.checked); load(preset, endpoint, e.target.checked); }} /> Show paid models too</label>
          )}
          <label className="check"><input type="checkbox" checked={auto} onChange={(e) => setAuto(e.target.checked)} /> Switch to another model automatically when this one is too small for the question, busy or out of its free quota</label>
          {chosen?.context ? <p className="dim small">Context window of {chosen.id}: {chosen.context.toLocaleString()} tokens.</p> : null}
        </div>
      )}
      {msg && <div className={`banner ${msg.ok ? "" : "bad"}`} role={msg.ok ? "status" : "alert"}>{msg.text}</div>}
      <div className="banner">What is sent: your question, and the <b>names</b> of the tables, columns and foreign keys of the selected database. Never rows or values. The answer is only put in the editor; it is never run. The key is kept with your saved passwords.</div>
    </div>
  );
}
