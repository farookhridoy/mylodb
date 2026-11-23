import { useEffect, useState } from "react";
import { plainError, secretsApi, type SecretsStatus } from "./api";

const MIN = 8;

/** Preferences → Security → Saved passwords. These actions happen at once; they are not held back by the dialog's Save button. */
export default function SecuritySettings() {
  const [st, setSt] = useState<SecretsStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [pw, setPw] = useState(""); const [pw2, setPw2] = useState("");
  const [cur, setCur] = useState(""); const [next, setNext] = useState(""); const [next2, setNext2] = useState("");
  const [confirmBack, setConfirmBack] = useState(false);

  const refresh = () => secretsApi.status().then(setSt).catch((e) => setMsg({ ok: false, text: plainError(e) }));
  useEffect(() => { refresh(); }, []);

  const run = async (job: () => Promise<string>) => {
    setBusy(true); setMsg(null);
    try { const text = await job(); setMsg({ ok: true, text }); setPw(""); setPw2(""); setCur(""); setNext(""); setNext2(""); setConfirmBack(false); await refresh(); }
    catch (e) { setMsg({ ok: false, text: plainError(e) }); }
    finally { setBusy(false); }
  };
  const plural = (n: number) => `${n} saved password${n === 1 ? "" : "s"}`;

  if (!st) return <div className="prefs-page dim">Loading…</div>;
  return (
    <div className="prefs-page security-page">
      <p>Connection passwords (database, SSH password and key passphrase) are kept
        {st.mode === "file" ? <b> in an encrypted file</b> : <b> in the macOS Keychain</b>}.</p>
      {st.mode === "keychain" ? (
        <>
          <p className="dim small">The Keychain is the default and the most convenient. Because MyLoDB is not signed with a paid Apple certificate, macOS asks for your login password again for each saved connection after every update. An encrypted file avoids those prompts: you type one master password per session instead.</p>
          <h5>Use an encrypted file</h5>
          <label>Master password (at least {MIN} characters)
            <input type="password" autoComplete="new-password" value={pw} onChange={(e) => setPw(e.target.value)} disabled={busy} /></label>
          <label>Type it again
            <input type="password" autoComplete="new-password" value={pw2} onChange={(e) => setPw2(e.target.value)} disabled={busy} /></label>
          <div className="banner">If you forget the master password the file cannot be opened by anyone, including us. You would type each connection password again. Write it down somewhere safe.</div>
          <button className="primary" disabled={busy || pw.length < MIN || pw !== pw2}
            onClick={() => run(async () => `Moved ${plural(await secretsApi.useFile(pw))} out of the Keychain into the encrypted file.`)}>
            {busy ? "Moving…" : "Move my passwords to an encrypted file"}</button>
          {pw2 && pw !== pw2 && <span className="dim small"> The two passwords differ.</span>}
        </>
      ) : (
        <>
          <p className="dim small">{st.unlocked ? "Unlocked for this session." : "Locked: you will be asked for the master password when a connection needs it."} Encryption: XChaCha20-Poly1305, key from your master password with Argon2id. The file never leaves this Mac.</p>
          <h5>Change the master password</h5>
          <label>Current password<input type="password" autoComplete="off" value={cur} onChange={(e) => setCur(e.target.value)} disabled={busy} /></label>
          <label>New password (at least {MIN} characters)<input type="password" autoComplete="new-password" value={next} onChange={(e) => setNext(e.target.value)} disabled={busy} /></label>
          <label>Type the new one again<input type="password" autoComplete="new-password" value={next2} onChange={(e) => setNext2(e.target.value)} disabled={busy} /></label>
          <button disabled={busy || !cur || next.length < MIN || next !== next2} onClick={() => run(async () => { await secretsApi.changePassword(cur, next); return "The master password was changed."; })}>Change password</button>
          <h5>Go back to the Keychain</h5>
          {!st.unlocked && <div className="dim small">Unlock first: connect to any saved connection and type the master password.</div>}
          {confirmBack ? (
            <div className="banner">
              <p>Every saved password moves back into the macOS Keychain and the encrypted file is deleted.</p>
              <button className="primary" disabled={busy || !st.unlocked} onClick={() => run(async () => `Moved ${plural(await secretsApi.useKeychain())} back into the Keychain.`)}>Move them back</button>
              <button onClick={() => setConfirmBack(false)}>Cancel</button>
            </div>
          ) : <button disabled={busy || !st.unlocked} onClick={() => setConfirmBack(true)}>Move passwords back to the Keychain…</button>}
        </>
      )}
      {msg && <div className={`banner ${msg.ok ? "" : "bad"}`} role="status">{msg.text}</div>}
    </div>
  );
}
