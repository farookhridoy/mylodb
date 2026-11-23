import { useEffect } from "react";
import { SHORTCUT_GROUPS } from "./shortcuts";

/** Help → Keyboard Shortcuts (⌘/): every shortcut that works, grouped by where it applies. */
export default function ShortcutsDialog(props: { onClose(): void }) {
  useEffect(() => {
    const on = (e: KeyboardEvent) => { if (e.key === "Escape") { e.stopPropagation(); props.onClose(); } };
    window.addEventListener("keydown", on, true);
    return () => window.removeEventListener("keydown", on, true);
  }, [props]);
  return (
    <div className="overlay" onMouseDown={props.onClose}>
      <div className="dialog shortcuts-dialog" role="dialog" aria-label="Keyboard shortcuts" onMouseDown={(e) => e.stopPropagation()}>
        <h3>Keyboard shortcuts</h3>
        <div className="shortcut-groups">
          {SHORTCUT_GROUPS.map((g) => (
            <section key={g.title}>
              <h4>{g.title}</h4>
              <dl>
                {g.keys.map(([k, v]) => <div key={k + v}><dt><kbd>{k}</kbd></dt><dd>{v}</dd></div>)}
              </dl>
            </section>
          ))}
        </div>
        <div className="actions"><button className="primary" autoFocus onClick={props.onClose}>Close</button></div>
      </div>
    </div>
  );
}
