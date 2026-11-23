import { useEffect, useMemo, useRef, useState } from "react";
import type { CellValue } from "./api";

/** The error text of `JSON.parse`, or null when `text` is valid JSON. */
export function jsonProblem(text: string): string | null {
  try { JSON.parse(text); return null; } catch (e) { return e instanceof Error ? e.message : String(e); }
}

/**
 * The editor window for a long value (TEXT, MEDIUMTEXT, LONGTEXT, JSON): a big monospace box instead of a one-line cell, like SQLyog's.
 * Apply stages the value in the grid (nothing is written until Save); a read-only table or connection opens it as a viewer.
 */
export default function CellModal(props: {
  column: string;
  type: string;
  isJson: boolean;
  initial: string | null;
  editable: boolean;
  nullable: boolean;
  /** The shown value was cut at the preview limit and the full one could not be fetched. */
  cut?: boolean;
  onApply(v: CellValue): void;
  onClose(): void;
}) {
  const [text, setText] = useState(props.initial ?? "");
  const [isNull, setIsNull] = useState(props.initial === null);
  const area = useRef<HTMLTextAreaElement>(null);
  useEffect(() => { area.current?.focus(); }, []);

  const problem = useMemo(() => (text.trim() ? jsonProblem(text) : null), [text]);
  const looksJson = useMemo(() => { const t = text.trim(); return (t.startsWith("{") || t.startsWith("[")) && !problem; }, [text, problem]);
  const bytes = useMemo(() => new TextEncoder().encode(text).length, [text]);
  const dirty = isNull ? props.initial !== null : text !== (props.initial ?? "") || props.initial === null;
  const blocked = props.isJson && !isNull && problem !== null && text.trim() !== "";
  const readOnly = !props.editable;

  const format = () => { try { setText(JSON.stringify(JSON.parse(text), null, 2)); setIsNull(false); } catch { /* the badge already says why */ } };
  const minify = () => { try { setText(JSON.stringify(JSON.parse(text))); setIsNull(false); } catch { /* the badge already says why */ } };
  const apply = () => {
    if (blocked || !dirty) return;
    props.onApply(isNull ? { kind: "null" } : { kind: "text", value: text });
  };

  return (
    <div className="overlay" onMouseDown={props.onClose}>
      <div className="dialog cell-modal" role="dialog" aria-label={`Value of ${props.column}`} onMouseDown={(e) => e.stopPropagation()}
           onKeyDown={(e) => {
             if (e.key === "Escape") { e.stopPropagation(); props.onClose(); }
             else if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && !readOnly) { e.preventDefault(); apply(); }
           }}>
        <h3>{props.column} <span className="dim">{props.type}{readOnly ? " · read-only" : ""}</span></h3>
        {props.cut && <div className="banner">Only the first part of this value is loaded, so it cannot be edited here.</div>}
        <textarea ref={area} className="cell-modal-text" value={isNull ? "" : text} readOnly={readOnly || props.cut} spellCheck={false}
                  placeholder={isNull ? "NULL" : undefined}
                  onChange={(e) => { setText(e.target.value); setIsNull(false); }} />
        <div className="cell-modal-foot">
          <span className="dim small">{isNull ? "NULL" : `${text.length.toLocaleString()} characters · ${bytes.toLocaleString()} bytes`}</span>
          {(props.isJson || looksJson) && !isNull && text.trim() !== "" && (
            <span className={`json-badge ${problem ? "bad" : "good"}`} title={problem ?? undefined}>{problem ? `Invalid JSON: ${problem}` : "Valid JSON"}</span>
          )}
          <span className="spacer" />
          {(props.isJson || looksJson) && !isNull && !problem && <>
            <button onClick={format} disabled={readOnly}>Format</button>
            <button onClick={minify} disabled={readOnly}>Minify</button>
          </>}
          <button onClick={() => navigator.clipboard?.writeText(text).catch(() => {})} disabled={isNull}>Copy</button>
        </div>
        <div className="actions">
          {!readOnly && <button onClick={() => setIsNull(true)} disabled={!props.nullable || isNull}>Set NULL</button>}
          <span className="spacer" />
          <button onClick={props.onClose}>{readOnly ? "Close" : "Cancel"}</button>
          {!readOnly && <button className="primary" disabled={blocked || !dirty || props.cut} onClick={apply} title="Stage this value (⌘↵). Nothing is written until you save the grid.">Apply</button>}
        </div>
      </div>
    </div>
  );
}
