import { useState } from "react";
import { open as pickFile } from "@tauri-apps/plugin-dialog";
import { api, describeError, type ConnectError, type SavedConnection, type SshConfig } from "./api";

const blank: SavedConnection = {
  id: null, name: "", color: "#4f8cff", host: "127.0.0.1", port: 3306,
  user: "root", database: null, ssl: "disabled", new_password: "",
  ssl_ca: null, ssl_cert: null, ssl_key: null, ssh: null, new_ssh_password: "", new_ssh_passphrase: "",
};

const blankSsh: SshConfig = { host: "", port: 22, user: "", auth: { method: "key", path: "~/.ssh/id_ed25519" } };

type Tab = "general" | "ssh" | "ssl";

/** A text field with a "Browse…" button that fills in a file path. */
function PathField(props: { label: string; value: string | null | undefined; onChange(v: string | null): void; placeholder?: string }) {
  return (
    <label>{props.label}
      <div className="path-field">
        <input value={props.value ?? ""} placeholder={props.placeholder} onChange={(e) => props.onChange(e.target.value || null)} />
        <button type="button" onClick={async () => { const p = await pickFile({ multiple: false, directory: false }); if (typeof p === "string") props.onChange(p); }}>Browse…</button>
      </div>
    </label>
  );
}

export default function ConnectionDialog(props: {
  initial?: SavedConnection;
  onClose: () => void;
  onSaved: (id: number) => void;
}) {
  const [c, setC] = useState<SavedConnection>(props.initial ?? blank);
  const [tab, setTab] = useState<Tab>("general");
  const [error, setError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const [unknownHost, setUnknownHost] = useState<{ host: string; fingerprint: string; then: "test" | "save" } | null>(null);
  const set = <K extends keyof SavedConnection>(k: K, v: SavedConnection[K]) => setC({ ...c, [k]: v });
  const ssh = c.ssh ?? null;
  const setSsh = (p: Partial<SshConfig>) => setC({ ...c, ssh: { ...(c.ssh ?? blankSsh), ...p } });

  /** Empty secret fields mean "unchanged", so they are sent as null. */
  const payload = (): SavedConnection => ({
    ...c,
    new_password: c.new_password || null,
    new_ssh_password: c.new_ssh_password || null,
    new_ssh_passphrase: c.new_ssh_passphrase || null,
  });

  const test = async (trust?: string) => {
    setTesting(true); setResult(null); setError(null);
    try {
      const r = await api.testConnection(payload(), trust);
      setResult(`Connected to MySQL ${r.version}${r.via_ssh ? " through the SSH tunnel" : ""}${r.ssl_cipher ? ` · encrypted (${r.ssl_cipher})` : " · not encrypted"}`);
    } catch (e) {
      const err = e as ConnectError;
      if (typeof err === "object" && err && err.kind === "unknown_host") setUnknownHost({ host: err.host, fingerprint: err.fingerprint, then: "test" });
      else setError(describeError(e) + (typeof err === "object" && err?.kind === "host_key_changed" ? ` Expected ${err.expected}, got ${err.actual}.` : ""));
    }
    setTesting(false);
  };

  const save = async () => {
    try { props.onSaved(await api.saveConnection(payload())); } catch (e) { setError(String(e)); }
  };

  const valid = c.name && c.host && c.user && (!ssh || (ssh.host && ssh.user && (ssh.auth.method === "password" || ssh.auth.path)));

  return (
    <div className="overlay">
      <div className="dialog conn-dialog">
        <h3>{c.id ? "Edit connection" : "New connection"}</h3>
        <div className="dz-tabs">
          {([["general", "General"], ["ssh", `SSH tunnel${ssh ? " ●" : ""}`], ["ssl", `SSL${c.ssl !== "disabled" ? " ●" : ""}`]] as [Tab, string][]).map(([t, label]) => (
            <button key={t} type="button" className={tab === t ? "active" : ""} onClick={() => setTab(t)}>{label}</button>
          ))}
        </div>

        {tab === "general" && (
          <>
            <label>Name<input value={c.name} onChange={(e) => set("name", e.target.value)} autoFocus /></label>
            <div className="row">
              <label>Host<input value={c.host} onChange={(e) => set("host", e.target.value)} /></label>
              <label className="narrow">Port<input type="number" value={c.port} onChange={(e) => set("port", Number(e.target.value))} /></label>
            </div>
            {ssh && <div className="dim small">With an SSH tunnel, host and port are as seen from the SSH server (often 127.0.0.1).</div>}
            <div className="row">
              <label>User<input value={c.user} onChange={(e) => set("user", e.target.value)} /></label>
              <label>Password<input type="password" placeholder={c.id ? "(unchanged)" : ""} value={c.new_password ?? ""} onChange={(e) => set("new_password", e.target.value)} /></label>
            </div>
            <div className="row">
              <label>Default database<input value={c.database ?? ""} onChange={(e) => set("database", e.target.value || null)} /></label>
              <label className="narrow">Tab color<input type="color" value={c.color} onChange={(e) => set("color", e.target.value)} /></label>
            </div>
          </>
        )}

        {tab === "ssh" && (
          <>
            <label className="check"><input type="checkbox" checked={!!ssh} onChange={(e) => setC({ ...c, ssh: e.target.checked ? blankSsh : null })} /> Connect through an SSH tunnel</label>
            {ssh && (
              <>
                <div className="row">
                  <label>SSH host<input value={ssh.host} onChange={(e) => setSsh({ host: e.target.value })} placeholder="bastion.example.com" /></label>
                  <label className="narrow">Port<input type="number" value={ssh.port} onChange={(e) => setSsh({ port: Number(e.target.value) })} /></label>
                </div>
                <label>SSH user<input value={ssh.user} onChange={(e) => setSsh({ user: e.target.value })} /></label>
                <label>Authentication
                  <select value={ssh.auth.method} onChange={(e) => setSsh({ auth: e.target.value === "password" ? { method: "password" } : { method: "key", path: "~/.ssh/id_ed25519" } })}>
                    <option value="key">Private key</option><option value="password">Password</option>
                  </select></label>
                {ssh.auth.method === "key" ? (
                  <>
                    <PathField label="Private key file" value={ssh.auth.path} onChange={(v) => setSsh({ auth: { method: "key", path: v ?? "" } })} />
                    <label>Key passphrase<input type="password" placeholder={c.id ? "(unchanged)" : "(none)"} value={c.new_ssh_passphrase ?? ""} onChange={(e) => set("new_ssh_passphrase", e.target.value)} /></label>
                  </>
                ) : (
                  <label>SSH password<input type="password" placeholder={c.id ? "(unchanged)" : ""} value={c.new_ssh_password ?? ""} onChange={(e) => set("new_ssh_password", e.target.value)} /></label>
                )}
                <div className="dim small">The SSH server's host key is shown for your confirmation the first time and checked on every connection after that.</div>
              </>
            )}
          </>
        )}

        {tab === "ssl" && (
          <>
            <label>SSL mode
              <select value={c.ssl} onChange={(e) => set("ssl", e.target.value as SavedConnection["ssl"])}>
                <option value="disabled">Disabled</option>
                <option value="required">Required: verify certificate and host name</option>
                <option value="verifyca">Required: verify certificate, ignore host name</option>
                <option value="skipverify">Required: do not verify (self-signed)</option>
              </select></label>
            {c.ssl !== "disabled" && (
              <>
                <PathField label="CA certificate (PEM)" value={c.ssl_ca} onChange={(v) => set("ssl_ca", v)} placeholder="(system certificates)" />
                <PathField label="Client certificate (PEM)" value={c.ssl_cert} onChange={(v) => set("ssl_cert", v)} placeholder="(only if the server requires one)" />
                <PathField label="Client key (PEM)" value={c.ssl_key} onChange={(v) => set("ssl_key", v)} />
              </>
            )}
            {ssh && c.ssl === "required" && <div className="dim small">Through an SSH tunnel the server certificate is checked against 127.0.0.1; use "verify certificate, ignore host name".</div>}
          </>
        )}

        {result && <div className="banner ok">{result}</div>}
        {error && <div className="error">{error}</div>}
        <div className="actions">
          <button type="button" disabled={!valid || testing} onClick={() => test()}>{testing ? "Testing…" : "Test connection"}</button>
          <span className="grow" />
          <button onClick={props.onClose}>Cancel</button>
          <button className="primary" disabled={!valid} onClick={save}>Save</button>
        </div>
      </div>

      {unknownHost && (
        <div className="overlay inner">
          <div className="dialog">
            <h3>Trust this SSH server?</h3>
            <p>The authenticity of <b>{unknownHost.host}</b> cannot be established. Its host key fingerprint is:</p>
            <code className="preview">{unknownHost.fingerprint}</code>
            <p className="dim small">Compare it with the fingerprint your server administrator gave you. If it does not match, do not continue.</p>
            <div className="actions">
              <button onClick={() => setUnknownHost(null)}>Cancel</button>
              <button className="primary" onClick={() => { const u = unknownHost; setUnknownHost(null); test(u.fingerprint); }}>Trust and continue</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
