import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import Select from "./Select";
import Icon from "./Icon";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import {
  dataApi, EXPORT_FORMATS, queryApi, type Assign, type Cursor, type CellValue, type Change, type ColumnMeta, type ExportFormat, type ForeignKey, type Page, type Sort, type TableMeta,
} from "./api";
import ContextMenu, { type MenuItem } from "./ContextMenu";
import FkPicker from "./FkPicker";
import {
  OPS, b64ToBytes, bytesToText, cellLabel, isNumericType, opInfo, toCsv, toFilter, toJson, toSqlInsert, toTsv, type FilterInput,
} from "./gridUtil";
import { ConfirmDialog } from "./SidePanels";
import { newJobId, ProgressBar, useJob } from "./Jobs";
import ValuePanel from "./ValuePanel";
import CellModal from "./CellModal";

/** Persisted per table tab: everything the user set up that should survive a restart. */
export interface GridView {
  filters: Record<string, FilterInput>;
  where: string;
  sort: Sort[];
  pageSize: number;
  hidden: string[];
  filterRow: boolean;
}

export const defaultView = (): GridView => ({ filters: {}, where: "", sort: [], pageSize: 500, hidden: [], filterRow: true });

type RowRef = { id: string; isNew: boolean; index: number }; // index into page.rows (db rows) or newRows
type Staged = { edits: Record<string, Record<string, CellValue>>; deleted: string[]; added: { id: string; values: Record<string, CellValue> }[] };
const noStaged = (): Staged => ({ edits: {}, deleted: [], added: [] });

let newSeq = 0;

/** Fixed row height: what lets the grid draw only the rows that are on screen. */
const ROW_H = 22;
/** Rows drawn above and below the visible ones, so scrolling does not show blanks. */
const OVERSCAN = 12;
/** Pages up to this many rows are drawn whole; bigger ones are windowed. */
const VIRTUALIZE_FROM = 150;
/** Below this many estimated rows the exact count is taken straight away (it is quick); above it the user asks for it. */
const AUTO_COUNT_BELOW = 200_000;

export default function DataGrid(props: {
  connectionId: number;
  db: string;
  table: string;
  initial: GridView;
  onViewChange(v: GridView): void;
  onOpenTable(db: string, table: string, filters: GridView["filters"]): void;
  /** The connection is read-only: no editing, even for tables that could be edited. */
  readOnlyConnection?: boolean;
  /** Ask before saving changes (production): the text shown in the confirmation. */
  confirmCommit?: string;
  /** A SQLite file or PostgreSQL: their TEXT columns are often short, so they only open in the editor window when the value is long or has line breaks. */
  sqlite?: boolean;
}) {
  const { connectionId: cid, db, table } = props;
  const [meta, setMeta] = useState<TableMeta | null>(null);
  const [page, setPage] = useState<Page | null>(null);
  const [total, setTotal] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [view, setView] = useState<GridView>(props.initial);
  const [whereDraft, setWhereDraft] = useState(props.initial.where);
  // Where the page is: `base` numbers the rows; `cursor` (keyset paging) replaces OFFSET when the table allows it; `approx` after a
  // jump to the end whose row numbers come from an estimate.
  const [pos, setPos] = useState<{ base: number; cursor: Cursor | null; approx: boolean }>({ base: 0, cursor: null, approx: false });
  const offset = pos.base;
  const setOffset = (n: number) => setPos({ base: n, cursor: null, approx: false });
  const [estimate, setEstimate] = useState<number | null>(null);

  const [counting, setCounting] = useState(false);
  // After a jump to the end, row numbers count back from the total (exact once it is known, else the estimate, marked with ≈).
  const atEnd = pos.cursor === "end";
  useEffect(() => {
    if (!atEnd || !page) return;
    const n = page.rows.length;
    setPos((cur) => (cur.cursor === "end" ? { ...cur, base: Math.max(0, (total ?? estimate ?? n) - n), approx: total === null } : cur));
  }, [atEnd, page, total, estimate]);
  const countRun = useRef<string | null>(null);
  const countFor = useRef("");
  const [gzip, setGzip] = useState(false);
  const exportJob = useJob<{ rows: number; warnings: string[]; path: string }>();
  const [scrollTop, setScrollTop] = useState(0);
  const [viewH, setViewH] = useState(600);
  const [headH, setHeadH] = useState(56);
  const thead = useRef<HTMLTableSectionElement>(null);
  const [tick, setTick] = useState(0);
  const [staged, setStaged] = useState<Staged>(noStaged);
  const [active, setActive] = useState<{ r: number; c: number } | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [editing, setEditing] = useState<{ r: number; c: number; draft: string } | null>(null);
  const [panel, setPanel] = useState<"none" | "value" | "record">("none");
  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuItem[] } | null>(null);
  const [popup, setPopup] = useState<"columns" | "export" | null>(null);
  const [sqlDialog, setSqlDialog] = useState<{ query: string; pending: string[] } | null>(null);
  const [fkPick, setFkPick] = useState<{ r: number; c: number; fk: ForeignKey } | null>(null);
  const [confirm, setConfirm] = useState<{ message: string; run(): void } | null>(null);
  const [widths, setWidths] = useState<Record<string, number>>({});
  const seq = useRef(0);
  const lastSpec = useRef("");
  const scroller = useRef<HTMLDivElement>(null);

  // ---------- derived ----------
  const activeFilters = useMemo(
    () => Object.entries(view.filters).map(([col, f]) => toFilter(col, f)).filter((f) => f !== null),
    [view.filters],
  );
  const specKey = JSON.stringify([activeFilters, view.where, view.sort, view.pageSize]);
  const pendingCount = Object.keys(staged.edits).length + staged.deleted.length + staged.added.length;

  const keyIdx = useMemo(() => (meta && page ? meta.row_key.map((n) => page.columns.findIndex((c) => c.name === n)) : []), [meta, page]);
  const editable = !!meta && !props.readOnlyConnection && !meta.read_only_reason && keyIdx.length > 0 && keyIdx.every((i) => i >= 0);
  const [confirmApply, setConfirmApply] = useState(false);
  /** Submit, after asking when the connection requires it. */
  const requestSubmit = () => { if (props.confirmCommit) setConfirmApply(true); else submit(); };
  const longSet = useMemo(() => new Set((page?.long_cells ?? []).map(([r, c]) => `${r}:${c}`)), [page]);
  const colMeta = useCallback((name: string) => meta?.columns.find((c) => c.name === name), [meta]);

  const rows: RowRef[] = useMemo(() => {
    if (!page) return [];
    const db_rows = page.rows.map((r, i) => ({
      id: editable ? JSON.stringify(keyIdx.map((k) => r[k])) : `idx:${i}`,
      isNew: false,
      index: i,
    }));
    return [...db_rows, ...staged.added.map((a, i) => ({ id: a.id, isNew: true, index: i }))];
  }, [page, staged.added, editable, keyIdx]);

  const visibleCols = useMemo(
    () => (page ? page.columns.map((c, i) => ({ c, i })).filter(({ c }) => !view.hidden.includes(c.name)) : []),
    [page, view.hidden],
  );

  /** The value shown in a cell, taking staged edits and new rows into account. */
  const cellOf = useCallback((row: RowRef, ci: number): { text: string | null; staged: CellValue | null; long: boolean } => {
    if (!page) return { text: null, staged: null, long: false };
    const name = page.columns[ci].name;
    const s = row.isNew ? staged.added[row.index]?.values[name] : staged.edits[row.id]?.[name];
    if (s) return { text: s.kind === "text" ? s.value : null, staged: s, long: false };
    if (row.isNew) return { text: null, staged: null, long: false };
    return { text: page.rows[row.index][ci], staged: null, long: longSet.has(`${row.index}:${ci}`) };
  }, [page, staged, longSet]);

  /** Original key values of a server row; null when the row has no key or a key cell was cut short. */
  const keyOf = useCallback((row: RowRef): Assign[] | null => {
    if (!editable || !meta || !page || row.isNew) return null;
    const out: Assign[] = [];
    for (let k = 0; k < keyIdx.length; k++) {
      const v = page.rows[row.index][keyIdx[k]];
      if (v === null || longSet.has(`${row.index}:${keyIdx[k]}`)) return null;
      out.push({ column: meta.row_key[k], value: { kind: "text", value: v } });
    }
    return out;
  }, [editable, meta, page, keyIdx, longSet]);

  // ---------- loading ----------
  const buildSpec = useCallback((off: number, limit = view.pageSize, cursor: Cursor | null = null) => ({
    filters: activeFilters, where_sql: view.where.trim() || null, sort: view.sort, limit, offset: off, cursor,
  }), [activeFilters, view.where, view.sort, view.pageSize]);
  /** What identifies the page in the load effect: the cursor, or the offset when there is none. */
  const posKey = JSON.stringify([pos.cursor, pos.cursor ? 0 : pos.base]);

  useEffect(() => {
    dataApi.meta(cid, db, table).then(setMeta).catch((e) => setError(String(e)));
  }, [cid, db, table, tick]);

  const startCount = useCallback(async (key: string) => {
    const run = newJobId();
    countRun.current = run;
    setCounting(true);
    try {
      const n = await dataApi.count(cid, db, table, activeFilters, view.where.trim() || null, run);
      if (countFor.current === key) setTotal(n);
    } catch { /* cancelled, or the server refused: the estimate stays */ }
    finally { if (countRun.current === run) { countRun.current = null; setCounting(false); } }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cid, db, table, activeFilters, view.where]);
  const cancelCount = () => { if (countRun.current) queryApi.cancel(countRun.current).catch(() => {}); };

  useEffect(() => {
    // A changed filter/sort/page size always restarts at the first page.
    if (specKey !== lastSpec.current && (pos.base !== 0 || pos.cursor)) { lastSpec.current = specKey; setPos({ base: 0, cursor: null, approx: false }); return; }
    const first = lastSpec.current === "";
    lastSpec.current = specKey;
    const mine = ++seq.current;
    const t = setTimeout(async () => {
      setLoading(true);
      try {
        const p = await dataApi.page(cid, db, table, buildSpec(pos.cursor ? 0 : pos.base, view.pageSize, pos.cursor));
        if (mine !== seq.current) return;
        setPage(p); setError(null); setActive(null); setSelected(new Set()); setEditing(null);
        // The total only changes with the filters (or a refresh), not with the page: it is looked up once per spec.
        const countKey = `${specKey}|${tick}`;
        if (countFor.current !== countKey) {
          countFor.current = countKey;
          setTotal(null); setEstimate(null);
          if (!p.has_prev && !p.has_more) setTotal(p.rows.length);
          else if (!pos.cursor && !p.has_more) setTotal(pos.base + p.rows.length);
          else {
            dataApi.estimate(cid, db, table, activeFilters, view.where.trim() || null).then((est) => {
              if (countFor.current !== countKey) return;
              setEstimate(est);
              // small tables are counted exactly straight away; for big ones that is the user's call
              if (est === null || est < AUTO_COUNT_BELOW) startCount(countKey);
            }).catch(() => {});
          }
        } else if (!pos.cursor && !p.has_more) setTotal(pos.base + p.rows.length); // reached the end by paging: now exact
      } catch (e) {
        if (mine === seq.current) setError(String(e));
      } finally {
        if (mine === seq.current) setLoading(false);
      }
    }, first ? 0 : 300);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [specKey, posKey, tick, cid, db, table]);

  // ---------- windowing: only the rows on screen are in the page's DOM ----------
  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setViewH(el.clientHeight));
    ro.observe(el);
    setViewH(el.clientHeight);
    return () => ro.disconnect();
  }, []);
  useEffect(() => { if (thead.current) setHeadH(thead.current.offsetHeight); }, [view.filterRow, page?.columns.length]);

  useEffect(() => { props.onViewChange(view); /* eslint-disable-next-line react-hooks/exhaustive-deps */ }, [view]);
  useEffect(() => { if (notice) { const t = setTimeout(() => setNotice(null), 5000); return () => clearTimeout(t); } }, [notice]);

  /** Run `fn`; if there are staged changes, ask before discarding them. */
  const guard = (fn: () => void) => {
    if (pendingCount === 0) return fn();
    setConfirm({ message: `Discard ${pendingCount} pending change(s)?`, run: () => { setStaged(noStaged()); fn(); } });
  };
  const patchView = (p: Partial<GridView>) => guard(() => setView((v) => ({ ...v, ...p })));

  // ---------- staging ----------
  const stage = (row: RowRef, ci: number, value: CellValue) => {
    if (!page) return;
    const name = page.columns[ci].name;
    setStaged((s) => {
      if (row.isNew) {
        const added = s.added.map((a, i) => (i === row.index ? { ...a, values: { ...a.values, [name]: value } } : a));
        return { ...s, added };
      }
      const orig = page.rows[row.index][ci];
      const same = (value.kind === "text" && orig === value.value && !longSet.has(`${row.index}:${ci}`)) || (value.kind === "null" && orig === null);
      const cur = { ...(s.edits[row.id] ?? {}) };
      if (same) delete cur[name]; else cur[name] = value;
      const edits = { ...s.edits };
      if (Object.keys(cur).length) edits[row.id] = cur; else delete edits[row.id];
      return { ...s, edits };
    });
  };

  const canEditCell = (row: RowRef, ci: number) => {
    const m = page && colMeta(page.columns[ci].name);
    if (!m || m.generated) return false;
    return row.isNew ? !meta?.is_view : editable;
  };

  /** Columns that hold long text open in an editor window, not in the one-line cell: TEXT, MEDIUMTEXT, LONGTEXT and JSON. */
  const wantsModal = (m: ColumnMeta | undefined, text: string | null, long: boolean) => {
    if (!m || m.binary) return false;
    const t = m.data_type.toLowerCase();
    if (!["text", "mediumtext", "longtext", "json", "jsonb", "clob"].includes(t)) return false;
    if (!props.sqlite || t !== "text") return true;
    return long || (text?.length ?? 0) > 100 || (text?.includes("\n") ?? false);
  };
  const [cellModal, setCellModal] = useState<{ r: number; c: number; initial: string | null; cut: boolean; editable: boolean } | null>(null);

  /** Double-click, Enter or F2 on a cell: the editor window for long text and JSON, the in-cell editor for everything else. */
  const openCell = async (r: number, c: number) => {
    const row = rows[r];
    if (!row || !page) return;
    const m = colMeta(page.columns[c].name);
    const { text, staged: st, long } = cellOf(row, c);
    if (!wantsModal(m, text, long)) return startEdit(r, c);
    let initial = st ? (st.kind === "text" ? st.value : null) : text;
    let cut = false;
    if (long && !st) {
      const key = keyOf(row);
      if (!key) cut = true; // the full value cannot be fetched without a row key: show what the grid has
      else {
        try {
          const d = await dataApi.cell(cid, db, table, key, page.columns[c].name);
          const t = d.base64 ? bytesToText(b64ToBytes(d.base64)) : "";
          if (d.is_null) initial = null;
          else if (d.truncated || t === null) cut = true;
          else initial = t;
        } catch (e) { setError(String(e)); return; }
      }
    }
    setCellModal({ r, c, initial, cut, editable: canEditCell(row, c) });
  };

  const startEdit = async (r: number, c: number) => {
    const row = rows[r];
    if (!row || !page || !canEditCell(row, c)) return;
    const { text, long } = cellOf(row, c);
    const m = colMeta(page.columns[c].name);
    if (long) {
      if (m?.binary) { setPanel("value"); return; }       // large binary data is edited through upload
      const key = keyOf(row);
      if (!key) { setError("This value is too long to edit inline and the row has no usable key."); return; }
      try {
        const d = await dataApi.cell(cid, db, table, key, page.columns[c].name);
        const t = d.base64 ? bytesToText(b64ToBytes(d.base64)) : "";
        if (d.truncated || t === null) { setPanel("value"); return; }
        setEditing({ r, c, draft: t });
      } catch (e) { setError(String(e)); }
      return;
    }
    setEditing({ r, c, draft: text ?? "" });
  };

  const commitEdit = (move?: "down" | "right") => {
    if (!editing) return;
    const { r, c, draft } = editing;
    const row = rows[r];
    setEditing(null);
    if (row) {
      const { text, staged: st } = cellOf(row, c);
      const unchanged = st ? (st.kind === "text" && st.value === draft) : (text ?? "") === draft && (text !== null || draft === "");
      if (!unchanged) stage(row, c, { kind: "text", value: draft });
    }
    if (move === "down") setActive({ r: Math.min(r + 1, rows.length - 1), c });
    if (move === "right") setActive({ r, c: Math.min(c + 1, (page?.columns.length ?? 1) - 1) });
    scroller.current?.focus();
  };

  // ---------- row operations ----------
  const targetRows = (): RowRef[] => {
    const idx = selected.size ? [...selected] : active ? [active.r] : [];
    return idx.map((i) => rows[i]).filter(Boolean);
  };

  const addRow = () => {
    if (props.readOnlyConnection) return;
    if (!meta || meta.is_view) return;
    const id = `new-${newSeq++}`;
    setStaged((s) => ({ ...s, added: [...s.added, { id, values: {} }] }));
    const r = (page?.rows.length ?? 0) + staged.added.length;
    setActive({ r, c: visibleCols[0]?.i ?? 0 });
    setSelected(new Set([r]));
    setTimeout(() => scroller.current?.scrollTo({ top: scroller.current.scrollHeight }), 0);
  };

  const duplicateRows = async () => {
    if (props.readOnlyConnection) return;
    if (!page || !meta) return;
    for (const row of targetRows()) {
      const values: Record<string, CellValue> = {};
      for (let ci = 0; ci < page.columns.length; ci++) {
        const m = colMeta(page.columns[ci].name);
        if (!m || m.generated || m.auto_increment) continue;
        const { text, staged: st, long } = cellOf(row, ci);
        if (st) { values[m.name] = st; continue; }
        if (text === null) { values[m.name] = { kind: "null" }; continue; }
        if (long) {   // never copy a cut-off preview: load the full value
          const key = keyOf(row);
          if (!key) { setError(`Cannot duplicate: column ${m.name} is too long and the row has no usable key.`); return; }
          const d = await dataApi.cell(cid, db, table, key, m.name).catch(() => null);
          const t = d?.base64 && !d.truncated ? (m.binary ? "0x" + [...b64ToBytes(d.base64)].map((b) => b.toString(16).padStart(2, "0")).join("") : bytesToText(b64ToBytes(d.base64))) : null;
          if (t === null || t === undefined) { setError(`Cannot duplicate: column ${m.name} is too large.`); return; }
          values[m.name] = { kind: "text", value: t };
        } else values[m.name] = { kind: "text", value: text };
      }
      setStaged((s) => ({ ...s, added: [...s.added, { id: `new-${newSeq++}`, values }] }));
    }
  };

  const deleteRows = () => {
    if (props.readOnlyConnection) return;
    const targets = targetRows();
    if (!targets.length) return;
    setStaged((s) => {
      const newIds = new Set(targets.filter((t) => t.isNew).map((t) => t.id));
      const dbIds = targets.filter((t) => !t.isNew).map((t) => t.id);
      const allDeleted = dbIds.length > 0 && dbIds.every((i) => s.deleted.includes(i));
      return {
        edits: s.edits,
        added: s.added.filter((a) => !newIds.has(a.id)),
        deleted: allDeleted ? s.deleted.filter((i) => !dbIds.includes(i)) : [...new Set([...s.deleted, ...dbIds])],
      };
    });
  };

  // ---------- submit ----------
  const buildChanges = (): Change[] | string => {
    if (!page) return "no data";
    const out: Change[] = [];
    const byId = new Map(rows.filter((r) => !r.isNew).map((r) => [r.id, r]));
    for (const [id, edit] of Object.entries(staged.edits)) {
      if (staged.deleted.includes(id)) continue;
      const row = byId.get(id);
      const key = row && keyOf(row);
      if (!key) return "A changed row has no usable key (a key value is missing or too long).";
      out.push({ op: "update", key, set: Object.entries(edit).map(([column, value]) => ({ column, value })) });
    }
    for (const id of staged.deleted) {
      const row = byId.get(id);
      const key = row && keyOf(row);
      if (!key) return "A deleted row has no usable key.";
      out.push({ op: "delete", key });
    }
    for (const a of staged.added) out.push({ op: "insert", values: Object.entries(a.values).map(([column, value]) => ({ column, value })) });
    return out;
  };

  const submit = async () => {
    const changes = buildChanges();
    if (typeof changes === "string") return setError(changes);
    try {
      const r = await dataApi.apply(cid, db, table, changes);
      setStaged(noStaged());
      setError(null);
      setNotice(`${r.applied} change(s) submitted`);
      setTick((t) => t + 1);
    } catch (e) { setError(String(e)); }
  };

  const showSql = async () => {
    const changes = buildChanges();
    const pending = typeof changes === "string" ? [] : await dataApi.preview(cid, db, table, changes).catch((e) => [`-- ${e}`]);
    setSqlDialog({ query: page?.sql ?? "", pending });
  };

  // ---------- copy / export ----------
  const clip = (s: string) => navigator.clipboard.writeText(s);
  const rowsAsCells = (rs: RowRef[]) => rs.map((row) => visibleCols.map(({ i }) => cellOf(row, i).text));
  const copyRows = (kind: "tsv" | "csv" | "json" | "sql") => {
    const rs = targetRows();
    const names = visibleCols.map(({ c }) => c.name);
    const cells = rowsAsCells(rs);
    const numeric = visibleCols.map(({ c }) => isNumericType(colMeta(c.name)));
    const binary = visibleCols.map(({ c }) => c.binary);
    clip(kind === "tsv" ? toTsv(cells) : kind === "csv" ? toCsv(names, cells) : kind === "json" ? toJson(names, cells, numeric) : toSqlInsert(table, names, cells, numeric, binary));
    setNotice(`Copied ${rs.length} row(s)`);
  };

  const copyCell = async (r: number, c: number) => {
    const row = rows[r];
    if (!row || !page) return;
    const { text, long } = cellOf(row, c);
    let value = text;
    const key = keyOf(row);
    if (long && key) {
      const d = await dataApi.cell(cid, db, table, key, page.columns[c].name).catch(() => null);
      if (d?.base64 && !d.truncated && !page.columns[c].binary) value = bytesToText(b64ToBytes(d.base64)) ?? value;
    }
    clip(value ?? "");
  };

  const doExport = async (format: ExportFormat) => {
    setPopup(null);
    const ext = EXPORT_FORMATS.find((f) => f.format === format)!.ext;
    const zip = gzip && format !== "xlsx";
    const path = await saveDialog({ defaultPath: `${table}.${ext}${zip ? ".gz" : ""}` });
    if (!path) return;
    // a job: progress while it runs, and Cancel stops it (the partial file is removed)
    exportJob.run(async (jobId) => ({ ...(await dataApi.exportTable(cid, jobId, db, table, buildSpec(0), format, "NULL", zip, path)), path }), `Export ${db}.${table}`);
  };

  // ---------- foreign keys ----------
  const fkFor = (name: string) => meta?.foreign_keys.find((f) => f.columns.includes(name));
  const goToReferenced = (r: number, c: number) => {
    const row = rows[r];
    const fk = page && fkFor(page.columns[c].name);
    if (!row || !page || !fk) return;
    const filters: GridView["filters"] = {};
    for (let k = 0; k < fk.columns.length; k++) {
      const ci = page.columns.findIndex((x) => x.name === fk.columns[k]);
      const v = ci >= 0 ? cellOf(row, ci).text : null;
      if (v === null) { setNotice("The foreign key value is NULL: nothing to open."); return; }
      filters[fk.ref_columns[k]] = { op: "eq", text: v, text2: "" };
    }
    props.onOpenTable(fk.ref_schema, fk.ref_table, filters);
  };

  // ---------- context menu / keyboard ----------
  const filterBy = (ci: number, op: "eq" | "ne" | "is_null" | "not_null" | "contains", text: string) => {
    if (!page) return;
    patchView({ filters: { ...view.filters, [page.columns[ci].name]: { op, text, text2: "" } }, filterRow: true });
  };

  const openMenu = (e: React.MouseEvent, r: number, c: number) => {
    e.preventDefault();
    if (!page || !rows[r]) return;
    setActive({ r, c });
    if (!selected.has(r)) setSelected(new Set([r]));
    const row = rows[r];
    const { text } = cellOf(row, c);
    const m = colMeta(page.columns[c].name);
    const fk = fkFor(page.columns[c].name);
    const can = canEditCell(row, c);
    const items: MenuItem[] = [
      { label: wantsModal(m, text, false) && !can ? "View value" : "Edit cell", disabled: !can && !wantsModal(m, text, false), onClick: () => openCell(r, c) },
      { label: "Set NULL", disabled: !can || !m?.nullable, onClick: () => stage(row, c, { kind: "null" }) },
      { label: "Set DEFAULT", disabled: !can, onClick: () => stage(row, c, { kind: "default" }) },
      { label: "Pick from referenced table…", disabled: !can || !fk || fk.columns.length !== 1, onClick: () => fk && setFkPick({ r, c, fk }) },
      { label: "Open in value viewer", onClick: () => setPanel("value") },
      { separator: true },
      { label: "Filter by this value", disabled: row.isNew, onClick: () => (text === null ? filterBy(c, "is_null", "") : filterBy(c, "eq", text)) },
      { label: "Exclude this value", disabled: row.isNew, onClick: () => (text === null ? filterBy(c, "not_null", "") : filterBy(c, "ne", text)) },
      { label: "Filter: contains this text", disabled: row.isNew || text === null, onClick: () => text !== null && filterBy(c, "contains", text) },
      { label: "Go to referenced row", disabled: !fk, onClick: () => goToReferenced(r, c) },
      { separator: true },
      { label: "Copy value", onClick: () => copyCell(r, c) },
      { label: "Copy rows as", children: [
        { label: "Tab-separated", onClick: () => copyRows("tsv") }, { label: "CSV", onClick: () => copyRows("csv") },
        { label: "JSON", onClick: () => copyRows("json") }, { label: "SQL INSERT", onClick: () => copyRows("sql") },
      ] },
      { separator: true },
      { label: "Duplicate row", disabled: !meta || meta.is_view, onClick: duplicateRows },
      { label: "Delete row(s)", disabled: !editable && !row.isNew, onClick: deleteRows },
    ];
    setMenu({ x: e.clientX, y: e.clientY, items });
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (editing || !active || !page) return;
    const mod = e.metaKey || e.ctrlKey;
    const lastC = page.columns.length - 1;
    const go = (r: number, c: number) => { e.preventDefault(); const rr = Math.max(0, Math.min(rows.length - 1, r)); setActive({ r: rr, c: Math.max(0, Math.min(lastC, c)) }); reveal(rr); };
    if (e.key === "ArrowDown") go(active.r + 1, active.c);
    else if (e.key === "ArrowUp") go(active.r - 1, active.c);
    else if (e.key === "ArrowLeft") go(active.r, active.c - 1);
    else if (e.key === "ArrowRight") go(active.r, active.c + 1);
    else if (e.key === "PageDown") go(active.r + pageRows, active.c);
    else if (e.key === "PageUp") go(active.r - pageRows, active.c);
    else if (e.key === "Home" && mod) go(0, active.c);
    else if (e.key === "End" && mod) go(rows.length - 1, active.c);
    else if (e.key === "Enter" && mod) { e.preventDefault(); if (pendingCount) requestSubmit(); }
    else if (e.key === "Enter" || e.key === "F2") { e.preventDefault(); openCell(active.r, active.c); }
    else if (mod && e.key === "c") { e.preventDefault(); copyCell(active.r, active.c); }
    else if (e.key === "Delete") { e.preventDefault(); deleteRows(); }
  };

  const toggleSort = (name: string, additive: boolean) => {
    const cur = view.sort.find((s) => s.column === name);
    const next: Sort | null = !cur ? { column: name, desc: false } : !cur.desc ? { column: name, desc: true } : null;
    const others = additive ? view.sort.filter((s) => s.column !== name) : [];
    patchView({ sort: next ? [...others, next] : others });
  };

  // The row-number column ("SL") sorts by the primary key (ascending, descending, off) and is resizable like the others.
  const RN = "\u0000rownum";
  const rnSort = (() => {
    const pk = meta?.primary_key ?? [];
    if (pk.length === 0 || view.sort.length !== pk.length || !pk.every((k, i) => view.sort[i]?.column === k)) return null;
    return view.sort[0].desc ? ("desc" as const) : ("asc" as const);
  })();
  const toggleRowSort = () => {
    const pk = meta?.primary_key ?? [];
    if (pk.length === 0) { setNotice("Sorting by row number needs a primary key: this table has none."); return; }
    const next = rnSort === null ? false : rnSort === "asc" ? true : null;
    patchView({ sort: next === null ? [] : pk.map((column) => ({ column, desc: next })) });
  };
  const startResize = (name: string, startX: number, startW: number) => {
    const move = (e: MouseEvent) => setWidths((w) => ({ ...w, [name]: Math.max(name === RN ? 36 : 50, startW + e.clientX - startX) }));
    const up = () => { window.removeEventListener("mousemove", move); window.removeEventListener("mouseup", up); };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };
  const widthOf = (name: string, type: string) => widths[name] ?? (/INT|LONG|SHORT|TINY|DECIMAL|FLOAT|DOUBLE/.test(type) ? 110 : /DATE|TIME/.test(type) ? 170 : /BLOB/.test(type) ? 130 : 180);

  // ---------- render ----------
  const range = page ? (page.rows.length ? `${pos.approx ? "≈ " : ""}${(offset + 1).toLocaleString()}–${(offset + page.rows.length).toLocaleString()}` : "0") : "…";
  const lastOffset = total !== null && total > 0 ? Math.floor((total - 1) / view.pageSize) * view.pageSize : null;
  // Keyset paging (a seek by primary key) is used when the table allows it and the user has not sorted; otherwise OFFSET.
  const keysetOK = !!page?.keyset && view.sort.length === 0;
  const goFirst = () => guard(() => setPos({ base: 0, cursor: null, approx: false }));
  const goNext = () => guard(() => setPos({ base: offset + view.pageSize, cursor: keysetOK && page?.last_key ? { after: page.last_key } : null, approx: pos.approx }));
  const goPrev = () => guard(() => setPos({ base: Math.max(0, offset - view.pageSize), cursor: keysetOK && page?.first_key ? { before: page.first_key } : null, approx: pos.approx }));
  const goLast = () => guard(() => {
    if (keysetOK) setPos({ base: lastOffset ?? offset, cursor: "end", approx: lastOffset === null });
    else if (lastOffset !== null) setOffset(lastOffset);
  });
  const activeRow = active ? rows[active.r] : undefined;
  const activeCol = active && page ? colMeta(page.columns[active.c].name) : undefined;
  const activeCell = activeRow && active ? cellOf(activeRow, active.c) : null;

  const virtual = rows.length > VIRTUALIZE_FROM;
  const bodyTop = Math.max(0, scrollTop - headH);
  const winStart = virtual ? Math.max(0, Math.floor(bodyTop / ROW_H) - OVERSCAN) : 0;
  const winEnd = virtual ? Math.min(rows.length, Math.ceil((bodyTop + viewH) / ROW_H) + OVERSCAN) : rows.length;
  const pageRows = Math.max(1, Math.floor((viewH - headH) / ROW_H) - 1);

  /** Scroll so that row `r` is on screen (the row may not be in the DOM). */
  const reveal = (r: number) => {
    const el = scroller.current;
    if (!el) return;
    const top = r * ROW_H, bottom = top + ROW_H;
    if (top < el.scrollTop) el.scrollTop = top;
    else if (bottom + headH > el.scrollTop + el.clientHeight) el.scrollTop = bottom + headH - el.clientHeight;
  };

  if (error && !page && !meta) return <div className="error">{error}</div>;

  return (
    <div className="datagrid">
      <div className="dg-toolbar">
        <button title="Reload" onClick={() => guard(() => setTick((t) => t + 1))}>⟳</button>
        <span className="sep" />
        <button disabled={!page?.has_prev} onClick={goFirst} title="First page"><Icon name="first" /></button>
        <button disabled={!page?.has_prev} onClick={goPrev} title="Previous page"><Icon name="prev" /></button>
        <span className="dim pager" title={total === null && estimate !== null ? "An estimate from the table's statistics" : undefined}>
          {range} of {total !== null ? total.toLocaleString() : estimate !== null ? `≈ ${estimate.toLocaleString()}` : "…"}
        </span>
        {total === null && !counting && estimate !== null && (
          <button className="mini count-btn" onClick={() => startCount(countFor.current)} title="Count the rows exactly (can take a while on a big table)">Count</button>
        )}
        {counting && <><span className="dim">counting…</span><button className="mini" onClick={cancelCount} title="Stop counting">Stop</button></>}
        <button disabled={!page?.has_more} onClick={goNext} title="Next page"><Icon name="next" /></button>
        <button disabled={!page?.has_more || (!keysetOK && lastOffset === null)} onClick={goLast} title="Last page"><Icon name="last" /></button>
        <Select value={view.pageSize} onChange={(e) => patchView({ pageSize: Number(e.target.value) })} title="Rows per page">
          {[100, 250, 500, 1000, 5000].map((n) => <option key={n} value={n}>{n} rows</option>)}
        </Select>
        <span className="sep" />
        <button disabled={!meta || meta.is_view || props.readOnlyConnection} onClick={addRow} title="Add row">＋</button>
        <button disabled={!rows.length || props.readOnlyConnection} onClick={deleteRows} title="Delete selected row(s) (Delete)">－</button>
        <button disabled={!active || !!meta?.is_view || props.readOnlyConnection} onClick={duplicateRows} title="Duplicate row"><Icon name="copy" /></button>
        <button disabled={!pendingCount} onClick={() => setStaged(noStaged())} title="Revert pending changes">↶</button>
        <button className="primary" disabled={!pendingCount} onClick={requestSubmit} title="Submit changes (⌘↵)"><Icon name="check" /> Submit{pendingCount ? ` (${pendingCount})` : ""}</button>
        <button onClick={showSql} title="View queries">SQL</button>
        <span className="grow" />
        <button className={view.filterRow ? "toggled" : ""} onClick={() => setView({ ...view, filterRow: !view.filterRow })} title="Column filters">
          <Icon name="filter" /> Filter{activeFilters.length ? ` (${activeFilters.length})` : ""}
        </button>
        {activeFilters.length > 0 && <button onClick={() => patchView({ filters: {} })} title="Clear all column filters"><Icon name="close" size={10} /></button>}
        <div className="popup-host">
          <button onClick={() => setPopup(popup === "columns" ? null : "columns")}>Columns</button>
          {popup === "columns" && page && (
            <div className="popup" onMouseLeave={() => setPopup(null)}>
              <button className="link" onClick={() => setView({ ...view, hidden: [] })}>Show all</button>
              {page.columns.map((c) => (
                <label key={c.name}><input type="checkbox" checked={!view.hidden.includes(c.name)}
                  onChange={(e) => setView({ ...view, hidden: e.target.checked ? view.hidden.filter((h) => h !== c.name) : [...view.hidden, c.name] })} /> {c.name}</label>
              ))}
            </div>
          )}
        </div>
        <div className="popup-host">
          <button onClick={() => setPopup(popup === "export" ? null : "export")}>Export ▾</button>
          {popup === "export" && (
            <div className="popup" onMouseLeave={() => setPopup(null)}>
              <div className="dim small">All rows matching the current filters</div>
              {EXPORT_FORMATS.map((f) => <button key={f.format} className="link" onClick={() => doExport(f.format)}>{f.label}…</button>)}
              <label className="small"><input type="checkbox" checked={gzip} onChange={(e) => setGzip(e.target.checked)} /> Compress with gzip (.gz)</label>
            </div>
          )}
        </div>
        <button className={panel === "value" ? "toggled" : ""} onClick={() => setPanel(panel === "value" ? "none" : "value")}>Value</button>
        <button className={panel === "record" ? "toggled" : ""} onClick={() => setPanel(panel === "record" ? "none" : "record")}>Form</button>
      </div>

      <div className="dg-where">
        <span className="dim">WHERE</span>
        <input placeholder="SQL condition, e.g. total > 100 AND note IS NOT NULL" value={whereDraft}
          onChange={(e) => setWhereDraft(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && patchView({ where: whereDraft })}
          onBlur={() => whereDraft !== view.where && patchView({ where: whereDraft })} />
        {view.sort.length > 0 && (
          <span className="dim">ORDER BY {view.sort.map((s) => `${s.column}${s.desc ? " DESC" : ""}`).join(", ")}
            <button className="mini" onClick={() => patchView({ sort: [] })}><Icon name="close" size={10} /></button></span>
        )}
      </div>

      {props.readOnlyConnection && <div className="banner"><Icon name="lock" size={12} /> Read-only connection: rows cannot be changed here.</div>}
      {!props.readOnlyConnection && meta?.read_only_reason && <div className="banner">Read-only: {meta.read_only_reason}.</div>}
      {error && <div className="banner bad">{error} <button className="mini" onClick={() => setError(null)}><Icon name="close" size={10} /></button></div>}
      {exportJob.running && <div className="dg-job"><ProgressBar p={exportJob.progress} running onCancel={exportJob.cancel} /></div>}
      {exportJob.cancelled && <div className="banner">Export cancelled. The partial file was removed.</div>}
      {exportJob.error && <div className="banner bad">{exportJob.error}</div>}
      {exportJob.result && !exportJob.running && <div className="banner ok">Exported {exportJob.result.rows.toLocaleString()} row(s) to {exportJob.result.path}{exportJob.result.warnings.length ? " — " + exportJob.result.warnings.join("; ") : ""}</div>}
      {notice && <div className="banner ok">{notice}</div>}

      <div className="dg-main">
        <div className="dg-scroll" ref={scroller} tabIndex={0} onKeyDown={onKeyDown} onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}>
          {page && (
            <table className="grid dg" style={{ width: 44 + visibleCols.reduce((n, { c }) => n + widthOf(c.name, c.type_name), 0) }}>
              <colgroup>
                <col style={{ width: widths[RN] ?? 44 }} />
                {visibleCols.map(({ c }) => <col key={c.name} style={{ width: widthOf(c.name, c.type_name) }} />)}
              </colgroup>
              <thead ref={thead}>
                <tr>
                  <th className="rownum" onClick={toggleRowSort} title={meta?.primary_key.length ? "Row number. Click to sort by the primary key: ascending, descending, off" : "Row number (this table has no primary key to sort by)"}>
                    <div className="th rn-head"><span className="rn-label">SL</span>{rnSort && <span className="sortmark">{rnSort === "desc" ? "▼" : "▲"}</span>}</div>
                    <div className="resizer" onClick={(e) => e.stopPropagation()}
                         onMouseDown={(e) => { e.preventDefault(); e.stopPropagation(); startResize(RN, e.clientX, widths[RN] ?? 44); }} />
                  </th>
                  {visibleCols.map(({ c }) => {
                    const m = colMeta(c.name);
                    const s = view.sort.findIndex((x) => x.column === c.name);
                    return (
                      <th key={c.name} onClick={(e) => toggleSort(c.name, e.shiftKey)} title={`${m?.column_type ?? c.type_name}${m?.comment ? " — " + m.comment : ""}\nClick: sort · Shift-click: add to sort`}>
                        <div className="th">
                          {meta?.primary_key.includes(c.name) && <span title="Primary key"><Icon name="key" size={12} className="ico-pk" /></span>}
                          {fkFor(c.name) && <span title={`→ ${fkFor(c.name)!.ref_table}`}><Icon name="link" size={12} /></span>}
                          <span className="th-name">{c.name}</span>
                          {s >= 0 && <span className="sortmark">{view.sort[s].desc ? "▼" : "▲"}{view.sort.length > 1 ? s + 1 : ""}</span>}
                        </div>
                        <div className="resizer" onClick={(e) => e.stopPropagation()}
                             onMouseDown={(e) => { e.preventDefault(); e.stopPropagation(); startResize(c.name, e.clientX, widthOf(c.name, c.type_name)); }} />
                      </th>
                    );
                  })}
                </tr>
                {view.filterRow && (
                  <tr className="filter-row">
                    <th className="rownum" />
                    {visibleCols.map(({ c }) => (
                      <th key={c.name}>
                        <FilterCell
                          value={view.filters[c.name]}
                          numeric={isNumericType(colMeta(c.name))}
                          onChange={(f) => {
                            const filters = { ...view.filters };
                            if (f) filters[c.name] = f; else delete filters[c.name];
                            // Only a change to the *effective* filter set reloads data (and so needs the discard prompt).
                            const effective = (x: GridView["filters"]) => JSON.stringify(Object.entries(x).map(([k, v]) => toFilter(k, v)).filter(Boolean));
                            if (effective(filters) === effective(view.filters)) setView({ ...view, filters });
                            else patchView({ filters });
                          }}
                        />
                      </th>
                    ))}
                  </tr>
                )}
              </thead>
              <tbody>
                {virtual && winStart > 0 && <tr className="vspacer" aria-hidden="true" style={{ height: winStart * ROW_H }}><td colSpan={visibleCols.length + 1} /></tr>}
                {rows.slice(winStart, winEnd).map((row, k) => {
                  const r = winStart + k;
                  const isDeleted = staged.deleted.includes(row.id);
                  return (
                    <tr key={row.id} className={`${row.isNew ? "row-new" : ""} ${isDeleted ? "row-deleted" : ""} ${selected.has(r) ? "row-selected" : ""}`}>
                      <td className="rownum" onClick={(e) => {
                        const n = new Set(e.metaKey || e.ctrlKey ? selected : []);
                        if (e.shiftKey && active) { for (let i = Math.min(active.r, r); i <= Math.max(active.r, r); i++) n.add(i); }
                        else if (n.has(r)) n.delete(r); else n.add(r);
                        setSelected(n); setActive({ r, c: active?.c ?? 0 });
                      }}>{row.isNew ? "＋" : offset + row.index + 1}</td>
                      {visibleCols.map(({ c: col, i: ci }) => {
                        const cell = cellOf(row, ci);
                        const isActive = active?.r === r && active.c === ci;
                        const isEditing = editing?.r === r && editing.c === ci;
                        const fk = fkFor(col.name);
                        const cls = [
                          isActive ? "active" : "", cell.staged ? "edited" : "",
                          cell.text === null && !cell.staged && !row.isNew ? "null" : "",
                          isNumericType(colMeta(col.name)) ? "num" : "",
                          cell.staged?.kind === "default" || (row.isNew && !cell.staged) ? "dflt" : "",
                        ].join(" ");
                        return (
                          <td key={col.name} className={cls}
                              onClick={(e) => {
                                setActive({ r, c: ci });
                                if (!(e.shiftKey || e.metaKey || e.ctrlKey)) setSelected(new Set([r]));
                                if ((e.metaKey || e.ctrlKey) && fk) goToReferenced(r, ci);
                              }}
                              onDoubleClick={() => openCell(r, ci)}
                              onContextMenu={(e) => openMenu(e, r, ci)}>
                            {isEditing ? (
                              <div className="cell-edit">
                                <input autoFocus value={editing!.draft}
                                  onChange={(e) => setEditing({ ...editing!, draft: e.target.value })}
                                  onBlur={() => commitEdit()}
                                  onKeyDown={(e) => {
                                    e.stopPropagation();
                                    if (e.key === "Enter") { e.preventDefault(); commitEdit("down"); }
                                    else if (e.key === "Tab") { e.preventDefault(); commitEdit("right"); }
                                    else if (e.key === "Escape") { setEditing(null); scroller.current?.focus(); }
                                  }} />
                                {fk && fk.columns.length === 1 && (
                                  <button className="mini" title="Pick from referenced table" onMouseDown={(e) => { e.preventDefault(); setFkPick({ r, c: ci, fk }); }}>…</button>
                                )}
                              </div>
                            ) : cell.staged ? (
                              cell.staged.kind === "null" ? "NULL" : cell.staged.kind === "default" ? "DEFAULT" : cellLabel(cell.staged)
                            ) : row.isNew ? "" : cell.text === null ? "NULL" : (<>{cell.text}{cell.long && <span className="dim">…</span>}</>)}
                          </td>
                        );
                      })}
                    </tr>
                  );
                })}
                {virtual && winEnd < rows.length && <tr className="vspacer" aria-hidden="true" style={{ height: (rows.length - winEnd) * ROW_H }}><td colSpan={visibleCols.length + 1} /></tr>}
              </tbody>
            </table>
          )}
          {page && rows.length === 0 && <div className="empty">{activeFilters.length || view.where ? "No rows match the filters" : "Table is empty"}</div>}
          {loading && <div className="loading">Loading…</div>}
        </div>

        {panel === "value" && meta && (
          <aside className="dg-side">
            <ValuePanel
              connectionId={cid} db={db} table={table} column={activeCol}
              shown={activeCell?.text ?? null} isLong={!!activeCell?.long}
              key_={activeRow ? keyOf(activeRow) : null}
              editable={!!activeRow && !!active && canEditCell(activeRow, active.c)}
              onStage={(v) => activeRow && active && stage(activeRow, active.c, v)}
            />
          </aside>
        )}
        {panel === "record" && page && meta && (
          <aside className="dg-side record">
            {!activeRow ? <div className="empty">Select a row</div> : (
              <>
                <div className="vp-head"><b>Row {activeRow.isNew ? "(new)" : offset + activeRow.index + 1}</b>
                  <span className="grow" />
                  <button className="mini" disabled={active!.r === 0} onClick={() => setActive({ r: active!.r - 1, c: active!.c })}><Icon name="prev" size={12} /></button>
                  <button className="mini" disabled={active!.r >= rows.length - 1} onClick={() => setActive({ r: active!.r + 1, c: active!.c })}><Icon name="next" size={12} /></button>
                </div>
                <div className="record-fields">
                  {page.columns.map((col, ci) => {
                    const cell = cellOf(activeRow, ci);
                    const m = colMeta(col.name);
                    const can = canEditCell(activeRow, ci) && !(cell.long && m?.binary);
                    const fk = fkFor(col.name);
                    return (
                      <label key={`${activeRow.id}:${col.name}:${cell.text}:${cell.staged?.kind}`} className={cell.staged ? "edited" : ""}>
                        <span>{meta.primary_key.includes(col.name) ? <Icon name="key" size={12} className="ico-pk" /> : null}{col.name} <span className="dim">{m?.column_type}</span></span>
                        <div className="rf-input">
                          <input disabled={!can || cell.long} defaultValue={cell.staged && cell.staged.kind !== "text" ? "" : cell.text ?? ""}
                            placeholder={cell.staged?.kind === "default" ? "DEFAULT" : cell.text === null && !activeRow.isNew ? "NULL" : ""}
                            onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
                            onBlur={(e) => {
                              if (e.target.value === (cell.text ?? "")) return;
                              stage(activeRow, ci, { kind: "text", value: e.target.value });
                            }} />
                          {can && fk && fk.columns.length === 1 && <button className="mini" onClick={() => setFkPick({ r: active!.r, c: ci, fk })}>…</button>}
                          {can && m?.nullable && <button className="mini" title="Set NULL" onClick={() => stage(activeRow, ci, { kind: "null" })}>∅</button>}
                        </div>
                      </label>
                    );
                  })}
                </div>
              </>
            )}
          </aside>
        )}
      </div>

      <div className="grid-foot">
        {page ? `${page.rows.length.toLocaleString()} row(s) loaded · ${page.elapsed_ms} ms` : ""}
        {pendingCount > 0 && <span className="pending"> · {pendingCount} pending change(s)</span>}
      </div>

      {menu && <ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />}
      {fkPick && (
        <FkPicker connectionId={cid} fk={fkPick.fk} onClose={() => setFkPick(null)}
          onPick={(value) => { const row = rows[fkPick.r]; if (row) stage(row, fkPick.c, { kind: "text", value }); setFkPick(null); setEditing(null); }} />
      )}
      {cellModal && page && (() => {
        const m = colMeta(page.columns[cellModal.c].name);
        const row = rows[cellModal.r];
        return (
          <CellModal column={page.columns[cellModal.c].name} type={m?.column_type ?? ""} isJson={m?.data_type.toLowerCase().startsWith("json") ?? false}
                     initial={cellModal.initial} editable={cellModal.editable} nullable={!!m?.nullable} cut={cellModal.cut}
                     onApply={(v) => { if (row) stage(row, cellModal.c, v); setCellModal(null); scroller.current?.focus(); }}
                     onClose={() => { setCellModal(null); scroller.current?.focus(); }} />
        );
      })()}
      {confirmApply && (
        <ConfirmDialog message={`${props.confirmCommit} ${pendingCount} change${pendingCount === 1 ? "" : "s"} to ${db}.${table} will be saved.`} confirmLabel="Save changes"
                       onCancel={() => setConfirmApply(false)} onConfirm={() => { setConfirmApply(false); submit(); }} />
      )}
      {confirm && <ConfirmDialog message={confirm.message} confirmLabel="Discard" onCancel={() => setConfirm(null)} onConfirm={() => { const c = confirm; setConfirm(null); c.run(); }} />}
      {sqlDialog && (
        <div className="overlay" onMouseDown={() => setSqlDialog(null)}>
          <div className="dialog wide" onMouseDown={(e) => e.stopPropagation()}>
            <h3>Queries</h3>
            <div className="dim">Current page</div>
            <code className="preview">{sqlDialog.query}</code>
            {sqlDialog.pending.length > 0 && (<><div className="dim">Pending changes (one transaction)</div>
              <code className="preview">{sqlDialog.pending.join(";\n") + ";"}</code></>)}
            <div className="actions">
              <button onClick={() => clip([sqlDialog.query, ...sqlDialog.pending].join(";\n"))}>Copy</button>
              <button className="primary" onClick={() => setSqlDialog(null)}>Close</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

/** One column's filter: operator + value input(s). Empty input means "no filter" (except NULL operators). */
function FilterCell(props: { value: FilterInput | undefined; numeric: boolean; onChange(v: FilterInput | null): void }) {
  const v = props.value ?? { op: props.numeric ? "eq" : "contains", text: "", text2: "" } as FilterInput;
  const arity = opInfo(v.op).arity;
  const set = (p: Partial<FilterInput>) => props.onChange({ ...v, ...p });
  const active = toFilter("x", v) !== null;
  return (
    <div className={`fcell ${active ? "on" : ""}`} onClick={(e) => e.stopPropagation()}>
      <Select value={v.op} onChange={(e) => set({ op: e.target.value as FilterInput["op"] })} title="Filter operator">
        {OPS.map((o) => <option key={o.op} value={o.op}>{o.label}</option>)}
      </Select>
      {arity !== 0 && <input value={v.text} placeholder={arity === "many" ? "a, b, c" : arity === 2 ? "from" : "filter"} onChange={(e) => set({ text: e.target.value })} />}
      {arity === 2 && <input value={v.text2} placeholder="to" onChange={(e) => set({ text2: e.target.value })} />}
      {props.value && <button className="mini" title="Clear" onClick={() => props.onChange(null)}><Icon name="close" size={10} /></button>}
    </div>
  );
}
