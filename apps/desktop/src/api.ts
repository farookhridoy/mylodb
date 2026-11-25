import { invoke } from "@tauri-apps/api/core";

export type Kind = "database" | "table" | "view" | "procedure" | "function" | "trigger" | "event" | "column";
export interface Node { database: string; kind: Kind; name: string; parent: string | null }
export type SslMode = "disabled" | "required" | "verifyca" | "skipverify";
export type SshAuth = { method: "password" } | { method: "key"; path: string };
export interface SshConfig { host: string; port: number; user: string; auth: SshAuth }
export type Environment = "development" | "staging" | "production";

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
  /// Safety: what the connection points at, whether the server session is read-only, a time limit per run, and the destructive-SQL warning.
  environment?: Environment;
  read_only?: boolean;
  query_timeout_secs?: number | null;
  /// Collation of the connection (SET NAMES … COLLATE …); the server default when empty.
  collation?: string | null;
  confirm_destructive?: boolean;
  /// Folder the connection is listed under in the picker; empty or null: ungrouped.
  group?: string | null;
  /// MySQL / MariaDB (default) or a SQLite file.
  engine?: "mysql" | "sqlite";
  /// The database file, for `engine = "sqlite"`.
  file?: string | null;
  /// false + a default database: only that database is listed.
  show_all_databases?: boolean;
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

export interface SecretsStatus { mode: "keychain" | "file"; unlocked: boolean }
/** Where saved passwords are kept: the macOS Keychain, or an encrypted file behind a master password. */
export const secretsApi = {
  status: () => invoke<SecretsStatus>("secrets_status"),
  unlock: (password: string) => invoke<SecretsStatus>("secrets_unlock", { password }),
  useFile: (password: string) => invoke<number>("secrets_use_file", { password }),
  useKeychain: () => invoke<number>("secrets_use_keychain"),
  changePassword: (old: string, next: string) => invoke<void>("secrets_change_password", { old, new: next }),
  reset: () => invoke<void>("secrets_reset"),
};

/** The AI assistant: a service that speaks the common chat protocol (Ollama on this computer, Groq, OpenRouter, Gemini…) or Claude's own API. */
export interface AiModel { id: string; name: string; context: number | null; free: boolean }
export interface AiSettings { preset: string; endpoint: string; model: string; fallbacks: { id: string; context: number | null }[]; context: number | null }
export interface AiTurn { role: "user" | "assistant"; content: string }
export interface AiAnswer { text: string; sql: string | null; model: string; note: string | null }
export const aiApi = {
  chat: (id: number, database: string | null, history: AiTurn[], request: string, settings: AiSettings) => invoke<AiAnswer>("ai_chat", { id, database, history, request, settings }),
  models: (preset: string, endpoint: string, all: boolean) => invoke<AiModel[]>("ai_models", { preset, endpoint, all }),
  hasKey: (preset: string) => invoke<boolean>("ai_key_status", { preset }),
  setKey: (preset: string, key: string) => invoke<void>("ai_set_key", { preset, key }),
};
/** The server's error text without the machine-readable code in front (`vault_locked: …`). */
export const plainError = (e: unknown) => describeError(e).replace(/^vault_[a-z_]+: /, "");
export const isVaultLocked = (e: unknown) => describeError(e).startsWith("vault_locked");

export const api = {
  listConnections: () => invoke<SavedConnection[]>("list_connections"),
  saveConnection: (connection: SavedConnection) => invoke<number>("save_connection", { connection }),
  deleteConnection: (id: number) => invoke<void>("delete_connection", { id }),
  /** `reuse`: when the connection is already open, keep its session (switching between open connections) instead of reconnecting. */
  connect: (id: number, trustFingerprint?: string, reuse?: boolean) => invoke<Node[]>("connect", { id, trustFingerprint: trustFingerprint ?? null, reuse: reuse ?? false }),
  testConnection: (connection: SavedConnection, trustFingerprint?: string) => invoke<TestResult>("test_connection", { connection, trustFingerprint: trustFingerprint ?? null }),
  forgetSshHost: (id: number) => invoke<void>("forget_ssh_host", { id }),
  openConnections: () => invoke<number[]>("open_connections"),
  disconnect: (id: number) => invoke<void>("disconnect", { id }),
  /** `scope` = the selected database: only its objects are searched. Without one, only database names are. */
  filterIndex: (id: number, query: string, scope: string | null, kinds: Kind[] | null) => invoke<number[]>("filter_index", { id, query, scope, kinds }),
  tableStats: (id: number, db: string) => invoke<TableStat[]>("db_table_stats", { id, db }),
  columnDetails: (id: number, db: string, table: string) => invoke<ColumnDetail[]>("table_column_details", { id, db, table }),
  indexDetails: (id: number, db: string, table: string) => invoke<IndexDetail[]>("table_index_details", { id, db, table }),
  objectDefinition: (id: number, kind: Kind, db: string, name: string) => invoke<string>("object_definition", { id, kind, db, name }),
  dropObject: (id: number, kind: Kind, db: string, name: string) => invoke<string>("drop_object", { id, kind, db, name }),
  tableOverview: (id: number, db: string, table: string) => invoke<TableOverview>("table_overview", { id, db, table }),
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
  /** `foreignKeyChecks`: false runs the script with foreign key checks off, true forces them on, null keeps the server's setting. */
  run: (id: number, runId: string, database: string | null, sql: string, foreignKeyChecks: boolean | null = null) =>
    invoke<StatementResult[]>("run_query", { id, runId, database, sql, foreignKeyChecks }),
  cancel: (runId: string) => invoke<void>("cancel_query", { runId }),
  /** Advisory review of a script before it runs (destructive statements, estimated rows). */
  review: (id: number, database: string | null, sql: string) => invoke<Review>("review_sql", { id, database, sql }),
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
/** A position in primary-key order (keyset paging): much cheaper than OFFSET on big tables. */
export type Cursor = { after: string[] } | { before: string[] } | "end";
export interface PageSpec { filters: ColumnFilter[]; where_sql: string | null; sort: Sort[]; limit: number; offset: number; cursor?: Cursor | null }
export interface Page {
  columns: ColumnInfo[]; rows: (string | null)[][]; long_cells: [number, number][]; has_more: boolean; elapsed_ms: number; sql: string;
  /** Rows exist before this page. */
  has_prev: boolean;
  /** Keyset navigation works for this table and request; `first_key` / `last_key` are the cursors for previous and next. */
  keyset: boolean; first_key: string[] | null; last_key: string[] | null;
}
export type CellValue = { kind: "null" } | { kind: "default" } | { kind: "text"; value: string } | { kind: "file"; value: string };
export interface Assign { column: string; value: CellValue }
export type Change =
  | { op: "update"; key: Assign[]; set: Assign[] }
  | { op: "insert"; values: Assign[] }
  | { op: "delete"; key: Assign[] };
export interface CellData { is_null: boolean; len: number; truncated: boolean; base64: string | null }
export interface LookupItem { value: string; label: string | null }
export type ExportFormat = "csv" | "json" | "json_lines" | "sql_insert" | "xml" | "html" | "xlsx";
export const EXPORT_FORMATS: { format: ExportFormat; label: string; ext: string }[] = [
  { format: "csv", label: "CSV", ext: "csv" }, { format: "json", label: "JSON", ext: "json" }, { format: "json_lines", label: "JSON Lines", ext: "jsonl" }, { format: "sql_insert", label: "SQL INSERT", ext: "sql" },
  { format: "xml", label: "XML", ext: "xml" }, { format: "html", label: "HTML table", ext: "html" }, { format: "xlsx", label: "Excel (XLSX)", ext: "xlsx" },
];

export const dataApi = {
  meta: (id: number, db: string, table: string) => invoke<TableMeta>("table_meta", { id, db, table }),
  page: (id: number, db: string, table: string, spec: PageSpec) => invoke<Page>("table_page", { id, db, table, spec }),
  /** Exact count. With a run id it can be cancelled through `queryApi.cancel`. */
  count: (id: number, db: string, table: string, filters: ColumnFilter[], whereSql: string | null, runId: string | null = null) =>
    invoke<number>("table_count", { id, db, table, filters, whereSql, runId }),
  /** A quick estimate (table statistics, or EXPLAIN when filtered); null when the server has none. */
  estimate: (id: number, db: string, table: string, filters: ColumnFilter[], whereSql: string | null) =>
    invoke<number | null>("table_count_estimate", { id, db, table, filters, whereSql }),
  preview: (id: number, db: string, table: string, changes: Change[]) => invoke<string[]>("preview_changes", { id, db, table, changes }),
  apply: (id: number, db: string, table: string, changes: Change[]) =>
    invoke<{ applied: number; insert_ids: number[] }>("apply_changes", { id, db, table, changes }),
  cell: (id: number, db: string, table: string, key: Assign[], column: string) =>
    invoke<CellData>("fetch_cell", { id, db, table, key, column }),
  saveCell: (id: number, db: string, table: string, key: Assign[], column: string, path: string) =>
    invoke<number>("save_cell_to_file", { id, db, table, key, column, path }),
  lookup: (id: number, refSchema: string, refTable: string, refColumn: string, search: string) =>
    invoke<LookupItem[]>("fk_lookup", { id, refSchema, refTable, refColumn, search }),
  /** Export as a job: progress arrives through `onJobProgress(jobId)`, `transferApi.cancel(jobId)` stops it (the promise then rejects with "cancelled"). */
  exportTable: (id: number, jobId: string, db: string, table: string, spec: PageSpec, format: ExportFormat, nullText: string, gzip: boolean, path: string) =>
    invoke<{ rows: number; warnings: string[] }>("export_table", { id, jobId, db, table, spec, options: { format, null_text: nullText, gzip }, path }),
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
  preview: (db: string, original: TableDef | null, desired: TableDef, sqlite?: boolean) =>
    invoke<{ statements: string[]; errors: string[] }>("ddl_preview", { db, original, desired, sqlite: sqlite ?? false }),
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

export interface TableHit { table: string; rows: number; columns: [string, number][] }
export interface SearchResult { hits: TableHit[]; tables_total: number; tables_scanned: number; cancelled: boolean; skipped: string[] }
export interface SearchRequest { needle: string; exact: boolean; all_columns: boolean; tables?: string[] | null }
export const searchApi = {
  run: (id: number, jobId: string, db: string, request: SearchRequest) => invoke<SearchResult>("search_data", { id, jobId, db, request }),
  /** Stops the search between tables and kills the statement that is running. */
  cancel: async (jobId: string) => { await invoke<void>("cancel_job", { jobId }); await invoke<void>("cancel_query", { runId: jobId }).catch(() => {}); },
};
export interface IndexFinding { table: string; kind: "duplicate" | "redundant" | "missing_foreign_key_index"; index: string; detail: string; sql: string }
export interface IndexAdvice { findings: IndexFinding[]; unused: [string, string][]; unused_note: string; tables: number }
export const adviceApi = { indexes: (id: number, db: string) => invoke<IndexAdvice>("index_advice", { id, db }) };
/** The visual query builder's spec (mirrors `myq_schema::builder::QuerySpec`). */
export interface BuilderTable { db: string; table: string; alias: string }
export type BuilderOp = "eq" | "ne" | "lt" | "le" | "gt" | "ge" | "contains" | "starts_with" | "ends_with" | "in" | "between" | "is_null" | "not_null";
export interface QuerySpec {
  from: BuilderTable;
  joins: { kind: "inner" | "left"; table: BuilderTable; on: [{ alias: string; column: string }, string][] }[];
  select: { alias: string; column: string; name?: string | null; agg?: "count" | "sum" | "avg" | "min" | "max" | null }[];
  distinct: boolean;
  filters: { alias: string; column: string; op: BuilderOp; values: string[]; numeric: boolean }[];
  order: { alias: string; column: string; desc: boolean }[];
  limit: number | null;
}
export const builderApi = { build: (id: number, spec: QuerySpec) => invoke<string>("build_query", { id, spec }) };

export interface ErColumn { name: string; column_type: string; primary: boolean; nullable: boolean }
export interface ErTable { name: string; columns: ErColumn[] }
export interface ErRelation { name: string; table: string; columns: string[]; ref_schema: string; ref_table: string; ref_columns: string[]; on_delete: string; on_update: string }
export interface ErModel { tables: ErTable[]; relations: ErRelation[] }
export const erApi = { model: (id: number, db: string) => invoke<ErModel>("er_model", { id, db }) };

export type ChangeKind = "added" | "changed" | "removed";
export interface TableChange { table: string; kind: ChangeKind; statements: string[]; deferred: string[]; destructive: boolean }
export interface SchemaCompare { changes: TableChange[]; identical: number; warnings: string[] }
export const compareApi = {
  schemas: (source_id: number, source_db: string, target_id: number, target_db: string) => invoke<SchemaCompare>("schema_compare", { sourceId: source_id, sourceDb: source_db, targetId: target_id, targetDb: target_db }),
};

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
export type SuggestionKind = "column" | "table" | "view" | "database" | "alias" | "function" | "keyword" | "join" | "related";
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
export interface TableInfo {
  is_view: boolean; engine: string | null; collation: string | null; charset: string | null; auto_increment: number | null; comment: string; partitioned: boolean;
  row_format: string | null; rows: number | null; avg_row_length: number | null; data_length: number | null; index_length: number | null; data_free: number | null;
  created: string | null; updated: string | null;
}
export interface ColumnFull { name: string; ordinal: number; column_type: string; nullable: boolean; key: string; default: string | null; extra: string; comment: string }
export interface ConstraintInfo { name: string; kind: string; columns: string[] }
/** `schema`/`table`/`columns` are the referencing side; `ref_*` the referenced side. */
export interface ForeignKeyInfo { name: string; schema: string; table: string; columns: string[]; ref_schema: string; ref_table: string; ref_columns: string[]; on_update: string; on_delete: string }
export interface TriggerInfo { name: string; timing: string; event: string; statement: string }
export interface PartitionInfo { name: string; method: string | null; expression: string | null; rows: number | null; data_length: number | null }
export interface TableOverview {
  info: TableInfo; columns: ColumnFull[]; indexes: IndexDetail[]; constraints: ConstraintInfo[]; outgoing: ForeignKeyInfo[]; incoming: ForeignKeyInfo[];
  triggers: TriggerInfo[]; partitions: PartitionInfo[]; ddl: string | null;
}
export interface RiskFinding { kind: string; severity: "warning" | "danger"; reason: string }
export interface ReviewItem { index: number; sql: string; findings: RiskFinding[]; estimated_rows: number | null }
export interface Review { items: ReviewItem[]; environment: Environment; read_only: boolean; confirm: boolean }
export interface IndexDetail { name: string; unique: boolean; kind: string; columns: string[] }
export interface ColumnDetail { name: string; column_type: string; nullable: boolean; key: string; extra: string }

export interface ProfileResult {
  elapsed_ms: number; rows_returned: number; stages: { name: string; ms: number }[]; stages_note: string | null;
  counters: { name: string; value: number }[];
}

export interface SqlDiagnostic { from: number; to: number; severity: "error" | "warning"; message: string }

/** The audit log: a file with one line per statement that changed something (off until turned on). */
export interface AuditStatus { enabled: boolean; path: string; size: number }
export interface AuditEntry { time: string; connection: string; who: string; database: string | null; ok: boolean; ms: number; sql: string }
export const auditApi = {
  status: () => invoke<AuditStatus>("audit_status"),
  set: (enabled: boolean) => invoke<AuditStatus>("audit_set", { enabled }),
  tail: (limit: number) => invoke<AuditEntry[]>("audit_tail", { limit }),
};

/** Data compare and sync, and copying a database between connections (MySQL and MariaDB). */
export interface DataCompareRequest {
  source_id: number; source_db: string; source_table: string; target_id: number; target_db: string; target_table: string;
  key: string[]; where_sql: string | null; delete_extra: boolean;
}
export type Cells = (string | null)[];
export interface DataCompareResult {
  columns: string[]; key: string[]; same: number; only_source_count: number; only_target_count: number; changed_count: number;
  only_source: Cells[]; only_target: Cells[]; changed: { key: Cells; diffs: { column: string; source: string | null; target: string | null }[] }[];
  statements_total: number; statements: string[]; skipped: string[];
}
export interface SyncApplied { statements: number; inserted: number; updated: number; deleted: number }
export interface CopySummary { tables: number; rows: number; statements: number; warnings: string[]; elapsed_ms: number }
export const syncApi = {
  compare: (request: DataCompareRequest) => invoke<DataCompareResult>("data_compare", { request }),
  apply: (request: DataCompareRequest, disableFkChecks: boolean) => invoke<SyncApplied>("data_sync_apply", { request, disableFkChecks }),
  copyDatabase: (jobId: string, sourceId: number, sourceDb: string, targetId: number, targetDb: string, withData: boolean) =>
    invoke<CopySummary>("copy_database", { jobId, sourceId, sourceDb, targetId, targetDb, withData }),
};

export interface Usage { kind: string; db: string; name: string; detail: string }
export const usagesApi = {
  find: (id: number, db: string, table: string, column: string | null) => invoke<Usage[]>("find_usages", { id, db, table, column }),
  /** The statements that point the views mentioning a table at its new name (run after the rename). */
  renameViews: (id: number, db: string, from: string, to: string) => invoke<string[]>("rename_view_statements", { id, db, from, to }),
};

export const sqliteApi = {
  maintain: (id: number, op: "integrity" | "quick" | "foreign_keys" | "vacuum" | "analyze" | "reindex" | "optimize") => invoke<string>("sqlite_maintain", { id, op }),
};

/** Scheduled backups: jobs that run while MyLoDB is open, with a log and a "keep the last N" limit. */
export type ScheduleRule = { kind: "hours"; hours: number } | { kind: "daily"; hour: number; minute: number } | { kind: "weekly"; weekday: number; hour: number; minute: number };
export interface ScheduleLog { time: string; ok: boolean; message: string }
export interface Schedule {
  id: string; name: string; connection_id: number; databases: string[]; kind: "backup" | "export"; sql: string; format: string; dir: string; rule: ScheduleRule; keep: number; compress: boolean; data: boolean; enabled: boolean;
  created: string; last_run: string | null; log: ScheduleLog[]; next_run?: string | null; running?: boolean;
}
export const scheduleApi = {
  list: () => invoke<Schedule[]>("schedules_list"),
  save: (job: Schedule) => invoke<Schedule[]>("schedule_save", { job }),
  remove: (id: string) => invoke<Schedule[]>("schedule_delete", { id }),
  runNow: (id: string) => invoke<string>("schedule_run_now", { id }),
};
export async function onScheduleDone(cb: (e: { name: string; ok: boolean; message: string }) => void): Promise<() => void> {
  const un = await listen<{ name: string; ok: boolean; message: string }>("schedule-done", (e) => cb(e.payload));
  return () => { un(); };
}

export const editorApi = {
  diagnose: (id: number, sql: string, database: string | null) => invoke<SqlDiagnostic[]>("diagnose_sql", { id, sql, database }),
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
export interface TopStatement { db: string; digest_text: string; calls: number; total_ms: number; avg_ms: number; max_ms: number; rows_examined: number; rows_sent: number; no_index: number; last_seen: string }
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
  topStatements: (id: number, sort: string, limit: number, search: string) => invoke<TopStatement[]>("top_statements", { id, sort, limit, search }),
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
  /** Load with foreign_key_checks = 0 (restored afterwards). */
  disable_fk_checks?: boolean;
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

export interface Cap { available: boolean; reason: string | null }
/** What the connected server can do; a feature it lacks comes with the reason. */
export interface Capabilities {
  flavor: "mysql" | "mariadb" | "sqlite";
  schemas: Cap; routines: Cap; triggers: Cap; events: Cap; users: Cap; replication: Cap; explain_analyze: Cap; read_only_session: Cap; row_estimates: Cap;
  server_tools: Cap; dump_restore: Cap; /** Export to a file; older builds of the backend do not send it. */ data_export?: Cap; writes: Cap;
}
export const capabilitiesApi = { get: (id: number) => invoke<Capabilities>("capabilities", { id }) };

/** Open one of the author's links in the browser (the backend refuses any other address). */
export const openUrl = (url: string) => invoke<void>("open_url", { url });

export const fileApi = {
  read: (path: string) => invoke<string>("read_text_file", { path }),
  write: (path: string, content: string) => invoke<void>("write_text_file", { path, content }),
};

/** The native menu bar tells the window which item was chosen (the Rust side only knows the id). */
export async function onMenuAction(cb: (id: string) => void): Promise<() => void> {
  const un = await listen<string>("menu-action", (e) => cb(e.payload));
  return () => { un(); };
}

export const transferApi = {
  sniff: (path: string) => invoke<string>("csv_sniff", { path }),
  /** A JSON / JSON Lines file as a temporary CSV (NULL is `null_text`) for the CSV import to continue with. */
  jsonPrepare: (path: string) => invoke<{ csv_path: string; rows: number; columns: string[]; null_text: string }>("json_prepare", { path }),
  preview: (path: string, format: CsvFormat, limit = 20) => invoke<CsvPreview>("csv_preview", { path, format, limit }),
  infer: (path: string, format: CsvFormat) => invoke<InferredColumn[]>("csv_infer", { path, format }),
  createTable: (id: number, db: string, table: string, columns: InferredColumn[]) => invoke<Node[]>("csv_create_table", { id, db, table, columns }),
  /** Counts and samples the statements of a script without running it. */
  previewSource: (path: string) => invoke<SourcePreview>("preview_sql_source", { path }),
  importCsv: (id: number, jobId: string, path: string, format: CsvFormat, plan: ImportPlan) => invoke<ImportResult>("import_csv_job", { id, jobId, path, format, plan }),
  runSource: (id: number, jobId: string, path: string, options: RunOptions) => invoke<RunResult>("run_sql_source_job", { id, jobId, path, options }),
  backup: (id: number, jobId: string, options: BackupOptions, dir: string, name: string) => invoke<BackupResult>("backup_job", { id, jobId, options, dir, name }),
  cancel: (jobId: string) => invoke<void>("cancel_job", { jobId }),
  exportQuery: (id: number, jobId: string, database: string | null, sql: string, format: ExportFormat, nullText: string, gzip: boolean, path: string) =>
    invoke<{ rows: number; warnings: string[] }>("export_query", { id, jobId, database, sql, options: { format, null_text: nullText, gzip }, path }),
};

// ---- plans: the SQL of a bulk or destructive operation, shown before it runs ----
export type PlanOp =
  | { op: "drop_databases"; names: string[] }
  | { op: "drop_tables"; db: string; objects: [string, boolean][] }
  | { op: "empty_tables"; db: string; objects: [string, boolean][] }
  | { op: "copy_tables"; db: string; copies: [string, string][]; with_data: boolean }
  | { op: "add_prefix"; db: string; tables: string[]; prefix: string }
  | { op: "replace_prefix"; db: string; tables: string[]; old: string; new: string }
  | { op: "copy_with_prefix"; db: string; tables: string[]; prefix: string; with_data: boolean }
  | { op: "checksum"; db: string; tables: string[] }
  | { op: "alter_database"; name: string; charset: string | null; collation: string | null }
  | { op: "copy_database"; src: string; dst: string; with_data: boolean; collation: string | null }
  | { op: "rename_database"; old: string; new: string; collation: string | null }
  | { op: "create_database"; name: string; collation: string | null }
  | { op: "drop_object"; db: string; kind: Kind; name: string }
  | { op: "rename_table"; db: string; from: string; to: string }
  | { op: "rename_routine"; db: string; kind: Kind; from: string; to: string; definition: string };

export const planApi = {
  /** The statements for an operation (nothing runs). Rejects with the reason when it cannot be done. */
  build: (id: number, op: PlanOp) => invoke<string[]>("build_plan", { id, op }),
  /** Run statements exactly as shown, stopping at the first error. */
  run: (id: number, statements: string[]) => invoke<StatementResult[]>("run_plan", { id, statements }),
};

// ---- admin pages ----
export interface DatabaseRow { name: string; collation: string; charset: string; system: boolean }
export interface TableRow { name: string; is_view: boolean; engine: string | null; rows: number | null; collation: string | null; bytes: number | null; overhead: number | null; comment: string }
export interface ServerHome {
  info: { version: string; version_comment: string; hostname: string; port: number; datadir: string; charset: string; collation: string; sql_mode: string; time_zone: string; max_connections: number; uptime_seconds: number; ssl_cipher: string | null };
  account: string; protocol: number | null; connections: number | null; connection_collation: string; databases: number;
}
export const adminApi = {
  databases: (id: number) => invoke<DatabaseRow[]>("database_list", { id }),
  /** [name, tables, bytes] per database. Slower than `databases`. */
  sizes: (id: number) => invoke<[string, number, number][]>("database_sizes", { id }),
  tables: (id: number, db: string) => invoke<TableRow[]>("database_tables", { id, db }),
  home: (id: number) => invoke<ServerHome>("server_home", { id }),
  catalog: (id: number) => invoke<ServerCatalog>("server_catalog", { id }),
};

export interface SourcePreview {
  files: number; statements: number; bytes: number; kinds: [string, number][]; destructive: number;
  first: { file: string; line: number; text: string }[]; errors: string[];
}
export interface Grid { columns: string[]; rows: (string | null)[][] }
export interface ServerCatalog { engines: Grid; charsets: Grid; replication: Grid; replication_note: string | null }
