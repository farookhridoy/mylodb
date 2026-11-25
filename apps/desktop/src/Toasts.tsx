import { useCallback, useEffect, useRef, useState } from "react";
import Icon from "./Icon";

export type Toast = { id: number; text: string; kind: "info" | "error" | "progress" };

/** Messages that only describe the current state; they stay in the status bar and do not pop up. */
const QUIET = /^(Connecting…|Not connected|Connected ·|Disconnected)/;
const ERROR = /^(Error|Not possible|Could not|Failed)/i;

/** Flash messages: every result a user should notice (done, failed, copied) appears here in addition to the status bar. */
export function useToasts() {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const next = useRef(1);
  const dismiss = useCallback((id: number) => setToasts((l) => l.filter((t) => t.id !== id)), []);
  const push = useCallback((text: string) => {
    if (!text || QUIET.test(text)) return;
    const kind: Toast["kind"] = ERROR.test(text) ? "error" : text.endsWith("…") ? "progress" : "info";
    // a progress message ("Optimizing…") is replaced by the result that follows it
    setToasts((l) => [...l.filter((t) => t.kind !== "progress"), { id: next.current++, text, kind }].slice(-4));
  }, []);
  return { toasts, push, dismiss };
}

function ToastItem({ t, onClose }: { t: Toast; onClose(): void }) {
  const [hover, setHover] = useState(false);
  const [copied, setCopied] = useState(false);
  // info goes away by itself; an error stays longer; a progress message stays until the result replaces it. Hovering holds it.
  const ms = t.kind === "error" ? 15000 : t.kind === "info" ? Math.min(12000, 4000 + t.text.length * 40) : 0;
  useEffect(() => {
    if (!ms || hover || copied) return;
    const h = setTimeout(onClose, ms);
    return () => clearTimeout(h);
  }, [ms, hover, copied, onClose]);
  const copy = async () => {
    try { await navigator.clipboard.writeText(t.text); setCopied(true); setTimeout(() => setCopied(false), 1500); } catch { /* the text can still be selected by hand */ }
  };
  return (
    <div className={`toast ${t.kind}`} role={t.kind === "error" ? "alert" : "status"} onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}>
      <span className="toast-text">{t.text}</span>
      <span className="toast-actions">
        <button className="toast-btn" title={copied ? "Copied" : "Copy message"} aria-label="Copy message" onClick={copy}>{copied ? "✓" : <Icon name="copy" size={14} />}</button>
        <button className="toast-btn" title="Dismiss" aria-label="Dismiss message" onClick={onClose}>×</button>
      </span>
    </div>
  );
}

export function ToastHost({ toasts, dismiss }: { toasts: Toast[]; dismiss(id: number): void }) {
  if (!toasts.length) return null;
  return <div className="toasts" aria-live="polite">{toasts.map((t) => <ToastItem key={t.id} t={t} onClose={() => dismiss(t.id)} />)}</div>;
}
