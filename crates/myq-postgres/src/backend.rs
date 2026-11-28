use crate::split::split;
use crate::tls::{client_config, connector::Make};
use futures_util::StreamExt;
use myq_core::{
    Backend, BackendError, BackendResult, Capabilities, ColumnDetail, ColumnInfo, DatabaseRow, Dialect, IndexDetail, Kind, Node, Postgres, ResultSet, StatementResult, TableRow,
};
use myq_driver::{ConnConfig, SslMode};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_postgres::config::SslMode as PgSsl;
use tokio_postgres::types::{ToSql, Type};
use tokio_postgres::{Client, NoTls, Row, SimpleQueryMessage};

/// Rows kept per result set; the rest are counted but dropped (the same limit as the other engines).
pub const MAX_ROWS: usize = 10_000;
/// Idle connections kept for the next call.
const IDLE: usize = 3;

/// The words of a server error, with its detail and hint.
pub(crate) fn explain(e: tokio_postgres::Error) -> BackendError {
    match e.as_db_error() {
        Some(d) => {
            let mut text = d.message().to_string();
            for (label, part) in [("Detail", d.detail()), ("Hint", d.hint())] {
                if let Some(p) = part {
                    text.push_str(&format!("\n{label}: {p}"));
                }
            }
            text.push_str(&format!(" ({})", d.code().code()));
            BackendError(text)
        }
        None => {
            // the cause ("password missing", "certificate unknown") is what helps; the outer text alone does not
            let mut text = e.to_string();
            let mut source = std::error::Error::source(&e);
            while let Some(s) = source {
                text.push_str(&format!(": {s}"));
                source = s.source();
            }
            BackendError(text)
        }
    }
}

async fn connect(cfg: &ConnConfig) -> BackendResult<Client> {
    let mut pg = tokio_postgres::Config::new();
    pg.host(&cfg.host).port(cfg.port).user(&cfg.user).application_name("MyLoDB").connect_timeout(Duration::from_secs(10));
    pg.dbname(cfg.database.as_deref().filter(|d| !d.is_empty()).unwrap_or("postgres"));
    if let Some(p) = &cfg.password {
        pg.password(p);
    }
    let client = if cfg.ssl == SslMode::Disabled {
        pg.ssl_mode(PgSsl::Disable);
        let (client, connection) = pg.connect(NoTls).await.map_err(explain)?;
        tokio::spawn(connection);
        client
    } else {
        // never fall back to an unencrypted connection when encryption was asked for
        pg.ssl_mode(PgSsl::Require);
        let tls = Make(Arc::new(client_config(cfg).map_err(BackendError)?));
        let (client, connection) = pg.connect(tls).await.map_err(explain)?;
        tokio::spawn(connection);
        client
    };
    Ok(client)
}

/// What a connection test reports.
pub struct Probe {
    pub version: String,
    pub ssl_cipher: Option<String>,
}

pub async fn probe(cfg: &ConnConfig) -> BackendResult<Probe> {
    let c = connect(cfg).await?;
    let row = c.query_one("SELECT current_setting('server_version'), (SELECT cipher::text FROM pg_catalog.pg_stat_ssl WHERE pid = pg_catalog.pg_backend_pid())", &[]).await.map_err(explain)?;
    Ok(Probe { version: format!("PostgreSQL {}", row.get::<_, String>(0)), ssl_cipher: row.get(1) })
}

/// Lets a running script be stopped from another task: the statement in progress is cancelled on the server and the ones after it
/// are not started.
#[derive(Default)]
pub struct CancelToken {
    handle: Mutex<Option<(tokio_postgres::CancelToken, ConnConfig)>>,
    stopped: AtomicBool,
}

impl CancelToken {
    pub fn new() -> CancelToken {
        CancelToken::default()
    }

    pub async fn cancel(&self) -> BackendResult<()> {
        self.stopped.store(true, Ordering::SeqCst);
        let handle = self.handle.lock().unwrap().clone();
        let Some((token, cfg)) = handle else { return Ok(()) };
        if cfg.ssl == SslMode::Disabled {
            token.cancel_query(NoTls).await.map_err(explain)
        } else {
            token.cancel_query(Make(Arc::new(client_config(&cfg).map_err(BackendError)?))).await.map_err(explain)
        }
    }

    fn stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }
}

/// A connection to one PostgreSQL database. Clones share the idle connections.
#[derive(Clone)]
pub struct PgBackend {
    cfg: Arc<ConnConfig>,
    idle: Arc<Mutex<Vec<Client>>>,
    /// Set for reads that resolve unqualified names in one schema (`with_schema`).
    schema: Option<Arc<str>>,
}

pub(crate) const SYSTEM: &str = "(n.nspname LIKE 'pg\\_%' OR n.nspname = 'information_schema')";

impl CancelToken {
    /// Point the token at the connection that is about to work, so that `cancel` reaches it.
    pub(crate) fn watch(&self, client: &Client, cfg: &ConnConfig) {
        *self.handle.lock().unwrap() = Some((client.cancel_token(), cfg.clone()));
    }

    pub(crate) fn release(&self) {
        *self.handle.lock().unwrap() = None;
    }
}

impl PgBackend {
    /// The same connections, reading with `schema` first in the search path.
    pub(crate) fn with_schema(&self, schema: &str) -> PgBackend {
        PgBackend { schema: Some(schema.into()), ..self.clone() }
    }

    pub fn read_only(&self) -> bool {
        self.cfg.read_only
    }

    pub(crate) fn config(&self) -> &ConnConfig {
        &self.cfg
    }

    /// A connection inside a read-only transaction; `give_back` rolls it back.
    pub(crate) async fn reading(&self) -> BackendResult<Client> {
        let c = self.checkout().await?;
        c.batch_execute("BEGIN TRANSACTION READ ONLY").await.map_err(explain)?;
        if let Some(s) = &self.schema {
            // LOCAL: it ends with the transaction
            c.batch_execute(&format!("SET LOCAL search_path TO {}, public", Postgres.quote_ident(s))).await.map_err(explain)?;
        }
        Ok(c)
    }

    /// `cfg.read_only` makes every statement run inside a read-only transaction that is rolled back.
    pub fn new(cfg: ConnConfig) -> PgBackend {
        PgBackend { cfg: Arc::new(cfg), idle: Default::default(), schema: None }
    }

    pub(crate) async fn checkout(&self) -> BackendResult<Client> {
        loop {
            let next = self.idle.lock().unwrap().pop();
            match next {
                Some(c) if !c.is_closed() && c.simple_query("").await.is_ok() => return Ok(c),
                Some(_) => continue,
                None => return connect(&self.cfg).await,
            }
        }
    }

    /// Put a connection back with its session as new (search path, settings, temporary tables, an open transaction).
    pub(crate) async fn give_back(&self, c: Client) {
        if c.batch_execute("ROLLBACK").await.is_ok() && c.batch_execute("DISCARD ALL").await.is_ok() {
            let mut idle = self.idle.lock().unwrap();
            if idle.len() < IDLE {
                idle.push(c);
            }
        }
    }

    pub(crate) async fn query(&self, sql: &str, params: &[&(dyn ToSql + Sync)]) -> BackendResult<Vec<Row>> {
        let c = self.checkout().await?;
        let rows = c.query(sql, params).await.map_err(explain);
        self.give_back(c).await;
        rows
    }

    /// Run a script, one result per statement. `schema` comes first in the search path. The run stops when `token` is cancelled or
    /// after `timeout` seconds.
    pub async fn run_cancellable(&self, schema: Option<&str>, sql: &str, token: Arc<CancelToken>, timeout: Option<u64>) -> BackendResult<Vec<StatementResult>> {
        let client = self.checkout().await?;
        *token.handle.lock().unwrap() = Some((client.cancel_token(), (*self.cfg).clone()));
        if let Some(s) = schema.filter(|s| !s.is_empty()) {
            client.batch_execute(&format!("SET search_path TO {}, public", Postgres.quote_ident(s))).await.map_err(explain)?;
        }
        let timed_out = Arc::new(AtomicBool::new(false));
        let timer = timeout.map(|secs| {
            let (token, flag) = (token.clone(), timed_out.clone());
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(secs)).await;
                flag.store(true, Ordering::SeqCst);
                let _ = token.cancel().await;
            })
        });
        let mut results = Vec::new();
        for stmt in split(sql) {
            if token.stopped() {
                results.push(StatementResult { sql: stmt.text, error: Some("Cancelled before it started.".into()), ..Default::default() });
                continue;
            }
            let started = Instant::now();
            let mut res = StatementResult { sql: stmt.text.clone(), ..Default::default() };
            if let Err(e) = self.run_one(&client, &stmt.text, &mut res).await {
                res.sets.clear();
                res.error = Some(if timed_out.load(Ordering::SeqCst) {
                    format!("Stopped: the run took longer than the query timeout of {} s.", timeout.unwrap_or(0))
                } else if token.stopped() && e.0.contains("57014") {
                    "Cancelled.".into()
                } else {
                    e.0
                });
            }
            res.elapsed_ms = started.elapsed().as_millis() as u64;
            results.push(res);
        }
        if let Some(t) = timer {
            t.abort();
        }
        // a late cancel must not hit whoever uses this connection next
        *token.handle.lock().unwrap() = None;
        if !token.stopped() {
            self.give_back(client).await;
        }
        Ok(results)
    }

    async fn run_one(&self, client: &Client, sql: &str, res: &mut StatementResult) -> BackendResult<()> {
        let words: Vec<String> = sql.split(|c: char| !c.is_ascii_alphabetic()).filter(|w| !w.is_empty()).map(str::to_ascii_uppercase).collect();
        if words.first().is_some_and(|w| w == "COPY") && words.iter().any(|w| w == "STDIN" || w == "STDOUT") {
            return Err(BackendError("COPY to STDOUT or from STDIN is not supported here.".into()));
        }
        if !self.cfg.read_only {
            return execute(client, sql, res).await;
        }
        // The server refuses every write inside a read-only transaction, and the rollback undoes any setting the statement made,
        // so no statement can switch the connection back to writing.
        client.batch_execute("BEGIN TRANSACTION READ ONLY").await.map_err(explain)?;
        let outcome = execute(client, sql, res).await;
        let _ = client.batch_execute("ROLLBACK").await;
        outcome
    }
}

async fn execute(client: &Client, sql: &str, res: &mut StatementResult) -> BackendResult<()> {
    // preparing checks that this is one statement and tells the column types; the simple protocol then returns every value as text
    let types: Vec<Type> = client.prepare(sql).await.map_err(explain)?.columns().iter().map(|c| c.type_().clone()).collect();
    let stream = client.simple_query_raw(sql).await.map_err(explain)?;
    let mut stream = std::pin::pin!(stream);
    let mut set: Option<ResultSet> = None;
    while let Some(message) = stream.next().await {
        match message.map_err(explain)? {
            SimpleQueryMessage::RowDescription(columns) => {
                let columns = columns
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        let ty = types.get(i);
                        ColumnInfo { name: c.name().to_string(), type_name: ty.map(|t| t.name().to_string()).unwrap_or_default(), binary: ty == Some(&Type::BYTEA) }
                    })
                    .collect();
                set = Some(ResultSet { columns, rows: Vec::new(), total_rows: 0, truncated: false });
            }
            SimpleQueryMessage::Row(row) => {
                if let Some(set) = set.as_mut() {
                    set.total_rows += 1;
                    if set.rows.len() < MAX_ROWS {
                        let cells = set
                            .columns
                            .iter()
                            .enumerate()
                            .map(|(i, col)| {
                                let text = row.try_get(i).ok().flatten()?;
                                // bytea arrives as \x0a1b; show hex digits, as for the other engines
                                Some(match text.strip_prefix("\\x") {
                                    Some(hex) if col.binary => hex.to_ascii_uppercase(),
                                    _ => text.to_string(),
                                })
                            })
                            .collect();
                        set.rows.push(cells);
                    } else {
                        set.truncated = true;
                    }
                }
            }
            SimpleQueryMessage::CommandComplete(n) => {
                if set.is_none() {
                    res.affected_rows = n;
                }
            }
            _ => {}
        }
    }
    res.sets.extend(set);
    Ok(())
}

impl Backend for PgBackend {
    fn dialect(&self) -> &dyn Dialect {
        &Postgres
    }

    async fn capabilities(&self) -> BackendResult<Capabilities> {
        Ok(Capabilities::postgres(!self.cfg.read_only))
    }

    async fn index(&self, include_system: bool) -> BackendResult<Vec<Node>> {
        let schemas = self
            .query(&format!("SELECT n.nspname::text FROM pg_catalog.pg_namespace n WHERE ($1 OR NOT {SYSTEM}) AND pg_catalog.has_schema_privilege(n.oid, 'USAGE') ORDER BY (n.nspname <> 'public'), 1"), &[&include_system])
            .await?;
        let mut out: Vec<Node> = schemas.iter().map(|r| r.get::<_, String>(0)).map(|s| Node { database: s.clone(), kind: Kind::Database, name: s, parent: None }).collect();
        let rows = self
            .query(&format!("SELECT n.nspname::text, c.relname::text, c.relkind IN ('v','m') FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE c.relkind IN ('r','p','f','v','m') AND ($1 OR NOT {SYSTEM}) AND pg_catalog.has_schema_privilege(n.oid, 'USAGE') ORDER BY 1, 2"), &[&include_system])
            .await?;
        out.extend(rows.iter().map(|r| Node { database: r.get(0), kind: if r.get(2) { Kind::View } else { Kind::Table }, name: r.get(1), parent: None }));
        out.extend(crate::objects::nodes(self, include_system).await?);
        Ok(out)
    }

    async fn column_nodes(&self, database: &str) -> BackendResult<Vec<Node>> {
        let rows = self
            .query("SELECT c.relname::text, a.attname::text FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid = a.attrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1 AND c.relkind IN ('r','p','f','v','m') AND a.attnum > 0 AND NOT a.attisdropped ORDER BY c.relname, a.attnum", &[&database])
            .await?;
        Ok(rows.iter().map(|r| Node { database: database.to_string(), kind: Kind::Column, name: r.get(1), parent: Some(r.get(0)) }).collect())
    }

    /// The schemas of the connected database.
    async fn databases(&self) -> BackendResult<Vec<DatabaseRow>> {
        let rows = self
            .query(&format!("SELECT n.nspname::text, d.datcollate::text, pg_catalog.pg_encoding_to_char(d.encoding)::text, {SYSTEM} FROM pg_catalog.pg_namespace n, pg_catalog.pg_database d WHERE d.datname = pg_catalog.current_database() AND pg_catalog.has_schema_privilege(n.oid, 'USAGE') ORDER BY 1"), &[])
            .await?;
        Ok(rows.iter().map(|r| DatabaseRow { name: r.get(0), collation: r.get(1), charset: r.get(2), system: r.get(3) }).collect())
    }

    async fn tables(&self, database: &str) -> BackendResult<Vec<TableRow>> {
        let rows = self
            .query("SELECT c.relname::text, c.relkind::text, c.reltuples::float8, CASE WHEN c.relkind IN ('r','m') THEN pg_catalog.pg_total_relation_size(c.oid) END, COALESCE(pg_catalog.obj_description(c.oid, 'pg_class'), '') FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1 AND c.relkind IN ('r','p','f','v','m') ORDER BY 1", &[&database])
            .await?;
        Ok(rows
            .iter()
            .map(|r| {
                let kind: String = r.get(1);
                let estimate: f64 = r.get(2);
                let engine = match kind.as_str() {
                    "p" => Some("partitioned"),
                    "f" => Some("foreign"),
                    "m" => Some("materialized view"),
                    "v" => None,
                    _ => Some("heap"),
                };
                TableRow {
                    name: r.get(0),
                    is_view: kind == "v" || kind == "m",
                    engine: engine.map(String::from),
                    // -1: never analysed
                    rows: (estimate >= 0.0 && kind != "v").then_some(estimate as u64),
                    collation: None,
                    bytes: r.get::<_, Option<i64>>(3).map(|b| b.max(0) as u64),
                    overhead: None,
                    comment: r.get(4),
                }
            })
            .collect())
    }

    async fn columns(&self, database: &str, table: &str) -> BackendResult<Vec<ColumnDetail>> {
        let rows = self
            .query(
                "SELECT a.attname::text, pg_catalog.format_type(a.atttypid, a.atttypmod), a.attnotnull,
                    CASE WHEN EXISTS (SELECT 1 FROM pg_catalog.pg_index i WHERE i.indrelid = c.oid AND i.indisprimary AND a.attnum = ANY(i.indkey)) THEN 'PRI'
                         WHEN EXISTS (SELECT 1 FROM pg_catalog.pg_index i WHERE i.indrelid = c.oid AND i.indisunique AND i.indnkeyatts = 1 AND i.indkey[0] = a.attnum) THEN 'UNI'
                         WHEN EXISTS (SELECT 1 FROM pg_catalog.pg_index i WHERE i.indrelid = c.oid AND i.indkey[0] = a.attnum) THEN 'MUL' ELSE '' END,
                    CASE WHEN a.attidentity <> '' THEN 'identity' WHEN a.attgenerated <> '' THEN 'stored generated'
                         WHEN pg_catalog.pg_get_expr(d.adbin, d.adrelid) LIKE 'nextval(%' THEN 'auto_increment' ELSE '' END
                 FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid = a.attrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
                 LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
                 WHERE n.nspname = $1 AND c.relname = $2 AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
                &[&database, &table],
            )
            .await?;
        Ok(rows.iter().map(|r| ColumnDetail { name: r.get(0), column_type: r.get(1), nullable: !r.get::<_, bool>(2), key: r.get(3), extra: r.get(4) }).collect())
    }

    async fn indexes(&self, database: &str, table: &str) -> BackendResult<Vec<IndexDetail>> {
        let rows = self
            .query(
                "SELECT x.relname::text, i.indisunique, i.indisprimary, m.amname::text,
                    ARRAY(SELECT pg_catalog.pg_get_indexdef(i.indexrelid, k, true) FROM generate_series(1, i.indnkeyatts::int) k ORDER BY k)
                 FROM pg_catalog.pg_index i JOIN pg_catalog.pg_class c ON c.oid = i.indrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
                 JOIN pg_catalog.pg_class x ON x.oid = i.indexrelid JOIN pg_catalog.pg_am m ON m.oid = x.relam
                 WHERE n.nspname = $1 AND c.relname = $2 ORDER BY NOT i.indisprimary, 1",
                &[&database, &table],
            )
            .await?;
        Ok(rows
            .iter()
            .map(|r| IndexDetail {
                // the UI knows the primary key by this name, whatever PostgreSQL calls its index
                name: if r.get(2) { "PRIMARY".into() } else { r.get(0) },
                unique: r.get(1),
                kind: r.get::<_, String>(3).to_ascii_uppercase(),
                columns: r.get(4),
            })
            .collect())
    }

    async fn run(&self, database: Option<&str>, sql: &str) -> BackendResult<Vec<StatementResult>> {
        self.run_cancellable(database, sql, Arc::new(CancelToken::new()), None).await
    }
}
