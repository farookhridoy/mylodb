import { Fragment, useCallback, useEffect, useMemo, useState } from "react";
import Select from "./Select";
import Icon from "./Icon";
import {
  schemaApi, type ColumnDef, type ColumnDefault, type ForeignKeyDef, type IndexDef, type IndexKind, type Node, type RefAction,
  type ServerOptions, type TableDef,
} from "./api";

interface TypeInfo { name: string; group: string; params?: string; numeric?: boolean; integer?: boolean; text?: boolean; hint?: string }

const TYPES: TypeInfo[] = [
  { name: "tinyint", group: "Integer", numeric: true, integer: true }, { name: "smallint", group: "Integer", numeric: true, integer: true },
  { name: "mediumint", group: "Integer", numeric: true, integer: true }, { name: "int", group: "Integer", numeric: true, integer: true },
  { name: "bigint", group: "Integer", numeric: true, integer: true },
  { name: "decimal", group: "Number", numeric: true, params: "10,2", hint: "precision,scale" },
  { name: "float", group: "Number", numeric: true }, { name: "double", group: "Number", numeric: true },
  { name: "bit", group: "Number", params: "1", hint: "bits" },
  { name: "char", group: "Text", text: true, params: "1", hint: "length" }, { name: "varchar", group: "Text", text: true, params: "255", hint: "length" },
  { name: "tinytext", group: "Text", text: true }, { name: "text", group: "Text", text: true },
  { name: "mediumtext", group: "Text", text: true }, { name: "longtext", group: "Text", text: true },
  { name: "enum", group: "List", text: true, params: "'a','b'", hint: "'a','b',…" }, { name: "set", group: "List", text: true, params: "'a','b'", hint: "'a','b',…" },
  { name: "binary", group: "Binary", params: "16", hint: "length" }, { name: "varbinary", group: "Binary", params: "255", hint: "length" },
  { name: "tinyblob", group: "Binary" }, { name: "blob", group: "Binary" }, { name: "mediumblob", group: "Binary" }, { name: "longblob", group: "Binary" },
  { name: "date", group: "Date/time" }, { name: "time", group: "Date/time", hint: "fsp 0–6" }, { name: "datetime", group: "Date/time", hint: "fsp 0–6" },
  { name: "timestamp", group: "Date/time", hint: "fsp 0–6" }, { name: "year", group: "Date/time" },
  { name: "json", group: "Other" },
];
const typeInfo = (n: string) => TYPES.find((t) => t.name === n);
const GROUPS = [...new Set(TYPES.map((t) => t.group))];
const REF_ACTIONS: RefAction[] = ["restrict", "cascade", "set_null", "no_action", "set_default"];
const KINDS: IndexKind[] = ["primary", "unique", "index", "fulltext", "spatial"];

export const newColumn = (name = ""): ColumnDef => ({
  name, original_name: null, data_type: "varchar", params: "255", unsigned: false, zerofill: false, nullable: true, default: { kind: "none" },
  auto_increment: false, charset: null, collation: null, comment: "", on_update: null, generated: null, invisible: false,
});

export function blankTable(opts: ServerOptions | null): TableDef {
  const cs = opts?.charsets.find((c) => c[0] === "utf8mb4");
  return {
    name: "", engine: "InnoDB", charset: cs ? "utf8mb4" : null, collation: cs ? cs[2] : null, comment: "",
    columns: [{ ...newColumn("id"), data_type: "int", params: null, unsigned: true, nullable: false, auto_increment: true }],
    indexes: [{ name: "PRIMARY", original_name: null, kind: "primary", columns: [{ name: "id", length: null, desc: false }] }],
    foreign_keys: [],
  };
}

/** A blank table with the name and number of columns asked for: `id` first, then `column_2`… as nullable varchar(255). */
function withInitial(def: TableDef, initial?: { name: string; columns: number }): TableDef {
  if (!initial) return def;
  const extra = Array.from({ length: Math.max(0, Math.min(initial.columns, 200) - 1) }, (_, i) => ({ ...newColumn(`column_${i + 2}`), data_type: "varchar", params: "255", nullable: true }));
  return { ...def, name: initial.name, columns: [...def.columns, ...extra] };
}

type Tab = "columns" | "indexes" | "fks" | "options" | "sql";

export default function TableDesigner(props: {
  connectionId: number; db: string; table: string | null; nodes: Node[]; ensureColumns(db: string): Promise<unknown>;
  onApplied(newName: string, statements: string[]): void;
  onOpenData(db: string, table: string): void;
  /** The connection is read-only: the definition can be looked at but not applied. */
  readOnly?: boolean;
  /** For a new table: the name and how many columns to start with (the Create new table box of the Structure page). */
  initial?: { name: string; columns: number };
}) {
  const { connectionId: cid, db } = props;
  const [original, setOriginal] = useState<TableDef | null>(null);
  const [draft, setDraft] = useState<TableDef | null>(null);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [opts, setOpts] = useState<ServerOptions | null>(null);
  const [preview, setPreview] = useState<{ statements: string[]; errors: string[] }>({ statements: [], errors: [] });
  const [tab, setTab] = useState<Tab>("columns");
  const [sel, setSel] = useState(0);
  const [selIdx, setSelIdx] = useState(0);
  const [selFk, setSelFk] = useState(0);
  const [moreOpen, setMoreOpen] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      const o = await schemaApi.serverOptions(cid);
      setOpts(o);
      if (props.table) {
        const r = await schemaApi.tableDef(cid, db, props.table);
        setOriginal(r.def); setDraft(structuredClone(r.def)); setWarnings(r.warnings);
      } else {
        setOriginal(null); setDraft(withInitial(blankTable(o), props.initial)); setWarnings([]);
      }
      setError(null);
    } catch (e) { setError(String(e)); }
  }, [cid, db, props.table]);
  useEffect(() => { load(); }, [load]);

  // Live preview: the diff engine is pure, so it can run on every edit.
  useEffect(() => {
    if (!draft) return;
    let live = true;
    const t = setTimeout(() => schemaApi.preview(db, original, draft).then((p) => live && setPreview(p)).catch(() => {}), 150);
    return () => { live = false; clearTimeout(t); };
  }, [draft, original, db]);

  // the foreign key editor needs the columns of the referenced database as well as of this one
  useEffect(() => { props.ensureColumns(db); }, [db]); // eslint-disable-line react-hooks/exhaustive-deps
  const dbs = useMemo(() => props.nodes.filter((n) => n.kind === "database").map((n) => n.name), [props.nodes]);
  const tablesOf = (schema: string) => props.nodes.filter((n) => n.database === schema && (n.kind === "table")).map((n) => n.name);
  const columnsOf = (schema: string, table: string) => props.nodes.filter((n) => n.database === schema && n.kind === "column" && n.parent === table).map((n) => n.name);

  if (error && !draft) return <div className="error">{error}</div>;
  if (!draft) return <div className="empty">Loading…</div>;

  const set = (patch: Partial<TableDef>) => setDraft({ ...draft, ...patch });
  const dirty = preview.statements.length > 0;
  const pk = draft.indexes.find((i) => i.kind === "primary");

  // ---------- column edits (names are kept in sync with indexes and foreign keys) ----------
  const updateCol = (i: number, patch: Partial<ColumnDef>) => {
    const old = draft.columns[i];
    const next = { ...old, ...patch };
    const info = typeInfo(next.data_type);
    if (patch.data_type && patch.data_type !== old.data_type) {
      next.params = info?.params ?? null;
      if (!info?.numeric) { next.unsigned = false; next.zerofill = false; }
      if (!info?.integer) next.auto_increment = false;
      if (!info?.text) { next.charset = null; next.collation = null; }
    }
    if (next.auto_increment) next.nullable = false;
    const columns = draft.columns.map((c, k) => (k === i ? next : c));
    let { indexes, foreign_keys } = draft;
    if (patch.name !== undefined && patch.name !== old.name) {
      indexes = indexes.map((ix) => ({ ...ix, columns: ix.columns.map((c) => (c.name === old.name ? { ...c, name: next.name } : c)) }));
      foreign_keys = foreign_keys.map((f) => ({ ...f, columns: f.columns.map((c) => (c === old.name ? next.name : c)) }));
    }
    setDraft({ ...draft, columns, indexes, foreign_keys });
  };

  const addColumn = () => {
    const n = draft.columns.length + 1;
    const at = Math.min(sel + 1, draft.columns.length);
    const columns = [...draft.columns];
    columns.splice(at, 0, newColumn(`column${n}`));
    set({ columns });
    setSel(at);
  };

  const removeColumn = (i: number) => {
    const gone = draft.columns[i].name;
    const indexes = draft.indexes
      .map((ix) => ({ ...ix, columns: ix.columns.filter((c) => c.name !== gone) }))
      .filter((ix) => ix.columns.length > 0);
    const foreign_keys = draft.foreign_keys.filter((f) => !f.columns.includes(gone));
    setDraft({ ...draft, columns: draft.columns.filter((_, k) => k !== i), indexes, foreign_keys });
    setSel(Math.max(0, i - 1));
  };

  const moveColumn = (i: number, d: -1 | 1) => {
    const j = i + d;
    if (j < 0 || j >= draft.columns.length) return;
    const columns = [...draft.columns];
    [columns[i], columns[j]] = [columns[j], columns[i]];
    set({ columns });
    setSel(j);
  };

  const togglePk = (name: string) => {
    // One state update: indexes and the NOT NULL flag must change together (a second setDraft would
    // start from the stale draft and undo the first).
    const indexes = [...draft.indexes];
    const at = indexes.findIndex((i) => i.kind === "primary");
    const adding = !(at >= 0 && indexes[at].columns.some((c) => c.name === name));
    if (at < 0) {
      indexes.unshift({ name: "PRIMARY", original_name: null, kind: "primary", columns: [{ name, length: null, desc: false }] });
    } else {
      const cols = adding ? [...indexes[at].columns, { name, length: null, desc: false }] : indexes[at].columns.filter((c) => c.name !== name);
      if (cols.length === 0) indexes.splice(at, 1); else indexes[at] = { ...indexes[at], columns: cols };
    }
    // a primary key column cannot be NULL
    const columns = adding ? draft.columns.map((c) => (c.name === name ? { ...c, nullable: false } : c)) : draft.columns;
    setDraft({ ...draft, indexes, columns });
  };

  const apply = async () => {
    setBusy(true);
    try {
      const statements = await schemaApi.apply(cid, db, original, draft);
      setNotice(`Applied ${statements.length} statement(s)`);
      setError(null);
      props.onApplied(draft.name, statements);
      if (props.table || true) {
        const r = await schemaApi.tableDef(cid, db, draft.name);
        setOriginal(r.def); setDraft(structuredClone(r.def)); setWarnings(r.warnings);
      }
    } catch (e) { setError(String(e)); }
    setBusy(false);
  };

  const collationsOf = (cs: string | null) => opts?.charsets.find((c) => c[0] === cs)?.[1] ?? [];

  return (
    <div className="designer">
      <div className="dz-head">
        <input className="dz-name" placeholder="table_name" value={draft.name} onChange={(e) => set({ name: e.target.value })} />
        <span className="dim">{db}{props.table ? "" : " · new table"}</span>
        <span className="grow" />
        {props.table && <button onClick={() => props.onOpenData(db, props.table!)}>Open data</button>}
        <button disabled={!dirty} onClick={() => original ? setDraft(structuredClone(original)) : setDraft(blankTable(opts))}>Revert</button>
        <button className="primary" disabled={!dirty || preview.errors.length > 0 || busy || props.readOnly} onClick={apply} title={props.readOnly ? "This connection is read-only" : undefined}>
          {busy ? "Applying…" : props.table ? "Apply changes" : "Create table"}
        </button>
      </div>
      {props.readOnly && <div className="banner"><Icon name="lock" size={12} /> Read-only connection: you can look at the definition and the SQL, but not apply changes.</div>}
      {warnings.map((w) => <div key={w} className="banner">{w}</div>)}
      {error && <div className="banner bad">{error} <button className="mini" onClick={() => setError(null)}><Icon name="close" size={10} /></button></div>}
      {notice && <div className="banner ok">{notice}</div>}
      {preview.errors.length > 0 && <div className="banner bad">{preview.errors.map((e) => <div key={e}>• {e}</div>)}</div>}

      <div className="dz-tabs">
        {([["columns", `Columns (${draft.columns.length})`], ["indexes", `Indexes (${draft.indexes.length})`], ["fks", `Foreign keys (${draft.foreign_keys.length})`], ["options", "Options"], ["sql", `SQL${dirty ? " •" : ""}`]] as [Tab, string][]).map(([t, label]) => (
          <button key={t} className={tab === t ? "active" : ""} onClick={() => setTab(t)}>{label}</button>
        ))}
      </div>

      <div className="dz-body">
        {tab === "columns" && (
          <>
            <div className="dz-actions">
              <button onClick={addColumn}>＋ Add column</button>
              <button disabled={draft.columns.length < 2} onClick={() => removeColumn(sel)}>－ Remove</button>
              <button onClick={() => moveColumn(sel, -1)} disabled={sel === 0}>↑</button>
              <button onClick={() => moveColumn(sel, 1)} disabled={sel >= draft.columns.length - 1}>↓</button>
            </div>
            <table className="grid dz-grid">
              <thead><tr><th>PK</th><th>Name</th><th>Type</th><th>Length / values</th><th>Unsigned</th><th>NOT NULL</th><th>Auto&nbsp;inc</th><th>Default</th><th>Comment</th><th /></tr></thead>
              <tbody>
                {draft.columns.map((c, i) => {
                  const info = typeInfo(c.data_type);
                  const isPk = !!pk?.columns.some((x) => x.name === c.name);
                  return (
                    <Fragment key={i}>
                      <tr className={`${i === sel ? "row-selected" : ""} ${c.original_name === null ? "row-new" : ""}`} onClick={() => setSel(i)}>
                        <td><input type="checkbox" checked={isPk} onChange={() => togglePk(c.name)} title="Primary key" /></td>
                        <td><input className="cell-input" value={c.name} onChange={(e) => updateCol(i, { name: e.target.value })} /></td>
                        <td>
                          <Select value={c.data_type} onChange={(e) => updateCol(i, { data_type: e.target.value })}>
                            {!info && <option value={c.data_type}>{c.data_type}</option>}
                            {GROUPS.map((g) => <optgroup key={g} label={g}>{TYPES.filter((t) => t.group === g).map((t) => <option key={t.name} value={t.name}>{t.name}</option>)}</optgroup>)}
                          </Select>
                        </td>
                        <td><input className="cell-input" disabled={!info?.params && !info?.hint} placeholder={info?.hint ?? ""} value={c.params ?? ""} onChange={(e) => updateCol(i, { params: e.target.value || null })} /></td>
                        <td><input type="checkbox" disabled={!info?.numeric} checked={c.unsigned} onChange={(e) => updateCol(i, { unsigned: e.target.checked })} /></td>
                        <td><input type="checkbox" checked={!c.nullable} onChange={(e) => updateCol(i, { nullable: !e.target.checked })} /></td>
                        <td><input type="checkbox" disabled={!info?.integer} checked={c.auto_increment} onChange={(e) => updateCol(i, { auto_increment: e.target.checked })} /></td>
                        <td><DefaultEditor value={c.default} disabled={!!c.generated} onChange={(d) => updateCol(i, { default: d })} /></td>
                        <td><input className="cell-input" value={c.comment} onChange={(e) => updateCol(i, { comment: e.target.value })} /></td>
                        <td><button className="mini" title="More options" onClick={(e) => { e.stopPropagation(); setMoreOpen(moreOpen === i ? null : i); }}>⋯</button></td>
                      </tr>
                      {moreOpen === i && (
                        <tr className="more-row"><td colSpan={10}>
                          <div className="more">
                            {info?.text && (
                              <>
                                <label>Character set
                                  <Select value={c.charset ?? ""} onChange={(e) => updateCol(i, { charset: e.target.value || null, collation: null })}>
                                    <option value="">(table default)</option>{opts?.charsets.map((x) => <option key={x[0]} value={x[0]}>{x[0]}</option>)}
                                  </Select></label>
                                <label>Collation
                                  <Select value={c.collation ?? ""} disabled={!c.charset} onChange={(e) => updateCol(i, { collation: e.target.value || null })}>
                                    <option value="">(default)</option>{collationsOf(c.charset).map((x) => <option key={x} value={x}>{x}</option>)}
                                  </Select></label>
                              </>
                            )}
                            {(c.data_type === "datetime" || c.data_type === "timestamp") && (
                              <label>On update<input value={c.on_update ?? ""} placeholder="CURRENT_TIMESTAMP" onChange={(e) => updateCol(i, { on_update: e.target.value || null })} /></label>
                            )}
                            <label>Generated expression
                              <input value={c.generated?.expression ?? ""} placeholder="e.g. price * qty (leave empty for a normal column)"
                                onChange={(e) => updateCol(i, { generated: e.target.value ? { expression: e.target.value, stored: c.generated?.stored ?? false } : null })} /></label>
                            {c.generated && <label className="check"><input type="checkbox" checked={c.generated.stored} onChange={(e) => updateCol(i, { generated: { ...c.generated!, stored: e.target.checked } })} /> Stored</label>}
                            <label className="check"><input type="checkbox" checked={c.invisible} onChange={(e) => updateCol(i, { invisible: e.target.checked })} /> Invisible</label>
                          </div>
                        </td></tr>
                      )}
                    </Fragment>
                  );
                })}
              </tbody>
            </table>
          </>
        )}

        {tab === "indexes" && (
          <div className="dz-split">
            <div className="dz-list">
              <div className="dz-actions">
                <button onClick={() => { set({ indexes: [...draft.indexes, { name: `idx_${draft.indexes.length + 1}`, original_name: null, kind: "index", columns: [] }] }); setSelIdx(draft.indexes.length); }}>＋ Add index</button>
                <button disabled={!draft.indexes.length} onClick={() => { set({ indexes: draft.indexes.filter((_, k) => k !== selIdx) }); setSelIdx(0); }}>－ Remove</button>
              </div>
              {draft.indexes.map((ix, i) => (
                <div key={i} className={`dz-item ${i === selIdx ? "active" : ""}`} onClick={() => setSelIdx(i)}>
                  <b>{ix.kind === "primary" ? "PRIMARY" : ix.name}</b> <span className="dim">{ix.kind} · {ix.columns.map((c) => c.name).join(", ") || "no columns"}</span>
                </div>
              ))}
            </div>
            <div className="dz-edit">
              {draft.indexes[selIdx] ? (
                <IndexEditor ix={draft.indexes[selIdx]} columns={draft.columns.map((c) => c.name)}
                  onChange={(ix) => set({ indexes: draft.indexes.map((x, k) => (k === selIdx ? ix : x)) })} />
              ) : <div className="empty">No indexes</div>}
            </div>
          </div>
        )}

        {tab === "fks" && (
          <div className="dz-split">
            <div className="dz-list">
              <div className="dz-actions">
                <button onClick={() => { set({ foreign_keys: [...draft.foreign_keys, { name: `fk_${draft.name || "t"}_${draft.foreign_keys.length + 1}`, original_name: null, columns: [], ref_schema: null, ref_table: "", ref_columns: [], on_delete: "restrict", on_update: "restrict" }] }); setSelFk(draft.foreign_keys.length); }}>＋ Add foreign key</button>
                <button disabled={!draft.foreign_keys.length} onClick={() => { set({ foreign_keys: draft.foreign_keys.filter((_, k) => k !== selFk) }); setSelFk(0); }}>－ Remove</button>
              </div>
              {draft.foreign_keys.map((f, i) => (
                <div key={i} className={`dz-item ${i === selFk ? "active" : ""}`} onClick={() => setSelFk(i)}>
                  <b>{f.name}</b> <span className="dim">({f.columns.join(", ")}) → {f.ref_table || "?"}</span>
                </div>
              ))}
            </div>
            <div className="dz-edit">
              {draft.foreign_keys[selFk] ? (
                <FkEditor fk={draft.foreign_keys[selFk]} db={db} dbs={dbs} ensureColumns={props.ensureColumns} localColumns={draft.columns.map((c) => c.name)}
                  tablesOf={tablesOf} columnsOf={columnsOf}
                  onChange={(f) => set({ foreign_keys: draft.foreign_keys.map((x, k) => (k === selFk ? f : x)) })} />
              ) : <div className="empty">No foreign keys</div>}
            </div>
          </div>
        )}

        {tab === "options" && (
          <div className="dz-form">
            <label>Engine
              <Select value={draft.engine ?? ""} onChange={(e) => set({ engine: e.target.value || null })}>
                <option value="">(server default)</option>{opts?.engines.map((e) => <option key={e} value={e}>{e}</option>)}
              </Select></label>
            <label>Character set
              <Select value={draft.charset ?? ""} onChange={(e) => { const cs = e.target.value || null; set({ charset: cs, collation: opts?.charsets.find((c) => c[0] === cs)?.[2] ?? null }); }}>
                <option value="">(server default)</option>{opts?.charsets.map((c) => <option key={c[0]} value={c[0]}>{c[0]}</option>)}
              </Select></label>
            <label>Collation
              <Select value={draft.collation ?? ""} disabled={!draft.charset} onChange={(e) => set({ collation: e.target.value || null })}>
                <option value="">(default)</option>{collationsOf(draft.charset).map((c) => <option key={c} value={c}>{c}</option>)}
              </Select></label>
            <label>Comment<input value={draft.comment} onChange={(e) => set({ comment: e.target.value })} /></label>
            <p className="dim small">Changing the character set affects new columns; existing columns keep their own until altered.</p>
          </div>
        )}

        {tab === "sql" && (
          <div className="dz-sql">
            {preview.statements.length === 0 ? <div className="empty">No changes</div> : (
              <>
                <code className="preview">{preview.statements.join(";\n\n") + ";"}</code>
                <div className="actions"><button onClick={() => navigator.clipboard.writeText(preview.statements.join(";\n\n") + ";")}>Copy</button></div>
                {preview.statements.length > 1 && <p className="dim small">MySQL DDL is not transactional: statements run in this order and an earlier one is not undone if a later one fails.</p>}
              </>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

function DefaultEditor(props: { value: ColumnDefault; disabled: boolean; onChange(d: ColumnDefault): void }) {
  const v = props.value;
  const text = v.kind === "literal" || v.kind === "expression" ? v.value : "";
  return (
    <div className="def-edit">
      <Select disabled={props.disabled} value={v.kind} onChange={(e) => {
        const k = e.target.value as ColumnDefault["kind"];
        props.onChange(k === "literal" ? { kind: "literal", value: text } : k === "expression" ? { kind: "expression", value: text || "CURRENT_TIMESTAMP" } : { kind: k });
      }}>
        <option value="none">—</option><option value="null">NULL</option><option value="literal">value</option><option value="expression">expression</option>
      </Select>
      {(v.kind === "literal" || v.kind === "expression") && (
        <input className="cell-input" value={text} onChange={(e) => props.onChange({ kind: v.kind, value: e.target.value } as ColumnDefault)} />
      )}
    </div>
  );
}

function IndexEditor(props: { ix: IndexDef; columns: string[]; onChange(ix: IndexDef): void }) {
  const { ix } = props;
  const free = props.columns.filter((c) => !ix.columns.some((x) => x.name === c));
  const setCol = (i: number, p: Partial<IndexDef["columns"][number]>) => props.onChange({ ...ix, columns: ix.columns.map((c, k) => (k === i ? { ...c, ...p } : c)) });
  return (
    <div className="dz-form">
      <label>Name<input disabled={ix.kind === "primary"} value={ix.kind === "primary" ? "PRIMARY" : ix.name} onChange={(e) => props.onChange({ ...ix, name: e.target.value })} /></label>
      <label>Type
        <Select value={ix.kind} onChange={(e) => props.onChange({ ...ix, kind: e.target.value as IndexKind, name: e.target.value === "primary" ? "PRIMARY" : ix.name === "PRIMARY" ? "idx" : ix.name })}>
          {KINDS.map((k) => <option key={k} value={k}>{k}</option>)}
        </Select></label>
      <div className="dz-cols">
        <div className="dim">Columns (order matters)</div>
        {ix.columns.map((c, i) => (
          <div key={c.name} className="dz-colrow">
            <span className="grow">{c.name}</span>
            <input className="cell-input narrow" placeholder="prefix" title="Prefix length" value={c.length ?? ""} onChange={(e) => setCol(i, { length: e.target.value ? Number(e.target.value) : null })} />
            <label className="check"><input type="checkbox" checked={c.desc} onChange={(e) => setCol(i, { desc: e.target.checked })} /> DESC</label>
            <button className="mini" disabled={i === 0} onClick={() => { const cols = [...ix.columns]; [cols[i - 1], cols[i]] = [cols[i], cols[i - 1]]; props.onChange({ ...ix, columns: cols }); }}>↑</button>
            <button className="mini" disabled={i === ix.columns.length - 1} onClick={() => { const cols = [...ix.columns]; [cols[i + 1], cols[i]] = [cols[i], cols[i + 1]]; props.onChange({ ...ix, columns: cols }); }}>↓</button>
            <button className="mini" onClick={() => props.onChange({ ...ix, columns: ix.columns.filter((_, k) => k !== i) })}><Icon name="close" size={10} /></button>
          </div>
        ))}
        <Select value="" onChange={(e) => e.target.value && props.onChange({ ...ix, columns: [...ix.columns, { name: e.target.value, length: null, desc: false }] })}>
          <option value="">＋ add column…</option>{free.map((c) => <option key={c} value={c}>{c}</option>)}
        </Select>
      </div>
    </div>
  );
}

function FkEditor(props: {
  fk: ForeignKeyDef; db: string; dbs: string[]; localColumns: string[]; ensureColumns(db: string): Promise<unknown>;
  tablesOf(schema: string): string[]; columnsOf(schema: string, table: string): string[]; onChange(f: ForeignKeyDef): void;
}) {
  const { fk } = props;
  const schema = fk.ref_schema ?? props.db;
  useEffect(() => { props.ensureColumns(schema); }, [schema]); // eslint-disable-line react-hooks/exhaustive-deps
  const refCols = fk.ref_table ? props.columnsOf(schema, fk.ref_table) : [];
  const setPair = (i: number, local: string | null, remote: string | null) => {
    const columns = [...fk.columns]; const ref_columns = [...fk.ref_columns];
    if (local !== null) columns[i] = local;
    if (remote !== null) ref_columns[i] = remote;
    props.onChange({ ...fk, columns, ref_columns });
  };
  return (
    <div className="dz-form">
      <label>Name<input value={fk.name} onChange={(e) => props.onChange({ ...fk, name: e.target.value })} /></label>
      <label>Referenced database
        <Select value={schema} onChange={(e) => props.onChange({ ...fk, ref_schema: e.target.value === props.db ? null : e.target.value, ref_table: "", ref_columns: [] })}>
          {props.dbs.map((d) => <option key={d} value={d}>{d}</option>)}
        </Select></label>
      <label>Referenced table
        <Select value={fk.ref_table} onChange={(e) => props.onChange({ ...fk, ref_table: e.target.value, ref_columns: [] })}>
          <option value="">(choose)</option>{props.tablesOf(schema).map((t) => <option key={t} value={t}>{t}</option>)}
        </Select></label>
      <div className="dz-cols">
        <div className="dim">Column pairs</div>
        {fk.columns.map((c, i) => (
          <div key={i} className="dz-colrow">
            <Select value={c} onChange={(e) => setPair(i, e.target.value, null)}>{props.localColumns.map((x) => <option key={x} value={x}>{x}</option>)}</Select>
            <span>→</span>
            <Select value={fk.ref_columns[i] ?? ""} onChange={(e) => setPair(i, null, e.target.value)}>
              <option value="">(choose)</option>{refCols.map((x) => <option key={x} value={x}>{x}</option>)}
            </Select>
            <button className="mini" onClick={() => props.onChange({ ...fk, columns: fk.columns.filter((_, k) => k !== i), ref_columns: fk.ref_columns.filter((_, k) => k !== i) })}><Icon name="close" size={10} /></button>
          </div>
        ))}
        <button onClick={() => props.onChange({ ...fk, columns: [...fk.columns, props.localColumns[0] ?? ""], ref_columns: [...fk.ref_columns, ""] })}>＋ Add pair</button>
      </div>
      <label>On delete<Select value={fk.on_delete} onChange={(e) => props.onChange({ ...fk, on_delete: e.target.value as RefAction })}>{REF_ACTIONS.map((a) => <option key={a} value={a}>{a.replace("_", " ").toUpperCase()}</option>)}</Select></label>
      <label>On update<Select value={fk.on_update} onChange={(e) => props.onChange({ ...fk, on_update: e.target.value as RefAction })}>{REF_ACTIONS.map((a) => <option key={a} value={a}>{a.replace("_", " ").toUpperCase()}</option>)}</Select></label>
    </div>
  );
}
