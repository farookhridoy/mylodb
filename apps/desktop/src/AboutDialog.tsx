import { openUrl } from "./api";
import Icon from "./Icon";
import appIcon from "../mylodb-icon.png";

const LINKS = {
  author: "https://farookhridoy.com",
  company: "https://thelarasoft.com",
  repo: "https://github.com/farookhridoy/mylodb",
};

/** An external link that opens in the system browser (the backend only opens the author's own addresses). */
export function ExtLink(props: { href: string; children: React.ReactNode }) {
  return <a href={props.href} className="ext-link" onClick={(e) => { e.preventDefault(); openUrl(props.href).catch(() => {}); }}>{props.children}</a>;
}

/** About MyLoDB: version, licence, author and where to find the source and the releases. */
export default function AboutDialog(props: { onClose(): void; onManual(): void; autoUpdate: boolean; onAutoUpdate(on: boolean): void; onCheckUpdates(): void }) {
  return (
    <div className="overlay" onMouseDown={props.onClose}>
      <div className="dialog about" role="dialog" aria-label="About MyLoDB" onMouseDown={(e) => e.stopPropagation()}>
        <div className="about-head">
          <img src={appIcon} alt="" width={56} height={56} />
          <div>
            <h3>MyLoDB</h3>
            <div className="dim">Version {__APP_VERSION__}</div>
          </div>
        </div>
        <p>An open-source MySQL, MariaDB and SQLite manager for macOS, written in Rust and React. Built for people who miss SQLyog on the Mac.</p>
        <table className="info kv">
          <tbody>
            <tr><th>Author</th><td>Omar Farook Hridoy (<ExtLink href={LINKS.author}>farookhridoy.com</ExtLink>)</td></tr>
            <tr><th>Company</th><td><ExtLink href={LINKS.company}>thelarasoft.com</ExtLink></td></tr>
            <tr><th>Source code</th><td><ExtLink href={LINKS.repo}>github.com/farookhridoy/mylodb</ExtLink></td></tr>
            <tr><th>Releases</th><td><ExtLink href={`${LINKS.repo}/releases`}>Download and release notes</ExtLink></td></tr>
            <tr><th>Licence</th><td>Apache-2.0</td></tr>
          </tbody>
        </table>
        <p className="dim small">MyLoDB is independent and is not affiliated with SQLyog or Webyog. It sends no telemetry and talks only to the servers you connect to.</p>
        <label className="check"><input type="checkbox" checked={props.autoUpdate} onChange={(e) => props.onAutoUpdate(e.target.checked)} /> Check for updates automatically</label>
        <div className="actions">
          <button onClick={props.onCheckUpdates}>Check for updates</button>
          <button onClick={props.onManual}><Icon name="info" size={12} /> User manual</button>
          <button className="primary" onClick={props.onClose}>Close</button>
        </div>
      </div>
    </div>
  );
}
