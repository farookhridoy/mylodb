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
