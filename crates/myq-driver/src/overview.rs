//! Everything the table "Properties" and "Diagram" views show, read from `information_schema`:
//! settings, columns, constraints, foreign keys in both directions, indexes, triggers, partitions, statistics and the DDL.

use crate::{index_details, object_definition, IndexDetail, Result};
use myq_schema::filter::Kind;
use mysql_async::{from_value_opt, prelude::Queryable, Pool, Row, Value};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct TableInfo {
    pub is_view: bool,
    pub engine: Option<String>,
    pub collation: Option<String>,
    pub charset: Option<String>,
    pub auto_increment: Option<u64>,
    pub comment: String,
    pub partitioned: bool,
    pub row_format: Option<String>,
    /// Estimates from the server's statistics (MySQL 8 caches them), not live counts.
    pub rows: Option<u64>,
    pub avg_row_length: Option<u64>,
    pub data_length: Option<u64>,
    pub index_length: Option<u64>,
    pub data_free: Option<u64>,
    pub created: Option<String>,
    pub updated: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ColumnFull {
    pub name: String,
    pub ordinal: u64,
    pub column_type: String,
    pub nullable: bool,
    pub key: String,
    pub default: Option<String>,
    pub extra: String,
    pub comment: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Constraint {
    pub name: String,
    /// `PRIMARY KEY`, `UNIQUE`, `FOREIGN KEY` or `CHECK`.
    pub kind: String,
    pub columns: Vec<String>,
}

/// A foreign key. `schema`/`table`/`columns` are the referencing side, `ref_*` the referenced side.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ForeignKey {
    pub name: String,
    pub schema: String,
    pub table: String,
    pub columns: Vec<String>,
    pub ref_schema: String,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
    pub on_update: String,
    pub on_delete: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TriggerInfo {
    pub name: String,
    pub timing: String,
    pub event: String,
    pub statement: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PartitionInfo {
    pub name: String,
    pub method: Option<String>,
    pub expression: Option<String>,
    pub rows: Option<u64>,
    pub data_length: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct TableOverview {
    pub info: TableInfo,
    pub columns: Vec<ColumnFull>,
    pub indexes: Vec<IndexDetail>,
    pub constraints: Vec<Constraint>,
    /// Foreign keys of this table (it references others).
    pub outgoing: Vec<ForeignKey>,
    /// Foreign keys of other tables that reference this one.
    pub incoming: Vec<ForeignKey>,
    pub triggers: Vec<TriggerInfo>,
    pub partitions: Vec<PartitionInfo>,
    pub ddl: Option<String>,
}

/// A cell as `T`, or None when it is NULL or has an unexpected type (columns differ a little between MySQL and MariaDB).
fn cell<T: mysql_async::prelude::FromValue>(row: &Row, i: usize) -> Option<T> {
    match row.as_ref(i)? {
        Value::NULL => None,
        v => from_value_opt::<T>(v.clone()).ok(),
    }
}

/// Group `(constraint, referencing/referenced column data)` rows into foreign keys, keeping column order.
fn group_fks(rows: Vec<Row>, rules: &BTreeMap<(String, String, String), (String, String)>) -> Vec<ForeignKey> {
    // row: CONSTRAINT_NAME, TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME, REFERENCED_TABLE_SCHEMA, REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME
    let mut out: Vec<ForeignKey> = Vec::new();
    for r in rows {
        let (Some(name), Some(schema), Some(table), Some(col), Some(rs), Some(rt), Some(rc)) =
            (cell::<String>(&r, 0), cell::<String>(&r, 1), cell::<String>(&r, 2), cell::<String>(&r, 3), cell::<String>(&r, 4), cell::<String>(&r, 5), cell::<String>(&r, 6))
        else {
            continue;
        };
        match out.iter_mut().find(|f| f.name == name && f.schema == schema && f.table == table) {
            Some(f) => {
                f.columns.push(col);
                f.ref_columns.push(rc);
            }
            None => {
                let (on_update, on_delete) = rules.get(&(schema.clone(), table.clone(), name.clone())).cloned().unwrap_or_default();
                out.push(ForeignKey { name, schema, table, columns: vec![col], ref_schema: rs, ref_table: rt, ref_columns: vec![rc], on_update, on_delete });
            }
        }
    }
    out
}

pub async fn table_overview(pool: &Pool, database: &str, table: &str) -> Result<TableOverview> {
    let mut conn = pool.get_conn().await?;
    let mut o = TableOverview::default();

    // ---- settings and statistics
    let row: Option<Row> = conn
        .exec_first(
            "SELECT ENGINE, TABLE_COLLATION, AUTO_INCREMENT, TABLE_COMMENT, CREATE_OPTIONS, TABLE_ROWS, AVG_ROW_LENGTH, DATA_LENGTH, INDEX_LENGTH, DATA_FREE, \
                    ROW_FORMAT, CAST(CREATE_TIME AS CHAR), CAST(UPDATE_TIME AS CHAR), TABLE_TYPE \
             FROM information_schema.TABLES WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?",
            (database, table),
        )
        .await?;
    if let Some(r) = row {
        let collation = cell::<String>(&r, 1);
        o.info = TableInfo {
            is_view: cell::<String>(&r, 13).is_some_and(|t| t.contains("VIEW")),
            engine: cell(&r, 0),
            auto_increment: cell(&r, 2),
            comment: cell::<String>(&r, 3).unwrap_or_default(),
            partitioned: cell::<String>(&r, 4).is_some_and(|c| c.to_lowercase().contains("partitioned")),
            rows: cell(&r, 5),
            avg_row_length: cell(&r, 6),
            data_length: cell(&r, 7),
            index_length: cell(&r, 8),
            data_free: cell(&r, 9),
            row_format: cell(&r, 10),
            created: cell(&r, 11),
            updated: cell(&r, 12),
            charset: None,
            collation: collation.clone(),
        };
        if let Some(c) = collation {
            let cs: Option<Option<String>> = conn.exec_first("SELECT CHARACTER_SET_NAME FROM information_schema.COLLATIONS WHERE COLLATION_NAME = ?", (c.as_str(),)).await?;
            o.info.charset = cs.flatten().or_else(|| myq_schema::ddl::charset_of_collation(&c).map(String::from));
        }
    }

    // ---- columns
    let mariadb = crate::is_mariadb(&mut conn).await?;
    let rows: Vec<Row> = conn
        .exec(
            "SELECT COLUMN_NAME, ORDINAL_POSITION, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY, COLUMN_DEFAULT, EXTRA, COLUMN_COMMENT \
             FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION",
            (database, table),
        )
        .await?;
    for r in rows {
        if let (Some(name), Some(column_type)) = (cell::<String>(&r, 0), cell::<String>(&r, 2)) {
            o.columns.push(ColumnFull {
                name,
                ordinal: cell(&r, 1).unwrap_or(0),
                column_type,
                nullable: cell::<String>(&r, 3).is_some_and(|n| n == "YES"),
                key: cell(&r, 4).unwrap_or_default(),
                default: crate::parse_column_default(mariadb, cell::<String>(&r, 5).as_deref(), &cell::<String>(&r, 6).unwrap_or_default()).display(),
                extra: cell(&r, 6).unwrap_or_default(),
                comment: cell(&r, 7).unwrap_or_default(),
            });
        }
    }

    o.indexes = index_details(pool, database, table).await?;

    // ---- constraints (PRIMARY KEY / UNIQUE / FOREIGN KEY / CHECK) with their columns
    let mut cols_of: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let kcu: Vec<(String, String)> = conn
        .exec(
            "SELECT CONSTRAINT_NAME, COLUMN_NAME FROM information_schema.KEY_COLUMN_USAGE WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY CONSTRAINT_NAME, ORDINAL_POSITION",
            (database, table),
        )
        .await?;
    for (n, c) in kcu {
        cols_of.entry(n).or_default().push(c);
    }
    let tc: Vec<(String, String)> = conn
        .exec(
            "SELECT CONSTRAINT_NAME, CONSTRAINT_TYPE FROM information_schema.TABLE_CONSTRAINTS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY CONSTRAINT_TYPE = 'PRIMARY KEY' DESC, CONSTRAINT_NAME",
            (database, table),
        )
        .await?;
    o.constraints = tc.into_iter().map(|(name, kind)| Constraint { columns: cols_of.get(&name).cloned().unwrap_or_default(), name, kind }).collect();

    // ---- foreign keys, both directions
    let rules_rows: Vec<(String, String, String, String, String)> = conn
        .exec(
            "SELECT CONSTRAINT_SCHEMA, TABLE_NAME, CONSTRAINT_NAME, UPDATE_RULE, DELETE_RULE FROM information_schema.REFERENTIAL_CONSTRAINTS \
             WHERE (CONSTRAINT_SCHEMA = ? AND TABLE_NAME = ?) OR (UNIQUE_CONSTRAINT_SCHEMA = ? AND REFERENCED_TABLE_NAME = ?)",
            (database, table, database, table),
        )
        .await?;
    let rules: BTreeMap<(String, String, String), (String, String)> =
        rules_rows.into_iter().map(|(s, t, n, u, d)| ((s, t, n), (u, d))).collect();
    let out_rows: Vec<Row> = conn
        .exec(
            "SELECT CONSTRAINT_NAME, TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME, REFERENCED_TABLE_SCHEMA, REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME \
             FROM information_schema.KEY_COLUMN_USAGE WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND REFERENCED_TABLE_NAME IS NOT NULL ORDER BY CONSTRAINT_NAME, ORDINAL_POSITION",
            (database, table),
        )
        .await?;
    o.outgoing = group_fks(out_rows, &rules);
    let in_rows: Vec<Row> = conn
        .exec(
            "SELECT CONSTRAINT_NAME, TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME, REFERENCED_TABLE_SCHEMA, REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME \
             FROM information_schema.KEY_COLUMN_USAGE WHERE REFERENCED_TABLE_SCHEMA = ? AND REFERENCED_TABLE_NAME = ? ORDER BY TABLE_SCHEMA, TABLE_NAME, CONSTRAINT_NAME, ORDINAL_POSITION",
            (database, table),
        )
        .await?;
    o.incoming = group_fks(in_rows, &rules);

    // ---- triggers and partitions
    let trg: Vec<Row> = conn
        .exec(
            "SELECT TRIGGER_NAME, ACTION_TIMING, EVENT_MANIPULATION, ACTION_STATEMENT FROM information_schema.TRIGGERS \
             WHERE EVENT_OBJECT_SCHEMA = ? AND EVENT_OBJECT_TABLE = ? ORDER BY TRIGGER_NAME",
            (database, table),
        )
        .await?;
    for r in trg {
        if let Some(name) = cell::<String>(&r, 0) {
            o.triggers.push(TriggerInfo { name, timing: cell(&r, 1).unwrap_or_default(), event: cell(&r, 2).unwrap_or_default(), statement: cell(&r, 3).unwrap_or_default() });
        }
    }
    let parts: Vec<Row> = conn
        .exec(
            "SELECT PARTITION_NAME, PARTITION_METHOD, PARTITION_EXPRESSION, TABLE_ROWS, DATA_LENGTH FROM information_schema.PARTITIONS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND PARTITION_NAME IS NOT NULL ORDER BY PARTITION_ORDINAL_POSITION",
            (database, table),
        )
        .await?;
    for r in parts {
        if let Some(name) = cell::<String>(&r, 0) {
            o.partitions.push(PartitionInfo { name, method: cell(&r, 1), expression: cell(&r, 2), rows: cell(&r, 3), data_length: cell(&r, 4) });
        }
    }
    drop(conn);

    // ---- DDL (a view's definition for views)
    o.ddl = object_definition(pool, if o.info.is_view { Kind::View } else { Kind::Table }, database, table).await.unwrap_or(None);
    Ok(o)
}
