import { useCallback, useEffect, useState } from "react";
import { api, describeError, type TableOverview } from "./api";

// Last result per table, shown immediately while a fresh one loads (schemas change rarely; the view always refreshes on open).
const cache = new Map<string, TableOverview>();

/** Overview of one table for the Properties and Diagram views. */
export function useTableOverview(connectionId: number, db: string, table: string) {
  const key = `${connectionId}:${db}.${table}`;
  const [data, setData] = useState<TableOverview | undefined>(() => cache.get(key));
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const o = await api.tableOverview(connectionId, db, table);
      cache.set(key, o);
      setData(o);
      setError(null);
    } catch (e) {
      setError(describeError(e));
    } finally {
      setLoading(false);
    }
  }, [connectionId, db, table, key]);

  useEffect(() => { setData(cache.get(key)); load(); }, [key, load]);
  return { data, loading, error, refresh: load };
}
