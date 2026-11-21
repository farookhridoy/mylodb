import { useCallback, useRef, useState } from "react";
import { api, type ColumnDetail, type IndexDetail, type TableStat } from "./api";

/** "1.4 MB", "312 KB", "0 B". Binary units, one decimal from MB up. */
export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`;
  const units = ["MB", "GB", "TB"];
  let v = n / (1024 * 1024), i = 0;
  while (v >= 1024 && i < units.length - 1) { v /= 1024; i++; }
  return `${v.toFixed(v < 10 ? 2 : 1)} ${units[i]}`;
}

export type ColumnsOf = ColumnDetail[] | "loading" | { error: string };

/**
 * What the object browser shows beyond names: table sizes per database and column types per table.
 * Both are fetched when first needed (a database expanded, a table selected) and cached until `reset`.
 */
export function useBrowserMeta(connectionId: number | null) {
  const [stats, setStats] = useState<Record<string, Record<string, TableStat>>>({});
  const [details, setDetails] = useState<Record<string, ColumnsOf>>({});
  const [indexes, setIndexes] = useState<Record<string, IndexDetail[]>>({});
  // requests in flight or done, so two renders never ask twice; cleared by reset()
  const asked = useRef(new Set<string>());
  const generation = useRef(0);

  const reset = useCallback(() => {
    generation.current++;
    asked.current.clear();
    setStats({});
    setDetails({});
    setIndexes({});
  }, []);

  const ensureStats = useCallback((db: string) => {
    if (connectionId === null || asked.current.has(`s\0${db}`)) return;
    asked.current.add(`s\0${db}`);
    const gen = generation.current;
    api.tableStats(connectionId, db)
      .then((rows) => gen === generation.current && setStats((s) => ({ ...s, [db]: Object.fromEntries(rows.map((r) => [r.name, r])) })))
      .catch(() => asked.current.delete(`s\0${db}`)); // sizes are decoration: retry on next expand, never block the tree
  }, [connectionId]);

  const ensureDetails = useCallback((db: string, table: string) => {
    const key = `${db}\0${table}`;
    if (connectionId === null || asked.current.has(`c\0${key}`)) return;
    asked.current.add(`c\0${key}`);
    const gen = generation.current;
    setDetails((d) => ({ ...d, [key]: "loading" }));
    // indexes are decoration next to the columns: a failure here must not hide the columns
    api.indexDetails(connectionId, db, table)
      .then((rows) => gen === generation.current && setIndexes((d) => ({ ...d, [key]: rows })))
      .catch(() => {});
    api.columnDetails(connectionId, db, table)
      .then((cols) => gen === generation.current && setDetails((d) => ({ ...d, [key]: cols })))
      .catch((e) => { if (gen === generation.current) { asked.current.delete(`c\0${key}`); setDetails((d) => ({ ...d, [key]: { error: String(e) } })); } });
  }, [connectionId]);

  return { stats, details, indexes, ensureStats, ensureDetails, reset };
}

/** Column filter used when a table is selected: every space-separated term must be in the name or the type; `*` and `?` are wildcards. */
export function columnMatches(query: string, c: ColumnDetail): boolean {
  const terms = query.split(/\s+/).filter(Boolean);
  if (terms.length === 0) return true;
  const name = c.name.toLowerCase(), type = c.column_type.toLowerCase();
  return terms.every((raw) => {
    const t = raw.toLowerCase();
    if (/[*?]/.test(t)) {
      const re = new RegExp("^" + t.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".") + "$");
      return re.test(name) || re.test(type);
    }
    return name.includes(t) || type.includes(t) || (t === "pk" && c.key === "PRI") || (t === "null" && c.nullable);
  });
}
