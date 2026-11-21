//! Small session-level queries the application layer needs, so that it never touches the driver types directly.

use crate::{ConnConfig, Result};
use mysql_async::{from_value_opt, prelude::Queryable, Conn, Row, Value};
use serde::Serialize;

/// Turn foreign key checks on or off for this session (the CSV import and "Run" switches).
pub async fn set_foreign_key_checks(conn: &mut Conn, on: bool) -> Result<()> {
    conn.query_drop(format!("SET SESSION foreign_key_checks = {}", on as u8)).await?;
    Ok(())
}

/// Put foreign key checks back on before a connection returns to the pool. Best effort: a dead connection is dropped by the pool anyway.
pub async fn restore_foreign_key_checks(conn: &mut Conn) {
    let _ = set_foreign_key_checks(conn, true).await;
}

/// What this server can do, from its `SELECT VERSION()` (see `myq_core::Capabilities`).
pub async fn capabilities(conn: &mut Conn) -> Result<myq_core::Capabilities> {
    let version: Option<String> = conn.query_first("SELECT VERSION()").await?;
    Ok(myq_core::Capabilities::mysql_family(&version.unwrap_or_default()))
}

/// Indexes the server reports as unused since it started, as `(table, index)` (`sys.schema_unused_indexes`, which needs performance_schema).
pub async fn unused_indexes(conn: &mut Conn, database: &str) -> Result<Vec<(String, String)>> {
    Ok(conn.exec("SELECT object_name, index_name FROM sys.schema_unused_indexes WHERE object_schema = ? ORDER BY 1, 2", (database,)).await?)
}

/// What a throwaway connection learned about the server.
#[derive(Debug, Serialize)]
pub struct Probe {
    pub version: String,
    pub ssl_cipher: Option<String>,
}

/// Connect once, read the version and the TLS cipher, and disconnect (the connection dialog's "Test").
pub async fn probe(cfg: &ConnConfig) -> Result<Probe> {
    let mut conn = Conn::new(cfg.to_opts()).await?;
    let version: Option<String> = conn.query_first("SELECT VERSION()").await?;
    let cipher: Option<(String, String)> = conn.query_first("SHOW SESSION STATUS LIKE 'Ssl_cipher'").await?;
    let _ = conn.disconnect().await;
    Ok(Probe { version: version.unwrap_or_default(), ssl_cipher: cipher.map(|c| c.1).filter(|c| !c.is_empty()) })
}

/// A result table as plain text (SHOW ENGINES, SHOW CHARACTER SET, SHOW REPLICA STATUS). NULL is `None`.
#[derive(Debug, Serialize, Default)]
pub struct Grid {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Option<String>>>,
}

pub async fn query_grid(conn: &mut Conn, sql: &str) -> Result<Grid> {
    let rows: Vec<Row> = conn.query(sql).await?;
    let columns = rows.first().map(|r| r.columns_ref().iter().map(|c| c.name_str().to_string()).collect()).unwrap_or_default();
    let rows = rows
        .iter()
        .map(|r| (0..r.len()).map(|i| match r.as_ref(i) { Some(Value::NULL) | None => None, Some(v) => from_value_opt::<String>(v.clone()).ok() }).collect())
        .collect();
    Ok(Grid { columns, rows })
}

/// Engines, character sets and collations the server supports.
#[derive(Debug, Serialize)]
pub struct ServerOptions {
    pub engines: Vec<String>,
    /// (charset, collations, default collation)
    pub charsets: Vec<(String, Vec<String>, String)>,
}

pub async fn server_options(conn: &mut Conn) -> Result<ServerOptions> {
    let engines: Vec<String> = conn.query("SELECT ENGINE FROM information_schema.ENGINES WHERE SUPPORT IN ('YES','DEFAULT') ORDER BY ENGINE").await?;
    // COLLATION_CHARACTER_SET_APPLICABILITY pairs every collation with every character set it can be used with, on both servers.
    // MariaDB 10.10+ also lists generic UCA 14 collations there (`uca1400_ai_ci`, with a NULL character set in COLLATIONS); the usable
    // name for a character set is then prefixed (`utf8mb4_uca1400_ai_ci`). CHARACTER_SETS names each set's default collation
    // (COLLATIONS.IS_DEFAULT is empty for most rows on MariaDB).
    let pairs: Vec<(String, String)> = conn
        .query("SELECT COLLATION_NAME, CHARACTER_SET_NAME FROM information_schema.COLLATION_CHARACTER_SET_APPLICABILITY ORDER BY COLLATION_NAME")
        .await?;
    let defaults: Vec<(String, String)> = conn.query("SELECT CHARACTER_SET_NAME, DEFAULT_COLLATE_NAME FROM information_schema.CHARACTER_SETS").await?;
    let mut charsets: Vec<(String, Vec<String>, String)> = Vec::new();
    for (name, charset) in pairs {
        let collation = if name == charset || name.starts_with(&format!("{charset}_")) { name } else { format!("{charset}_{name}") };
        let entry = match charsets.iter().position(|c| c.0 == charset) {
            Some(i) => i,
            None => {
                let default = defaults.iter().find(|d| d.0 == charset).map(|d| d.1.clone()).unwrap_or_default();
                charsets.push((charset, vec![], default));
                charsets.len() - 1
            }
        };
        charsets[entry].1.push(collation);
    }
    for c in &mut charsets {
        c.1.sort();
        c.1.dedup();
    }
    charsets.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(ServerOptions { engines, charsets })
}

/// The per-session facts the server home page shows next to `server_info`.
#[derive(Debug)]
pub struct SessionFacts {
    pub account: String,
    pub protocol: Option<u32>,
    pub threads_connected: Option<u64>,
    pub connection_collation: String,
}

pub async fn session_facts(conn: &mut Conn) -> Result<SessionFacts> {
    let account: Option<String> = conn.query_first("SELECT CURRENT_USER()").await?;
    let protocol: Option<u32> = conn.query_first("SELECT @@protocol_version").await.ok().flatten();
    let threads: Option<(String, String)> = conn.query_first("SHOW GLOBAL STATUS LIKE 'Threads_connected'").await.ok().flatten();
    let collation: Option<String> = conn.query_first("SELECT @@collation_connection").await?;
    Ok(SessionFacts {
        account: account.unwrap_or_default(),
        protocol,
        threads_connected: threads.and_then(|(_, v)| v.parse().ok()),
        connection_collation: collation.unwrap_or_default(),
    })
}
