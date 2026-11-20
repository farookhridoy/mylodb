//! Thin Tauri bridge. All logic lives in the `myq-*` crates; this only exposes it as commands.

use myq_data::{Assign, Change, CellData, ColumnFilter, ExportOptions, LookupItem, Page, PageSpec, TableMeta, ApplyOutcome};
use myq_admin::{Account, AccountEdit, Grants, ScopeGrant, Sql};
use myq_schema::ddl::TableDef;
use myq_driver::{execute_script, introspect, kill_query, pool, IntrospectOptions, Pool, StatementResult};
use myq_schema::filter::{Filter, Node};
use myq_store::{Favorite, HistoryEntry, Keychain, SavedConnection, Store};
use std::{collections::HashMap, sync::Mutex};
use tauri::Manager;

/// A live connection: its pool and the cached name index the filter runs over.
struct Session {
    #[allow(dead_code)]
    pool: Pool,
    index: Vec<Node>,
}

struct AppState {
    store: Mutex<Store>,
    sessions: Mutex<HashMap<i64, Session>>,
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

/// Connect, build the name index, and return it to the UI.
#[tauri::command]
async fn connect(state: tauri::State<'_, AppState>, id: i64) -> Result<Vec<Node>, String> {
    let cfg = state
        .store
        .lock()
        .unwrap()
        .config_for(id)
        .map_err(err)?
        .ok_or_else(|| "connection not found".to_string())?;
    let p = pool(&cfg);
    let index = introspect(&p, &IntrospectOptions { include_columns: true, ..Default::default() })
        .await
        .map_err(err)?;
    state.sessions.lock().unwrap().insert(id, Session { pool: p, index: index.clone() });
    Ok(index)
}

#[tauri::command]
async fn disconnect(state: tauri::State<'_, AppState>, id: i64) -> Result<(), String> {
    let session = state.sessions.lock().unwrap().remove(&id);
    if let Some(s) = session {
        s.pool.disconnect().await.map_err(err)?;
    }
    Ok(())
}

/// Indices (into the index returned by `connect`) that match the filter query.
#[tauri::command]
fn filter_index(state: tauri::State<AppState>, id: i64, query: String) -> Result<Vec<usize>, String> {
    let filter = Filter::parse(&query).map_err(|e| format!("{e:?}"))?;
    let sessions = state.sessions.lock().unwrap();
    let s = sessions.get(&id).ok_or_else(|| "not connected".to_string())?;
    Ok(filter.apply(&s.index))
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

/// Export every row matching `spec` to `path`. A partial file is removed on failure.
#[tauri::command]
async fn export_table(state: tauri::State<'_, AppState>, id: i64, db: String, table: String, spec: PageSpec, options: ExportOptions, path: String) -> Result<u64, String> {
    let mut c = conn_for(&state, id).await?;
    let meta = myq_data::table_meta(&mut c, &db, &table).await.map_err(err)?;
    let file = std::fs::File::create(&path).map_err(err)?;
    let mut out = std::io::BufWriter::new(file);
    let result = myq_data::export_rows(&mut c, &meta, &spec, &options, &mut out).await;
    let flushed = std::io::Write::flush(&mut out);
    match (result, flushed) {
        (Ok(n), Ok(())) => Ok(n),
        (Err(e), _) => { let _ = std::fs::remove_file(&path); Err(err(e)) }
        (_, Err(e)) => { let _ = std::fs::remove_file(&path); Err(err(e)) }
    }
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

/// Re-read the object index after the schema changed, so the tree and the filter see it.
async fn reindex(state: &tauri::State<'_, AppState>, id: i64) -> Result<Vec<Node>, String> {
    let pool = state.sessions.lock().unwrap().get(&id).map(|s| s.pool.clone()).ok_or("not connected")?;
    let index = introspect(&pool, &IntrospectOptions { include_columns: true, ..Default::default() }).await.map_err(err)?;
    if let Some(s) = state.sessions.lock().unwrap().get_mut(&id) {
        s.index = index.clone();
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
            app.manage(AppState { store: Mutex::new(store), sessions: Mutex::new(HashMap::new()), running: Mutex::new(HashMap::new()) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_connections,
            save_connection,
            delete_connection,
            connect,
            disconnect,
            filter_index,
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
            account_drop
        ])
        .run(tauri::generate_context!())
        .expect("error while running MyLoDB");
}
