import { useEffect, useState } from "react";
import Icon, { type IconName } from "./Icon";
import { adminApi, describeError, type ServerHome } from "./api";

function uptime(sec: number): string {
  const d = Math.floor(sec / 86400), h = Math.floor((sec % 86400) / 3600), m = Math.floor((sec % 3600) / 60);
  return [d ? `${d} d` : "", h ? `${h} h` : "", `${m} min`].filter(Boolean).join(" ");
}

/** The page that opens after connecting (phpMyAdmin's home): who you are on which server, collation, and the common places to go. */
export default function HomePage(props: {
  connectionId: number;
  name: string;
  readOnly: boolean;
  environment: string;
  actions: { label: string; icon: IconName; run(): void; hint?: string }[];
}) {
  const [home, setHome] = useState<ServerHome | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    adminApi.home(props.connectionId).then((h) => live && setHome(h)).catch((e) => live && setError(describeError(e)));
    return () => { live = false; };
  }, [props.connectionId]);
  const i = home?.info;
  const row = (k: string, v: React.ReactNode) => <tr key={k}><th>{k}</th><td>{v}</td></tr>;
  return (
    <div className="admin-page">
      <h2><Icon name="server" size={16} /> {props.name}</h2>
      <div className="home-grid">
        <section className="admin-card">
          <h4>Server</h4>
          {error && <div className="banner bad">{error}</div>}
          {!home && !error && <div className="dim small">Loading…</div>}
          {home && i && (
            <table className="info kv" aria-label="Server">
              <tbody>
                {row("Version", `${i.version} (${i.version_comment})`)}
                {row("Host", `${i.hostname}:${i.port}`)}
                {row("Logged in as", home.account)}
                {row("Uptime", uptime(i.uptime_seconds))}
                {row("Connections", `${home.connections ?? "?"} open · max ${i.max_connections}`)}
                {row("Databases", home.databases)}
                {row("Protocol", home.protocol ?? "")}
                {row("SSL", i.ssl_cipher ?? "not used")}
              </tbody>
            </table>
          )}
        </section>
        <section className="admin-card">
          <h4>Character set and collation</h4>
          {home && i && (
            <table className="info kv" aria-label="Collation">
              <tbody>
                {row("Server character set", i.charset)}
                {row("Server collation", i.collation)}
                {row("This connection uses", <b>{home.connection_collation}</b>)}
                {row("Time zone", i.time_zone)}
              </tbody>
            </table>
          )}
          <div className="dim small" style={{ marginTop: 8 }}>The connection collation is set in the connection settings (Edit connection).</div>
          <div style={{ marginTop: 8 }}>
            <span className={`env-badge env-${props.environment}`}>{props.environment}</span>{props.readOnly && <span className="dim small"> · read-only</span>}
          </div>
        </section>
      </div>
      <section className="admin-card">
        <h4>Go to</h4>
        <div className="home-actions">
          {props.actions.map((a) => (
            <button key={a.label} className="home-action" onClick={a.run} title={a.hint}><Icon name={a.icon} size={16} /> {a.label}</button>
          ))}
        </div>
      </section>
    </div>
  );
}
