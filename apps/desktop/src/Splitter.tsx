import { useEffect, useState } from "react";

/** A size in pixels that is remembered between sessions and kept within limits. `adjust` adds to the current size (safe to call on every mouse move). */
export function useSize(key: string, initial: number, min: number, max: () => number) {
  const clamp = (n: number) => Math.max(min, Math.min(max(), n));
  const [size, setSizeState] = useState(() => {
    const saved = Number(localStorage.getItem(key));
    return clamp(Number.isFinite(saved) && saved > 0 ? saved : initial);
  });
  useEffect(() => { localStorage.setItem(key, String(Math.round(size))); }, [key, size]);
  const adjust = (delta: number) => setSizeState((prev) => clamp(prev + delta));
  const reset = () => setSizeState(clamp(initial));
  return { size, adjust, reset };
}

/**
 * A drag handle. `axis="x"` sits between side-by-side panes, `axis="y"` between stacked ones.
 * `onDrag` receives the distance moved since the last event; double-click resets.
 */
export default function Splitter({ axis, onDrag, onReset, label }: { axis: "x" | "y"; onDrag(delta: number): void; onReset?(): void; label: string }) {
  const start = (e: React.MouseEvent) => {
    e.preventDefault();
    let last = axis === "x" ? e.clientX : e.clientY;
    document.body.classList.add(axis === "x" ? "resizing-x" : "resizing-y");
    const move = (ev: MouseEvent) => {
      const now = axis === "x" ? ev.clientX : ev.clientY;
      onDrag(now - last);
      last = now;
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      document.body.classList.remove("resizing-x", "resizing-y");
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };
  return <div className={`splitter splitter-${axis}`} role="separator" aria-orientation={axis === "x" ? "vertical" : "horizontal"} aria-label={label} title="Drag to resize (double-click to reset)" onMouseDown={start} onDoubleClick={onReset} />;
}
