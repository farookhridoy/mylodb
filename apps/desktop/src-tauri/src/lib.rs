//! Thin Tauri bridge. All logic lives in the `myq-*` crates; this only exposes it as commands.

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
            session_set
        ])
        .run(tauri::generate_context!())
        .expect("error while running MyLoDB");
}
