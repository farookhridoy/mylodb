import { invoke } from "@tauri-apps/api/core";

export type Kind = "database" | "table" | "view" | "procedure" | "function" | "trigger" | "event" | "column";
export interface Node { database: string; kind: Kind; name: string; parent: string | null }
export type SslMode = "disabled" | "required" | "verifyca" | "skipverify";
export type SshAuth = { method: "password" } | { method: "key"; path: string };
export interface SshConfig { host: string; port: number; user: string; auth: SshAuth }
export interface SavedConnection {
  id: number | null;
  name: string;
  color: string;
  host: string;
  port: number;
  user: string;
  database: string | null;
  ssl: SslMode;
  ssl_ca?: string | null;
  ssl_cert?: string | null;
  ssl_key?: string | null;
  ssh?: SshConfig | null;
  new_password?: string | null;
  new_ssh_password?: string | null;
  new_ssh_passphrase?: string | null;
}

/** Why a connection failed. The SSH host-key cases need the user's decision. */
export type ConnectError =
  | { kind: "unknown_host"; host: string; fingerprint: string }
  | { kind: "host_key_changed"; host: string; expected: string; actual: string }
  | { kind: "failed"; message: string };
export interface TestResult { version: string; ssl_cipher: string | null; via_ssh: boolean }
export const describeError = (e: unknown): string => {
  const c = e as Partial<ConnectError> | string;
  if (typeof c === "object" && c && "kind" in c) {
    if (c.kind === "failed") return (c as { message: string }).message;
    if (c.kind === "unknown_host") return `The SSH server ${(c as { host: string }).host} is not known yet.`;
    if (c.kind === "host_key_changed") return `The host key of ${(c as { host: string }).host} changed.`;
  }
  return String(e);
};

export const api = {
  listConnections: () => invoke<SavedConnection[]>("list_connections"),
  saveConnection: (connection: SavedConnection) => invoke<number>("save_connection", { connection }),
  deleteConnection: (id: number) => invoke<void>("delete_connection", { id }),
  connect: (id: number, trustFingerprint?: string) => invoke<Node[]>("connect", { id, trustFingerprint: trustFingerprint ?? null }),
  testConnection: (connection: SavedConnection, trustFingerprint?: string) => invoke<TestResult>("test_connection", { connection, trustFingerprint: trustFingerprint ?? null }),
  forgetSshHost: (id: number) => invoke<void>("forget_ssh_host", { id }),
  disconnect: (id: number) => invoke<void>("disconnect", { id }),
  /** `scope` = the selected database: only its objects are searched. Without one, only database names are. */
  filterIndex: (id: number, query: string, scope: string | null) => invoke<number[]>("filter_index", { id, query, scope }),
  tableStats: (id: number, db: string) => invoke<TableStat[]>("db_table_stats", { id, db }),
  columnDetails: (id: number, db: string, table: string) => invoke<ColumnDetail[]>("table_column_details", { id, db, table }),
  loadColumns: (id: number, db: string) => invoke<{ start: number; nodes: Node[] }>("load_columns", { id, db }),
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
export type ExportFormat = "csv" | "json" | "sql_insert" | "xml" | "html" | "xlsx";
export const EXPORT_FORMATS: { format: ExportFormat; label: string; ext: string }[] = [
  { format: "csv", label: "CSV", ext: "csv" }, { format: "json", label: "JSON", ext: "json" }, { format: "sql_insert", label: "SQL INSERT", ext: "sql" },
  { format: "xml", label: "XML", ext: "xml" }, { format: "html", label: "HTML table", ext: "html" }, { format: "xlsx", label: "Excel (XLSX)", ext: "xlsx" },
];

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
    invoke<{ rows: number; warnings: string[] }>("export_table", { id, db, table, spec, options: { format, null_text: nullText }, path }),
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
/** Size (data + index bytes) and row estimate of a table; views have none. */
export interface TableStat { name: string; bytes: number | null; rows: number | null }
export interface ColumnDetail { name: string; column_type: string; nullable: boolean; key: string; extra: string }

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

// ---- server tools ----
export interface ServerInfo {
  version: string; version_comment: string; hostname: string; port: number; datadir: string; charset: string; collation: string;
  sql_mode: string; time_zone: string; max_connections: number; uptime_seconds: number; ssl_cipher: string | null;
}
export interface Process { id: number; user: string; host: string; db: string | null; command: string; time: number; state: string | null; info: string | null }
export interface Variable { name: string; value: string; source: string | null }
export type VarScope = "global" | "session";
export type KillKind = "query" | "connection";
export type CheckOption = "quick" | "fast" | "changed" | "medium" | "extended";
export type Maintenance =
  | { op: "check"; option: CheckOption | null } | { op: "analyze" } | { op: "optimize" }
  | { op: "repair"; quick: boolean; extended: boolean; use_frm: boolean };
export interface MaintenanceRow { table: string; op: string; msg_type: string; msg_text: string }
export interface LogSettings {
  general_log: boolean; slow_query_log: boolean; log_output: string; long_query_time: string; log_queries_not_using_indexes: boolean;
  log_error: string | null; slow_query_log_file: string | null; general_log_file: string | null; log_bin: boolean; error_log_in_table: boolean;
}
export interface LogLine { time: string; level: string; source: string; text: string }
export interface SlowQuery { start_time: string; user_host: string; query_time: string; lock_time: string; rows_sent: number; rows_examined: number; db: string; sql_text: string }
export interface BinaryLog { name: string; size: number }
export interface BinlogEvent { pos: number; event_type: string; end_pos: number; info: string }

export const serverApi = {
  info: (id: number) => invoke<ServerInfo>("server_info", { id }),
  processes: (id: number) => invoke<{ processes: Process[]; self_id: number }>("process_list", { id }),
  kill: (id: number, processId: number, kind: KillKind) => invoke<void>("kill_process", { id, processId, kind }),
  variables: (id: number, scope: VarScope) => invoke<Variable[]>("server_variables", { id, scope }),
  status: (id: number, scope: VarScope) => invoke<[string, string][]>("server_status", { id, scope }),
  setGlobal: (id: number, name: string, value: string) => invoke<string[]>("set_global_variable", { id, name, value }),
  maintain: (id: number, db: string, tables: string[], operation: Maintenance) => invoke<MaintenanceRow[]>("maintain_tables", { id, db, tables, operation }),
  logSettings: (id: number) => invoke<LogSettings>("log_settings", { id }),
  errorLog: (id: number, limit: number, levels: string[], search: string) => invoke<LogLine[]>("error_log", { id, limit, levels, search }),
  slowLog: (id: number, limit: number, search: string) => invoke<SlowQuery[]>("slow_log", { id, limit, search }),
  binaryLogs: (id: number) => invoke<BinaryLog[]>("binary_logs", { id }),
  binlogEvents: (id: number, file: string, fromPos: number, limit: number) => invoke<BinlogEvent[]>("binlog_events", { id, file, fromPos, limit }),
};

// ---- backup, restore, import, export ----
import { listen } from "@tauri-apps/api/event";

export interface JobProgress { job_id: string; phase: string; done: number; total: number | null; message: string }
export interface CsvFormat { delimiter: string; quote: string; has_header: boolean; encoding: string; null_text: string | null; trim: boolean }
export interface CsvPreview { headers: string[]; rows: string[][]; column_count: number; more: boolean }
export interface InferredColumn { name: string; column_type: string }
export type ImportMode = "insert" | "ignore" | "replace" | "update";
export interface ImportPlan {
  database: string; table: string; mapping: (string | null)[]; mode: ImportMode; batch_rows: number;
  stop_on_error: boolean; truncate_first: boolean; empty_as_null: boolean;
}
export interface ImportResult {
  rows_read: number; affected_rows: number; errors: { line: number; message: string }[]; ragged_rows: number; aborted: boolean;
  rolled_back: boolean; warnings: string[]; elapsed_ms: number;
}
export interface RunOptions { stop_on_error: boolean; disable_fk_checks: boolean; database: string | null }
export interface RunResult {
  files: number; statements: number; failed: number; aborted: boolean; elapsed_ms: number;
  errors: { file: string; line: number; statement: string; message: string }[];
}
export interface BackupOptions {
  databases: { name: string; tables: string[] | null }[]; structure: boolean; data: boolean; drop_if_exists: boolean; create_database: boolean;
  views: boolean; routines: boolean; triggers: boolean; events: boolean; rows_per_insert: number; snapshot: boolean; disable_fk_checks: boolean;
  strip_definer: boolean; file_per_table: boolean; compress: boolean;
}
export interface BackupResult { path: string; files: number; databases: number; tables: number; rows: number; bytes: number; warnings: string[]; elapsed_ms: number }

/** Subscribe to one job's progress. Returns the function that stops listening. */
export async function onJobProgress(jobId: string, cb: (p: JobProgress) => void): Promise<() => void> {
  const un = await listen<JobProgress>("job-progress", (e) => { if (e.payload.job_id === jobId) cb(e.payload); });
  return () => { un(); };
}

export const transferApi = {
  sniff: (path: string) => invoke<string>("csv_sniff", { path }),
  preview: (path: string, format: CsvFormat, limit = 20) => invoke<CsvPreview>("csv_preview", { path, format, limit }),
  infer: (path: string, format: CsvFormat) => invoke<InferredColumn[]>("csv_infer", { path, format }),
  createTable: (id: number, db: string, table: string, columns: InferredColumn[]) => invoke<Node[]>("csv_create_table", { id, db, table, columns }),
  importCsv: (id: number, jobId: string, path: string, format: CsvFormat, plan: ImportPlan) => invoke<ImportResult>("import_csv_job", { id, jobId, path, format, plan }),
  runSource: (id: number, jobId: string, path: string, options: RunOptions) => invoke<RunResult>("run_sql_source_job", { id, jobId, path, options }),
  backup: (id: number, jobId: string, options: BackupOptions, dir: string, name: string) => invoke<BackupResult>("backup_job", { id, jobId, options, dir, name }),
  cancel: (jobId: string) => invoke<void>("cancel_job", { jobId }),
  exportQuery: (id: number, database: string | null, sql: string, format: ExportFormat, nullText: string, path: string) =>
    invoke<{ rows: number; warnings: string[] }>("export_query", { id, database, sql, options: { format, null_text: nullText }, path }),
};
