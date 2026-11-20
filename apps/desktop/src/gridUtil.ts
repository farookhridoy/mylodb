import type { CellValue, ColumnFilter, ColumnMeta, Op } from "./api";

export interface OpInfo { op: Op; label: string; arity: 0 | 1 | 2 | "many" }

export const OPS: OpInfo[] = [
  { op: "contains", label: "contains", arity: 1 },
  { op: "not_contains", label: "doesn't contain", arity: 1 },
  { op: "eq", label: "=", arity: 1 },
  { op: "ne", label: "≠", arity: 1 },
  { op: "starts_with", label: "starts with", arity: 1 },
  { op: "ends_with", label: "ends with", arity: 1 },
  { op: "gt", label: ">", arity: 1 },
  { op: "ge", label: "≥", arity: 1 },
  { op: "lt", label: "<", arity: 1 },
  { op: "le", label: "≤", arity: 1 },
  { op: "between", label: "between", arity: 2 },
  { op: "in", label: "in (a, b, …)", arity: "many" },
  { op: "regexp", label: "regexp", arity: 1 },
  { op: "is_null", label: "is NULL", arity: 0 },
  { op: "not_null", label: "is not NULL", arity: 0 },
  { op: "is_empty", label: "is empty", arity: 0 },
];

export const opInfo = (op: Op) => OPS.find((o) => o.op === op)!;

/** Raw filter-row input for one column. */
export interface FilterInput { op: Op; text: string; text2: string }

/** Turn the filter-row input into a server filter, or null while it is incomplete. */
export function toFilter(column: string, f: FilterInput): ColumnFilter | null {
  const a = opInfo(f.op).arity;
  if (a === 0) return { column, op: f.op, values: [] };
  if (a === 1) return f.text === "" ? null : { column, op: f.op, values: [f.text] };
  if (a === 2) return f.text === "" || f.text2 === "" ? null : { column, op: f.op, values: [f.text, f.text2] };
  const parts = f.text.split(",").map((s) => s.trim()).filter((s) => s !== "");
  return parts.length === 0 ? null : { column, op: f.op, values: parts };
}

export function cellLabel(v: CellValue): string {
  switch (v.kind) {
    case "null": return "NULL";
    case "default": return "DEFAULT";
    case "text": return v.value;
    case "file": return `[file] ${v.value.split("/").pop()}`;
  }
}

export const isNumericType = (c: ColumnMeta | undefined) =>
  !!c && /^(tinyint|smallint|mediumint|int|integer|bigint|decimal|numeric|float|double|real|year|bit)$/.test(c.data_type.toLowerCase());

// ---- copy formats (operate on the displayed strings; null = SQL NULL) ----
type Cells = (string | null)[][];

export const toTsv = (rows: Cells) => rows.map((r) => r.map((v) => v ?? "").join("\t")).join("\n");

const csvField = (s: string) => (/[",\n\r]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s);
export const toCsv = (cols: string[], rows: Cells) =>
  [cols.map(csvField).join(","), ...rows.map((r) => r.map((v) => (v === null ? "" : csvField(v))).join(","))].join("\n");

export function toJson(cols: string[], rows: Cells, numeric: boolean[]) {
  return JSON.stringify(
    rows.map((r) => Object.fromEntries(cols.map((c, i) => [c, r[i] !== null && numeric[i] && !isNaN(Number(r[i])) ? Number(r[i]) : r[i]]))),
    null, 2,
  );
}

export function toSqlInsert(table: string, cols: string[], rows: Cells, numeric: boolean[], binary: boolean[]) {
  const q = (s: string) => "`" + s.replace(/`/g, "``") + "`";
  const lit = (v: string | null, i: number) =>
    v === null ? "NULL" : numeric[i] ? v : binary[i] && v.startsWith("0x") ? `X'${v.slice(2)}'` : `'${v.replace(/\\/g, "\\\\").replace(/'/g, "''")}'`;
  return rows.map((r) => `INSERT INTO ${q(table)} (${cols.map(q).join(", ")}) VALUES (${r.map(lit).join(", ")});`).join("\n");
}

// ---- binary helpers ----
export function b64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export function imageMime(b: Uint8Array): string | null {
  const is = (...sig: number[]) => sig.every((x, i) => b[i] === x);
  if (is(0x89, 0x50, 0x4e, 0x47)) return "image/png";
  if (is(0xff, 0xd8, 0xff)) return "image/jpeg";
  if (is(0x47, 0x49, 0x46, 0x38)) return "image/gif";
  if (is(0x42, 0x4d)) return "image/bmp";
  if (is(0x52, 0x49, 0x46, 0x46) && b[8] === 0x57 && b[9] === 0x45) return "image/webp";
  return null;
}

/** UTF-8 text when the bytes are valid UTF-8, else null. */
export function bytesToText(b: Uint8Array): string | null {
  try { return new TextDecoder("utf-8", { fatal: true }).decode(b); } catch { return null; }
}

export function hexDump(b: Uint8Array, limit = 4096): string {
  const lines: string[] = [];
  for (let o = 0; o < Math.min(b.length, limit); o += 16) {
    const chunk = b.slice(o, o + 16);
    const hex = [...chunk].map((x) => x.toString(16).padStart(2, "0")).join(" ");
    const ascii = [...chunk].map((x) => (x >= 32 && x < 127 ? String.fromCharCode(x) : ".")).join("");
    lines.push(`${o.toString(16).padStart(8, "0")}  ${hex.padEnd(47)}  ${ascii}`);
  }
  if (b.length > limit) lines.push(`… ${b.length - limit} more bytes`);
  return lines.join("\n");
}
