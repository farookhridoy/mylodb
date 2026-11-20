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

export interface ColumnInfo { name: string; type_name: string; binary: boolean }
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

// ---- table data grid ----
export interface ColumnMeta {
  name: string; data_type: string; column_type: string; nullable: boolean; key: string;
  default: string | null; extra: string; comment: string; binary: boolean; generated: boolean; auto_increment: boolean;
}
export interface ForeignKey { name: string; columns: string[]; ref_schema: string; ref_table: string; ref_columns: string[] }
export interface TableMeta {
  database: string; table: string; is_view: boolean; columns: ColumnMeta[]; primary_key: string[];
  row_key: string[]; foreign_keys: ForeignKey[]; read_only_reason: string | null;
}
export type Op =
  | "contains" | "not_contains" | "eq" | "ne" | "starts_with" | "ends_with" | "lt" | "le" | "gt" | "ge"
  | "between" | "in" | "regexp" | "is_null" | "not_null" | "is_empty";
export interface ColumnFilter { column: string; op: Op; values: string[] }
export interface Sort { column: string; desc: boolean }
export interface PageSpec { filters: ColumnFilter[]; where_sql: string | null; sort: Sort[]; limit: number; offset: number }
export interface Page {
  columns: ColumnInfo[]; rows: (string | null)[][]; long_cells: [number, number][]; has_more: boolean; elapsed_ms: number; sql: string;
}
export type CellValue = { kind: "null" } | { kind: "default" } | { kind: "text"; value: string } | { kind: "file"; value: string };
export interface Assign { column: string; value: CellValue }
export type Change =
  | { op: "update"; key: Assign[]; set: Assign[] }
  | { op: "insert"; values: Assign[] }
  | { op: "delete"; key: Assign[] };
export interface CellData { is_null: boolean; len: number; truncated: boolean; base64: string | null }
export interface LookupItem { value: string; label: string | null }
export type ExportFormat = "csv" | "json" | "sql_insert";

export const dataApi = {
  meta: (id: number, db: string, table: string) => invoke<TableMeta>("table_meta", { id, db, table }),
  page: (id: number, db: string, table: string, spec: PageSpec) => invoke<Page>("table_page", { id, db, table, spec }),
  count: (id: number, db: string, table: string, filters: ColumnFilter[], whereSql: string | null) =>
    invoke<number>("table_count", { id, db, table, filters, whereSql }),
  preview: (id: number, db: string, table: string, changes: Change[]) => invoke<string[]>("preview_changes", { id, db, table, changes }),
  apply: (id: number, db: string, table: string, changes: Change[]) =>
    invoke<{ applied: number; insert_ids: number[] }>("apply_changes", { id, db, table, changes }),
  cell: (id: number, db: string, table: string, key: Assign[], column: string) =>
    invoke<CellData>("fetch_cell", { id, db, table, key, column }),
  saveCell: (id: number, db: string, table: string, key: Assign[], column: string, path: string) =>
    invoke<number>("save_cell_to_file", { id, db, table, key, column, path }),
  lookup: (id: number, refSchema: string, refTable: string, refColumn: string, search: string) =>
    invoke<LookupItem[]>("fk_lookup", { id, refSchema, refTable, refColumn, search }),
  exportTable: (id: number, db: string, table: string, spec: PageSpec, format: ExportFormat, nullText: string, path: string) =>
    invoke<number>("export_table", { id, db, table, spec, options: { format, null_text: nullText }, path }),
};

// ---- schema designer ----
export type ColumnDefault = { kind: "none" } | { kind: "null" } | { kind: "literal"; value: string } | { kind: "expression"; value: string };
export interface ColumnDef {
  name: string; original_name: string | null; data_type: string; params: string | null; unsigned: boolean; zerofill: boolean;
  nullable: boolean; default: ColumnDefault; auto_increment: boolean; charset: string | null; collation: string | null;
  comment: string; on_update: string | null; generated: { expression: string; stored: boolean } | null; invisible: boolean;
}
export type IndexKind = "primary" | "unique" | "index" | "fulltext" | "spatial";
export interface IndexColumn { name: string; length: number | null; desc: boolean }
export interface IndexDef { name: string; original_name: string | null; kind: IndexKind; columns: IndexColumn[] }
export type RefAction = "restrict" | "cascade" | "set_null" | "no_action" | "set_default";
export interface ForeignKeyDef {
  name: string; original_name: string | null; columns: string[]; ref_schema: string | null; ref_table: string; ref_columns: string[];
  on_delete: RefAction; on_update: RefAction;
}
export interface TableDef {
  name: string; engine: string | null; charset: string | null; collation: string | null; comment: string;
  columns: ColumnDef[]; indexes: IndexDef[]; foreign_keys: ForeignKeyDef[];
}
export interface ServerOptions { engines: string[]; charsets: [string, string[], string][] }

export type TableOp =
  | { op: "truncate" } | { op: "drop" } | { op: "rename"; to: string } | { op: "duplicate"; to: string; with_data: boolean };
export type DatabaseOp = { op: "create"; name: string; charset: string | null; collation: string | null } | { op: "drop"; name: string };

export const schemaApi = {
  tableDef: (id: number, db: string, table: string) => invoke<{ def: TableDef; warnings: string[] }>("table_def", { id, db, table }),
  preview: (db: string, original: TableDef | null, desired: TableDef) =>
    invoke<{ statements: string[]; errors: string[] }>("ddl_preview", { db, original, desired }),
  apply: (id: number, db: string, original: TableDef | null, desired: TableDef) => invoke<string[]>("ddl_apply", { id, db, original, desired }),
  refreshIndex: (id: number) => invoke<Node[]>("refresh_index", { id }),
  tableOp: (id: number, db: string, table: string, operation: TableOp) => invoke<string[]>("table_op", { id, db, table, operation }),
  databaseOp: (id: number, operation: DatabaseOp) => invoke<string>("database_op", { id, operation }),
  serverOptions: (id: number) => invoke<ServerOptions>("server_options", { id }),
};

// ---- users and privileges ----
export interface Account { user: string; host: string; locked: boolean; password_expired: boolean; plugin: string }
export interface AccountEdit { user: string; host: string; password: string | null; plugin: string | null; locked: boolean; expire_password: boolean }
export type Scope = { level: "global" } | { level: "database"; db: string } | { level: "table"; db: string; table: string };
export type ScopeGrant = Scope & { privileges: string[]; columns: Record<string, string[]>; grant_option: boolean };
export interface Grants { scopes: ScopeGrant[]; unsupported: string[] }

export const userApi = {
  list: (id: number) => invoke<Account[]>("accounts_list", { id }),
  grants: (id: number, user: string, host: string) => invoke<Grants>("account_grants", { id, user, host }),
  preview: (id: number, original: Account | null, edit: AccountEdit, desired: ScopeGrant[] | null) =>
    invoke<string[]>("account_preview", { id, original, edit, desired }),
  apply: (id: number, original: Account | null, edit: AccountEdit, desired: ScopeGrant[] | null) =>
    invoke<number>("account_apply", { id, original, edit, desired }),
  drop: (id: number, user: string, host: string) => invoke<void>("account_drop", { id, user, host }),
};

// ---- editor intelligence ----
export type SuggestionKind = "column" | "table" | "view" | "database" | "alias" | "function" | "keyword";
export interface Suggestion { label: string; kind: SuggestionKind; detail: string | null; insert: string }
export interface Completions { from: number; to: number; items: Suggestion[] }
export interface FormatOptions {
  indent: { style: "spaces"; width: number } | { style: "tab" };
  keyword_case: "upper" | "lower" | "preserve";
  lines_between_statements: number;
}
export interface Hint { severity: "warning" | "info" | "good"; message: string; table: string | null }
export interface ExplainResult { plan: ResultSet; json: string | null; tree: string | null; hints: Hint[] }
export interface ProfileResult {
  elapsed_ms: number; rows_returned: number; stages: { name: string; ms: number }[]; stages_note: string | null;
  counters: { name: string; value: number }[];
}

export const editorApi = {
  complete: (id: number, sql: string, cursor: number, database: string | null) =>
    invoke<Completions>("complete_sql", { id, sql, cursor, database }),
  format: (sql: string, options: FormatOptions) => invoke<string>("format_sql", { sql, options }),
  statementAt: (sql: string, cursor: number) => invoke<string | null>("statement_at", { sql, cursor }),
  explain: (id: number, database: string | null, sql: string, analyze: boolean) =>
    invoke<ExplainResult>("explain_query", { id, database, sql, analyze }),
  profile: (id: number, database: string | null, sql: string) => invoke<ProfileResult>("profile_query", { id, database, sql }),
};
