import { useEffect, useMemo, useState } from "react";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { dataApi, type Assign, type CellValue, type ColumnMeta } from "./api";
import { b64ToBytes, bytesToText, hexDump, imageMime } from "./gridUtil";

type Mode = "text" | "json" | "hex" | "image";

/** Viewer/editor for the active cell: text, JSON, hex and image views; upload and save for binary data. */
export default function ValuePanel(props: {
  connectionId: number; db: string; table: string;
  column: ColumnMeta | undefined;
  shown: string | null;            // what the grid displays (may be a truncated preview)
  isLong: boolean;                 // the displayed text was cut
  key_: Assign[] | null;           // row key, when the row exists on the server
  editable: boolean;
  onStage(v: CellValue): void;
}) {
  const { column, key_, isLong } = props;
  const [bytes, setBytes] = useState<Uint8Array | null>(null);
  const [isNull, setIsNull] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [truncated, setTruncated] = useState(false);
  const [mode, setMode] = useState<Mode>("text");
  const [draft, setDraft] = useState("");

  // Load the full value for binary or long cells; short text is shown as displayed.
  useEffect(() => {
    let live = true;
    setBytes(null); setLoadError(null); setTruncated(false); setIsNull(props.shown === null);
    if (!column) return;
    const needsFetch = (column.binary || isLong) && key_ && props.shown !== null;
    if (needsFetch) {
      dataApi.cell(props.connectionId, props.db, props.table, key_!, column.name).then((d) => {
        if (!live) return;
        setIsNull(d.is_null);
        setTruncated(d.truncated);
        setBytes(d.base64 ? b64ToBytes(d.base64) : null);
      }).catch((e) => live && setLoadError(String(e)));
    } else if (props.shown !== null) {
      setBytes(new TextEncoder().encode(props.shown));
    }
    return () => { live = false; };
  }, [props.connectionId, props.db, props.table, column, key_, isLong, props.shown]);

  const text = useMemo(() => (bytes ? bytesToText(bytes) : null), [bytes]);
  const mime = useMemo(() => (bytes ? imageMime(bytes) : null), [bytes]);
  const json = useMemo(() => {
    if (text === null) return null;
    try { const v = JSON.parse(text); return typeof v === "object" && v !== null ? JSON.stringify(v, null, 2) : null; } catch { return null; }
  }, [text]);

  useEffect(() => { setDraft(text ?? ""); setMode(mime ? "image" : json ? "json" : text !== null && !column?.binary ? "text" : "hex"); }, [text, mime, json, column]);

  if (!column) return <div className="empty">Select a cell</div>;
  const canEditText = props.editable && !column.generated && !column.binary && !truncated && !loadError && (!isLong || bytes !== null);

  const upload = async () => {
    const path = await openDialog({ multiple: false, directory: false });
    if (typeof path === "string") props.onStage({ kind: "file", value: path });
  };
  const saveFile = async () => {
    if (!key_) return;
    const path = await saveDialog({ defaultPath: column.name + (mime ? "." + mime.split("/")[1] : ".bin") });
    if (path) await dataApi.saveCell(props.connectionId, props.db, props.table, key_, column.name, path).catch((e) => setLoadError(String(e)));
  };

  return (
    <div className="value-panel">
      <div className="vp-head">
        <b>{column.name}</b> <span className="dim">{column.column_type}{bytes && ` · ${bytes.length.toLocaleString()} B`}</span>
      </div>
      <div className="vp-tabs">
        {(["text", "json", "hex", "image"] as Mode[]).filter((m) => (m === "json" ? json : m === "image" ? mime : true)).map((m) => (
          <button key={m} className={mode === m ? "active" : ""} onClick={() => setMode(m)}>{m}</button>
        ))}
      </div>
      {loadError && <div className="error small">{loadError}</div>}
      {truncated && <div className="error small">Value is larger than the transfer limit; showing the first part only.</div>}
      <div className="vp-body">
        {isNull ? <div className="empty">NULL</div> : !bytes && !loadError ? <div className="empty">…</div> : mode === "image" && mime ? (
          <img alt="" src={`data:${mime};base64,${btoa(String.fromCharCode(...bytes!.slice(0, 8_000_000)))}`} />
        ) : mode === "hex" && bytes ? <pre>{hexDump(bytes)}</pre>
        : mode === "json" && json ? <pre>{json}</pre>
        : canEditText ? <textarea value={draft} onChange={(e) => setDraft(e.target.value)} spellCheck={false} />
        : <pre>{text ?? ""}</pre>}
      </div>
      {props.editable && !column.generated && (
        <div className="vp-actions">
          {canEditText && mode === "text" && <button className="primary" disabled={draft === (text ?? "")} onClick={() => props.onStage({ kind: "text", value: draft })}>Apply</button>}
          <button onClick={() => props.onStage({ kind: "null" })} disabled={!column.nullable}>Set NULL</button>
          {column.binary && <button onClick={upload}>Upload…</button>}
          {column.binary && key_ && !isNull && <button onClick={saveFile}>Save…</button>}
        </div>
      )}
      {!props.editable && column.binary && key_ && !isNull && <div className="vp-actions"><button onClick={saveFile}>Save…</button></div>}
    </div>
  );
}
