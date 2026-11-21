import { useEffect, useRef, useState } from "react";
import Icon, { type IconName } from "./Icon";

export interface MenuItem {
  label?: string; onClick?: () => void; disabled?: boolean; separator?: boolean; children?: MenuItem[];
  icon?: IconName;
  /** Shown at the right, e.g. "F2". Only for shortcuts that really work. */
  shortcut?: string;
  /** Destructive actions are drawn in red. */
  danger?: boolean;
}

const ITEM_H = 27;

export default function ContextMenu(props: { x: number; y: number; items: MenuItem[]; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const close = (e: Event) => { if (!ref.current?.contains(e.target as Node)) props.onClose(); };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && props.onClose();
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    window.addEventListener("blur", props.onClose);
    return () => { window.removeEventListener("mousedown", close); window.removeEventListener("keydown", esc); window.removeEventListener("blur", props.onClose); };
  }, [props]);

  // Keep the menu inside the window.
  const x = Math.min(props.x, window.innerWidth - 260);
  const height = props.items.reduce((h, it) => h + (it.separator ? 9 : ITEM_H), 10);
  const y = Math.min(props.y, window.innerHeight - Math.min(height, window.innerHeight - 20));
  return (
    <div className="ctx" ref={ref} style={{ left: x, top: Math.max(4, y) }} onContextMenu={(e) => e.preventDefault()}>
      <Items items={props.items} onClose={props.onClose} />
    </div>
  );
}

function Items({ items, onClose }: { items: MenuItem[]; onClose: () => void }) {
  return (
    <>
      {items.map((it, i) =>
        it.separator ? <div key={i} className="ctx-sep" /> : it.children ? (
          <Sub key={i} item={it} onClose={onClose} />
        ) : (
          <div key={i} className={`ctx-item ${it.disabled ? "disabled" : ""} ${it.danger ? "danger" : ""}`}
               onClick={() => { if (!it.disabled) { onClose(); it.onClick?.(); } }}>
            <span className="ctx-icon">{it.icon && <Icon name={it.icon} size={14} />}</span>
            <span className="ctx-label">{it.label}</span>
            {it.shortcut && <span className="ctx-key">{it.shortcut}</span>}
          </div>
        ),
      )}
    </>
  );
}

/** An entry with a submenu. The submenu opens to the left or upwards when there is no room on the right or below. */
function Sub({ item, onClose }: { item: MenuItem; onClose: () => void }) {
  const [place, setPlace] = useState<{ left: boolean; shift: number }>({ left: false, shift: -5 });
  const measure = (e: React.MouseEvent<HTMLDivElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    const need = (item.children?.reduce((h, it) => h + (it.separator ? 9 : ITEM_H), 10) ?? 0);
    setPlace({ left: r.right + 250 > window.innerWidth, shift: Math.min(-5, window.innerHeight - (r.top + need) - 8) });
  };
  return (
    <div className="ctx-item sub" onMouseEnter={measure}>
      <span className="ctx-icon">{item.icon && <Icon name={item.icon} size={14} />}</span>
      <span className="ctx-label">{item.label}</span>
      <span className="ctx-arrow"><Icon name="chevron" size={11} /></span>
      <div className={`ctx ctx-nested ${place.left ? "left" : ""}`} style={{ top: place.shift }}>
        <Items items={item.children!} onClose={onClose} />
      </div>
    </div>
  );
}
