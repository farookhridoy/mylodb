import { useState } from "react";

export interface FormField { key: string; label: string; value: string; placeholder?: string }

/** A small dialog with a few text fields and an optional checkbox: prefixes, copy suffixes and the like. */
export default function FormDialog(props: {
  title: string;
  fields: FormField[];
  option?: { label: string; initial: boolean };
  confirmLabel?: string;
  hint?: string;
  onCancel(): void;
  onSubmit(values: Record<string, string>, option: boolean): void;
}) {
  const [values, setValues] = useState<Record<string, string>>(Object.fromEntries(props.fields.map((f) => [f.key, f.value])));
  const [option, setOption] = useState(props.option?.initial ?? false);
  const ready = props.fields.every((f) => values[f.key].trim() !== "");
  return (
    <div className="overlay">
      <form className="dialog" role="dialog" aria-label={props.title} onSubmit={(e) => { e.preventDefault(); if (ready) props.onSubmit(values, option); }}>
        <h3>{props.title}</h3>
        {props.hint && <div className="dim small">{props.hint}</div>}
        {props.fields.map((f, i) => (
          <label key={f.key}>{f.label}
            <input autoFocus={i === 0} value={values[f.key]} placeholder={f.placeholder} onChange={(e) => setValues({ ...values, [f.key]: e.target.value })} />
          </label>
        ))}
        {props.option && <label className="check"><input type="checkbox" checked={option} onChange={(e) => setOption(e.target.checked)} /> {props.option.label}</label>}
        <div className="actions">
          <button type="button" onClick={props.onCancel}>Cancel</button>
          <button type="submit" className="primary" disabled={!ready}>{props.confirmLabel ?? "Next"}</button>
        </div>
      </form>
    </div>
  );
}
