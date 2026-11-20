import { useEffect, useRef } from "react";

export interface MenuItem { label?: string; onClick?: () => void; disabled?: boolean; separator?: boolean; children?: MenuItem[] }

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
  const x = Math.min(props.x, window.innerWidth - 230);
  const y = Math.min(props.y, window.innerHeight - Math.min(props.items.length * 26 + 12, window.innerHeight - 20));
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
          <div key={i} className="ctx-item sub">
            {it.label} <span className="dim">▸</span>
            <div className="ctx ctx-nested"><Items items={it.children} onClose={onClose} /></div>
          </div>
        ) : (
          <div key={i} className={`ctx-item ${it.disabled ? "disabled" : ""}`}
               onClick={() => { if (!it.disabled) { onClose(); it.onClick?.(); } }}>
            {it.label}
          </div>
        ),
      )}
    </>
  );
}
