//! Thin Tauri bridge. All logic lives in the `myq-*` crates; this only exposes it as commands.

mod ai;
mod audit;
mod schedule;
mod engine;
use engine::Engine;
use myq_data::{Assign, Change, CellData, ColumnFilter, ExportOptions, LookupItem, Page, PageSpec, TableMeta, ApplyOutcome};
use myq_admin::{Account, AccountEdit, Grants, ScopeGrant, Sql};
use myq_schema::ddl::TableDef;
use myq_core::Backend;
use myq_driver::{kill_query, pool, Pool, StatementResult};
use myq_schema::filter::{Filter, Kind, Node};
use myq_store::{SecretStore, vault::Vault, Favorite, HistoryEntry, Keychain, Mode, SavedConnection, SecretRouter, Store};
use std::{collections::HashMap, sync::Mutex};
use tauri::{Emitter, Manager};

/// The safety settings of a connection, fixed when it is opened.
#[derive(Clone, Copy, Debug, Default)]
struct Safety {
    environment: myq_store::Environment,
    read_only: bool,
    timeout_secs: Option<u32>,
    confirm_destructive: bool,
}

/// With a default database set and "show all databases" off, only that database is listed (and everything below it).
fn restrict_databases(index: Vec<Node>, only: Option<&str>) -> Vec<Node> {
    match only {
        Some(db) => index.into_iter().filter(|n| n.database == db).collect(),
        None => index,
    }
}

/// A live connection: its pool and the cached name index the filter runs over.
struct Session {
    /// The MySQL pool; `None` for a SQLite file.
    #[allow(dead_code)]
    pool: Option<Pool>,
    /// The engine behind this session; the read commands go through it.
    engine: Engine,
    index: Vec<Node>,
    /// Index positions per database, so a filter scoped to one database never walks the whole server.
    by_db: HashMap<String, Vec<usize>>,
    /// Positions of the database nodes themselves (for filtering the database list by name).
    db_positions: Vec<usize>,
    /// Databases whose columns are in the index (they are loaded on demand, not at connect).
    columns_loaded: std::collections::HashSet<String>,
    /// Autocomplete's view: the index plus `completion_nodes`, rebuilt whenever either changes.
    catalog: myq_sql::Catalog,
    /// Columns loaded only for autocomplete. They are NOT in `index`: the UI holds a copy of the index and filter results are
    /// positions into it, so the index may only grow through `load_columns`, which tells the UI where the new nodes start.
    completion_nodes: Vec<Node>,
    completion_loaded: std::collections::HashSet<String>,
    /// Foreign keys per database, loaded for autocomplete's join suggestions (MySQL / MariaDB only).
    completion_fks: Vec<myq_sql::Fk>,
    fks_loaded: std::collections::HashSet<String>,
    safety: Safety,
    /// Only this database is shown (see `restrict_databases`).
    only_database: Option<String>,
    /// Keeps the SSH tunnel alive for as long as the session lives (dropping it closes the tunnel).
    #[allow(dead_code)]
    tunnel: Option<myq_tunnel::Tunnel>,
}

impl Session {
    fn new(pool: Option<Pool>, engine: Engine, index: Vec<Node>, tunnel: Option<myq_tunnel::Tunnel>, safety: Safety, only_database: Option<String>) -> Session {
        let mut s = Session { pool, engine, index: Vec::new(), by_db: HashMap::new(), db_positions: Vec::new(), columns_loaded: Default::default(), catalog: myq_sql::Catalog::default(), completion_nodes: Vec::new(), completion_loaded: Default::default(), completion_fks: Vec::new(), fks_loaded: Default::default(), safety, only_database, tunnel };
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
        self.index = index;
        self.rebuild_catalog();
    }

    fn rebuild_catalog(&mut self) {
        // completion-only columns of a database the UI has since loaded would be duplicates
        let all: Vec<Node> = self.index.iter().cloned().chain(self.completion_nodes.iter().filter(|n| !self.columns_loaded.contains(&n.database)).cloned()).collect();
        self.catalog = myq_sql::Catalog::from_nodes(&all);
        self.catalog.fks = self.completion_fks.clone();
    }

    /// Make the columns of `db` available to autocomplete without touching the index.
    fn add_completion_columns(&mut self, db: &str, cols: Vec<Node>) {
        if self.completion_loaded.insert(db.to_string()) && !self.columns_loaded.contains(db) {
            self.completion_nodes.extend(cols);
            self.rebuild_catalog();
        }
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
    /// Where saved passwords live: the Keychain, or an encrypted file behind a master password.
    secrets: std::sync::Arc<SecretRouter>,
    sessions: Mutex<HashMap<i64, Session>>,
    /// Cancel flags of the running backup / restore / import jobs, by job id.
    jobs: Mutex<HashMap<String, std::sync::Arc<std::sync::atomic::AtomicBool>>>,
    /// run_id -> (connection id, server-side connection id) of queries currently executing.
    running: Mutex<HashMap<String, (i64, u32)>>,
    /// SQLite runs that can be cancelled, by run id.
    sqlite_runs: Mutex<HashMap<String, std::sync::Arc<myq_sqlite::CancelToken>>>,
    /// Scheduled backups (kept in `schedules.json`), and the ones running right now.
    schedules: Mutex<Vec<schedule::Schedule>>,
    /// The audit log of changes (off until the user turns it on).
    audit: audit::Audit,
    schedule_file: std::path::PathBuf,
    schedule_running: Mutex<std::collections::HashSet<String>>,
}

impl AppState {
    /// Note a statement in the history and, when the audit log is on, in the audit log if it changed something.
    fn record(&self, id: i64, sql: &str, ms: u64, ok: bool) -> myq_store::Result<()> {
        let result = self.store.lock().unwrap().add_history(id, sql, ms, ok);
        if self.audit.enabled() && audit::should_log(sql) {
            let (connection, who, database) = match self.store.lock().unwrap().connection(id) {
                Ok(Some(c)) => (c.name.clone(), if c.engine == myq_store::EngineKind::Sqlite { c.file.clone().unwrap_or_default() } else { format!("{}@{}", c.config.user, c.config.host) }, c.config.database.clone()),
                _ => (format!("#{id}"), String::new(), None),
            };
            self.audit.record(audit::Entry { time: schedule::format_time(now_local()), connection, who, database, ok, ms, sql: sql.to_string() });
        }
        result
    }
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Refuse an operation that changes data, schema, accounts or settings on a read-only connection. The server refuses writes in a
/// read-only session as well; this gives a clear message first and also covers operations that run outside that session.
fn guard_writable(state: &tauri::State<'_, AppState>, id: i64, what: &str) -> Result<(), String> {
    let sessions = state.sessions.lock().unwrap();
    match sessions.get(&id) {
        Some(s) if s.safety.read_only => Err(format!("This connection is read-only, so {what} is not allowed. Edit the connection to turn read-only off.")),
        _ => Ok(()),
    }
}

#[tauri::command]
fn list_connections(state: tauri::State<AppState>) -> Result<Vec<SavedConnection>, String> {
    state.store.lock().unwrap().list().map_err(err)
}

#[tauri::command]
fn save_connection(state: tauri::State<AppState>, connection: SavedConnection) -> Result<i64, String> {
    state.store.lock().unwrap().save(&connection).map_err(err)
}

// ---- where saved passwords are kept ----

#[derive(serde::Serialize)]
struct SecretsStatus {
    /// "keychain" or "file".
    mode: &'static str,
    /// For "file": whether the master password has been typed in this run.
    unlocked: bool,
}

fn secrets_status_of(r: &SecretRouter) -> SecretsStatus {
    SecretsStatus { mode: if r.mode() == Mode::File { "file" } else { "keychain" }, unlocked: r.mode() == Mode::Keychain || r.unlocked() }
}

#[tauri::command]
fn secrets_status(state: tauri::State<AppState>) -> SecretsStatus {
    secrets_status_of(&state.secrets)
}

/// Type the master password: opens the encrypted file for this run.
#[tauri::command]
async fn secrets_unlock(state: tauri::State<'_, AppState>, password: String) -> Result<SecretsStatus, String> {
    let r = state.secrets.clone();
    // key derivation takes a moment by design, so it stays off the UI thread
    tauri::async_runtime::spawn_blocking(move || r.unlock(&password).map(|_| secrets_status_of(&r))).await.map_err(err)?.map_err(err)
}

/// Move every saved password out of the Keychain into the encrypted file. Returns how many were moved.
#[tauri::command]
async fn secrets_use_file(state: tauri::State<'_, AppState>, password: String) -> Result<usize, String> {
    let keys = state.store.lock().unwrap().secret_keys().map_err(err)?;
    let r = state.secrets.clone();
    tauri::async_runtime::spawn_blocking(move || r.use_file(&password, &keys)).await.map_err(err)?.map_err(err)
}

/// Move the passwords back into the Keychain and delete the file. Needs the file to be unlocked.
#[tauri::command]
fn secrets_use_keychain(state: tauri::State<AppState>) -> Result<usize, String> {
    let keys = state.store.lock().unwrap().secret_keys().map_err(err)?;
    state.secrets.use_keychain(&keys).map_err(err)
}

#[tauri::command]
async fn secrets_change_password(state: tauri::State<'_, AppState>, old: String, new: String) -> Result<(), String> {
    let r = state.secrets.clone();
    tauri::async_runtime::spawn_blocking(move || { r.unlock(&old)?; r.change_password(&new) }).await.map_err(err)?.map_err(err)
}

/// For a forgotten master password: delete the file. Saved passwords are lost; connections stay and ask for theirs again.
#[tauri::command]
fn secrets_reset(state: tauri::State<AppState>) -> Result<(), String> {
    state.secrets.reset().map_err(err)
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
async fn connect(state: tauri::State<'_, AppState>, id: i64, trust_fingerprint: Option<String>, reuse: Option<bool>) -> Result<Vec<Node>, ConnectError> {
    // Switching back to a connection that is already open keeps its session: no new pool or tunnel, no keychain read, no waiting.
    // Connecting after editing a connection leaves `reuse` off, so the new settings take effect.
    if reuse == Some(true) {
        if let Some(s) = state.sessions.lock().unwrap().get(&id) {
            return Ok(s.index.clone());
        }
    }
    let saved = state.store.lock().unwrap().connection(id).map_err(err)?;
    if let Some(c) = saved.as_ref().filter(|c| c.engine == myq_store::EngineKind::Sqlite) {
        // a SQLite file: no network, no tunnel. Writable unless the connection says read-only (then the file is opened read-only too)
        let file = c.file.clone().filter(|f| !f.trim().is_empty()).ok_or_else(|| "this connection has no database file".to_string())?;
        let read_only = c.config.read_only;
        let backend = if read_only { myq_sqlite::SqliteBackend::open(file)? } else { myq_sqlite::SqliteBackend::open_writable(file)? };
        let engine = Engine::Sqlite(backend);
        let index = engine.index(false).await.map_err(err)?;
        let safety = Safety { environment: c.environment, read_only, timeout_secs: None, confirm_destructive: c.confirm_destructive };
        state.sessions.lock().unwrap().insert(id, Session::new(None, engine, index.clone(), None, safety, None));
        return Ok(index);
    }
    let (cfg, ssh) = {
        let store = state.store.lock().unwrap();
        let cfg = store.config_for(id).map_err(err)?.ok_or_else(|| "connection not found".to_string())?;
        (cfg, store.ssh_for(id).map_err(err)?)
    };
    let (cfg, tunnel) = establish(&state.store, cfg, ssh, trust_fingerprint).await?;
    let p = pool(&cfg);
    let (safety, only_database) = {
        let store = state.store.lock().unwrap();
        let saved = store.connection(id).map_err(err)?;
        let safety = saved.as_ref().map(|c| Safety { environment: c.environment, read_only: c.config.read_only, timeout_secs: c.query_timeout_secs, confirm_destructive: c.confirm_destructive }).unwrap_or_default();
        let only = saved.and_then(|c| if c.show_all_databases { None } else { c.config.database.filter(|d| !d.is_empty()) });
        (safety, only)
    };
    // names only: columns are loaded per database when needed, so connecting to a huge server stays fast
    let engine = Engine::mysql(p.clone());
    let index = restrict_databases(engine.index(false).await.map_err(err)?, only_database.as_deref());
    state.sessions.lock().unwrap().insert(id, Session::new(Some(p), engine, index.clone(), tunnel, safety, only_database));
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
    if connection.engine == myq_store::EngineKind::Sqlite {
        let file = connection.file.clone().filter(|f| !f.trim().is_empty()).ok_or_else(|| "choose a database file".to_string())?;
        myq_sqlite::SqliteBackend::open(file)?;
        return Ok(TestResult { version: format!("SQLite {}", myq_sqlite::sqlite_version()), ssl_cipher: None, via_ssh: false });
    }
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
    let probe = myq_driver::probe(&cfg).await.map_err(err)?;
    Ok(TestResult { version: probe.version, ssl_cipher: probe.ssl_cipher, via_ssh })
}

/// Forget the saved host key of a connection's SSH server (after the server was legitimately re-keyed).
#[derive(serde::Serialize)]
struct SchemaCompareResult {
    #[serde(flatten)]
    diff: myq_schema::compare::SchemaDiff,
    /// Things a table definition holds that the designer cannot represent (kept as they are); the comparison may miss differences there.
    warnings: Vec<String>,
}

/// The base tables of one database as table definitions, in name order, plus the warnings from reading them.
async fn read_defs(state: &tauri::State<'_, AppState>, id: i64, db: &str) -> Result<(Vec<TableDef>, Vec<String>), String> {
    let engine = session_engine(state, id)?;
    let names: Vec<String> = engine.tables(db).await.map_err(err)?.into_iter().filter(|t| !t.is_view).map(|t| t.name).collect();
    let mut c = conn_for(state, id).await?;
    let (mut defs, mut warnings) = (Vec::new(), Vec::new());
    for name in names {
        let (def, w) = myq_admin::read_table_def(&mut c, db, &name).await.map_err(err)?;
        warnings.extend(w.into_iter().map(|w| format!("{db}.{name}: {w}")));
        defs.push(def);
    }
    Ok((defs, warnings))
}

#[derive(serde::Serialize)]
struct IndexAdvice {
    findings: Vec<myq_schema::advisor::Finding>,
    /// Indexes the server has not used since it started (from `sys.schema_unused_indexes`): (table, index).
    unused: Vec<(String, String)>,
    /// Why `unused` is empty or how to read it.
    unused_note: String,
    tables: usize,
}

/// Index findings for a database: duplicates, redundant prefixes and foreign keys without an index (from the definitions), plus the
/// indexes the server reports as unused. Nothing is changed.
#[tauri::command]
async fn index_advice(state: tauri::State<'_, AppState>, id: i64, db: String) -> Result<IndexAdvice, String> {
    let (defs, _) = read_defs(&state, id, &db).await?;
    let findings = myq_schema::advisor::analyze(&db, &defs);
    let mut c = conn_for(&state, id).await?;
    let unused = myq_driver::unused_indexes(&mut c, &db).await;
    let (unused, unused_note) = match unused {
        Ok(rows) => (rows, "Unused means: no query has used the index since the server started (performance_schema). Check the uptime before dropping: a rarely used index may serve a monthly report.".to_string()),
        Err(e) => (vec![], format!("Unused indexes are not available on this server: {e}")),
    };
    Ok(IndexAdvice { findings, unused, unused_note, tables: defs.len() })
}

/// Every table of a database with its columns and every foreign key, for the ER diagram.
#[tauri::command]
async fn er_model(state: tauri::State<'_, AppState>, id: i64, db: String) -> Result<myq_driver::ErModel, String> {
    myq_driver::er_model(&session_pool(&state, id)?, &db).await.map_err(err)
}

/// The SELECT for the query builder page, in the dialect of the connection's engine. Nothing runs.
#[tauri::command]
fn build_query(state: tauri::State<AppState>, id: i64, spec: myq_schema::builder::QuerySpec) -> Result<String, String> {
    let engine = state.sessions.lock().unwrap().get(&id).map(|s| s.engine.clone()).ok_or("not connected")?;
    myq_schema::builder::build_with(&spec, engine.dialect())
}

/// What has to change in the target database so that its tables match the source database's. Nothing is changed; the UI previews the
/// statements and runs them through `run_plan` on the target connection.
#[tauri::command]
async fn schema_compare(state: tauri::State<'_, AppState>, source_id: i64, source_db: String, target_id: i64, target_db: String) -> Result<SchemaCompareResult, String> {
    if source_id == target_id && source_db == target_db {
        return Err("choose two different databases".into());
    }
    let (source, mut warnings) = read_defs(&state, source_id, &source_db).await?;
    let (target, w2) = read_defs(&state, target_id, &target_db).await?;
    warnings.extend(w2);
    warnings.sort();
    warnings.dedup();
    let diff = myq_schema::compare::compare(&source, &source_db, &target, &target_db)?;
    Ok(SchemaCompareResult { diff, warnings })
}

/// What the connected server can do; the UI hides or disables what it cannot, with the reason.
#[tauri::command]
async fn capabilities(state: tauri::State<'_, AppState>, id: i64) -> Result<myq_core::Capabilities, String> {
    session_engine(&state, id)?.capabilities().await.map_err(err)
}

/// Links the About and Manual pages may open in the browser. Anything else is refused, so a page can never launch an arbitrary URL.
const ALLOWED_LINKS: [&str; 10] = [
    "https://farookhridoy.com", "https://thelarasoft.com", "https://github.com/farookhridoy/mylodb", "https://github.com/farookhridoy",
    // where a user makes a key for the AI assistant (the "Connect" buttons open these in the browser)
    "https://console.anthropic.com/settings/keys", "https://console.groq.com/keys", "https://openrouter.ai/keys", "https://aistudio.google.com/apikey", "https://ollama.com/download", "https://ollama.com/library",
];

fn link_allowed(url: &str) -> bool {
    ALLOWED_LINKS.iter().any(|a| url == *a || url.strip_prefix(a).is_some_and(|rest| rest.starts_with('/') || rest.starts_with('#')))
}

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !link_allowed(&url) {
        return Err("this link is not allowed".into());
    }
    // the link was checked against the allow-list above; each system has its own opener
    #[cfg(target_os = "windows")]
    let opener = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", &url]).spawn();
    #[cfg(target_os = "macos")]
    let opener = std::process::Command::new("open").arg(&url).spawn();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let opener = std::process::Command::new("xdg-open").arg(&url).spawn();
    opener.map(|_| ()).map_err(err)
}

#[tauri::command]
fn forget_ssh_host(state: tauri::State<AppState>, id: i64) -> Result<(), String> {
    let store = state.store.lock().unwrap();
    if let Some((ssh, _)) = store.ssh_for(id).map_err(err)? {
        store.ssh_forget(&ssh.host, ssh.port).map_err(err)?;
    }
    Ok(())
}

/// Ids of the connections that are open right now. The window asks on start, so a reload of the page
/// picks the open connections up again instead of showing a disconnected screen.
#[tauri::command]
fn open_connections(state: tauri::State<'_, AppState>) -> Vec<i64> {
    let mut ids: Vec<i64> = state.sessions.lock().unwrap().keys().copied().collect();
    ids.sort_unstable();
    ids
}

#[tauri::command]
async fn disconnect(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    let session = state.sessions.lock().unwrap().remove(&id);
    if let Some(s) = session {
        if let Some(pool) = s.pool {
            pool.disconnect().await.map_err(err)?;
        }
    }
    Ok(())
}

/// Indices (into the session's index) that match the filter query.
///
/// With a `scope` (the selected database) only that database's objects are searched, like "Filter tables in
/// sakila". Without one the query matches database names only, so typing never walks every table of the server.
/// Drop the hits whose kind the user has switched off in the filter options. A query that names kinds itself
/// (`col:email`, `type:view`) is explicit and is left alone.
fn restrict_kinds(index: &[Node], mut hits: Vec<usize>, filter: &Filter, allowed: Option<&[Kind]>) -> Vec<usize> {
    if let Some(allowed) = allowed {
        if !filter.names_kinds() {
            hits.retain(|&i| index.get(i).is_some_and(|n| allowed.contains(&n.kind)));
        }
    }
    hits
}

#[tauri::command]
fn filter_index(state: tauri::State<AppState>, id: i64, query: String, scope: Option<String>, kinds: Option<Vec<Kind>>) -> Result<Vec<usize>, String> {
    let filter = Filter::parse(&query).map_err(|e| format!("{e:?}"))?;
    let sessions = state.sessions.lock().unwrap();
    let s = sessions.get(&id).ok_or_else(|| "not connected".to_string())?;
    Ok(match scope {
        Some(db) => s.by_db.get(&db).map_or_else(Vec::new, |positions| restrict_kinds(&s.index, filter.apply_positions(&s.index, positions), &filter, kinds.as_deref())),
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
    let engine = {
        let sessions = state.sessions.lock().unwrap();
        let s = sessions.get(&id).ok_or("not connected")?;
        if s.columns_loaded.contains(db) || !s.by_db.contains_key(db) {
            return Ok((s.index.len(), vec![]));
        }
        s.engine.clone()
    };
    let cols = engine.column_nodes(db).await.map_err(err)?;
    let mut sessions = state.sessions.lock().unwrap();
    let s = sessions.get_mut(&id).ok_or("not connected")?;
    if s.columns_loaded.contains(db) {
        return Ok((s.index.len(), vec![])); // another request got there first
    }
    let start = s.add_columns(db, cols.clone());
    Ok((start, cols))
}

fn session_engine(state: &tauri::State<'_, AppState>, id: i64) -> Result<Engine, String> {
    state.sessions.lock().unwrap().get(&id).map(|s| s.engine.clone()).ok_or_else(|| "not connected".to_string())
}

/// The MySQL pool of a session. A SQLite session has none: the features that need it say so.
fn session_pool(state: &tauri::State<'_, AppState>, id: i64) -> Result<Pool, String> {
    let sessions = state.sessions.lock().unwrap();
    let s = sessions.get(&id).ok_or("not connected")?;
    s.pool.clone().ok_or_else(|| "this feature needs a MySQL or MariaDB connection; it is not available for a SQLite file".to_string())
}

/// Sizes and row estimates of a database's tables (one query), for the object browser.
#[tauri::command]
async fn db_table_stats(state: tauri::State<'_, AppState>, id: i64, db: String) -> Result<Vec<myq_driver::TableStat>, String> {
    myq_driver::table_stats(&session_pool(&state, id)?, &db).await.map_err(err)
}

/// Declared types, keys and nullability of one table's columns, for the object browser.
#[tauri::command]
async fn table_column_details(state: tauri::State<'_, AppState>, id: i64, db: String, table: String) -> Result<Vec<myq_driver::ColumnDetail>, String> {
    session_engine(&state, id)?.columns(&db, &table).await.map_err(err)
}

/// Indexes of one table, for the object browser.
#[tauri::command]
async fn table_index_details(state: tauri::State<'_, AppState>, id: i64, db: String, table: String) -> Result<Vec<myq_driver::IndexDetail>, String> {
    session_engine(&state, id)?.indexes(&db, &table).await.map_err(err)
}

/// Everything the table Properties and Diagram views show.
#[tauri::command]
async fn table_overview(state: tauri::State<'_, AppState>, id: i64, db: String, table: String) -> Result<myq_driver::TableOverview, String> {
    myq_driver::table_overview(&session_pool(&state, id)?, &db, &table).await.map_err(err)
}

/// The CREATE statement of a view, routine, trigger or event.
#[tauri::command]
async fn object_definition(state: tauri::State<'_, AppState>, id: i64, kind: Kind, db: String, name: String) -> Result<String, String> {
    myq_driver::object_definition(&session_pool(&state, id)?, kind, &db, &name)
        .await
        .map_err(err)?
        .ok_or_else(|| format!("the server returned no definition for {db}.{name} (your account may not be allowed to read it)"))
}

/// Drop a view, routine, trigger or event. Tables go through `table_op`, databases through `database_op`.
#[tauri::command]
async fn drop_object(state: tauri::State<'_, AppState>, id: i64, kind: Kind, db: String, name: String) -> Result<String, String> {
    guard_writable(&state, id, "dropping an object")?;
    let sql = myq_schema::ddl::drop_object_sql(kind, &db, &name).ok_or_else(|| format!("{kind:?} cannot be dropped here"))?;
    let mut c = conn_for(&state, id).await?;
    let result = myq_admin::run_statements(&mut c, std::slice::from_ref(&sql)).await;
    let _ = state.record(id, &sql, 0, result.is_ok());
    result.map_err(err)?;
    reindex(&state, id).await?;
    Ok(sql)
}

/// Execute a script on a dedicated server connection. `run_id` lets the UI cancel it.
#[tauri::command]
async fn run_query(
    state: tauri::State<'_, AppState>,
    id: i64,
    run_id: String,
    database: Option<String>,
    sql: String,
    // `Some(false)` runs the script with foreign key checks off, `Some(true)` forces them on; `None` leaves the server's setting.
    foreign_key_checks: Option<bool>,
) -> Result<Vec<StatementResult>, String> {
    let (engine, safety) = state.sessions.lock().unwrap().get(&id).map(|s| (s.engine.clone(), s.safety)).ok_or("not connected")?;
    // A read-only connection is read-only on the server (the session). This check refuses the obvious writes, and attempts to switch the
    // session back to read-write, before anything is sent, with a message that says why.
    if safety.read_only {
        if let Some((n, text, why)) = myq_sql::read_only_violations(&sql).into_iter().next() {
            return Err(format!("Not run: this connection is read-only, and statement {} ({}) {why}. Edit the connection to turn read-only off.", n + 1, text.lines().next().unwrap_or("").chars().take(60).collect::<String>()));
        }
    }
    if let Engine::Sqlite(backend) = &engine {
        // a SQLite file: no foreign-key switch (a read-only connection also opened the file read-only); Cancel interrupts the statement
        let started = std::time::Instant::now();
        let token = std::sync::Arc::new(myq_sqlite::CancelToken::new());
        state.sqlite_runs.lock().unwrap().insert(run_id.clone(), token.clone());
        let results = backend.run_cancellable(&sql, token).await.map_err(err);
        state.sqlite_runs.lock().unwrap().remove(&run_id);
        let results = results?;
        let ok = results.iter().all(|r| r.error.is_none());
        let _ = state.record(id, sql.trim(), started.elapsed().as_millis() as u64, ok);
        return Ok(results);
    }
    let pool = session_pool(&state, id)?;
    let mut conn = pool.get_conn().await.map_err(err)?;
    if let Some(on) = foreign_key_checks {
        myq_driver::set_foreign_key_checks(&mut conn, on).await.map_err(err)?;
    }
    state.running.lock().unwrap().insert(run_id.clone(), (id, conn.id()));
    // The query timeout: the whole run may take this long, then the running statement is killed (like the Cancel button).
    let started = std::time::Instant::now();
    let (results, _timed_out) = myq_driver::execute_script_with_timeout(&pool, &mut conn, database.as_deref(), &sql, safety.timeout_secs).await;
    state.running.lock().unwrap().remove(&run_id);
    if foreign_key_checks.is_some() {
        // the connection goes back to the pool: leave it as the next caller expects
        myq_driver::restore_foreign_key_checks(&mut conn).await;
    }
    let ok = results.iter().all(|r| r.error.is_none());
    // History is best-effort: a logging failure must not hide the query result.
    let _ = state.record(id, sql.trim(), started.elapsed().as_millis() as u64, ok);
    Ok(results)
}

/// A statement the UI should show for confirmation before it runs, with an estimate of how many rows it touches.
#[derive(serde::Serialize)]
struct ReviewItem {
    #[serde(flatten)]
    risk: myq_sql::StatementRisk,
    estimated_rows: Option<u64>,
}

#[derive(serde::Serialize)]
struct Review {
    items: Vec<ReviewItem>,
    environment: myq_store::Environment,
    read_only: bool,
    /// Whether the connection asks for confirmation (the "warn before destructive SQL" setting).
    confirm: bool,
}

/// Advisory review of a script before it runs. Sends nothing that changes data: row estimates use EXPLAIN.
#[tauri::command]
async fn review_sql(state: tauri::State<'_, AppState>, id: i64, database: Option<String>, sql: String) -> Result<Review, String> {
    let (pool, safety) = state.sessions.lock().unwrap().get(&id).map(|s| (s.pool.clone(), s.safety)).ok_or("not connected")?;
    let risks = myq_sql::review(&sql);
    let mut items = Vec::new();
    // a SQLite session has no pool: the review still lists the risks, without a row estimate
    let mut conn = match (&pool, risks.is_empty()) {
        (Some(p), false) => Some(p.get_conn().await.map_err(err)?),
        _ => None,
    };
    for risk in risks {
        let writes_rows = risk.findings.iter().any(|f| matches!(f.kind, myq_sql::RiskKind::DeleteWithoutWhere | myq_sql::RiskKind::UpdateWithoutWhere));
        let estimated_rows = match (&mut conn, writes_rows || is_row_statement(&risk.sql)) {
            (Some(c), true) => myq_driver::estimate_rows(c, database.as_deref(), &risk.sql).await.ok().flatten(),
            _ => None,
        };
        items.push(ReviewItem { risk, estimated_rows });
    }
    Ok(Review { items, environment: safety.environment, read_only: safety.read_only, confirm: safety.confirm_destructive })
}

/// DELETE or UPDATE: the statements whose reach EXPLAIN can estimate.
fn is_row_statement(sql: &str) -> bool {
    let w = sql.trim_start().split(|c: char| !c.is_ascii_alphabetic()).next().unwrap_or("").to_ascii_uppercase();
    w == "DELETE" || w == "UPDATE"
}

#[tauri::command]
async fn cancel_query(state: tauri::State<'_, AppState>, run_id: String) -> Result<(), String> {
    if let Some(token) = state.sqlite_runs.lock().unwrap().get(&run_id).cloned() {
        token.cancel();
        return Ok(());
    }
    let target = state.running.lock().unwrap().get(&run_id).copied();
    let Some((id, server_conn)) = target else { return Ok(()) };
    let pool = state.sessions.lock().unwrap().get(&id).map(|s| s.pool.clone()).ok_or("not connected")?;
    // only MySQL sessions have a pool; SQLite runs were handled above
    match pool {
        Some(pool) => kill_query(&pool, server_conn).await.map_err(err),
        None => Ok(()),
    }
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
    session_pool(state, id)?.get_conn().await.map_err(err)
}

/// The file of a SQLite session, or `None` for a MySQL one.
fn sqlite_file(state: &tauri::State<'_, AppState>, id: i64) -> Option<std::path::PathBuf> {
    state.sessions.lock().unwrap().get(&id).and_then(|s| s.engine.sqlite_path().map(|p| p.to_path_buf()))
}

/// The file of a SQLite session and whether this connection may change it.
fn sqlite_target(state: &tauri::State<'_, AppState>, id: i64) -> Option<(std::path::PathBuf, bool)> {
    state.sessions.lock().unwrap().get(&id).and_then(|s| s.engine.sqlite_path().map(|p| (p.to_path_buf(), s.engine.sqlite_writable())))
}

/// Run blocking SQLite work off the async runtime's threads.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tokio::task::spawn_blocking(f).await.map_err(err)?
}

#[tauri::command]
async fn table_meta(state: tauri::State<'_, AppState>, id: i64, db: String, table: String) -> Result<TableMeta, String> {
    if let Some((path, writable)) = sqlite_target(&state, id) {
        return blocking(move || myq_sqlite::table_meta(&path, &table, writable)).await;
    }
    let mut c = conn_for(&state, id).await?;
    myq_data::table_meta(&mut c, &db, &table).await.map_err(err)
}

#[tauri::command]
async fn table_page(state: tauri::State<'_, AppState>, id: i64, db: String, table: String, spec: PageSpec) -> Result<Page, String> {
    if let Some((path, writable)) = sqlite_target(&state, id) {
        return blocking(move || {
            let meta = myq_sqlite::table_meta(&path, &table, writable)?;
            myq_sqlite::fetch_page(&path, &meta, &spec)
        })
        .await;
    }
    let mut c = conn_for(&state, id).await?;
    let meta = myq_data::table_meta(&mut c, &db, &table).await.map_err(err)?;
    myq_data::fetch_page(&mut c, &meta, &spec).await.map_err(err)
}

#[tauri::command]
async fn table_count(state: tauri::State<'_, AppState>, id: i64, db: String, table: String, filters: Vec<ColumnFilter>, where_sql: Option<String>, run_id: Option<String>) -> Result<u64, String> {
    if let Some((path, writable)) = sqlite_target(&state, id) {
        return blocking(move || {
            let meta = myq_sqlite::table_meta(&path, &table, writable)?;
            myq_sqlite::count_rows(&path, &meta, &filters, where_sql.as_deref())
        })
        .await;
    }
    let mut c = conn_for(&state, id).await?;
    let meta = myq_data::table_meta(&mut c, &db, &table).await.map_err(err)?;
    // an exact COUNT(*) can take long on a big table: with a run id it can be cancelled like a query (KILL QUERY)
    if let Some(run) = &run_id {
        state.running.lock().unwrap().insert(run.clone(), (id, c.id()));
    }
    let n = myq_data::count_rows(&mut c, &meta, &filters, where_sql.as_deref()).await.map_err(err);
    if let Some(run) = &run_id {
        state.running.lock().unwrap().remove(run);
    }
    n
}

/// A quick row estimate (table statistics, or EXPLAIN when filtered). None when the server has none.
#[tauri::command]
async fn table_count_estimate(state: tauri::State<'_, AppState>, id: i64, db: String, table: String, filters: Vec<ColumnFilter>, where_sql: Option<String>) -> Result<Option<u64>, String> {
    if sqlite_file(&state, id).is_some() {
        return Ok(None); // SQLite keeps no statistics; the grid counts exactly
    }
    let mut c = conn_for(&state, id).await?;
    let meta = myq_data::table_meta(&mut c, &db, &table).await.map_err(err)?;
    myq_data::estimate_count(&mut c, &meta, &filters, where_sql.as_deref()).await.map_err(err)
}

#[tauri::command]
async fn preview_changes(state: tauri::State<'_, AppState>, id: i64, db: String, table: String, changes: Vec<Change>) -> Result<Vec<String>, String> {
    if let Some((path, writable)) = sqlite_target(&state, id) {
        return blocking(move || myq_sqlite::preview_changes(&myq_sqlite::table_meta(&path, &table, writable)?, &changes)).await;
    }
    let mut c = conn_for(&state, id).await?;
    let meta = myq_data::table_meta(&mut c, &db, &table).await.map_err(err)?;
    myq_data::preview_changes(&meta, &changes).map_err(err)
}

#[tauri::command]
async fn apply_changes(state: tauri::State<'_, AppState>, id: i64, db: String, table: String, changes: Vec<Change>) -> Result<ApplyOutcome, String> {
    guard_writable(&state, id, "saving changes to rows")?;
    if let Some((path, writable)) = sqlite_target(&state, id) {
        let (sqls, outcome) = blocking(move || {
            let meta = myq_sqlite::table_meta(&path, &table, writable)?;
            let outcome = myq_sqlite::apply_changes(&path, &meta, &changes)?;
            Ok((myq_sqlite::preview_changes(&meta, &changes).unwrap_or_default(), outcome))
        })
        .await?;
        let _ = state.record(id, &sqls.join(";\n"), 0, true);
        return Ok(outcome);
    }
    let mut c = conn_for(&state, id).await?;
    let meta = myq_data::table_meta(&mut c, &db, &table).await.map_err(err)?;
    let outcome = myq_data::apply_changes(&mut c, &meta, &changes).await.map_err(err)?;
    // Edits are part of the audit trail: log what ran, best-effort.
    if let Ok(sqls) = myq_data::preview_changes(&meta, &changes) {
        let _ = state.record(id, &sqls.join(";\n"), 0, true);
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

/// Export every row matching `spec` to `path`, as a job: progress events while it runs, and `cancel_job` stops it.
/// A partial file is removed on failure or cancel (the error is then "cancelled").
#[tauri::command]
async fn export_table(app: tauri::AppHandle, state: tauri::State<'_, AppState>, id: i64, job_id: String, db: String, table: String, spec: PageSpec, options: ExportOptions, path: String) -> Result<ExportSummary, String> {
    if let Some(file) = sqlite_file(&state, id) {
        // a SQLite file: the rows are streamed from the file into the same writers
        let meta = myq_sqlite::table_meta(&file, &table, false)?;
        let options = ExportOptions { name: options.name.clone().or_else(|| Some(table.clone())), ..options };
        let total = myq_sqlite::count_rows(&file, &meta, &spec.filters, spec.where_sql.as_deref()).ok();
        let guard = JobGuard::start(&state, &job_id);
        let mut progress = job_progress(app, job_id);
        let flag = guard.flag.clone();
        let out_path = path.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut tick = move |rows: u64| {
                progress(myq_transfer::Progress { phase: "export".into(), done: rows, total, message: format!("{rows} rows written") });
                !flag.load(std::sync::atomic::Ordering::Relaxed)
            };
            myq_sqlite::export_table(&file, &meta, &spec, &options, myq_data::Dest::File(std::path::Path::new(&out_path)), &mut tick)
        })
        .await
        .map_err(err)?;
        return match result {
            Ok(o) => Ok(ExportSummary { rows: o.rows, warnings: o.warnings }),
            Err(e) => {
                let _ = std::fs::remove_file(&path);
                Err(e)
            }
        };
    }
    let mut c = conn_for(&state, id).await?;
    let meta = myq_data::table_meta(&mut c, &db, &table).await.map_err(err)?;
    let options = ExportOptions { name: options.name.clone().or_else(|| Some(table.clone())), ..options };
    // the estimate only draws the progress bar; it is not used for anything that must be exact
    let total = myq_data::estimate_count(&mut c, &meta, &spec.filters, spec.where_sql.as_deref()).await.ok().flatten();
    let guard = JobGuard::start(&state, &job_id);
    let mut progress = job_progress(app, job_id);
    let flag = guard.flag.clone();
    let mut tick = move |rows: u64| {
        progress(myq_transfer::Progress { phase: "export".into(), done: rows, total, message: format!("{rows} rows written") });
        !flag.load(std::sync::atomic::Ordering::Relaxed)
    };
    match myq_data::export_rows(&mut c, &meta, &spec, &options, myq_data::Dest::File(std::path::Path::new(&path)), &mut tick).await {
        Ok(o) => Ok(ExportSummary { rows: o.rows, warnings: o.warnings }),
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            Err(err(e))
        }
    }
}

/// Export the result of one SELECT to a file, as a job (see `export_table`).
#[tauri::command]
async fn export_query(app: tauri::AppHandle, state: tauri::State<'_, AppState>, id: i64, job_id: String, database: Option<String>, sql: String, options: ExportOptions, path: String) -> Result<ExportSummary, String> {
    if let Some(file) = sqlite_file(&state, id) {
        let guard = JobGuard::start(&state, &job_id);
        let mut progress = job_progress(app, job_id);
        let flag = guard.flag.clone();
        let out_path = path.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut tick = move |rows: u64| {
                progress(myq_transfer::Progress { phase: "export".into(), done: rows, total: None, message: format!("{rows} rows written") });
                !flag.load(std::sync::atomic::Ordering::Relaxed)
            };
            myq_sqlite::export_query(&file, &sql, &options, myq_data::Dest::File(std::path::Path::new(&out_path)), &mut tick)
        })
        .await
        .map_err(err)?;
        return match result {
            Ok(o) => Ok(ExportSummary { rows: o.rows, warnings: o.warnings }),
            Err(e) => {
                let _ = std::fs::remove_file(&path);
                Err(e)
            }
        };
    }
    let mut c = conn_for(&state, id).await?;
    let guard = JobGuard::start(&state, &job_id);
    let mut progress = job_progress(app, job_id);
    let flag = guard.flag.clone();
    let mut tick = move |rows: u64| {
        progress(myq_transfer::Progress { phase: "export".into(), done: rows, total: None, message: format!("{rows} rows written") });
        !flag.load(std::sync::atomic::Ordering::Relaxed)
    };
    match myq_data::export_query(&mut c, database.as_deref(), &sql, &options, myq_data::Dest::File(std::path::Path::new(&path)), &mut tick).await {
        Ok(o) => Ok(ExportSummary { rows: o.rows, warnings: o.warnings }),
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            Err(err(e))
        }
    }
}

// ---- admin pages: the databases list, a database's tables, the server home ----

/// Databases the account may see, honouring "show all databases = off" (a hosted account sees only its own).
#[tauri::command]
async fn database_list(state: tauri::State<'_, AppState>, id: i64) -> Result<Vec<myq_driver::DatabaseRow>, String> {
    let (engine, only) = state.sessions.lock().unwrap().get(&id).map(|s| (s.engine.clone(), s.only_database.clone())).ok_or("not connected")?;
    let mut rows = engine.databases().await.map_err(err)?;
    if let Some(db) = only {
        rows.retain(|r| r.name == db);
    }
    Ok(rows)
}

/// Table counts and sizes per database. Slower than the list (it reads every table's statistics), so the page asks for it separately.
#[tauri::command]
async fn database_sizes(state: tauri::State<'_, AppState>, id: i64) -> Result<Vec<(String, u64, u64)>, String> {
    let (pool, only, tables) = {
        let sessions = state.sessions.lock().unwrap();
        let s = sessions.get(&id).ok_or("not connected")?;
        (s.pool.clone(), s.only_database.clone(), s.index.iter().filter(|n| n.kind == Kind::Table).count() as u64)
    };
    let Some(pool) = pool else {
        // a SQLite file: one database, no size statistics
        return Ok(vec![("main".to_string(), tables, 0)]);
    };
    let mut rows = myq_driver::database_sizes(&pool).await.map_err(err)?;
    if let Some(db) = only {
        rows.retain(|r| r.0 == db);
    }
    Ok(rows)
}

#[tauri::command]
async fn database_tables(state: tauri::State<'_, AppState>, id: i64, db: String) -> Result<Vec<myq_driver::TableRow>, String> {
    session_engine(&state, id)?.tables(&db).await.map_err(err)
}

#[derive(serde::Serialize)]
struct ServerHome {
    info: myq_admin::ServerInfo,
    /// `user@host` the server sees (CURRENT_USER()).
    account: String,
    protocol: Option<u32>,
    connections: Option<u64>,
    /// The collation this connection uses right now.
    connection_collation: String,
    databases: usize,
}

#[tauri::command]
async fn server_home(state: tauri::State<'_, AppState>, id: i64) -> Result<ServerHome, String> {
    let databases = {
        let sessions = state.sessions.lock().unwrap();
        sessions.get(&id).ok_or("not connected")?.db_positions.len()
    };
    let mut c = conn_for(&state, id).await?;
    let info = myq_admin::server_info(&mut c).await.map_err(err)?;
    let facts = myq_driver::session_facts(&mut c).await.map_err(err)?;
    Ok(ServerHome {
        info,
        account: facts.account,
        protocol: facts.protocol,
        connections: facts.threads_connected,
        connection_collation: facts.connection_collation,
        databases,
    })
}

#[derive(serde::Serialize)]
struct ServerCatalog {
    engines: myq_driver::Grid,
    charsets: myq_driver::Grid,
    /// Empty when this server is not a replica (or the account may not look).
    replication: myq_driver::Grid,
    replication_note: Option<String>,
}

#[tauri::command]
async fn server_catalog(state: tauri::State<'_, AppState>, id: i64) -> Result<ServerCatalog, String> {
    let mut c = conn_for(&state, id).await?;
    let engines = myq_driver::query_grid(&mut c, "SHOW ENGINES").await.map_err(err)?;
    let charsets = myq_driver::query_grid(&mut c, "SHOW CHARACTER SET").await.map_err(err)?;
    // MySQL 8.0.22+ and MariaDB 10.5.1+ know REPLICA; older servers only SLAVE.
    let (replication, replication_note) = match myq_driver::query_grid(&mut c, "SHOW REPLICA STATUS").await {
        Ok(g) => (g, None),
        Err(_) => match myq_driver::query_grid(&mut c, "SHOW SLAVE STATUS").await {
            Ok(g) => (g, None),
            Err(e) => (myq_driver::Grid::default(), Some(format!("not available: {e}"))),
        },
    };
    Ok(ServerCatalog { engines, charsets, replication, replication_note })
}

// ---- plans: the SQL a bulk or destructive operation will run, shown before it runs ----

/// An operation the UI wants to preview. Building the statements is pure (see `myq_schema::plan`); nothing here touches the server.
#[derive(serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum PlanOp {
    DropDatabases { names: Vec<String> },
    DropTables { db: String, objects: Vec<(String, bool)> },
    EmptyTables { db: String, objects: Vec<(String, bool)> },
    CopyTables { db: String, copies: Vec<(String, String)>, with_data: bool },
    AddPrefix { db: String, tables: Vec<String>, prefix: String },
    ReplacePrefix { db: String, tables: Vec<String>, old: String, new: String },
    CopyWithPrefix { db: String, tables: Vec<String>, prefix: String, with_data: bool },
    Checksum { db: String, tables: Vec<String> },
    AlterDatabase { name: String, charset: Option<String>, collation: Option<String> },
    CopyDatabase { src: String, dst: String, with_data: bool, collation: Option<String> },
    RenameDatabase { old: String, new: String, collation: Option<String> },
    DropObject { db: String, kind: Kind, name: String },
    CreateDatabase { name: String, collation: Option<String> },
    RenameTable { db: String, from: String, to: String },
    RenameRoutine { db: String, kind: Kind, from: String, to: String, definition: String },
}

/// The statements for `op`, or the reason it cannot be done.
#[tauri::command]
fn build_plan(state: tauri::State<AppState>, id: i64, op: PlanOp) -> Result<Vec<String>, String> {
    use myq_schema::plan as p;
    // table names and the number of other objects of a database come from the session's index
    let tables_of = |db: &str| -> Result<(Vec<String>, usize), String> {
        let sessions = state.sessions.lock().unwrap();
        let s = sessions.get(&id).ok_or("not connected")?;
        let nodes = s.by_db.get(db).map(|v| v.iter().map(|&i| &s.index[i]).collect::<Vec<_>>()).unwrap_or_default();
        let tables = nodes.iter().filter(|n| n.kind == Kind::Table).map(|n| n.name.clone()).collect();
        let other = nodes.iter().filter(|n| matches!(n.kind, Kind::View | Kind::Procedure | Kind::Function | Kind::Trigger | Kind::Event)).count();
        Ok((tables, other))
    };
    match op {
        PlanOp::DropDatabases { names } => p::drop_databases(&names),
        PlanOp::DropTables { db, objects } => p::drop_tables(&db, &objects),
        PlanOp::EmptyTables { db, objects } => p::empty_tables(&db, &objects),
        PlanOp::CopyTables { db, copies, with_data } => p::copy_tables(&db, &copies, with_data),
        PlanOp::AddPrefix { db, tables, prefix } => p::add_prefix(&db, &tables, &prefix),
        PlanOp::ReplacePrefix { db, tables, old, new } => p::replace_prefix(&db, &tables, &old, &new),
        PlanOp::CopyWithPrefix { db, tables, prefix, with_data } => p::copy_with_prefix(&db, &tables, &prefix, with_data),
        PlanOp::Checksum { db, tables } => p::checksum_tables(&db, &tables),
        PlanOp::AlterDatabase { name, charset, collation } => p::alter_database(&name, charset.as_deref(), collation.as_deref()),
        PlanOp::CopyDatabase { src, dst, with_data, collation } => {
            let (tables, _) = tables_of(&src)?;
            p::copy_database(&src, &dst, &tables, with_data, collation.as_deref())
        }
        PlanOp::CreateDatabase { name, collation } => p::create_database(&name, collation.as_deref()),
        PlanOp::DropObject { db, kind, name } => myq_schema::ddl::drop_object_sql(kind, &db, &name).map(|sql| vec![format!("{sql};")]).ok_or_else(|| format!("{kind:?} cannot be dropped this way")),
        PlanOp::RenameRoutine { db, kind, from, to, definition } => p::rename_routine(&db, kind, &from, &to, &definition),
        PlanOp::RenameTable { db, from, to } => {
            if to.trim().is_empty() || to == from {
                Err("choose a different, non-empty name".into())
            } else {
                Ok(vec![format!("{};", myq_schema::ddl::rename_table_sql(&db, &from, &to))])
            }
        }
        PlanOp::RenameDatabase { old, new, collation } => {
            let (tables, other) = tables_of(&old)?;
            p::rename_database(&old, &new, &tables, other, collation.as_deref())
        }
    }
}

/// Run a plan exactly as shown, on one connection, stopping at the first error. The foreign key switch is part of the statements
/// (see `myq_schema::plan::with_fk_checks`); it is also put back afterwards, so a failed run cannot leave checks off on a pooled connection.
#[tauri::command]
async fn run_plan(state: tauri::State<'_, AppState>, id: i64, statements: Vec<String>) -> Result<Vec<StatementResult>, String> {
    guard_writable(&state, id, "running this operation")?;
    if statements.is_empty() {
        return Err("there is nothing to run".into());
    }
    let safety = state.sessions.lock().unwrap().get(&id).map(|s| s.safety).ok_or("not connected")?;
    let pool = session_pool(&state, id)?;
    let mut conn = pool.get_conn().await.map_err(err)?;
    let sql = statements.join("\n");
    let started = std::time::Instant::now();
    let (results, _) = myq_driver::execute_script_with_timeout(&pool, &mut conn, None, &sql, safety.timeout_secs).await;
    myq_driver::restore_foreign_key_checks(&mut conn).await;
    let ok = results.iter().all(|r| r.error.is_none());
    let _ = state.record(id, sql.trim(), started.elapsed().as_millis() as u64, ok);
    let _ = reindex(&state, id).await;
    Ok(results)
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

/// Search a database for a value (Search → Data search). `job_id` lets the UI cancel it and follow its progress ("job-progress"); a
/// cancel also stops the statement that is running, and what was found so far is returned.
#[tauri::command]
async fn search_data(app: tauri::AppHandle, state: tauri::State<'_, AppState>, id: i64, job_id: String, db: String, request: myq_data::SearchRequest) -> Result<myq_data::SearchResult, String> {
    let mut c = conn_for(&state, id).await?;
    let guard = JobGuard::start(&state, &job_id);
    state.running.lock().unwrap().insert(job_id.clone(), (id, c.id()));
    let mut tick = job_progress(app, job_id.clone());
    let flag = guard.flag.clone();
    let is_cancelled = move || flag.load(std::sync::atomic::Ordering::Relaxed);
    let mut progress = |done: usize, total: usize| tick(myq_transfer::Progress { phase: "Searching".into(), done: done as u64, total: Some(total as u64), message: String::new() });
    let result = myq_data::search_database(&mut c, &db, &request, &is_cancelled, &mut progress).await.map_err(err);
    state.running.lock().unwrap().remove(&job_id);
    result
}

#[tauri::command]
fn cancel_job(state: tauri::State<AppState>, job_id: String) {
    if let Some(flag) = state.jobs.lock().unwrap().get(&job_id) {
        flag.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

#[derive(serde::Serialize)]
struct JsonPrepared {
    /// The CSV the import continues with (a temporary file).
    csv_path: String,
    rows: u64,
    columns: Vec<String>,
    /// The text that means NULL in that CSV.
    null_text: &'static str,
}

/// Turn a JSON / JSON Lines file into a temporary CSV so that the CSV import (preview, mapping, modes) can load it. Temporary files of
/// earlier conversions older than a day are removed first.
#[tauri::command]
fn json_prepare(path: String) -> Result<JsonPrepared, String> {
    let dir = std::env::temp_dir().join("mylodb-json");
    std::fs::create_dir_all(&dir).map_err(err)?;
    for e in std::fs::read_dir(&dir).map_err(err)?.flatten() {
        if e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > std::time::Duration::from_secs(86_400)) {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let csv = dir.join(format!("{}-{stamp}.csv", std::process::id()));
    let conv = myq_transfer::json_to_csv(std::path::Path::new(&path), &csv).map_err(err)?;
    Ok(JsonPrepared { csv_path: csv.to_string_lossy().into_owned(), rows: conv.rows, columns: conv.columns, null_text: myq_transfer::NULL_TEXT })
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
    guard_writable(&state, id, "creating a table")?;
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
    let _ = state.record(id, &sql, 0, true);
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
    guard_writable(&state, id, "importing data")?;
    let mut c = conn_for(&state, id).await?;
    let guard = JobGuard::start(&state, &job_id);
    let mut progress = job_progress(app, job_id);
    let result = myq_transfer::import_csv(&mut c, std::path::Path::new(&path), &format, &plan, &mut progress, &guard.flag).await;
    let _ = state.record(id, &format!("IMPORT CSV {path} INTO {}.{}", plan.database, plan.table), 0, result.is_ok());
    result.map_err(err)
}

/// Count and sample the statements of a script without running anything (the preview before "Run script").
#[tauri::command]
async fn preview_sql_source(path: String) -> Result<myq_transfer::SourcePreview, String> {
    tokio::task::spawn_blocking(move || myq_transfer::preview_source(std::path::Path::new(&path), 50))
        .await
        .map_err(|e| e.to_string())?
        .map_err(err)
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
    guard_writable(&state, id, "running a SQL file")?;
    let result = {
        let mut c = conn_for(&state, id).await?;
        let guard = JobGuard::start(&state, &job_id);
        let mut progress = job_progress(app, job_id);
        myq_transfer::run_source(&mut c, std::path::Path::new(&path), &options, &mut progress, &guard.flag).await
    };
    let _ = state.record(id, &format!("RESTORE {path}"), 0, result.is_ok());
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
    let _ = state.record(id, &format!("BACKUP {}", options.databases.iter().map(|d| d.name.as_str()).collect::<Vec<_>>().join(", ")), 0, result.is_ok());
    result.map_err(err)
}

// ---- data compare and sync, and copying a database ----

#[derive(serde::Deserialize, Clone)]
struct DataCompareRequest {
    source_id: i64,
    source_db: String,
    source_table: String,
    target_id: i64,
    target_db: String,
    target_table: String,
    /// The key columns; empty means the source's primary key.
    #[serde(default)]
    key: Vec<String>,
    /// Compare only the rows that match this condition (applied to both tables).
    #[serde(default)]
    where_sql: Option<String>,
    #[serde(default)]
    delete_extra: bool,
}

#[derive(serde::Serialize)]
struct DataCompareResult {
    columns: Vec<String>,
    key: Vec<String>,
    same: u64,
    only_source_count: usize,
    only_target_count: usize,
    changed_count: usize,
    only_source: Vec<myq_transfer::Cells>,
    only_target: Vec<myq_transfer::Cells>,
    changed: Vec<myq_transfer::Changed>,
    statements_total: usize,
    statements: Vec<String>,
    /// Columns only one of the two tables has (not compared).
    skipped: Vec<String>,
}

const SAMPLE: usize = 100;

/// Read both tables and work out the difference. Needs a MySQL or MariaDB server on both sides.
async fn data_diff(state: &tauri::State<'_, AppState>, r: &DataCompareRequest) -> Result<(myq_transfer::Diff, Vec<String>, Vec<String>), String> {
    for id in [r.source_id, r.target_id] {
        if sqlite_file(state, id).is_some() {
            return Err("Comparing data needs MySQL or MariaDB connections on both sides.".into());
        }
    }
    let mut a = conn_for(state, r.source_id).await?;
    let mut b = conn_for(state, r.target_id).await?;
    let key = if r.key.is_empty() { myq_transfer::primary_key(&mut a, &r.source_db, &r.source_table).await.map_err(err)? } else { r.key.clone() };
    if key.is_empty() {
        return Err(format!("{}.{} has no primary key. Choose the column(s) that identify a row.", r.source_db, r.source_table));
    }
    let source = myq_transfer::read_table(&mut a, &r.source_db, &r.source_table, r.where_sql.as_deref()).await.map_err(err)?;
    let target = myq_transfer::read_table(&mut b, &r.target_db, &r.target_table, r.where_sql.as_deref()).await.map_err(err)?;
    let skipped: Vec<String> = source.cols.iter().filter(|c| !target.cols.iter().any(|t| t.name.eq_ignore_ascii_case(&c.name))).map(|c| c.name.clone()).chain(target.cols.iter().filter(|c| !source.cols.iter().any(|s| s.name.eq_ignore_ascii_case(&c.name))).map(|c| c.name.clone())).collect();
    let diff = myq_transfer::diff_tables(&source, &target, &key).map_err(err)?;
    let statements = myq_transfer::sync_statements(&diff, &format!("{}.{}", myq_core::q(&r.target_db), myq_core::q(&r.target_table)), myq_transfer::SyncOptions { delete_extra: r.delete_extra, ..Default::default() });
    Ok((diff, statements, skipped))
}

#[tauri::command]
async fn data_compare(state: tauri::State<'_, AppState>, request: DataCompareRequest) -> Result<DataCompareResult, String> {
    let (d, statements, skipped) = data_diff(&state, &request).await?;
    Ok(DataCompareResult {
        columns: d.columns.iter().map(|c| c.name.clone()).collect(),
        key: d.key.clone(),
        same: d.same,
        only_source_count: d.only_source.len(),
        only_target_count: d.only_target.len(),
        changed_count: d.changed.len(),
        only_source: d.only_source.iter().take(SAMPLE).cloned().collect(),
        only_target: d.only_target.iter().take(SAMPLE).cloned().collect(),
        changed: d.changed_view(SAMPLE),
        statements_total: statements.len(),
        statements: statements.into_iter().take(SAMPLE).collect(),
        skipped,
    })
}

#[derive(serde::Serialize)]
struct SyncApplied {
    statements: u64,
    inserted: usize,
    updated: usize,
    deleted: usize,
}

/// Make the target match the source: the comparison is run again here, and its statements are run in one transaction on the target.
#[tauri::command]
async fn data_sync_apply(state: tauri::State<'_, AppState>, request: DataCompareRequest, disable_fk_checks: bool) -> Result<SyncApplied, String> {
    guard_writable(&state, request.target_id, "changing the target table")?;
    let (d, statements, _) = data_diff(&state, &request).await?;
    if statements.is_empty() {
        return Ok(SyncApplied { statements: 0, inserted: 0, updated: 0, deleted: 0 });
    }
    let mut c = conn_for(&state, request.target_id).await?;
    let result = myq_transfer::apply_sync(&mut c, &statements, disable_fk_checks).await;
    let deleted = if request.delete_extra { d.only_target.len() } else { 0 };
    let label = format!("DATA SYNC {}.{} from {}.{}: {} inserted, {} updated, {} deleted\n{}", request.target_db, request.target_table, request.source_db, request.source_table, d.only_source.len(), d.changed.len(), deleted, statements.iter().take(20).cloned().collect::<Vec<_>>().join(";\n"));
    let _ = state.record(request.target_id, &label, 0, result.is_ok());
    let n = result.map_err(err)?;
    Ok(SyncApplied { statements: n, inserted: d.only_source.len(), updated: d.changed.len(), deleted })
}

#[derive(serde::Serialize)]
struct CopySummary {
    tables: u64,
    rows: u64,
    statements: u64,
    warnings: Vec<String>,
    elapsed_ms: u64,
}

/// Copy `source_db` of one connection into `target_db` of another (or the same) connection, as a job: a dump into a temporary
/// folder, then the dump run on the target. A table of the same name in the target is replaced.
#[tauri::command]
async fn copy_database(app: tauri::AppHandle, state: tauri::State<'_, AppState>, job_id: String, source_id: i64, source_db: String, target_id: i64, target_db: String, with_data: bool) -> Result<CopySummary, String> {
    guard_writable(&state, target_id, "copying a database into it")?;
    if source_id == target_id && source_db.eq_ignore_ascii_case(&target_db) {
        return Err("The source and the target are the same database.".into());
    }
    if target_db.trim().is_empty() {
        return Err("Name the database to copy into.".into());
    }
    for id in [source_id, target_id] {
        if sqlite_file(&state, id).is_some() {
            return Err("Copying a database needs MySQL or MariaDB connections.".into());
        }
    }
    let started = std::time::Instant::now();
    let guard = JobGuard::start(&state, &job_id);
    let mut src = conn_for(&state, source_id).await?;
    let mut dst = conn_for(&state, target_id).await?;
    // the target database: made like the source's (character set and collation) when it is not there yet
    {
        use myq_driver::mysql_async::prelude::Queryable;
        let row: Option<(String, String)> = src.query_first(format!("SHOW CREATE DATABASE {}", myq_core::q(&source_db))).await.map_err(err)?;
        let create = row.ok_or_else(|| format!("database {source_db} does not exist"))?.1;
        let create = create.replacen("CREATE DATABASE", "CREATE DATABASE IF NOT EXISTS", 1).replacen(&myq_core::q(&source_db), &myq_core::q(&target_db), 1);
        dst.query_drop(create).await.map_err(err)?;
    }
    let dir = std::env::temp_dir().join(format!("mylodb_copy_{}", job_id.replace(|c: char| !c.is_ascii_alphanumeric(), "_")));
    std::fs::create_dir_all(&dir).map_err(err)?;
    let cleanup = |dir: &std::path::Path| { let _ = std::fs::remove_dir_all(dir); };
    let options: myq_transfer::BackupOptions = serde_json::from_value(serde_json::json!({
        "databases": [{ "name": source_db }], "data": with_data, "create_database": false, "drop_if_exists": true, "compress": false, "file_per_table": false,
    })).map_err(err)?;
    let mut progress = job_progress(app.clone(), job_id.clone());
    let made = myq_transfer::backup(&mut src, &options, &myq_transfer::BackupTarget { dir: dir.clone(), name: "copy".into() }, &mut |p: myq_transfer::Progress| progress(myq_transfer::Progress { phase: format!("reading · {}", p.phase), ..p }), &guard.flag).await;
    let made = match made { Ok(m) => m, Err(e) => { cleanup(&dir); return Err(err(e)); } };
    // a view or trigger body names its database: point it at the new one
    if !source_db.eq_ignore_ascii_case(&target_db) {
        if let Err(e) = rename_database_in_dump(&made.path, &source_db, &target_db) { cleanup(&dir); return Err(e); }
    }
    let mut progress = job_progress(app, job_id.clone());
    let ran = myq_transfer::run_source(&mut dst, &made.path, &myq_transfer::RunOptions { stop_on_error: true, disable_fk_checks: true, database: Some(target_db.clone()) }, &mut |p: myq_transfer::Progress| progress(myq_transfer::Progress { phase: format!("writing · {}", p.phase), ..p }), &guard.flag).await;
    cleanup(&dir);
    let ran = ran.map_err(err);
    let _ = state.record(target_id, &format!("COPY DATABASE {source_db} -> {target_db}"), started.elapsed().as_millis() as u64, ran.is_ok());
    let ran = ran?;
    if let Some(e) = ran.errors.first() {
        return Err(format!("the copy stopped: {} (statement {})", e.message, e.statement));
    }
    let _ = reindex(&state, target_id).await;
    Ok(CopySummary { tables: made.tables, rows: made.rows, statements: ran.statements, warnings: made.warnings, elapsed_ms: started.elapsed().as_millis() as u64 })
}

/// Rewrite `` `old`. `` to `` `new`. `` in a dump file, line by line (so a large dump is not held in memory).
fn rename_database_in_dump(path: &std::path::Path, old: &str, new: &str) -> Result<(), String> {
    use std::io::{BufRead, Write};
    let from = format!("{}.", myq_core::q(old));
    let to = format!("{}.", myq_core::q(new));
    let tmp = path.with_extension("sql.renamed");
    {
        let input = std::io::BufReader::new(std::fs::File::open(path).map_err(err)?);
        let mut out = std::io::BufWriter::new(std::fs::File::create(&tmp).map_err(err)?);
        for line in input.split(b'\n') {
            let line = line.map_err(err)?;
            // row data is never touched (a text value could contain the old name); definitions are
            if line.starts_with(b"INSERT INTO") || line.starts_with(b"REPLACE INTO") {
                out.write_all(&line).map_err(err)?;
                out.write_all(b"\n").map_err(err)?;
                continue;
            }
            match String::from_utf8(line) {
                Ok(text) => out.write_all(text.replace(&from, &to).as_bytes()),
                Err(bad) => out.write_all(bad.as_bytes()),
            }.map_err(err)?;
            out.write_all(b"\n").map_err(err)?;
        }
        out.flush().map_err(err)?;
    }
    std::fs::rename(tmp, path).map_err(err)
}

// ---- the audit log ----

#[derive(serde::Serialize)]
struct AuditStatus {
    enabled: bool,
    path: String,
    size: u64,
}

#[tauri::command]
fn audit_status(state: tauri::State<'_, AppState>) -> AuditStatus {
    AuditStatus { enabled: state.audit.enabled(), path: state.audit.path().to_string_lossy().into_owned(), size: state.audit.size() }
}

#[tauri::command]
fn audit_set(state: tauri::State<'_, AppState>, enabled: bool) -> Result<AuditStatus, String> {
    state.audit.set_enabled(enabled).map_err(err)?;
    Ok(audit_status(state))
}

#[tauri::command]
fn audit_tail(state: tauri::State<'_, AppState>, limit: usize) -> Vec<audit::Entry> {
    state.audit.tail(limit.clamp(1, 500))
}

// ---- where is this used ----

/// The statements that point the views mentioning a table at its new name, for the rename dialog to run right after the rename.
/// (SQLite updates its views itself when a table is renamed.)
#[tauri::command]
async fn rename_view_statements(state: tauri::State<'_, AppState>, id: i64, db: String, from: String, to: String) -> Result<Vec<String>, String> {
    if sqlite_file(&state, id).is_some() {
        return Ok(vec![]);
    }
    let mut c = conn_for(&state, id).await?;
    myq_admin::rename_view_statements(&mut c, &db, &from, &to).await.map_err(err)
}

/// The views, routines, triggers, events and foreign keys that mention a table (or one of its columns). A rename does not change them.
#[tauri::command]
async fn find_usages(state: tauri::State<'_, AppState>, id: i64, db: String, table: String, column: Option<String>) -> Result<Vec<myq_admin::Usage>, String> {
    let engine = state.sessions.lock().unwrap().get(&id).ok_or("not connected")?.engine.clone();
    if engine.sqlite_path().is_some() {
        let res = engine.run(None, "SELECT type, name, tbl_name, sql FROM sqlite_master WHERE sql IS NOT NULL").await.map_err(err)?;
        let rows: Vec<Vec<String>> = res.iter().flat_map(|r| r.sets.iter()).flat_map(|s| s.rows.iter()).map(|r| r.iter().map(|c| c.clone().unwrap_or_default()).collect()).collect();
        let mut out = Vec::new();
        let mut defs = Vec::new();
        for r in rows.into_iter().filter(|r| r.len() == 4) {
            let (kind, name, on, sql) = (r[0].clone(), r[1].clone(), r[2].clone(), r[3].clone());
            match kind.as_str() {
                "index" if on.eq_ignore_ascii_case(&table) && !name.starts_with("sqlite_") && column.as_deref().map_or(true, |c| myq_admin::mentions(&sql, c)) => out.push(myq_admin::Usage { kind: "index".into(), db: db.clone(), name, detail: sql.lines().next().unwrap_or("").trim().to_string() }),
                "table" if !name.eq_ignore_ascii_case(&table) && myq_admin::mentions(&sql, &table) => out.push(myq_admin::Usage { kind: "foreign key (points here)".into(), db: db.clone(), name: name.clone(), detail: myq_admin::first_mention(&sql, &table).unwrap_or_default() }),
                "view" | "trigger" => defs.push((kind, db.clone(), name, sql)),
                _ => {}
            }
        }
        out.extend(myq_admin::filter_definitions(defs, &db, &table, column.as_deref()));
        return Ok(out);
    }
    let mut c = conn_for(&state, id).await?;
    myq_admin::find_usages(&mut c, &db, &table, column.as_deref()).await.map_err(err)
}

// ---- scheduled backups ----

#[derive(serde::Serialize, Clone)]
struct ScheduleView {
    #[serde(flatten)]
    job: schedule::Schedule,
    /// Local time of the next run, `YYYY-MM-DD HH:MM:SS`.
    next_run: Option<String>,
    running: bool,
}

fn now_local() -> chrono::NaiveDateTime {
    chrono::Local::now().naive_local()
}

fn schedule_views(state: &AppState) -> Vec<ScheduleView> {
    let now = now_local();
    let running = state.schedule_running.lock().unwrap().clone();
    state.schedules.lock().unwrap().iter().map(|s| ScheduleView { next_run: schedule::next_run(s, now).map(schedule::format_time), running: running.contains(&s.id), job: s.clone() }).collect()
}

#[tauri::command]
fn schedules_list(state: tauri::State<'_, AppState>) -> Vec<ScheduleView> {
    schedule_views(&state)
}

/// Create (empty `id`) or change a scheduled backup. A change keeps its log and last run.
#[tauri::command]
fn schedule_save(state: tauri::State<'_, AppState>, mut job: schedule::Schedule) -> Result<Vec<ScheduleView>, String> {
    schedule::valid(&job)?;
    {
        let mut list = state.schedules.lock().unwrap();
        // two jobs with one name in one folder would delete each other's files when they trim to the last N
        if list.iter().any(|s| s.id != job.id && s.name.trim().eq_ignore_ascii_case(job.name.trim()) && s.dir == job.dir) {
            return Err("Another job already uses this name in this folder. Give it a different name.".into());
        }
        match list.iter_mut().find(|s| !job.id.is_empty() && s.id == job.id) {
            Some(old) => {
                job.created = old.created.clone();
                job.last_run = old.last_run.clone();
                job.log = old.log.clone();
                *old = job;
            }
            None => {
                job.id = format!("{:x}", chrono::Local::now().timestamp_micros());
                job.created = schedule::format_time(now_local());
                job.last_run = None;
                job.log.clear();
                list.push(job);
            }
        }
        schedule::save(&state.schedule_file, &list).map_err(err)?;
    }
    Ok(schedule_views(&state))
}

#[tauri::command]
fn schedule_delete(state: tauri::State<'_, AppState>, id: String) -> Result<Vec<ScheduleView>, String> {
    {
        let mut list = state.schedules.lock().unwrap();
        list.retain(|s| s.id != id);
        schedule::save(&state.schedule_file, &list).map_err(err)?;
    }
    Ok(schedule_views(&state))
}

/// Run one scheduled backup now (also what the scheduler calls). Opens the connection if it is not open. Returns the log line.
async fn run_schedule(app: tauri::AppHandle, id: String) -> Result<String, String> {
    let state = app.state::<AppState>();
    if !state.schedule_running.lock().unwrap().insert(id.clone()) {
        return Err("This job is already running.".into());
    }
    let job = state.schedules.lock().unwrap().iter().find(|s| s.id == id).cloned();
    let result: Result<String, String> = async {
        let job = job.ok_or("The job no longer exists.")?;
        if !state.sessions.lock().unwrap().contains_key(&job.connection_id) {
            connect(app.state::<AppState>(), job.connection_id, None, None).await.map_err(|e| match e {
                ConnectError::Failed { message } => format!("could not connect: {message}"),
                ConnectError::UnknownHost { host, .. } | ConnectError::HostKeyChanged { host, .. } => format!("could not connect: the SSH host key of {host} is not confirmed. Connect once by hand and trust it"),
            })?;
        }
        std::fs::create_dir_all(&job.dir).map_err(|e| format!("cannot use the folder {}: {e}", job.dir))?;
        if let Some(file) = sqlite_file(&state, job.connection_id) {
            // a SQLite file: a backup is a consistent copy made by VACUUM INTO; an export streams the rows
            let stamp = now_local().format("%Y%m%d-%H%M%S").to_string();
            let safe: String = job.name.chars().map(|c| if c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ' ') { c } else { '_' }).collect();
            let started = std::time::Instant::now();
            let (path, ext) = if job.kind == "export" {
                let ext = schedule::export_extension(&job.format).ok_or("unknown file format")?;
                (std::path::Path::new(&job.dir).join(format!("{}-{stamp}.{ext}", safe.trim())), ext)
            } else {
                (std::path::Path::new(&job.dir).join(format!("{}-{stamp}.sqlite", safe.trim())), "sqlite")
            };
            let (p2, j2) = (path.clone(), job.clone());
            let rows = tokio::task::spawn_blocking(move || -> Result<Option<u64>, String> {
                if j2.kind == "export" {
                    let options: myq_data::ExportOptions = serde_json::from_value(serde_json::json!({ "format": j2.format, "name": j2.name })).map_err(err)?;
                    myq_sqlite::export_query(&file, &j2.sql, &options, myq_data::Dest::File(&p2), &mut |_| true).map(|o| Some(o.rows))
                } else {
                    myq_sqlite::backup_to(&file, &p2).map(|_| None)
                }
            })
            .await
            .map_err(err)?;
            let rows = match rows { Ok(r) => r, Err(e) => { let _ = std::fs::remove_file(&path); return Err(e); } };
            let removed = myq_transfer::prune_files(std::path::Path::new(&job.dir), &job.name, job.keep as usize, &[ext], false).unwrap_or_default().len();
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            return Ok(format!("{}: {}{:.1} MB in {:.1} s{}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), rows.map(|r| format!("{r} rows, ")).unwrap_or_default(), size as f64 / 1_048_576.0, started.elapsed().as_secs_f64(), if removed > 0 { format!("; removed {removed} old file(s)") } else { String::new() }));
        }
        let mut c = conn_for(&state, job.connection_id).await?;
        if job.kind == "export" {
            // the result of one SELECT, written to <name>-<stamp>.<ext>; older files of this job beyond the limit are removed
            let ext = schedule::export_extension(&job.format).ok_or("unknown file format")?;
            let stamp = now_local().format("%Y%m%d-%H%M%S");
            let safe: String = job.name.chars().map(|c| if c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ' ') { c } else { '_' }).collect();
            let path = std::path::Path::new(&job.dir).join(format!("{}-{stamp}.{ext}", safe.trim()));
            let options: myq_data::ExportOptions = serde_json::from_value(serde_json::json!({ "format": job.format, "name": job.name })).map_err(err)?;
            let started = std::time::Instant::now();
            let out = myq_data::export_query(&mut c, job.databases.first().map(String::as_str), &job.sql, &options, myq_data::Dest::File(&path), &mut |_rows: u64| true).await;
            let out = match out { Ok(o) => o, Err(e) => { let _ = std::fs::remove_file(&path); return Err(err(e)); } };
            let removed = myq_transfer::prune_files(std::path::Path::new(&job.dir), &job.name, job.keep as usize, &[ext], false).unwrap_or_default().len();
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            return Ok(format!("{}: {} rows, {:.1} MB in {:.1} s{}", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), out.rows, size as f64 / 1_048_576.0, started.elapsed().as_secs_f64(), if removed > 0 { format!("; removed {removed} old file(s)") } else { String::new() }));
        }
        let options: myq_transfer::BackupOptions = serde_json::from_value(serde_json::json!({
            "databases": job.databases.iter().map(|d| serde_json::json!({ "name": d })).collect::<Vec<_>>(),
            "data": job.data, "compress": job.compress,
        })).map_err(err)?;
        let target = myq_transfer::BackupTarget { dir: job.dir.clone().into(), name: job.name.clone() };
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let done = myq_transfer::backup(&mut c, &options, &target, &mut |_p: myq_transfer::Progress| {}, &cancel).await.map_err(err)?;
        let removed = myq_transfer::prune_backups(std::path::Path::new(&job.dir), &job.name, job.keep as usize).unwrap_or_default().len();
        let name = done.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(format!("{name}: {} table(s), {} rows, {:.1} MB in {:.1} s{}", done.tables, done.rows, done.bytes as f64 / 1_048_576.0, done.elapsed_ms as f64 / 1000.0, if removed > 0 { format!("; removed {removed} old backup(s)") } else { String::new() }))
    }.await;
    {
        let now = now_local();
        let mut list = state.schedules.lock().unwrap();
        if let Some(s) = list.iter_mut().find(|s| s.id == id) {
            s.last_run = Some(schedule::format_time(now));
            schedule::push_log(s, now, result.is_ok(), match &result { Ok(m) => m.clone(), Err(e) => e.clone() });
        }
        let _ = schedule::save(&state.schedule_file, &list);
    }
    state.schedule_running.lock().unwrap().remove(&id);
    let name = state.schedules.lock().unwrap().iter().find(|s| s.id == id).map(|s| s.name.clone()).unwrap_or_default();
    let _ = app.emit("schedule-done", serde_json::json!({ "name": name, "ok": result.is_ok(), "message": match &result { Ok(m) => m.clone(), Err(e) => e.clone() } }));
    result
}

#[tauri::command]
async fn schedule_run_now(app: tauri::AppHandle, id: String) -> Result<String, String> {
    run_schedule(app, id).await
}

/// Look for due jobs every 30 seconds while the app is open.
fn start_scheduler(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            let due: Vec<String> = {
                let state = app.state::<AppState>();
                let running = state.schedule_running.lock().unwrap().clone();
                let now = now_local();
                let ids: Vec<String> = state.schedules.lock().unwrap().iter().filter(|s| schedule::is_due(s, now) && !running.contains(&s.id)).map(|s| s.id.clone()).collect();
                ids
            };
            for id in due {
                let _ = run_schedule(app.clone(), id).await; // the outcome is in the job's log
            }
        }
    });
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
    guard_writable(&state, id, "changing a server setting")?;
    let mut c = conn_for(&state, id).await?;
    let result = myq_admin::set_variable(&mut c, myq_admin::VarScope::Global, &name, &value).await;
    let _ = state.record(id, &format!("SET GLOBAL {name} = {value}"), 0, result.is_ok());
    result.map_err(err)
}

#[tauri::command]
async fn maintain_tables(state: tauri::State<'_, AppState>, id: i64, db: String, tables: Vec<String>, operation: myq_admin::Maintenance) -> Result<Vec<myq_admin::MaintenanceRow>, String> {
    guard_writable(&state, id, "table maintenance")?;
    let mut c = conn_for(&state, id).await?;
    let result = myq_admin::maintain(&mut c, &db, &tables, operation).await;
    let _ = state.record(id, &format!("{operation:?} {db}: {}", tables.join(", ")), 0, result.is_ok());
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
async fn top_statements(state: tauri::State<'_, AppState>, id: i64, sort: String, limit: u32, search: String) -> Result<Vec<myq_admin::TopStatement>, String> {
    let mut c = conn_for(&state, id).await?;
    myq_admin::top_statements(&mut c, &sort, limit, &search).await.map_err(err)
}

/// Upkeep of a SQLite file: integrity and foreign key checks (read only), VACUUM, ANALYZE, REINDEX and PRAGMA optimize (these change the file).
#[tauri::command]
async fn sqlite_maintain(state: tauri::State<'_, AppState>, id: i64, op: String) -> Result<String, String> {
    let engine = state.sessions.lock().unwrap().get(&id).ok_or("not connected")?.engine.clone();
    if engine.sqlite_path().is_none() {
        return Err("This is only for SQLite files.".into());
    }
    let (sql, writes, label) = match op.as_str() {
        "integrity" => ("PRAGMA integrity_check", false, "Integrity check"),
        "quick" => ("PRAGMA quick_check", false, "Quick check"),
        "foreign_keys" => ("PRAGMA foreign_key_check", false, "Foreign key check"),
        "vacuum" => ("VACUUM", true, "VACUUM"),
        "analyze" => ("ANALYZE", true, "ANALYZE"),
        "reindex" => ("REINDEX", true, "REINDEX"),
        "optimize" => ("PRAGMA optimize", true, "PRAGMA optimize"),
        _ => return Err(format!("unknown action {op}")),
    };
    if writes && !engine.sqlite_writable() {
        return Err(format!("{label} changes the file, and this connection is read-only."));
    }
    let size = |e: Engine| async move {
        let one = |r: Vec<myq_core::StatementResult>| r.first().and_then(|s| s.sets.first()).and_then(|s| s.rows.first()).and_then(|r| r.first().cloned().flatten()).and_then(|v| v.parse::<u64>().ok());
        let pages = one(e.run(None, "PRAGMA page_count").await.ok()?)?;
        let page = one(e.run(None, "PRAGMA page_size").await.ok()?)?;
        Some(pages * page)
    };
    let before = if op == "vacuum" { size(engine.clone()).await } else { None };
    let res = engine.run(None, sql).await.map_err(err)?;
    if let Some(e) = res.iter().find_map(|r| r.error.clone()) {
        return Err(format!("{label}: {e}"));
    }
    let rows: Vec<String> = res.iter().flat_map(|r| r.sets.iter()).flat_map(|s| s.rows.iter()).map(|r| r.iter().map(|c| c.clone().unwrap_or_default()).collect::<Vec<_>>().join(" | ")).collect();
    let mb = |b: u64| format!("{:.2} MB", b as f64 / 1_048_576.0);
    Ok(match op.as_str() {
        "integrity" | "quick" if rows.len() == 1 && rows[0] == "ok" => format!("{label}: ok. The file is sound."),
        "integrity" | "quick" => format!("{label} found {} problem(s): {}", rows.len(), rows.iter().take(5).cloned().collect::<Vec<_>>().join("; ")),
        "foreign_keys" if rows.is_empty() => format!("{label}: no violations."),
        "foreign_keys" => format!("{label}: {} row(s) point at a missing parent (table | rowid | parent | key): {}", rows.len(), rows.iter().take(5).cloned().collect::<Vec<_>>().join("; ")),
        "vacuum" => match (before, size(engine.clone()).await) {
            (Some(a), Some(b)) => format!("VACUUM done: {} → {}{}", mb(a), mb(b), if b < a { format!(" ({} freed)", mb(a - b)) } else { String::new() }),
            _ => "VACUUM done.".into(),
        },
        _ => format!("{label} done."),
    })
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
/// Load, once each, what autocomplete and the error underlining need for the databases a statement names: their columns and foreign keys.
async fn load_completion_data(state: &AppState, id: i64, sql: &str, database: Option<&str>) -> Result<(), String> {
    let (engine, wanted): (Engine, Vec<String>) = {
        let sessions = state.sessions.lock().unwrap();
        let s = sessions.get(&id).ok_or("not connected")?;
        let mut w: Vec<String> = database.map(String::from).into_iter().collect();
        for word in sql.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$')) {
            if let Some(db) = s.by_db.keys().find(|d| d.eq_ignore_ascii_case(word)) {
                if !w.contains(db) {
                    w.push(db.clone());
                }
            }
        }
        (s.engine.clone(), w.into_iter().filter(|d| s.by_db.contains_key(d) && !s.columns_loaded.contains(d) && !s.completion_loaded.contains(d)).collect())
    };
    for db in wanted {
        if let Ok(cols) = engine.column_nodes(&db).await {
            if let Some(s) = state.sessions.lock().unwrap().get_mut(&id) {
                s.add_completion_columns(&db, cols);
            }
        }
    }
    // foreign keys of the same databases, once each, for the join suggestions; a failure only means no suggestions
    let (pool, fk_wanted): (Option<Pool>, Vec<String>) = {
        let sessions = state.sessions.lock().unwrap();
        let s = sessions.get(&id).ok_or("not connected")?;
        let mut w: Vec<String> = database.map(String::from).into_iter().collect();
        for word in sql.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$')) {
            if let Some(db) = s.by_db.keys().find(|d| d.eq_ignore_ascii_case(word)) {
                if !w.contains(db) { w.push(db.clone()); }
            }
        }
        (s.pool.clone(), w.into_iter().filter(|d| s.by_db.contains_key(d) && !s.fks_loaded.contains(d)).collect())
    };
    if let Some(pool) = pool {
        for db in fk_wanted {
            let rels = myq_driver::foreign_keys(&pool, &db).await.unwrap_or_default();
            if let Some(s) = state.sessions.lock().unwrap().get_mut(&id) {
                if s.fks_loaded.insert(db.clone()) {
                    s.completion_fks.extend(rels.into_iter().map(|r| myq_sql::Fk { name: r.name, db: db.clone(), table: r.table, columns: r.columns, ref_db: r.ref_schema, ref_table: r.ref_table, ref_columns: r.ref_columns }));
                    s.rebuild_catalog();
                }
            }
        }
    }
    Ok(())
}

#[tauri::command]
async fn complete_sql(state: tauri::State<'_, AppState>, id: i64, sql: String, cursor: usize, database: Option<String>) -> Result<myq_sql::Completions, String> {
    load_completion_data(&state, id, &sql, database.as_deref()).await?;
    let sessions = state.sessions.lock().unwrap();
    let s = sessions.get(&id).ok_or("not connected")?;
    Ok(myq_sql::complete(&sql, cursor, &s.catalog, database.as_deref(), 100))
}

/// Ask the configured AI service to write a query from a plain-language request (a chat: earlier turns give it context).
/// Only table and column names of `database` are sent. If the chosen model cannot take the question, the next model of the list is tried.
#[tauri::command]
async fn ai_chat(state: tauri::State<'_, AppState>, id: i64, database: Option<String>, history: Vec<ai::Turn>, request: String, settings: ai::AiSettings) -> Result<ai::Answer, String> {
    let db = database.filter(|d| !d.is_empty()).ok_or("Select a database first: the assistant needs to know which tables you mean.")?;
    if request.trim().is_empty() {
        return Err("Describe what you want to find or change.".into());
    }
    // load the columns and foreign keys of that database once, then read them from the catalog
    load_completion_data(&state, id, &db, Some(&db)).await?;
    let (schema, dialect) = {
        let sessions = state.sessions.lock().unwrap();
        let s = sessions.get(&id).ok_or("not connected")?;
        (ai::schema_text(&s.catalog, &db), if s.pool.is_some() { "MySQL / MariaDB" } else { "SQLite" })
    };
    if schema.is_empty() {
        return Err(format!("`{db}` has no tables the assistant can see."));
    }
    let key = state.secrets.get(ai::key_id(&settings.preset), myq_store::Slot::AiKey).map_err(err)?;
    ai::chat(&settings, key.as_deref(), ai::messages(dialect, &db, &schema, &history, request.trim())).await
}

/// The models a service offers (for OpenRouter only the free ones unless `all`). Also how a key is checked when the user connects.
#[tauri::command]
async fn ai_models(state: tauri::State<'_, AppState>, preset: String, endpoint: String, all: bool) -> Result<Vec<ai::ModelInfo>, String> {
    let key = state.secrets.get(ai::key_id(&preset), myq_store::Slot::AiKey).map_err(err)?;
    ai::list_models(&preset, &endpoint, key.as_deref(), all).await
}

#[tauri::command]
fn ai_key_status(state: tauri::State<'_, AppState>, preset: String) -> Result<bool, String> {
    Ok(state.secrets.get(ai::key_id(&preset), myq_store::Slot::AiKey).map_err(err)?.is_some())
}

#[tauri::command]
fn ai_set_key(state: tauri::State<'_, AppState>, preset: String, key: String) -> Result<(), String> {
    let (key, id) = (key.trim(), ai::key_id(&preset));
    if key.is_empty() { state.secrets.delete(id, myq_store::Slot::AiKey).map_err(err) } else { state.secrets.set(id, myq_store::Slot::AiKey, key).map_err(err) }
}

/// Problems in the editor text found without running it (unclosed brackets or quotes, unknown tables and columns), for underlining.
#[tauri::command]
async fn diagnose_sql(state: tauri::State<'_, AppState>, id: i64, sql: String, database: Option<String>) -> Result<Vec<myq_sql::Diagnostic>, String> {
    load_completion_data(&state, id, &sql, database.as_deref()).await?;
    let sessions = state.sessions.lock().unwrap();
    let s = sessions.get(&id).ok_or("not connected")?;
    Ok(myq_sql::diagnose(&sql, &s.catalog, database.as_deref()))
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
    if let Some(file) = sqlite_file(&state, id) {
        let def = tokio::task::spawn_blocking(move || myq_sqlite::read_table_def(&file, &table)).await.map_err(err)??;
        return Ok(TableDefResult { def, warnings: vec![] });
    }
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
fn ddl_preview(db: String, original: Option<TableDef>, desired: TableDef, sqlite: Option<bool>) -> DdlPreview {
    let result = if sqlite == Some(true) { myq_sqlite::ddl_statements(original.as_ref(), &desired) } else { ddl_statements(&db, &original, &desired) };
    match result {
        Ok(statements) => DdlPreview { statements, errors: vec![] },
        Err(errors) => DdlPreview { statements: vec![], errors },
    }
}

/// Re-read the object index after the schema changed, so the tree and the filter see it. Databases whose columns
/// were loaded get them again, so what the UI already shows stays in step.
async fn reindex(state: &tauri::State<'_, AppState>, id: i64) -> Result<Vec<Node>, String> {
    let (engine, loaded, only) = {
        let sessions = state.sessions.lock().unwrap();
        let s = sessions.get(&id).ok_or("not connected")?;
        let mut loaded: Vec<String> = s.columns_loaded.iter().cloned().collect();
        loaded.sort();
        (s.engine.clone(), loaded, s.only_database.clone())
    };
    let mut index = restrict_databases(engine.index(false).await.map_err(err)?, only.as_deref());
    let mut still = std::collections::HashSet::new();
    for db in loaded {
        if index.iter().any(|n| n.database == db) {
            index.extend(engine.column_nodes(&db).await.map_err(err)?);
            still.insert(db);
        }
    }
    let mut sessions = state.sessions.lock().unwrap();
    if let Some(s) = sessions.get_mut(&id) {
        s.completion_nodes.clear();
        s.completion_loaded.clear(); s.fks_loaded.clear(); s.completion_fks.clear();
        s.columns_loaded = still;
        s.set_index(index.clone());
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
    guard_writable(&state, id, "changing a table definition")?;
    if let Some((file, writable)) = sqlite_target(&state, id) {
        if !writable {
            return Err("This SQLite file is open read-only.".into());
        }
        let statements = myq_sqlite::ddl_statements(original.as_ref(), &desired).map_err(|e| e.join("; "))?;
        if statements.is_empty() {
            return Ok(statements);
        }
        let list = statements.clone();
        let result = tokio::task::spawn_blocking(move || myq_sqlite::apply_ddl(&file, &list)).await.map_err(err)?;
        let _ = state.record(id, &statements.join(";\n"), 0, result.is_ok());
        result?;
        reindex(&state, id).await?;
        return Ok(statements);
    }
    let statements = ddl_statements(&db, &original, &desired).map_err(|e| e.join("; "))?;
    if statements.is_empty() {
        return Ok(statements);
    }
    let mut c = conn_for(&state, id).await?;
    let result = myq_admin::run_statements(&mut c, &statements).await;
    let _ = state.record(id, &statements.join(";\n"), 0, result.is_ok());
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
    guard_writable(&state, id, "changing a table")?;
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
    let _ = state.record(id, &statements.join(";\n"), 0, result.is_ok());
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
    guard_writable(&state, id, "creating or dropping a database")?;
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
    let _ = state.record(id, &sql, 0, result.is_ok());
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
    let mut c = conn_for(&state, id).await?;
    let o = myq_driver::server_options(&mut c).await.map_err(err)?;
    Ok(Collations { engines: o.engines, charsets: o.charsets })
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
    guard_writable(&state, id, "changing an account")?;
    let mut c = conn_for(&state, id).await?;
    let sql = account_statements(&mut c, &original, &edit, &desired).await?;
    myq_admin::run_sql(&mut c, &sql).await.map_err(|(i, m)| format!("statement {} failed: {m}", i + 1))
}

#[tauri::command]
async fn account_drop(state: tauri::State<'_, AppState>, id: i64, user: String, host: String) -> Result<(), String> {
    guard_writable(&state, id, "dropping an account")?;
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

/// Read a text file the user picked (File > Open SQL File). Capped, so a wrong pick cannot freeze the editor.
#[tauri::command]
async fn read_text_file(path: String) -> Result<String, String> {
    const MAX: u64 = 32 * 1024 * 1024;
    let meta = std::fs::metadata(&path).map_err(|e| format!("{path}: {e}"))?;
    if meta.len() > MAX {
        return Err(format!("{path} is {} MB: too large for the editor (limit 32 MB). Use Tools > Run SQL file for big scripts.", meta.len() / 1_048_576));
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    String::from_utf8(bytes).map_err(|_| format!("{path} is not UTF-8 text"))
}

/// Write the editor's text to a file the user chose (File > Save As).
#[tauri::command]
async fn write_text_file(path: String, content: String) -> Result<(), String> {
    std::fs::write(&path, content).map_err(|e| format!("{path}: {e}"))
}

/// The native menu bar. Every custom item just tells the window what was chosen (`menu-action` with the item id): what it does
/// depends on the open connection and tabs, which only the UI knows. Shortcuts are handled by the window itself (so they are not
/// registered here twice). Standard Edit items use the system's own handlers.
fn build_menu(app: &tauri::AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
    // Items with an icon use the same line icons as the window (rendered by scripts/menu-icons.mjs).
    macro_rules! icon {
        ($name:literal) => {
            tauri::image::Image::from_bytes(include_bytes!(concat!("../icons/menu/", $name, ".png")))?
        };
    }
    let item = |id: &str, label: &str, accel: Option<&str>| {
        let b = MenuItemBuilder::with_id(id, label);
        match accel { Some(a) => b.accelerator(a).build(app), None => b.build(app) }
    };
    let with_icon = |id: &str, label: &str, icon: tauri::image::Image<'_>| tauri::menu::IconMenuItemBuilder::with_id(id, label).icon(icon).build(app);
    let app_menu = SubmenuBuilder::new(app, "MyLoDB")
        .item(&with_icon("about", "About MyLoDB", icon!("info"))?)
        .separator()
        .item(&with_icon("preferences", "Settings…", icon!("settings"))?)
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;
    let file = SubmenuBuilder::new(app, "File")
        .item(&with_icon("new_query", "New SQL Script", icon!("terminal"))?)
        .item(&with_icon("open_file", "Open SQL File…", icon!("folder"))?)
        .item(&with_icon("save_file", "Save", icon!("save"))?)
        .item(&with_icon("save_file_as", "Save As…", icon!("save"))?)
        .item(&with_icon("close_tab", "Close Tab", icon!("close"))?)
        .separator()
        .item(&with_icon("refresh_tree", "Refresh", icon!("refresh"))?)
        .separator()
        .item(&with_icon("import", "Import Data…", icon!("import"))?)
        .item(&with_icon("export", "Export Data…", icon!("export"))?)
        .item(&with_icon("backup", "Backup / Restore…", icon!("transfer"))?)
        .item(&item("schedules", "Scheduled Backups…", None)?)
        .separator()
        .item(&item("export_share", "Export Connections and Snippets…", None)?)
        .item(&item("import_share", "Import Connections and Snippets…", None)?)
        .separator()
        .item(&with_icon("run_file", "Run SQL File…", icon!("play"))?)
        .build()?;
    let edit = SubmenuBuilder::new(app, "Edit")
        .item(&PredefinedMenuItem::undo(app, None)?)
        .item(&PredefinedMenuItem::redo(app, None)?)
        .separator()
        .item(&PredefinedMenuItem::cut(app, None)?)
        .item(&PredefinedMenuItem::copy(app, None)?)
        .item(&PredefinedMenuItem::paste(app, None)?)
        .item(&PredefinedMenuItem::select_all(app, None)?)
        .separator()
        .item(&with_icon("format", "Format SQL", icon!("sql"))?)
        .build()?;
    let search = SubmenuBuilder::new(app, "Search")
        .item(&with_icon("focus_filter", "Search Objects (DB Metadata)", icon!("search"))?)
        .item(&with_icon("search_data", "Search Data in Database…", icon!("search"))?)
        .item(&with_icon("palette", "Command Palette…", icon!("terminal"))?)
        .build()?;
    let sql_editor = SubmenuBuilder::new(app, "SQL Editor")
        .item(&with_icon("new_query", "New SQL Script", icon!("terminal"))?)
        .item(&with_icon("new_query_db", "New Script in Selected Database", icon!("terminal"))?)
        .item(&with_icon("builder", "Visual Query Builder…", icon!("sql"))?)
        .item(&with_icon("er", "ER Diagram of Database…", icon!("diagram"))?)
        .item(&with_icon("open_file", "Open SQL File…", icon!("folder"))?)
        .separator()
        .item(&with_icon("run", "Execute", icon!("play"))?)
        .item(&with_icon("preview", "Preview Before Running…", icon!("preview"))?)
        .item(&with_icon("explain", "Explain Plan", icon!("info"))?)
        .item(&with_icon("format", "Format SQL", icon!("sql"))?)
        .build()?;
    let database = SubmenuBuilder::new(app, "Database")
        .item(&with_icon("new_connection", "New Database Connection…", icon!("plus"))?)
        .item(&with_icon("edit_connection", "Edit Connection…", icon!("edit"))?)
        .separator()
        .item(&with_icon("disconnect", "Disconnect", icon!("stop"))?)
        .separator()
        .item(&with_icon("home", "Server Home", icon!("home"))?)
        .item(&with_icon("databases", "Databases…", icon!("database"))?)
        .item(&with_icon("structure", "Tables of Selected Database", icon!("table"))?)
        .item(&with_icon("new_table", "New Table…", icon!("plus"))?)
        .item(&with_icon("compare", "Compare Schemas…", icon!("transfer"))?)
        .item(&item("data_compare", "Compare Data of Two Tables…", None)?)
        .item(&item("copy_database", "Copy Database to Another Connection…", None)?)
        .item(&with_icon("advisor", "Index Advisor…", icon!("table"))?)
        .separator()
        .item(&SubmenuBuilder::new(app, "Tools")
            .item(&with_icon("users", "Users and Privileges", icon!("user"))?)
            .item(&with_icon("server_tools", "Server Tools", icon!("pulse"))?)
            .item(&with_icon("catalog", "Engines, Charsets, Replication", icon!("info"))?)
            .build()?)
        .build()?;
    let ask_ai = SubmenuBuilder::new(app, "Ask AI")
        .item(&item("ask_ai", "Show or Hide the Ask AI Panel", None)?)
        .item(&item("ai_settings", "AI Settings…", None)?)
        .build()?;
    let view = SubmenuBuilder::new(app, "View")
        .item(&item("toggle_sidebar", "Toggle Sidebar", Some("CmdOrCtrl+B"))?)
        .separator()
        .item(&with_icon("theme_system", "Theme: System", icon!("auto"))?)
        .item(&with_icon("theme_light", "Theme: Light", icon!("sun"))?)
        .item(&with_icon("theme_dark", "Theme: Dark", icon!("moon"))?)
        .separator()
        .fullscreen()
        .build()?;
    let window = SubmenuBuilder::new(app, "Window").minimize().maximize().separator().close_window().build()?;
    let help = SubmenuBuilder::new(app, "Help")
        .item(&with_icon("manual", "User Manual", icon!("info"))?)
        .item(&with_icon("tour", "Quick Tour", icon!("play"))?)
        .item(&item("about_shortcuts", "Keyboard Shortcuts", None)?)
        .separator()
        .item(&item("check_updates", "Check for Updates…", None)?)
        .item(&with_icon("about", "About MyLoDB", icon!("info"))?)
        .build()?;
    MenuBuilder::new(app).items(&[&app_menu, &file, &edit, &search, &sql_editor, &database, &ask_ai, &view, &window, &help]).build()
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .menu(build_menu)
        .on_menu_event(|app, event| {
            use tauri::Emitter;
            let _ = app.emit("menu-action", event.id().0.as_str());
        })
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let secrets = std::sync::Arc::new(SecretRouter::open(&dir, Box::new(Keychain::default()), Vault::new(&dir.join("secrets.json"))));
            let store = Store::open(&dir.join("mylodb.sqlite"), Box::new(secrets.clone()))?;
            app.manage(AppState { store: Mutex::new(store), secrets, sessions: Mutex::new(HashMap::new()), running: Mutex::new(HashMap::new()), sqlite_runs: Mutex::new(HashMap::new()), jobs: Mutex::new(HashMap::new()), schedules: Mutex::new(schedule::load(&dir.join("schedules.json"))), schedule_file: dir.join("schedules.json"), schedule_running: Mutex::new(Default::default()), audit: audit::Audit::open(&dir) });
            start_scheduler(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_url,
            json_prepare,
            index_advice,
            er_model,
            build_query,
            schema_compare,
            search_data,
            capabilities,
            list_connections,
            save_connection,
            delete_connection,
            secrets_status,
            secrets_unlock,
            secrets_use_file,
            secrets_use_keychain,
            secrets_change_password,
            secrets_reset,
            connect,
            disconnect,
            filter_index,
            load_columns,
            db_table_stats,
            table_index_details,
            table_overview,
            object_definition,
            drop_object,
            table_column_details,
            run_query,
            preview_sql_source,
            server_catalog,
            read_text_file,
            write_text_file,
            database_list,
            database_sizes,
            database_tables,
            server_home,
            build_plan,
            run_plan,
            review_sql,
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
            table_count_estimate,
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
            open_connections,
            table_op,
            database_op,
            server_options,
            accounts_list,
            account_grants,
            account_preview,
            account_apply,
            account_drop,
            complete_sql,
            diagnose_sql,
            ai_chat,
            ai_models,
            ai_key_status,
            ai_set_key,
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
            top_statements,
            sqlite_maintain,
            find_usages,
            rename_view_statements,
            data_compare,
            data_sync_apply,
            copy_database,
            audit_status,
            audit_set,
            audit_tail,
            schedules_list,
            schedule_save,
            schedule_delete,
            schedule_run_now,
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

#[cfg(test)]
mod session_tests {
    use super::*;

    #[test]
    fn only_the_authors_links_can_be_opened() {
        assert!(link_allowed("https://farookhridoy.com"));
        assert!(link_allowed("https://thelarasoft.com/"));
        assert!(link_allowed("https://github.com/farookhridoy/mylodb/releases/latest"));
        assert!(!link_allowed("https://farookhridoy.com.evil.example"));
        assert!(!link_allowed("https://evil.example/https://farookhridoy.com"));
        assert!(!link_allowed("file:///etc/passwd"));
        assert!(!link_allowed("https://github.com/farookhridoyx"));
    }

    use myq_schema::filter::Kind;

    fn node(db: &str, kind: Kind, name: &str, parent: Option<&str>) -> Node {
        Node { database: db.into(), kind, name: name.into(), parent: parent.map(String::from) }
    }

    fn session() -> Session {
        // a pool opens no connection until it is used
        let pool = Pool::new("mysql://root@127.0.0.1:1/");
        Session::new(Some(pool.clone()), Engine::mysql(pool), vec![node("shop", Kind::Database, "shop", None), node("shop", Kind::Table, "orders", None)], None, Safety::default(), None)
    }

    fn column_suggested(s: &Session, col: &str) -> bool {
        let sql = "SELECT o. FROM orders o";
        let c = myq_sql::complete(sql, "SELECT o.".len(), &s.catalog, Some("shop"), 100);
        c.items.iter().any(|i| i.label == col)
    }

    #[test]
    fn a_hosted_account_sees_only_its_own_database() {
        let index = vec![
            node("u1_erp", Kind::Database, "u1_erp", None),
            node("u1_erp", Kind::Table, "orders", None),
            node("other", Kind::Database, "other", None),
            node("other", Kind::Table, "secret", None),
        ];
        let only = restrict_databases(index.clone(), Some("u1_erp"));
        assert_eq!(only.iter().map(|n| n.name.as_str()).collect::<Vec<_>>(), ["u1_erp", "orders"]);
        assert_eq!(restrict_databases(index.clone(), None), index, "no restriction when every database is shown");
    }

    #[test]
    fn switched_off_kinds_are_dropped_unless_the_query_names_kinds() {
        let index = vec![
            node("shop", Kind::Database, "shop", None),
            node("shop", Kind::Table, "supplier_x", None),
            node("shop", Kind::Column, "supplier_id", Some("orders")),
            node("shop", Kind::View, "supplier_v", None),
        ];
        let f = Filter::parse("supplier_").unwrap();
        let all = f.apply(&index);
        assert_eq!(all, vec![1, 2, 3]);
        assert_eq!(restrict_kinds(&index, all.clone(), &f, Some(&[Kind::Table, Kind::View])), vec![1, 3], "columns switched off");
        assert_eq!(restrict_kinds(&index, all.clone(), &f, Some(&[Kind::Table])), vec![1]);
        assert_eq!(restrict_kinds(&index, all.clone(), &f, None), all, "no restriction asked for");
        let col = Filter::parse("col:supplier").unwrap();
        let hits = col.apply(&index);
        assert_eq!(restrict_kinds(&index, hits.clone(), &col, Some(&[Kind::Table])), hits, "col: is explicit");
    }

    #[test]
    fn completion_columns_never_change_the_index_the_ui_mirrors() {
        let mut s = session();
        let before = s.index.len();
        s.add_completion_columns("shop", vec![node("shop", Kind::Column, "total", Some("orders"))]);
        assert_eq!(s.index.len(), before, "the filter index (and so the UI's copy) is untouched");
        assert!(column_suggested(&s, "total"), "but autocomplete knows the column");

        // the UI then loads the same database: the index grows once, and the column is not offered twice
        let start = s.add_columns("shop", vec![node("shop", Kind::Column, "total", Some("orders"))]);
        assert_eq!(start, before);
        assert_eq!(s.index.len(), before + 1);
        let sql = "SELECT o. FROM orders o";
        let c = myq_sql::complete(sql, "SELECT o.".len(), &s.catalog, Some("shop"), 100);
        assert_eq!(c.items.iter().filter(|i| i.label == "total").count(), 1);
    }
}
