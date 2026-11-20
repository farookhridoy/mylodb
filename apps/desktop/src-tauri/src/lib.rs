//! Thin Tauri bridge. All logic lives in the `myq-*` crates; this only exposes it as commands.

use myq_data::{Assign, Change, CellData, ColumnFilter, ExportOptions, LookupItem, Page, PageSpec, TableMeta, ApplyOutcome};
use myq_admin::{Account, AccountEdit, Grants, ScopeGrant, Sql};
use myq_schema::ddl::TableDef;
use myq_driver::{execute_script, introspect, introspect_columns, kill_query, pool, IntrospectOptions, Pool, StatementResult};
use myq_schema::filter::{Filter, Node};
use myq_store::{Favorite, HistoryEntry, Keychain, SavedConnection, Store};
use std::{collections::HashMap, sync::Mutex};
use tauri::{Emitter, Manager};

/// A live connection: its pool and the cached name index the filter runs over.
struct Session {
    #[allow(dead_code)]
    pool: Pool,
    index: Vec<Node>,
    /// Index positions per database, so a filter scoped to one database never walks the whole server.
    by_db: HashMap<String, Vec<usize>>,
    /// Positions of the database nodes themselves (for filtering the database list by name).
    db_positions: Vec<usize>,
    /// Databases whose columns are in the index (they are loaded on demand, not at connect).
    columns_loaded: std::collections::HashSet<String>,
    /// Autocomplete's view of the index, rebuilt whenever the index is.
    catalog: myq_sql::Catalog,
    /// Keeps the SSH tunnel alive for as long as the session lives (dropping it closes the tunnel).
    #[allow(dead_code)]
    tunnel: Option<myq_tunnel::Tunnel>,
}

impl Session {
    fn new(pool: Pool, index: Vec<Node>, tunnel: Option<myq_tunnel::Tunnel>) -> Session {
        let mut s = Session { pool, index: Vec::new(), by_db: HashMap::new(), db_positions: Vec::new(), columns_loaded: Default::default(), catalog: myq_sql::Catalog::default(), tunnel };
        s.set_index(index);
        s
    }

    /// Replace the index and rebuild everything derived from it.
    fn set_index(&mut self, index: Vec<Node>) {
        self.by_db.clear();
        self.db_positions.clear();
        for (i, n) in index.iter().enumerate() {
            self.by_db.entry(n.database.clone()).or_default().push(i);
            if n.kind == myq_schema::filter::Kind::Database {
                self.db_positions.push(i);
            }
        }
        self.catalog = myq_sql::Catalog::from_nodes(&index);
        self.index = index;
    }

    /// Add the columns of `db`; returns where in the index they start. A database already loaded is left alone.
    fn add_columns(&mut self, db: &str, cols: Vec<Node>) -> usize {
        let start = self.index.len();
        if self.columns_loaded.insert(db.to_string()) {
            let mut index = std::mem::take(&mut self.index);
            index.extend(cols);
            self.set_index(index);
        }
        start
    }
}

struct AppState {
    store: Mutex<Store>,
    sessions: Mutex<HashMap<i64, Session>>,
    /// Cancel flags of the running backup / restore / import jobs, by job id.
    jobs: Mutex<HashMap<String, std::sync::Arc<std::sync::atomic::AtomicBool>>>,
    /// run_id -> (connection id, server-side connection id) of queries currently executing.
    running: Mutex<HashMap<String, (i64, u32)>>,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
fn list_connections(state: tauri::State<AppState>) -> Result<Vec<SavedConnection>, String> {
    state.store.lock().unwrap().list().map_err(err)
}

#[tauri::command]
fn save_connection(state: tauri::State<AppState>, connection: SavedConnection) -> Result<i64, String> {
    state.store.lock().unwrap().save(&connection).map_err(err)
}

#[tauri::command]
fn delete_connection(state: tauri::State<AppState>, id: i64) -> Result<(), String> {
    state.sessions.lock().unwrap().remove(&id);
    state.store.lock().unwrap().delete(id).map_err(err)
}

/// Why a connection could not be made. The SSH host-key cases carry what the UI needs to ask the user.
#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ConnectError {
    UnknownHost { host: String, fingerprint: String },
    HostKeyChanged { host: String, expected: String, actual: String },
    Failed { message: String },
}

impl From<String> for ConnectError {
    fn from(message: String) -> Self {
        ConnectError::Failed { message }
    }
}

impl From<myq_tunnel::TunnelError> for ConnectError {
    fn from(e: myq_tunnel::TunnelError) -> Self {
        match e {
            myq_tunnel::TunnelError::UnknownHost { host, fingerprint } => ConnectError::UnknownHost { host, fingerprint },
            myq_tunnel::TunnelError::HostKeyChanged { host, expected, actual } => ConnectError::HostKeyChanged { host, expected, actual },
            other => ConnectError::Failed { message: other.to_string() },
        }
    }
}

/// Open the SSH tunnel for a connection, when it has one, and return the settings to connect with
/// (pointing at the local end of the tunnel). A host key that was just confirmed is remembered
/// only after authentication succeeded.
async fn establish(
    store: &Mutex<Store>,
    mut cfg: myq_driver::ConnConfig,
    ssh: Option<(myq_tunnel::SshConfig, myq_tunnel::SshSecrets)>,
    trust_fingerprint: Option<String>,
) -> Result<(myq_driver::ConnConfig, Option<myq_tunnel::Tunnel>), ConnectError> {
    let Some((ssh, secrets)) = ssh else { return Ok((cfg, None)) };
    let known = store.lock().unwrap().ssh_known(&ssh.host, ssh.port).map_err(err)?;
    let had_known = known.is_some();
    let trust = myq_tunnel::HostTrust { known, accept_new: trust_fingerprint };
    let tunnel = myq_tunnel::open(&ssh, &secrets, trust, &cfg.host, cfg.port).await?;
    if !had_known {
        store.lock().unwrap().ssh_trust(&ssh.host, ssh.port, &tunnel.fingerprint).map_err(err)?;
    }
    cfg.host = "127.0.0.1".into();
    cfg.port = tunnel.local_port;
    Ok((cfg, Some(tunnel)))
}

/// Connect, build the name index, and return it to the UI.
#[tauri::command]
async fn connect(state: tauri::State<'_, AppState>, id: i64, trust_fingerprint: Option<String>) -> Result<Vec<Node>, ConnectError> {
    let (cfg, ssh) = {
        let store = state.store.lock().unwrap();
        let cfg = store.config_for(id).map_err(err)?.ok_or_else(|| "connection not found".to_string())?;
        (cfg, store.ssh_for(id).map_err(err)?)
    };
    let (cfg, tunnel) = establish(&state.store, cfg, ssh, trust_fingerprint).await?;
    let p = pool(&cfg);
    // names only: columns are loaded per database when needed, so connecting to a huge server stays fast
    let index = introspect(&p, &IntrospectOptions::default()).await.map_err(err)?;
    state.sessions.lock().unwrap().insert(id, Session::new(p, index.clone(), tunnel));
    Ok(index)
}

#[derive(serde::Serialize)]
struct TestResult {
    version: String,
    ssl_cipher: Option<String>,
    via_ssh: bool,
}

/// Try a connection exactly as entered in the dialog, without saving it. A blank password or
/// passphrase falls back to the saved one when editing an existing connection.
#[tauri::command]
async fn test_connection(state: tauri::State<'_, AppState>, connection: SavedConnection, trust_fingerprint: Option<String>) -> Result<TestResult, ConnectError> {
    use myq_driver::mysql_async::prelude::Queryable;
    let mut cfg = connection.config.clone();
    let mut ssh = None;
    {
        let store = state.store.lock().unwrap();
        let stored = |slot| connection.id.map(|id| store.secret(id, slot)).transpose().map(Option::flatten);
        cfg.password = connection.new_password.clone().filter(|p| !p.is_empty()).or(stored(myq_store::Slot::DbPassword).map_err(err)?);
        if let Some(s) = &connection.ssh {
            let secrets = myq_tunnel::SshSecrets {
                password: connection.new_ssh_password.clone().filter(|p| !p.is_empty()).or(stored(myq_store::Slot::SshPassword).map_err(err)?),
                key_passphrase: connection.new_ssh_passphrase.clone().filter(|p| !p.is_empty()).or(stored(myq_store::Slot::SshPassphrase).map_err(err)?),
            };
            ssh = Some((s.clone(), secrets));
        }
    }
    let via_ssh = ssh.is_some();
    let (cfg, _tunnel) = establish(&state.store, cfg, ssh, trust_fingerprint).await?;
    let mut conn = myq_driver::Conn::new(cfg.to_opts()).await.map_err(err)?;
    let version: Option<String> = conn.query_first("SELECT VERSION()").await.map_err(err)?;
    let cipher: Option<(String, String)> = conn.query_first("SHOW SESSION STATUS LIKE 'Ssl_cipher'").await.map_err(err)?;
    let _ = conn.disconnect().await;
    Ok(TestResult { version: version.unwrap_or_default(), ssl_cipher: cipher.map(|c| c.1).filter(|c| !c.is_empty()), via_ssh })
}

/// Forget the saved host key of a connection's SSH server (after the server was legitimately re-keyed).
#[tauri::command]
fn forget_ssh_host(state: tauri::State<AppState>, id: i64) -> Result<(), String> {
    let store = state.store.lock().unwrap();
    if let Some((ssh, _)) = store.ssh_for(id).map_err(err)? {
        store.ssh_forget(&ssh.host, ssh.port).map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
async fn disconnect(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    let session = state.sessions.lock().unwrap().remove(&id);
    if let Some(s) = session {
        s.pool.disconnect().await.map_err(err)?;
    }
    Ok(())
}

/// Indices (into the session's index) that match the filter query.
///
/// With a `scope` (the selected database) only that database's objects are searched, like "Filter tables in
/// sakila". Without one the query matches database names only, so typing never walks every table of the server.
#[tauri::command]
fn filter_index(state: tauri::State<AppState>, id: i64, query: String, scope: Option<String>) -> Result<Vec<usize>, String> {
    let filter = Filter::parse(&query).map_err(|e| format!("{e:?}"))?;
    let sessions = state.sessions.lock().unwrap();
    let s = sessions.get(&id).ok_or_else(|| "not connected".to_string())?;
    Ok(match scope {
        Some(db) => s.by_db.get(&db).map_or_else(Vec::new, |positions| filter.apply_positions(&s.index, positions)),
        None => filter.apply_positions(&s.index, &s.db_positions),
    })
}

/// Columns of `db`, loaded on first use. Returns the position they were appended at and the nodes, so the UI
/// can place them at the same positions as the backend (filter results are index positions).
#[derive(serde::Serialize)]
struct ColumnsLoaded {
    start: usize,
    nodes: Vec<Node>,
}

#[tauri::command]
async fn load_columns(state: tauri::State<'_, AppState>, id: i64, db: String) -> Result<ColumnsLoaded, String> {
    ensure_columns(&state, id, &db).await.map(|(start, nodes)| ColumnsLoaded { start, nodes })
}

/// Load a database's columns unless they are already there. Returns (start, new nodes); empty when nothing was added.
async fn ensure_columns(state: &tauri::State<'_, AppState>, id: i64, db: &str) -> Result<(usize, Vec<Node>), String> {
    let pool = {
        let sessions = state.sessions.lock().unwrap();
        let s = sessions.get(&id).ok_or("not connected")?;
        if s.columns_loaded.contains(db) || !s.by_db.contains_key(db) {
            return Ok((s.index.len(), vec![]));
        }
        s.pool.clone()
    };
    let cols = introspect_columns(&pool, db).await.map_err(err)?;
    let mut sessions = state.sessions.lock().unwrap();
    let s = sessions.get_mut(&id).ok_or("not connected")?;
    if s.columns_loaded.contains(db) {
        return Ok((s.index.len(), vec![])); // another request got there first
    }
    let start = s.add_columns(db, cols.clone());
    Ok((start, cols))
}

fn session_pool(state: &tauri::State<'_, AppState>, id: i64) -> Result<Pool, String> {
    state.sessions.lock().unwrap().get(&id).map(|s| s.pool.clone()).ok_or_else(|| "not connected".to_string())
}

/// Sizes and row estimates of a database's tables (one query), for the object browser.
#[tauri::command]
async fn db_table_stats(state: tauri::State<'_, AppState>, id: i64, db: String) -> Result<Vec<myq_driver::TableStat>, String> {
    myq_driver::table_stats(&session_pool(&state, id)?, &db).await.map_err(err)
}

/// Declared types, keys and nullability of one table's columns, for the object browser.
#[tauri::command]
async fn table_column_details(state: tauri::State<'_, AppState>, id: i64, db: String, table: String) -> Result<Vec<myq_driver::ColumnDetail>, String> {
    myq_driver::column_details(&session_pool(&state, id)?, &db, &table).await.map_err(err)
}

/// Execute a script on a dedicated server connection. `run_id` lets the UI cancel it.
#[tauri::command]
async fn run_query(
    state: tauri::State<'_, AppState>,
    id: i64,
    run_id: String,
    database: Option<String>,
    sql: String,
) -> Result<Vec<StatementResult>, String> {
    let pool = state.sessions.lock().unwrap().get(&id).map(|s| s.pool.clone()).ok_or("not connected")?;
    let mut conn = pool.get_conn().await.map_err(err)?;
    state.running.lock().unwrap().insert(run_id.clone(), (id, conn.id()));
    let started = std::time::Instant::now();
    let results = execute_script(&mut conn, database.as_deref(), &sql).await;
    state.running.lock().unwrap().remove(&run_id);
    let ok = results.iter().all(|r| r.error.is_none());
    // History is best-effort: a logging failure must not hide the query result.
    let _ = state.store.lock().unwrap().add_history(id, sql.trim(), started.elapsed().as_millis() as u64, ok);
    Ok(results)
}

#[tauri::command]
async fn cancel_query(state: tauri::State<'_, AppState>, run_id: String) -> Result<(), String> {
    let target = state.running.lock().unwrap().get(&run_id).copied();
    let Some((id, server_conn)) = target else { return Ok(()) };
    let pool = state.sessions.lock().unwrap().get(&id).map(|s| s.pool.clone()).ok_or("not connected")?;
    kill_query(&pool, server_conn).await.map_err(err)
}

/// Databases visible on the connection, for the editor's database selector.
#[tauri::command]
fn list_databases(state: tauri::State<AppState>, id: i64) -> Result<Vec<String>, String> {
    let sessions = state.sessions.lock().unwrap();
    let s = sessions.get(&id).ok_or("not connected")?;
    Ok(s.index.iter().filter(|n| n.kind == myq_schema::filter::Kind::Database).map(|n| n.name.clone()).collect())
}

// ---- table data grid ----

/// A dedicated server connection from the session's pool.
async fn conn_for(state: &tauri::State<'_, AppState>, id: i64) -> Result<myq_driver::Conn, String> {
    let pool = state.sessions.lock().unwrap().get(&id).map(|s| s.pool.clone()).ok_or("not connected")?;
    pool.get_conn().await.map_err(err)
}

#[tauri::command]
async fn table_meta(state: tauri::State<'_, AppState>, id: i64, db: String, table: String) -> Result<TableMeta, String> {
    let mut c = conn_for(&state, id).await?;
    myq_data::table_meta(&mut c, &db, &table).await.map_err(err)
}

#[tauri::command]
async fn table_page(state: tauri::State<'_, AppState>, id: i64, db: String, table: String, spec: PageSpec) -> Result<Page, String> {
    let mut c = conn_for(&state, id).await?;
    let meta = myq_data::table_meta(&mut c, &db, &table).await.map_err(err)?;
    myq_data::fetch_page(&mut c, &meta, &spec).await.map_err(err)
}

#[tauri::command]
async fn table_count(state: tauri::State<'_, AppState>, id: i64, db: String, table: String, filters: Vec<ColumnFilter>, where_sql: Option<String>) -> Result<u64, String> {
    let mut c = conn_for(&state, id).await?;
    let meta = myq_data::table_meta(&mut c, &db, &table).await.map_err(err)?;
    myq_data::count_rows(&mut c, &meta, &filters, where_sql.as_deref()).await.map_err(err)
}

#[tauri::command]
async fn preview_changes(state: tauri::State<'_, AppState>, id: i64, db: String, table: String, changes: Vec<Change>) -> Result<Vec<String>, String> {
    let mut c = conn_for(&state, id).await?;
    let meta = myq_data::table_meta(&mut c, &db, &table).await.map_err(err)?;
    myq_data::preview_changes(&meta, &changes).map_err(err)
}

#[tauri::command]
async fn apply_changes(state: tauri::State<'_, AppState>, id: i64, db: String, table: String, changes: Vec<Change>) -> Result<ApplyOutcome, String> {
    let mut c = conn_for(&state, id).await?;
    let meta = myq_data::table_meta(&mut c, &db, &table).await.map_err(err)?;
    let outcome = myq_data::apply_changes(&mut c, &meta, &changes).await.map_err(err)?;
    // Edits are part of the audit trail: log what ran, best-effort.
    if let Ok(sqls) = myq_data::preview_changes(&meta, &changes) {
        let _ = state.store.lock().unwrap().add_history(id, &sqls.join(";\n"), 0, true);
    }
    Ok(outcome)
}

#[tauri::command]
async fn fetch_cell(state: tauri::State<'_, AppState>, id: i64, db: String, table: String, key: Vec<Assign>, column: String) -> Result<CellData, String> {
    let mut c = conn_for(&state, id).await?;
    let meta = myq_data::table_meta(&mut c, &db, &table).await.map_err(err)?;
    myq_data::fetch_cell(&mut c, &meta, &key, &column).await.map_err(err)
}

/// Write one cell's raw bytes to a file chosen by the user.
#[tauri::command]
async fn save_cell_to_file(state: tauri::State<'_, AppState>, id: i64, db: String, table: String, key: Vec<Assign>, column: String, path: String) -> Result<usize, String> {
    use base64::Engine;
    let mut c = conn_for(&state, id).await?;
    let meta = myq_data::table_meta(&mut c, &db, &table).await.map_err(err)?;
    let cell = myq_data::fetch_cell(&mut c, &meta, &key, &column).await.map_err(err)?;
    if cell.truncated {
        return Err(format!("value is {} bytes, larger than the {} byte transfer limit", cell.len, myq_data::FETCH_CELL_MAX));
    }
    let bytes = base64::engine::general_purpose::STANDARD.decode(cell.base64.unwrap_or_default()).map_err(err)?;
    std::fs::write(&path, &bytes).map_err(err)?;
    Ok(bytes.len())
}

#[tauri::command]
async fn fk_lookup(state: tauri::State<'_, AppState>, id: i64, ref_schema: String, ref_table: String, ref_column: String, search: String) -> Result<Vec<LookupItem>, String> {
    let mut c = conn_for(&state, id).await?;
    let meta = myq_data::table_meta(&mut c, &ref_schema, &ref_table).await.map_err(err)?;
    myq_data::lookup(&mut c, &meta, &ref_column, &search, 100).await.map_err(err)
}

#[derive(serde::Serialize)]
struct ExportSummary {
    rows: u64,
    warnings: Vec<String>,
}

/// Export every row matching `spec` to `path`. A partial file is removed on failure.
#[tauri::command]
async fn export_table(state: tauri::State<'_, AppState>, id: i64, db: String, table: String, spec: PageSpec, options: ExportOptions, path: String) -> Result<ExportSummary, String> {
    let mut c = conn_for(&state, id).await?;
    let meta = myq_data::table_meta(&mut c, &db, &table).await.map_err(err)?;
    let options = ExportOptions { name: options.name.clone().or_else(|| Some(table.clone())), ..options };
    match myq_data::export_rows(&mut c, &meta, &spec, &options, myq_data::Dest::File(std::path::Path::new(&path))).await {
        Ok(o) => Ok(ExportSummary { rows: o.rows, warnings: o.warnings }),
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            Err(err(e))
        }
    }
}

/// Export the result of one SELECT to a file.
#[tauri::command]
async fn export_query(state: tauri::State<'_, AppState>, id: i64, database: Option<String>, sql: String, options: ExportOptions, path: String) -> Result<ExportSummary, String> {
    let mut c = conn_for(&state, id).await?;
    match myq_data::export_query(&mut c, database.as_deref(), &sql, &options, myq_data::Dest::File(std::path::Path::new(&path))).await {
        Ok(o) => Ok(ExportSummary { rows: o.rows, warnings: o.warnings }),
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            Err(err(e))
        }
    }
}

// ---- backup, restore and import jobs ----

/// What the UI hears while a job runs. At most one event per 100 ms, plus the first and the last.
#[derive(serde::Serialize, Clone)]
struct JobEvent {
    job_id: String,
    #[serde(flatten)]
    progress: myq_transfer::Progress,
}

fn job_progress(app: tauri::AppHandle, job_id: String) -> impl FnMut(myq_transfer::Progress) + Send {
    let mut last: Option<std::time::Instant> = None;
    move |progress| {
        let finished = progress.total.is_some_and(|t| progress.done >= t);
        if finished || last.map_or(true, |t| t.elapsed().as_millis() >= 100) {
            last = Some(std::time::Instant::now());
            let _ = app.emit("job-progress", JobEvent { job_id: job_id.clone(), progress });
        }
    }
}

/// Register a cancel flag for the job and remove it when the guard drops.
struct JobGuard<'a> {
    state: &'a AppState,
    id: String,
    flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl<'a> JobGuard<'a> {
    fn start(state: &'a AppState, id: &str) -> JobGuard<'a> {
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        state.jobs.lock().unwrap().insert(id.to_string(), flag.clone());
        JobGuard { state, id: id.to_string(), flag }
    }
}

impl Drop for JobGuard<'_> {
    fn drop(&mut self) {
        self.state.jobs.lock().unwrap().remove(&self.id);
    }
}

#[tauri::command]
fn cancel_job(state: tauri::State<AppState>, job_id: String) {
    if let Some(flag) = state.jobs.lock().unwrap().get(&job_id) {
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

#[tauri::command]
fn csv_sniff(path: String) -> Result<char, String> {
    myq_transfer::sniff_delimiter(std::path::Path::new(&path)).map_err(err)
}

#[tauri::command]
fn csv_preview(path: String, format: myq_transfer::CsvFormat, limit: usize) -> Result<myq_transfer::CsvPreview, String> {
    myq_transfer::preview_csv(std::path::Path::new(&path), &format, limit.clamp(1, 500)).map_err(err)
}

#[tauri::command]
fn csv_infer(path: String, format: myq_transfer::CsvFormat) -> Result<Vec<myq_transfer::InferredColumn>, String> {
    myq_transfer::infer_schema(std::path::Path::new(&path), &format, 1000).map_err(err)
}

/// Create a table from inferred (and user-edited) columns, for "import into a new table".
#[tauri::command]
async fn csv_create_table(state: tauri::State<'_, AppState>, id: i64, db: String, table: String, columns: Vec<myq_transfer::InferredColumn>) -> Result<Vec<Node>, String> {
    use myq_schema::ddl::{create_table_sql, parse_column_type, ColumnDef, TableDef};
    let def = TableDef {
        name: table,
        columns: columns
            .iter()
            .map(|c| {
                let (data_type, params, unsigned, zerofill) = parse_column_type(&c.column_type);
                ColumnDef { name: c.name.clone(), data_type, params, unsigned, zerofill, nullable: true, ..Default::default() }
            })
            .collect(),
        ..Default::default()
    };
    let sql = create_table_sql(Some(&db), &def).map_err(err)?;
    let mut c = conn_for(&state, id).await?;
    myq_admin::run_statements(&mut c, std::slice::from_ref(&sql)).await.map_err(err)?;
    let _ = state.store.lock().unwrap().add_history(id, &sql, 0, true);
    reindex(&state, id).await
}

#[tauri::command]
async fn import_csv_job(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: i64,
    job_id: String,
    path: String,
    format: myq_transfer::CsvFormat,
    plan: myq_transfer::ImportPlan,
) -> Result<myq_transfer::ImportResult, String> {
    let mut c = conn_for(&state, id).await?;
    let guard = JobGuard::start(&state, &job_id);
    let mut progress = job_progress(app, job_id);
    let result = myq_transfer::import_csv(&mut c, std::path::Path::new(&path), &format, &plan, &mut progress, &guard.flag).await;
    let _ = state.store.lock().unwrap().add_history(id, &format!("IMPORT CSV {path} INTO {}.{}", plan.database, plan.table), 0, result.is_ok());
    result.map_err(err)
}

/// Run a `.sql` file, a `.zip` of them, or a folder of them. The schema may change, so the index is rebuilt after.
#[tauri::command]
async fn run_sql_source_job(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: i64,
    job_id: String,
    path: String,
    options: myq_transfer::RunOptions,
) -> Result<myq_transfer::RunResult, String> {
    let result = {
        let mut c = conn_for(&state, id).await?;
        let guard = JobGuard::start(&state, &job_id);
        let mut progress = job_progress(app, job_id);
        myq_transfer::run_source(&mut c, std::path::Path::new(&path), &options, &mut progress, &guard.flag).await
    };
    let _ = state.store.lock().unwrap().add_history(id, &format!("RESTORE {path}"), 0, result.is_ok());
    let _ = reindex(&state, id).await;
    result.map_err(err)
}

#[tauri::command]
async fn backup_job(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: i64,
    job_id: String,
    options: myq_transfer::BackupOptions,
    dir: String,
    name: String,
) -> Result<myq_transfer::BackupResult, String> {
    let mut c = conn_for(&state, id).await?;
    let guard = JobGuard::start(&state, &job_id);
    let mut progress = job_progress(app, job_id);
    let target = myq_transfer::BackupTarget { dir: dir.into(), name };
    let result = myq_transfer::backup(&mut c, &options, &target, &mut progress, &guard.flag).await;
    let _ = state.store.lock().unwrap().add_history(id, &format!("BACKUP {}", options.databases.iter().map(|d| d.name.as_str()).collect::<Vec<_>>().join(", ")), 0, result.is_ok());
    result.map_err(err)
}

// ---- server tools ----

#[tauri::command]
async fn server_info(state: tauri::State<'_, AppState>, id: i64) -> Result<myq_admin::ServerInfo, String> {
    let mut c = conn_for(&state, id).await?;
    myq_admin::server_info(&mut c).await.map_err(err)
}

#[tauri::command]
async fn process_list(state: tauri::State<'_, AppState>, id: i64) -> Result<myq_admin::ProcessList, String> {
    let mut c = conn_for(&state, id).await?;
    myq_admin::processlist(&mut c).await.map_err(err)
}

#[tauri::command]
async fn kill_process(state: tauri::State<'_, AppState>, id: i64, process_id: u64, kind: myq_admin::KillKind) -> Result<(), String> {
    let mut c = conn_for(&state, id).await?;
    myq_admin::kill(&mut c, process_id, kind).await.map_err(err)
}

#[tauri::command]
async fn server_variables(state: tauri::State<'_, AppState>, id: i64, scope: myq_admin::VarScope) -> Result<Vec<myq_admin::Variable>, String> {
    let mut c = conn_for(&state, id).await?;
    myq_admin::variables(&mut c, scope).await.map_err(err)
}

#[tauri::command]
async fn server_status(state: tauri::State<'_, AppState>, id: i64, scope: myq_admin::VarScope) -> Result<Vec<(String, String)>, String> {
    let mut c = conn_for(&state, id).await?;
    myq_admin::status(&mut c, scope).await.map_err(err)
}

/// Change a GLOBAL variable. SESSION changes are not offered: connections are pooled, so a session
/// setting would silently leak into unrelated queries. Returns the server's warnings.
#[tauri::command]
async fn set_global_variable(state: tauri::State<'_, AppState>, id: i64, name: String, value: String) -> Result<Vec<String>, String> {
    let mut c = conn_for(&state, id).await?;
    let result = myq_admin::set_variable(&mut c, myq_admin::VarScope::Global, &name, &value).await;
    let _ = state.store.lock().unwrap().add_history(id, &format!("SET GLOBAL {name} = {value}"), 0, result.is_ok());
    result.map_err(err)
}

#[tauri::command]
async fn maintain_tables(state: tauri::State<'_, AppState>, id: i64, db: String, tables: Vec<String>, operation: myq_admin::Maintenance) -> Result<Vec<myq_admin::MaintenanceRow>, String> {
    let mut c = conn_for(&state, id).await?;
    let result = myq_admin::maintain(&mut c, &db, &tables, operation).await;
    let _ = state.store.lock().unwrap().add_history(id, &format!("{operation:?} {db}: {}", tables.join(", ")), 0, result.is_ok());
    result.map_err(err)
}

#[tauri::command]
async fn log_settings(state: tauri::State<'_, AppState>, id: i64) -> Result<myq_admin::LogSettings, String> {
    let mut c = conn_for(&state, id).await?;
    myq_admin::log_settings(&mut c).await.map_err(err)
}

#[tauri::command]
async fn error_log(state: tauri::State<'_, AppState>, id: i64, limit: u32, levels: Vec<String>, search: String) -> Result<Vec<myq_admin::LogLine>, String> {
    let mut c = conn_for(&state, id).await?;
    myq_admin::error_log(&mut c, limit, &levels, &search).await.map_err(err)
}

#[tauri::command]
async fn slow_log(state: tauri::State<'_, AppState>, id: i64, limit: u32, search: String) -> Result<Vec<myq_admin::SlowQuery>, String> {
    let mut c = conn_for(&state, id).await?;
    myq_admin::slow_log(&mut c, limit, &search).await.map_err(err)
}

#[tauri::command]
async fn binary_logs(state: tauri::State<'_, AppState>, id: i64) -> Result<Vec<myq_admin::BinaryLog>, String> {
    let mut c = conn_for(&state, id).await?;
    myq_admin::binary_logs(&mut c).await.map_err(err)
}

#[tauri::command]
async fn binlog_events(state: tauri::State<'_, AppState>, id: i64, file: String, from_pos: u64, limit: u32) -> Result<Vec<myq_admin::BinlogEvent>, String> {
    let mut c = conn_for(&state, id).await?;
    myq_admin::binlog_events(&mut c, &file, from_pos, limit).await.map_err(err)
}

// ---- editor intelligence ----

/// Autocomplete candidates at `cursor` (a UTF-16 offset). A lookup in the session's catalog, cheap enough for every
/// keystroke. The columns of the current database, and of any database the statement names, are loaded the first time.
#[tauri::command]
async fn complete_sql(state: tauri::State<'_, AppState>, id: i64, sql: String, cursor: usize, database: Option<String>) -> Result<myq_sql::Completions, String> {
    let wanted: Vec<String> = {
        let sessions = state.sessions.lock().unwrap();
        let s = sessions.get(&id).ok_or("not connected")?;
        let mut w: Vec<String> = database.iter().cloned().collect();
        for word in sql.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$')) {
            if let Some(db) = s.by_db.keys().find(|d| d.eq_ignore_ascii_case(word)) {
                if !w.contains(db) {
                    w.push(db.clone());
                }
            }
        }
        w.into_iter().filter(|d| !s.columns_loaded.contains(d)).collect()
    };
    for db in wanted {
        let _ = ensure_columns(&state, id, &db).await;
    }
    let sessions = state.sessions.lock().unwrap();
    let s = sessions.get(&id).ok_or("not connected")?;
    Ok(myq_sql::complete(&sql, cursor, &s.catalog, database.as_deref(), 100))
}

#[tauri::command]
fn format_sql(sql: String, options: myq_sql::FormatOptions) -> Result<String, String> {
    myq_sql::format_sql(&sql, &options).map_err(err)
}

/// The statement around `cursor` (a UTF-16 offset), for "explain the statement I am in".
#[tauri::command]
fn statement_at(sql: String, cursor: usize) -> Option<String> {
    myq_sql::statement_at(&sql, myq_sql::utf16_to_byte(&sql, cursor)).map(|s| s.text)
}

#[tauri::command]
async fn explain_query(state: tauri::State<'_, AppState>, id: i64, database: Option<String>, sql: String, analyze: bool) -> Result<myq_profile::ExplainResult, String> {
    let mut c = conn_for(&state, id).await?;
    myq_profile::explain(&mut c, database.as_deref(), &sql, analyze).await.map_err(err)
}

#[tauri::command]
async fn profile_query(state: tauri::State<'_, AppState>, id: i64, database: Option<String>, sql: String) -> Result<myq_profile::ProfileResult, String> {
    let mut c = conn_for(&state, id).await?;
    myq_profile::profile(&mut c, database.as_deref(), &sql).await.map_err(err)
}

// ---- schema designer ----

#[derive(serde::Serialize)]
struct TableDefResult {
    def: TableDef,
    warnings: Vec<String>,
}

#[tauri::command]
async fn table_def(state: tauri::State<'_, AppState>, id: i64, db: String, table: String) -> Result<TableDefResult, String> {
    let mut c = conn_for(&state, id).await?;
    let (def, warnings) = myq_admin::read_table_def(&mut c, &db, &table).await.map_err(err)?;
    Ok(TableDefResult { def, warnings })
}

#[derive(serde::Serialize)]
struct DdlPreview {
    statements: Vec<String>,
    errors: Vec<String>,
}

/// The statements a design would run. Pure: needs no server, so the designer can call it on every edit.
fn ddl_statements(db: &str, original: &Option<TableDef>, desired: &TableDef) -> Result<Vec<String>, Vec<String>> {
    let result = match original {
        Some(o) => myq_schema::ddl::alter_table_sql(Some(db), o, desired),
        None => myq_schema::ddl::create_table_sql(Some(db), desired).map(|s| vec![s]),
    };
    result.map_err(|e| e.0)
}

#[tauri::command]
fn ddl_preview(db: String, original: Option<TableDef>, desired: TableDef) -> DdlPreview {
    match ddl_statements(&db, &original, &desired) {
        Ok(statements) => DdlPreview { statements, errors: vec![] },
        Err(errors) => DdlPreview { statements: vec![], errors },
    }
}

/// Re-read the object index after the schema changed, so the tree and the filter see it. Databases whose columns
/// were loaded get them again, so what the UI already shows stays in step.
async fn reindex(state: &tauri::State<'_, AppState>, id: i64) -> Result<Vec<Node>, String> {
    let (pool, loaded) = {
        let sessions = state.sessions.lock().unwrap();
        let s = sessions.get(&id).ok_or("not connected")?;
        let mut loaded: Vec<String> = s.columns_loaded.iter().cloned().collect();
        loaded.sort();
        (s.pool.clone(), loaded)
    };
    let mut index = introspect(&pool, &IntrospectOptions::default()).await.map_err(err)?;
    let mut still = std::collections::HashSet::new();
    for db in loaded {
        if index.iter().any(|n| n.database == db) {
            index.extend(introspect_columns(&pool, &db).await.map_err(err)?);
            still.insert(db);
        }
    }
    let mut sessions = state.sessions.lock().unwrap();
    if let Some(s) = sessions.get_mut(&id) {
        s.set_index(index.clone());
        s.columns_loaded = still;
    }
    Ok(index)
}

#[tauri::command]
async fn refresh_index(state: tauri::State<'_, AppState>, id: i64) -> Result<Vec<Node>, String> {
    reindex(&state, id).await
}

/// Apply a design. The statements are regenerated here from the same inputs the preview used,
/// never taken from the UI, so what runs is exactly what the diff engine produced.
#[tauri::command]
async fn ddl_apply(state: tauri::State<'_, AppState>, id: i64, db: String, original: Option<TableDef>, desired: TableDef) -> Result<Vec<String>, String> {
    let statements = ddl_statements(&db, &original, &desired).map_err(|e| e.join("; "))?;
    if statements.is_empty() {
        return Ok(statements);
    }
    let mut c = conn_for(&state, id).await?;
    let result = myq_admin::run_statements(&mut c, &statements).await;
    let _ = state.store.lock().unwrap().add_history(id, &statements.join(";\n"), 0, result.is_ok());
    result.map_err(err)?;
    reindex(&state, id).await?;
    Ok(statements)
}

#[derive(serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum TableOp {
    Truncate,
    Drop,
    Rename { to: String },
    Duplicate { to: String, with_data: bool },
}

#[tauri::command]
async fn table_op(state: tauri::State<'_, AppState>, id: i64, db: String, table: String, operation: TableOp) -> Result<Vec<String>, String> {
    use myq_schema::ddl as d;
    let mut c = conn_for(&state, id).await?;
    let statements = match operation {
        TableOp::Truncate => vec![d::truncate_table_sql(&db, &table)],
        TableOp::Drop => vec![d::drop_table_sql(&db, &table)],
        TableOp::Rename { to } => vec![d::rename_table_sql(&db, &table, &to)],
        TableOp::Duplicate { to, with_data } => {
            let cols = if with_data {
                let (def, _) = myq_admin::read_table_def(&mut c, &db, &table).await.map_err(err)?;
                Some(def.columns.iter().filter(|c| c.generated.is_none()).map(|c| c.name.clone()).collect::<Vec<_>>())
            } else {
                None
            };
            d::duplicate_table_sql(&db, &table, &to, cols.as_deref())
        }
    };
    let result = myq_admin::run_statements(&mut c, &statements).await;
    let _ = state.store.lock().unwrap().add_history(id, &statements.join(";\n"), 0, result.is_ok());
    result.map_err(err)?;
    reindex(&state, id).await?;
    Ok(statements)
}

#[derive(serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum DatabaseOp {
    Create { name: String, charset: Option<String>, collation: Option<String> },
    Drop { name: String },
}

#[tauri::command]
async fn database_op(state: tauri::State<'_, AppState>, id: i64, operation: DatabaseOp) -> Result<String, String> {
    use myq_schema::ddl as d;
    let sql = match operation {
        DatabaseOp::Create { name, charset, collation } => {
            if name.trim().is_empty() { return Err("database name is empty".into()); }
            d::create_database_sql(&name, charset.as_deref(), collation.as_deref())
        }
        DatabaseOp::Drop { name } => d::drop_database_sql(&name),
    };
    let mut c = conn_for(&state, id).await?;
    let result = myq_admin::run_statements(&mut c, std::slice::from_ref(&sql)).await;
    let _ = state.store.lock().unwrap().add_history(id, &sql, 0, result.is_ok());
    result.map_err(err)?;
    reindex(&state, id).await?;
    Ok(sql)
}

#[derive(serde::Serialize)]
struct Collations {
    engines: Vec<String>,
    /// (charset, collations, default collation)
    charsets: Vec<(String, Vec<String>, String)>,
}

/// Engines, character sets and collations the server supports, for the designer's dropdowns.
#[tauri::command]
async fn server_options(state: tauri::State<'_, AppState>, id: i64) -> Result<Collations, String> {
    use myq_driver::mysql_async::prelude::Queryable;
    let mut c = conn_for(&state, id).await?;
    let engines: Vec<String> = c.query("SELECT ENGINE FROM information_schema.ENGINES WHERE SUPPORT IN ('YES','DEFAULT') ORDER BY ENGINE").await.map_err(err)?;
    let rows: Vec<(String, String, String)> = c
        .query("SELECT COLLATION_NAME, CHARACTER_SET_NAME, IS_DEFAULT FROM information_schema.COLLATIONS ORDER BY CHARACTER_SET_NAME, COLLATION_NAME")
        .await
        .map_err(err)?;
    let mut charsets: Vec<(String, Vec<String>, String)> = Vec::new();
    for (collation, charset, is_default) in rows {
        let entry = match charsets.iter().position(|c| c.0 == charset) {
            Some(i) => i,
            None => { charsets.push((charset.clone(), vec![], String::new())); charsets.len() - 1 }
        };
        if is_default == "Yes" { charsets[entry].2 = collation.clone(); }
        charsets[entry].1.push(collation);
    }
    Ok(Collations { engines, charsets })
}

// ---- users and privileges ----

#[tauri::command]
async fn accounts_list(state: tauri::State<'_, AppState>, id: i64) -> Result<Vec<Account>, String> {
    let mut c = conn_for(&state, id).await?;
    myq_admin::list_accounts(&mut c).await.map_err(err)
}

#[tauri::command]
async fn account_grants(state: tauri::State<'_, AppState>, id: i64, user: String, host: String) -> Result<Grants, String> {
    let mut c = conn_for(&state, id).await?;
    myq_admin::read_grants(&mut c, &user, &host).await.map_err(err)
}

/// Statements for creating or changing an account plus its privileges. `original` is the existing
/// account being edited; `desired` the full set of privileges wanted (None leaves them alone).
async fn account_statements(c: &mut myq_driver::Conn, original: &Option<Account>, edit: &AccountEdit, desired: &Option<Vec<ScopeGrant>>) -> Result<Vec<Sql>, String> {
    let mut sql = myq_admin::account_sql(original.as_ref(), edit)?;
    if let Some(desired) = desired {
        let current = match original {
            Some(o) => myq_admin::read_grants(c, &o.user, &o.host).await.map_err(err)?.scopes,
            None => vec![],
        };
        sql.extend(myq_admin::grant_diff_sql(&edit.user, &edit.host, &current, desired));
    }
    Ok(sql)
}

/// What an account edit would run, with passwords masked.
#[tauri::command]
async fn account_preview(state: tauri::State<'_, AppState>, id: i64, original: Option<Account>, edit: AccountEdit, desired: Option<Vec<ScopeGrant>>) -> Result<Vec<String>, String> {
    let mut c = conn_for(&state, id).await?;
    Ok(account_statements(&mut c, &original, &edit, &desired).await?.into_iter().map(|s| s.shown).collect())
}

#[tauri::command]
async fn account_apply(state: tauri::State<'_, AppState>, id: i64, original: Option<Account>, edit: AccountEdit, desired: Option<Vec<ScopeGrant>>) -> Result<usize, String> {
    let mut c = conn_for(&state, id).await?;
    let sql = account_statements(&mut c, &original, &edit, &desired).await?;
    myq_admin::run_sql(&mut c, &sql).await.map_err(|(i, m)| format!("statement {} failed: {m}", i + 1))
}

#[tauri::command]
async fn account_drop(state: tauri::State<'_, AppState>, id: i64, user: String, host: String) -> Result<(), String> {
    let mut c = conn_for(&state, id).await?;
    myq_admin::run_sql(&mut c, &[myq_admin::drop_account_sql(&user, &host)]).await.map(|_| ()).map_err(|(_, m)| m)
}

#[tauri::command]
fn history_list(state: tauri::State<AppState>, id: i64, search: String) -> Result<Vec<HistoryEntry>, String> {
    state.store.lock().unwrap().list_history(id, &search, 200).map_err(err)
}

#[tauri::command]
fn history_clear(state: tauri::State<AppState>, id: i64) -> Result<(), String> {
    state.store.lock().unwrap().clear_history(id).map_err(err)
}

#[tauri::command]
fn favorites_list(state: tauri::State<AppState>) -> Result<Vec<Favorite>, String> {
    state.store.lock().unwrap().list_favorites().map_err(err)
}

#[tauri::command]
fn favorite_save(state: tauri::State<AppState>, favorite: Favorite) -> Result<i64, String> {
    state.store.lock().unwrap().save_favorite(&favorite).map_err(err)
}

#[tauri::command]
fn favorite_delete(state: tauri::State<AppState>, id: i64) -> Result<(), String> {
    state.store.lock().unwrap().delete_favorite(id).map_err(err)
}

#[tauri::command]
fn session_get(state: tauri::State<AppState>, id: i64) -> Result<Option<String>, String> {
    state.store.lock().unwrap().get_session(id).map_err(err)
}

#[tauri::command]
fn session_set(state: tauri::State<AppState>, id: i64, session: String) -> Result<(), String> {
    state.store.lock().unwrap().set_session(id, &session).map_err(err)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let store = Store::open(&dir.join("mylodb.sqlite"), Box::new(Keychain))?;
            app.manage(AppState { store: Mutex::new(store), sessions: Mutex::new(HashMap::new()), running: Mutex::new(HashMap::new()), jobs: Mutex::new(HashMap::new()) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_connections,
            save_connection,
            delete_connection,
            connect,
            disconnect,
            filter_index,
            load_columns,
            db_table_stats,
            table_column_details,
            run_query,
            cancel_query,
            list_databases,
            history_list,
            history_clear,
            favorites_list,
            favorite_save,
            favorite_delete,
            session_get,
            session_set,
            table_meta,
            table_page,
            table_count,
            preview_changes,
            apply_changes,
            fetch_cell,
            save_cell_to_file,
            fk_lookup,
            export_table,
            table_def,
            ddl_preview,
            ddl_apply,
            refresh_index,
            table_op,
            database_op,
            server_options,
            accounts_list,
            account_grants,
            account_preview,
            account_apply,
            account_drop,
            complete_sql,
            format_sql,
            statement_at,
            explain_query,
            profile_query,
            test_connection,
            forget_ssh_host,
            server_info,
            process_list,
            kill_process,
            server_variables,
            server_status,
            set_global_variable,
            maintain_tables,
            log_settings,
            error_log,
            slow_log,
            binary_logs,
            binlog_events,
            export_query,
            cancel_job,
            csv_sniff,
            csv_preview,
            csv_infer,
            csv_create_table,
            import_csv_job,
            run_sql_source_job,
            backup_job
        ])
        .run(tauri::generate_context!())
        .expect("error while running MyLoDB");
}
