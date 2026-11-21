import { useEffect, useRef } from "react";
import Icon, { type IconName } from "./Icon";

export interface FilterScope { tables: boolean; views: boolean; routines: boolean; triggers: boolean; events: boolean; columns: boolean }
export const DEFAULT_SCOPE: FilterScope = { tables: true, views: true, routines: true, triggers: true, events: true, columns: false };

const ROWS: { key: keyof FilterScope; label: string; icon: IconName }[] = [
  { key: "tables", label: "Tables", icon: "table" },
  { key: "views", label: "Views", icon: "view" },
  { key: "routines", label: "Procedures and functions", icon: "procedure" },
  { key: "triggers", label: "Triggers", icon: "trigger" },
  { key: "events", label: "Events", icon: "event" },
  { key: "columns", label: "Columns", icon: "column" },
];

export const isDefaultScope = (f: FilterScope) => (Object.keys(DEFAULT_SCOPE) as (keyof FilterScope)[]).every((k) => f[k] === DEFAULT_SCOPE[k]);

/** The popover behind the filter icon: what a search inside a locked database looks at. */
export default function FilterOptions({ scope, onChange, onClose }: { scope: FilterScope; onChange(f: FilterScope): void; onClose(): void }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const away = (e: MouseEvent) => { if (!ref.current?.contains(e.target as Node) && !(e.target as HTMLElement).closest?.(".filter-opts")) onClose(); };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("mousedown", away);
    window.addEventListener("keydown", esc);
    return () => { window.removeEventListener("mousedown", away); window.removeEventListener("keydown", esc); };
  }, [onClose]);

  const set = (key: keyof FilterScope, on: boolean) => {
    const next = { ...scope, [key]: on };
    if ((Object.values(next) as boolean[]).some(Boolean)) onChange(next); // at least one kind must stay on
  };
  return (
    <div className="popup filter-pop" ref={ref} role="dialog" aria-label="Filter options">
      <div className="pop-title"><Icon name="filter" size={12} /> Search in</div>
      {ROWS.map((r) => (
        <label key={r.key} className="pop-row">
          <input type="checkbox" checked={scope[r.key]} onChange={(e) => set(r.key, e.target.checked)} />
          <Icon name={r.icon} size={13} className={`ico-${r.icon}`} /> {r.label}
        </label>
      ))}
      <p className="dim small pop-note">Applies inside a locked database. With none locked the box searches database names. Columns are loaded when you switch them on.</p>
      <button className="link" disabled={isDefaultScope(scope)} onClick={() => onChange(DEFAULT_SCOPE)}>Reset to defaults</button>
    </div>
  );
}
