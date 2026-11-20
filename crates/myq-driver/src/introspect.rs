use crate::Result;
use mysql_async::{prelude::Queryable, Pool};
use myq_schema::filter::{Kind, Node};

#[derive(Clone, Debug)]
pub struct IntrospectOptions {
    /// Include information_schema, performance_schema, mysql and sys.
    pub include_system: bool,
    /// Columns can be huge on big servers; index them only on request.
    pub include_columns: bool,
}

impl Default for IntrospectOptions {
    fn default() -> Self {
        Self { include_system: false, include_columns: false }
    }
}

const SYSTEM_DBS: [&str; 4] = ["information_schema", "performance_schema", "mysql", "sys"];

/// Build the flat name index the object-browser filter runs over.
/// One query per object kind (not per database), so cost is independent of database count.
pub async fn introspect(pool: &Pool, opts: &IntrospectOptions) -> Result<Vec<Node>> {
    let mut conn = pool.get_conn().await?;
    let keep = |db: &str| opts.include_system || !SYSTEM_DBS.contains(&db.to_lowercase().as_str());
    let mut out = Vec::new();

    let dbs: Vec<String> = conn.query("SELECT SCHEMA_NAME FROM information_schema.SCHEMATA ORDER BY 1").await?;
    for d in dbs.into_iter().filter(|d| keep(d)) {
        out.push(Node { database: d.clone(), kind: Kind::Database, name: d, parent: None });
    }

    let rows: Vec<(String, String, String)> = conn
        .query("SELECT TABLE_SCHEMA, TABLE_NAME, TABLE_TYPE FROM information_schema.TABLES ORDER BY 1,2")
        .await?;
    for (db, name, ty) in rows.into_iter().filter(|r| keep(&r.0)) {
        let kind = if ty.contains("VIEW") { Kind::View } else { Kind::Table };
        out.push(Node { database: db, kind, name, parent: None });
    }

    let rows: Vec<(String, String, String)> = conn
        .query("SELECT ROUTINE_SCHEMA, ROUTINE_NAME, ROUTINE_TYPE FROM information_schema.ROUTINES ORDER BY 1,2")
        .await?;
    for (db, name, ty) in rows.into_iter().filter(|r| keep(&r.0)) {
        let kind = if ty == "PROCEDURE" { Kind::Procedure } else { Kind::Function };
        out.push(Node { database: db, kind, name, parent: None });
    }

    let rows: Vec<(String, String, String)> = conn
        .query("SELECT TRIGGER_SCHEMA, TRIGGER_NAME, EVENT_OBJECT_TABLE FROM information_schema.TRIGGERS ORDER BY 1,2")
        .await?;
    for (db, name, table) in rows.into_iter().filter(|r| keep(&r.0)) {
        out.push(Node { database: db, kind: Kind::Trigger, name, parent: Some(table) });
    }

    let rows: Vec<(String, String)> =
        conn.query("SELECT EVENT_SCHEMA, EVENT_NAME FROM information_schema.EVENTS ORDER BY 1,2").await?;
    for (db, name) in rows.into_iter().filter(|r| keep(&r.0)) {
        out.push(Node { database: db, kind: Kind::Event, name, parent: None });
    }

    if opts.include_columns {
        let rows: Vec<(String, String, String)> = conn
            .query("SELECT TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME FROM information_schema.COLUMNS ORDER BY 1,2,ORDINAL_POSITION")
            .await?;
        for (db, table, name) in rows.into_iter().filter(|r| keep(&r.0)) {
            out.push(Node { database: db, kind: Kind::Column, name, parent: Some(table) });
        }
    }
    Ok(out)
}

/// Column nodes of one database (parent = table). Loaded on demand: the columns of a whole server can be
/// hundreds of thousands of rows and are only needed for the databases the user actually works in.
pub async fn introspect_columns(pool: &Pool, database: &str) -> Result<Vec<Node>> {
    let mut conn = pool.get_conn().await?;
    let rows: Vec<(String, String)> = conn
        .exec("SELECT TABLE_NAME, COLUMN_NAME FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME, ORDINAL_POSITION", (database,))
        .await?;
    Ok(rows.into_iter().map(|(table, name)| Node { database: database.to_string(), kind: Kind::Column, name, parent: Some(table) }).collect())
}

/// Size and row estimate of one table or view, for the object browser. Views have no size.
/// MySQL 8 caches these statistics (`information_schema_stats_expiry`), so they are estimates, not live counts.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct TableStat {
    pub name: String,
    /// Data plus index bytes.
    pub bytes: Option<u64>,
    pub rows: Option<u64>,
}

/// One query for all tables of a database, so expanding a database costs one round trip however many tables it has.
pub async fn table_stats(pool: &Pool, database: &str) -> Result<Vec<TableStat>> {
    let mut conn = pool.get_conn().await?;
    let rows: Vec<(String, Option<u64>, Option<u64>, Option<u64>)> = conn
        .exec(
            "SELECT TABLE_NAME, DATA_LENGTH, INDEX_LENGTH, TABLE_ROWS FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME",
            (database,),
        )
        .await?;
    Ok(rows
        .into_iter()
        .map(|(name, data, index, rows)| TableStat {
            name,
            bytes: data.map(|d| d.saturating_add(index.unwrap_or(0))),
            rows,
        })
        .collect())
}

/// A column as the object browser shows it: `bigint unsigned`, `varchar(255)`, key and nullability.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ColumnDetail {
    pub name: String,
    /// The full declared type (`COLUMN_TYPE`), e.g. `bigint(20) unsigned` or `varchar(255)`.
    pub column_type: String,
    pub nullable: bool,
    /// `PRI`, `UNI`, `MUL` or empty.
    pub key: String,
    /// e.g. `auto_increment`.
    pub extra: String,
}

/// Columns of one table in declared order. Loaded when the table is selected, not for the whole database.
pub async fn column_details(pool: &Pool, database: &str, table: &str) -> Result<Vec<ColumnDetail>> {
    let mut conn = pool.get_conn().await?;
    let rows: Vec<(String, String, String, String, String)> = conn
        .exec(
            "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY, EXTRA FROM information_schema.COLUMNS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION",
            (database, table),
        )
        .await?;
    Ok(rows
        .into_iter()
        .map(|(name, column_type, nullable, key, extra)| ColumnDetail { name, column_type, nullable: nullable == "YES", key, extra })
        .collect())
}
