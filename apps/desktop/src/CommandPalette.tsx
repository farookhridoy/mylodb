import { useEffect, useMemo, useRef, useState } from "react";

export interface Command { id: string; label: string; hint?: string; run(): void }

/** ⌘K: type to filter every action and every table, ↑↓ to move, Enter to run. */
export default function CommandPalette(props: { commands: Command[]; onClose(): void }) {
  const [q, setQ] = useState("");
  const [sel, setSel] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => input.current?.focus(), []);
  const shown = useMemo(() => {
    const words = q.toLowerCase().split(/\s+/).filter(Boolean);
    return props.commands.filter((c) => words.every((w) => `${c.label} ${c.hint ?? ""}`.toLowerCase().includes(w))).slice(0, 12);
  }, [q, props.commands]);
  useEffect(() => setSel(0), [q]);
  const run = (c?: Command) => { if (!c) return; props.onClose(); c.run(); };
  return (
    <div className="overlay palette-overlay" onMouseDown={(e) => e.target === e.currentTarget && props.onClose()}>
      <div className="palette" role="dialog" aria-label="Command palette">
        <input ref={input} value={q} placeholder="Type a command or a table name…" aria-label="Command"
               onChange={(e) => setQ(e.target.value)}
               onKeyDown={(e) => {
                 if (e.key === "Escape") props.onClose();
                 else if (e.key === "ArrowDown") { e.preventDefault(); setSel((s) => Math.min(shown.length - 1, s + 1)); }
                 else if (e.key === "ArrowUp") { e.preventDefault(); setSel((s) => Math.max(0, s - 1)); }
                 else if (e.key === "Enter") run(shown[sel]);
               }} />
        <ul role="listbox">
          {shown.map((c, i) => (
            <li key={c.id} role="option" aria-selected={i === sel} className={i === sel ? "sel" : ""} onMouseEnter={() => setSel(i)} onClick={() => run(c)}>
              <span>{c.label}</span>{c.hint && <span className="dim small">{c.hint}</span>}
            </li>
          ))}
          {shown.length === 0 && <li className="dim">Nothing matches</li>}
        </ul>
      </div>
    </div>
  );
}
