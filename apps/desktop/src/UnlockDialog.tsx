import { useEffect, useState } from "react";
import { plainError, secretsApi } from "./api";

/** Asks for the master password that opens the encrypted password file. Shown at start, or when a connection needs a saved password. */
export default function UnlockDialog(props: { onUnlocked(): void; onClose(): void }) {
  const [pw, setPw] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [forgot, setForgot] = useState(false);

  useEffect(() => {
    const on = (e: KeyboardEvent) => { if (e.key === "Escape" && !busy) { e.stopPropagation(); props.onClose(); } };
    window.addEventListener("keydown", on, true);
    return () => window.removeEventListener("keydown", on, true);
  }, [props, busy]);

  const unlock = async () => {
    setBusy(true); setError(null);
    try { await secretsApi.unlock(pw); props.onUnlocked(); }
    catch (e) { setError(plainError(e)); setPw(""); }
    finally { setBusy(false); }
  };
  const reset = async () => {
    setBusy(true);
    try { await secretsApi.reset(); props.onUnlocked(); } catch (e) { setError(plainError(e)); } finally { setBusy(false); }
  };

  return (
    <div className="overlay" onMouseDown={() => !busy && props.onClose()}>
      <form className="dialog unlock-dialog" role="dialog" aria-label="Unlock saved passwords" onMouseDown={(e) => e.stopPropagation()} onSubmit={(e) => { e.preventDefault(); if (pw && !busy) unlock(); }}>
        <h3>Unlock saved passwords</h3>
        <p className="dim">Your connection passwords are in an encrypted file. Type the master password to use them in this session.</p>
        <label>Master password
          <input type="password" autoFocus autoComplete="off" value={pw} onChange={(e) => setPw(e.target.value)} disabled={busy} />
        </label>
        {error && <div className="banner bad" role="alert">{error}</div>}
        {forgot ? (
          <div className="banner" role="alert">
            <p><b>There is no way to recover the file without the master password.</b> Resetting deletes it. Your connections stay, but you type each password again.</p>
            <button type="button" className="danger" disabled={busy} onClick={reset}>Delete the file and start again</button>
            <button type="button" onClick={() => setForgot(false)}>Keep it</button>
          </div>
        ) : <button type="button" className="link" onClick={() => setForgot(true)}>I forgot the master password</button>}
        <div className="actions">
          <button type="button" onClick={props.onClose} disabled={busy}>Not now</button>
          <button className="primary" type="submit" disabled={!pw || busy}>{busy ? "Unlocking…" : "Unlock"}</button>
        </div>
      </form>
    </div>
  );
}
