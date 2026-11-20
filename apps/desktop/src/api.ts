import { invoke } from "@tauri-apps/api/core";

export type Kind = "database" | "table" | "view" | "procedure" | "function" | "trigger" | "event" | "column";
export interface Node { database: string; kind: Kind; name: string; parent: string | null }
export type SslMode = "disabled" | "required" | "skipverify";
export interface SavedConnection {
  id: number | null;
  name: string;
  color: string;
  host: string;
  port: number;
  user: string;
  database: string | null;
  ssl: SslMode;
  new_password?: string | null;
}

export const api = {
  listConnections: () => invoke<SavedConnection[]>("list_connections"),
  saveConnection: (connection: SavedConnection) => invoke<number>("save_connection", { connection }),
  deleteConnection: (id: number) => invoke<void>("delete_connection", { id }),
  connect: (id: number) => invoke<Node[]>("connect", { id }),
  disconnect: (id: number) => invoke<void>("disconnect", { id }),
  filterIndex: (id: number, query: string) => invoke<number[]>("filter_index", { id, query }),
};

export interface ColumnInfo { name: string; type_name: string }
export interface ResultSet { columns: ColumnInfo[]; rows: (string | null)[][]; total_rows: number; truncated: boolean }
export interface StatementResult {
  sql: string;
  sets: ResultSet[];
  affected_rows: number;
  last_insert_id: number | null;
  warnings: number;
  elapsed_ms: number;
  error: string | null;
}
export interface HistoryEntry { id: number; sql: string; ran_at: string; duration_ms: number; ok: boolean }
export interface Favorite { id: number | null; folder: string; name: string; sql: string }

export const queryApi = {
  run: (id: number, runId: string, database: string | null, sql: string) =>
    invoke<StatementResult[]>("run_query", { id, runId, database, sql }),
  cancel: (runId: string) => invoke<void>("cancel_query", { runId }),
  databases: (id: number) => invoke<string[]>("list_databases", { id }),
  historyList: (id: number, search: string) => invoke<HistoryEntry[]>("history_list", { id, search }),
  historyClear: (id: number) => invoke<void>("history_clear", { id }),
  favoritesList: () => invoke<Favorite[]>("favorites_list"),
  favoriteSave: (favorite: Favorite) => invoke<number>("favorite_save", { favorite }),
  favoriteDelete: (id: number) => invoke<void>("favorite_delete", { id }),
  sessionGet: (id: number) => invoke<string | null>("session_get", { id }),
  sessionSet: (id: number, session: string) => invoke<void>("session_set", { id, session }),
};
