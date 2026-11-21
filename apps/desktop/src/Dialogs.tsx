import { useEffect, useState } from "react";
import Select from "./Select";
import { schemaApi } from "./api";

/** Ask for one line of text (and optionally a yes/no option). */
export function PromptDialog(props: {
  title: string; label: string; initial?: string; confirmLabel: string;
  option?: { label: string; initial: boolean };
  onCancel(): void; onSubmit(value: string, option: boolean): void;
}) {
  const [v, setV] = useState(props.initial ?? "");
  const [opt, setOpt] = useState(props.option?.initial ?? false);
  const go = () => v.trim() && props.onSubmit(v.trim(), opt);
  return (
    <div className="overlay" onMouseDown={props.onCancel}>
      <div className="dialog" onMouseDown={(e) => e.stopPropagation()}>
        <h3>{props.title}</h3>
        <label>{props.label}<input autoFocus value={v} onChange={(e) => setV(e.target.value)} onKeyDown={(e) => e.key === "Enter" && go()} /></label>
        {props.option && <label className="check"><input type="checkbox" checked={opt} onChange={(e) => setOpt(e.target.checked)} /> {props.option.label}</label>}
        <div className="actions">
          <button onClick={props.onCancel}>Cancel</button>
          <button className="primary" disabled={!v.trim()} onClick={go}>{props.confirmLabel}</button>
        </div>
      </div>
    </div>
  );
}

/** Destructive action: the user must type the object's name to enable the button. */
export function TypeConfirmDialog(props: { title: string; message: string; expect: string; confirmLabel: string; onCancel(): void; onConfirm(): void }) {
  const [v, setV] = useState("");
  return (
    <div className="overlay" onMouseDown={props.onCancel}>
      <div className="dialog" onMouseDown={(e) => e.stopPropagation()}>
        <h3>{props.title}</h3>
        <p>{props.message}</p>
        <label>Type <b>{props.expect}</b> to confirm<input autoFocus value={v} onChange={(e) => setV(e.target.value)} /></label>
        <div className="actions">
          <button onClick={props.onCancel}>Cancel</button>
          <button className="danger" disabled={v !== props.expect} onClick={props.onConfirm}>{props.confirmLabel}</button>
        </div>
      </div>
    </div>
  );
}

/** Pick a new default collation for a database; the statement itself goes through the Preview SQL dialog. */
export function AlterDatabaseDialog(props: { connectionId: number; database: string; onCancel(): void; onNext(collation: string): void }) {
  const [collations, setCollations] = useState<string[]>([]);
  const [collation, setCollation] = useState("");
  useEffect(() => { schemaApi.serverOptions(props.connectionId).then((o) => setCollations(o.charsets.flatMap(([, c]) => c))).catch(() => {}); }, [props.connectionId]);
  return (
    <div className="overlay" onMouseDown={props.onCancel}>
      <div className="dialog" role="dialog" aria-label="Alter database" onMouseDown={(e) => e.stopPropagation()}>
        <h3>Alter database {props.database}</h3>
        <label>Default collation
          <Select value={collation} onChange={(e) => setCollation(e.target.value)}>
            <option value="">Choose a collation…</option>
            {collations.map((c) => <option key={c} value={c}>{c}</option>)}
          </Select>
        </label>
        <div className="dim small">Applies to tables created later. Existing tables keep their own collation.</div>
        <div className="actions">
          <button onClick={props.onCancel}>Cancel</button>
          <button className="primary" disabled={!collation} onClick={() => props.onNext(collation)}>Preview SQL</button>
        </div>
      </div>
    </div>
  );
}
