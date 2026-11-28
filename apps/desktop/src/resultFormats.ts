import type { ResultSet } from "./api";
import { doubleQuotes, quoteIdent } from "./quoting";

/** Ways to copy a result: each takes the rows as the server returned them (strings, `null` for NULL) and returns text. */
export type CopyFormat = "tsv" | "csv" | "json" | "markdown" | "insert";
export const COPY_FORMATS: { id: CopyFormat; label: string }[] = [
  { id: "tsv", label: "Tab-separated (TSV)" }, { id: "csv", label: "CSV" }, { id: "json", label: "JSON" },
  { id: "markdown", label: "Markdown table" }, { id: "insert", label: "INSERT statements" },
];

const csvCell = (v: string | null) => (v === null ? "" : /[",\r\n]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v);
const tsvCell = (v: string | null) => (v === null ? "NULL" : v.replace(/[\t\r\n]/g, " "));
// PostgreSQL strings have no backslash escapes
const sqlValue = (v: string | null) => (v === null ? "NULL" : `'${(doubleQuotes() ? v : v.replace(/\\/g, "\\\\")).replace(/'/g, "''")}'`);
const ident = quoteIdent;

export function formatResult(set: ResultSet, format: CopyFormat, tableName = "table_name"): string {
  const names = set.columns.map((c) => c.name);
  switch (format) {
    case "tsv": return [names.join("\t"), ...set.rows.map((r) => r.map(tsvCell).join("\t"))].join("\n");
    case "csv": return [names.map((n) => csvCell(n)).join(","), ...set.rows.map((r) => r.map(csvCell).join(","))].join("\n");
    case "json": return JSON.stringify(set.rows.map((r) => Object.fromEntries(names.map((n, i) => [n, r[i]]))), null, 2);
    case "markdown": {
      const esc = (v: string | null) => (v === null ? "NULL" : v.replace(/\|/g, "\\|").replace(/\r?\n/g, " "));
      return [`| ${names.join(" | ")} |`, `| ${names.map(() => "---").join(" | ")} |`, ...set.rows.map((r) => `| ${r.map(esc).join(" | ")} |`)].join("\n");
    }
    case "insert":
      return set.rows.map((r) => `INSERT INTO ${ident(tableName)} (${names.map(ident).join(", ")}) VALUES (${r.map(sqlValue).join(", ")});`).join("\n");
  }
}

/** The mysql-client style box drawing, columns padded to their widest cell (capped at `cap` characters, longer values are cut with …). */
export function textTable(set: ResultSet, cap = 60): string {
  const cut = (v: string | null) => { const s = (v === null ? "NULL" : v).replace(/\r?\n/g, "↵"); return s.length > cap ? s.slice(0, cap - 1) + "…" : s; };
  const head = set.columns.map((c) => cut(c.name));
  const body = set.rows.map((r) => r.map(cut));
  const widths = head.map((h, i) => Math.max([...h].length, ...body.map((r) => [...r[i]].length)));
  const pad = (s: string, w: number) => s + " ".repeat(w - [...s].length);
  const line = "+" + widths.map((w) => "-".repeat(w + 2)).join("+") + "+";
  const row = (cells: string[]) => "| " + cells.map((c, i) => pad(c, widths[i])).join(" | ") + " |";
  return [line, row(head), line, ...body.map(row), line].join("\n");
}
