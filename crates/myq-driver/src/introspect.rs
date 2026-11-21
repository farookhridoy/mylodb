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

/// An index of a table, columns in order.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct IndexDetail {
    pub name: String,
    pub unique: bool,
    /// `BTREE`, `HASH`, `FULLTEXT`, `SPATIAL`.
    pub kind: String,
    pub columns: Vec<String>,
}

/// Indexes of one table (the primary key is the index named `PRIMARY`).
pub async fn index_details(pool: &Pool, database: &str, table: &str) -> Result<Vec<IndexDetail>> {
    let mut conn = pool.get_conn().await?;
    // expression indexes (MySQL 8 functional key parts) have a NULL column name; they are shown as `(expression)`
    let rows: Vec<(String, i64, String, Option<String>)> = conn
        .exec(
            "SELECT INDEX_NAME, NON_UNIQUE, INDEX_TYPE, COLUMN_NAME FROM information_schema.STATISTICS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY INDEX_NAME = 'PRIMARY' DESC, INDEX_NAME, SEQ_IN_INDEX",
            (database, table),
        )
        .await?;
    let mut out: Vec<IndexDetail> = Vec::new();
    for (name, non_unique, kind, column) in rows {
        let column = column.unwrap_or_else(|| "(expression)".to_string());
        match out.last_mut() {
            Some(last) if last.name == name => last.columns.push(column),
            _ => out.push(IndexDetail { name, unique: non_unique == 0, kind, columns: vec![column] }),
        }
    }
    Ok(out)
}

/// The `CREATE` statement of a view, routine, trigger or event, as the server reports it.
pub async fn object_definition(pool: &Pool, kind: Kind, database: &str, name: &str) -> Result<Option<String>> {
    use mysql_async::Row;
    let (what, column) = match kind {
        Kind::View => ("VIEW", "Create View"),
        Kind::Procedure => ("PROCEDURE", "Create Procedure"),
        Kind::Function => ("FUNCTION", "Create Function"),
        Kind::Trigger => ("TRIGGER", "SQL Original Statement"),
        Kind::Event => ("EVENT", "Create Event"),
        Kind::Table => ("TABLE", "Create Table"),
        Kind::Database | Kind::Column => return Ok(None),
    };
    let mut conn = pool.get_conn().await?;
    let ident = |s: &str| format!("`{}`", s.replace('`', "``"));
    let row: Option<Row> = conn.query_first(format!("SHOW CREATE {what} {}.{}", ident(database), ident(name))).await?;
    // the statement can be NULL for routines the account may not read: that is "no definition", not an error
    Ok(row.and_then(|r| r.get_opt::<Option<String>, _>(column)).and_then(|v| v.ok()).flatten())
}

/// A database as the Databases page lists it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DatabaseRow {
    pub name: String,
    pub collation: String,
    pub charset: String,
    /// `mysql`, `sys`, `information_schema`, `performance_schema`: listed on request, never selectable.
    pub system: bool,
}

/// Names and default collations: one quick query. Sizes come separately (`database_sizes`), because that one can be slow.
pub async fn database_list(pool: &Pool) -> Result<Vec<DatabaseRow>> {
    let mut conn = pool.get_conn().await?;
    let rows: Vec<(String, String, String)> = conn
        .query("SELECT SCHEMA_NAME, DEFAULT_COLLATION_NAME, DEFAULT_CHARACTER_SET_NAME FROM information_schema.SCHEMATA ORDER BY SCHEMA_NAME")
        .await?;
    Ok(rows
        .into_iter()
        .map(|(name, collation, charset)| DatabaseRow { system: SYSTEM_DBS.contains(&name.to_lowercase().as_str()), name, collation, charset })
        .collect())
}

/// Table count and data + index bytes per database, in one grouped query over `information_schema.TABLES` (views count as tables, with no size).
pub async fn database_sizes(pool: &Pool) -> Result<Vec<(String, u64, u64)>> {
    let mut conn = pool.get_conn().await?;
    let rows: Vec<(String, u64, Option<u64>)> = conn
        .query("SELECT TABLE_SCHEMA, COUNT(*), CAST(SUM(COALESCE(DATA_LENGTH,0) + COALESCE(INDEX_LENGTH,0)) AS UNSIGNED) FROM information_schema.TABLES GROUP BY TABLE_SCHEMA")
        .await?;
    Ok(rows.into_iter().map(|(n, c, b)| (n, c, b.unwrap_or(0))).collect())
}

/// A table or view as the Structure page lists it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct TableRow {
    pub name: String,
    pub is_view: bool,
    pub engine: Option<String>,
    /// The server's row estimate (exact for views: none).
    pub rows: Option<u64>,
    pub collation: Option<String>,
    /// Data plus index bytes.
    pub bytes: Option<u64>,
    /// Free space inside the table file that `OPTIMIZE TABLE` would give back (phpMyAdmin's "Overhead").
    pub overhead: Option<u64>,
    pub comment: String,
}

pub async fn database_tables(pool: &Pool, database: &str) -> Result<Vec<TableRow>> {
    use mysql_async::Row;
    let mut conn = pool.get_conn().await?;
    let rows: Vec<Row> = conn
        .exec(
            "SELECT TABLE_NAME, TABLE_TYPE, ENGINE, TABLE_ROWS, TABLE_COLLATION, DATA_LENGTH, INDEX_LENGTH, DATA_FREE, TABLE_COMMENT \
             FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? ORDER BY TABLE_NAME",
            (database,),
        )
        .await?;
    let get = |r: &Row, i: usize| r.as_ref(i).cloned();
    let text = |r: &Row, i: usize| match get(r, i) {
        Some(mysql_async::Value::NULL) | None => None,
        Some(v) => mysql_async::from_value_opt::<String>(v).ok(),
    };
    let num = |r: &Row, i: usize| match get(r, i) {
        Some(mysql_async::Value::NULL) | None => None,
        Some(v) => mysql_async::from_value_opt::<u64>(v).ok(),
    };
    Ok(rows
        .iter()
        .filter_map(|r| {
            let name = text(r, 0)?;
            let is_view = text(r, 1).is_some_and(|t| t.contains("VIEW"));
            let bytes = match (num(r, 5), num(r, 6)) {
                (None, None) => None,
                (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)),
            };
            Some(TableRow { name, is_view, engine: text(r, 2), rows: num(r, 3), collation: text(r, 4), bytes, overhead: if is_view { None } else { num(r, 7) }, comment: text(r, 8).unwrap_or_default() })
        })
        .collect())
}
