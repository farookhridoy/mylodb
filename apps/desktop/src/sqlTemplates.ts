import type { ColumnDetail } from "./api";

export type SampleKind = "select" | "insert" | "update" | "delete" | "upsert";

const q = (s: string) => "`" + s.replace(/`/g, "``") + "`";
const lit = (s: string) => "'" + s.replace(/\\/g, "\\\\").replace(/'/g, "''") + "'";

/**
 * A ready-to-edit sample statement for a table, with every column named (the SQLyog "Generate SQL" idea).
 * Values are the column names in quotes so they are easy to find and replace.
 * `qualify` adds the database name; leave it off when the console already runs in that database.
 */
export function sampleQuery(kind: SampleKind, db: string, table: string, columns: ColumnDetail[], qualify: boolean): string {
  const name = qualify ? `${q(db)}.${q(table)}` : q(table);
  const writable = columns.filter((c) => !/auto_increment|generated/i.test(c.extra));
  const keys = columns.filter((c) => c.key === "PRI");
  const where = keys.length > 0
    ? keys.map((c) => `${q(c.name)} = ${lit(c.name)}`).join("\n  AND ")
    : "1 = 0 /* add a condition: without a primary key every row would match */";
  const list = (cols: ColumnDetail[], sep: string) => cols.map((c) => q(c.name)).join(sep);
  switch (kind) {
    case "select":
      return `SELECT\n  ${list(columns.length ? columns : [], ",\n  ") || "*"}\nFROM ${name}\nWHERE 1 = 1\nLIMIT 100;`;
    case "insert": {
      const cols = writable.length ? writable : columns;
      return `INSERT INTO ${name}\n  (${list(cols, ", ")})\nVALUES\n  (${cols.map((c) => lit(c.name)).join(", ")});`;
    }
    case "update": {
      const cols = (writable.length ? writable : columns).filter((c) => !keys.includes(c));
      const set = (cols.length ? cols : writable).map((c) => `${q(c.name)} = ${lit(c.name)}`).join(",\n  ");
      return `UPDATE ${name}\nSET\n  ${set}\nWHERE ${where};`;
    }
    case "upsert": {
      const cols = writable.length ? writable : columns;
      const update = cols.filter((c) => !keys.includes(c));
      return `INSERT INTO ${name}\n  (${list(cols, ", ")})\nVALUES\n  (${cols.map((c) => lit(c.name)).join(", ")})\nON DUPLICATE KEY UPDATE\n  ${(update.length ? update : cols).map((c) => `${q(c.name)} = VALUES(${q(c.name)})`).join(",\n  ")};`;
    }
    case "delete":
      return `DELETE FROM ${name}\nWHERE ${where};`;
  }
}
