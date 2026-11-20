use crate::{Error, Result};
use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::Conn;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct ColumnMeta {
    pub name: String,
    pub data_type: String,
    pub column_type: String,
    pub nullable: bool,
    /// PRI / UNI / MUL / "".
    pub key: String,
    pub default: Option<String>,
    pub extra: String,
    pub comment: String,
    pub binary: bool,
    pub generated: bool,
    pub auto_increment: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForeignKey {
    pub name: String,
    pub columns: Vec<String>,
    pub ref_schema: String,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TableMeta {
    pub database: String,
    pub table: String,
    pub is_view: bool,
    pub columns: Vec<ColumnMeta>,
    pub primary_key: Vec<String>,
    /// Columns that identify one row for UPDATE/DELETE: the primary key, else the best unique key.
    /// Empty means the table cannot be edited safely.
    pub row_key: Vec<String>,
    pub foreign_keys: Vec<ForeignKey>,
    pub read_only_reason: Option<String>,
}

impl TableMeta {
    pub fn column(&self, name: &str) -> Result<&ColumnMeta> {
        self.columns.iter().find(|c| c.name == name).ok_or_else(|| Error::UnknownColumn(name.to_string()))
    }

    /// `` `db`.`table` ``
    pub fn qualified(&self) -> String {
        format!("{}.{}", crate::q(&self.database), crate::q(&self.table))
    }
}

const BINARY_TYPES: [&str; 7] = ["binary", "varbinary", "tinyblob", "blob", "mediumblob", "longblob", "geometry"];

pub async fn table_meta(conn: &mut Conn, database: &str, table: &str) -> Result<TableMeta> {
    let kind: Option<String> = conn
        .exec_first("SELECT TABLE_TYPE FROM information_schema.TABLES WHERE TABLE_SCHEMA=? AND TABLE_NAME=?", (database, table))
        .await?;
    let kind = kind.ok_or_else(|| Error::TableNotFound(format!("{database}.{table}")))?;
    let is_view = kind.contains("VIEW");

    let rows: Vec<(String, String, String, String, String, Option<String>, String, String)> = conn
        .exec(
            "SELECT COLUMN_NAME, DATA_TYPE, COLUMN_TYPE, IS_NULLABLE, COLUMN_KEY, COLUMN_DEFAULT, EXTRA, COLUMN_COMMENT
             FROM information_schema.COLUMNS WHERE TABLE_SCHEMA=? AND TABLE_NAME=? ORDER BY ORDINAL_POSITION",
            (database, table),
        )
        .await?;
    let columns: Vec<ColumnMeta> = rows
        .into_iter()
        .map(|(name, data_type, column_type, nullable, key, default, extra, comment)| ColumnMeta {
            binary: BINARY_TYPES.contains(&data_type.to_lowercase().as_str()),
            generated: extra.to_uppercase().contains("GENERATED"),
            auto_increment: extra.to_lowercase().contains("auto_increment"),
            nullable: nullable == "YES",
            data_type,
            column_type,
            key,
            default,
            extra,
            comment,
            name,
        })
        .collect();

    // Index columns grouped per index, in index order. Prefix indexes cannot identify a row.
    let idx: Vec<(String, i64, String, Option<i64>)> = conn
        .exec(
            "SELECT INDEX_NAME, NON_UNIQUE, COLUMN_NAME, SUB_PART FROM information_schema.STATISTICS
             WHERE TABLE_SCHEMA=? AND TABLE_NAME=? ORDER BY INDEX_NAME, SEQ_IN_INDEX",
            (database, table),
        )
        .await?;
    let mut unique: BTreeMap<String, (Vec<String>, bool)> = BTreeMap::new();
    for (name, non_unique, col, sub_part) in idx {
        if non_unique == 0 {
            let e = unique.entry(name).or_insert_with(|| (Vec::new(), true));
            e.0.push(col);
            e.1 &= sub_part.is_none();
        }
    }
    let nullable = |c: &str| columns.iter().find(|m| m.name == c).map_or(true, |m| m.nullable);
    let primary_key = unique.get("PRIMARY").map(|(c, _)| c.clone()).unwrap_or_default();
    let row_key = if !primary_key.is_empty() {
        primary_key.clone()
    } else {
        unique
            .values()
            .filter(|(cols, whole)| *whole && cols.iter().all(|c| !nullable(c)))
            .min_by_key(|(cols, _)| cols.len())
            .map(|(c, _)| c.clone())
            .unwrap_or_default()
    };

    let fk_rows: Vec<(String, String, String, String, String)> = conn
        .exec(
            "SELECT CONSTRAINT_NAME, COLUMN_NAME, REFERENCED_TABLE_SCHEMA, REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME
             FROM information_schema.KEY_COLUMN_USAGE
             WHERE TABLE_SCHEMA=? AND TABLE_NAME=? AND REFERENCED_TABLE_NAME IS NOT NULL
             ORDER BY CONSTRAINT_NAME, ORDINAL_POSITION",
            (database, table),
        )
        .await?;
    let mut foreign_keys: Vec<ForeignKey> = Vec::new();
    for (name, col, rs, rt, rc) in fk_rows {
        match foreign_keys.iter_mut().find(|f| f.name == name) {
            Some(f) => {
                f.columns.push(col);
                f.ref_columns.push(rc);
            }
            None => foreign_keys.push(ForeignKey { name, columns: vec![col], ref_schema: rs, ref_table: rt, ref_columns: vec![rc] }),
        }
    }

    let read_only_reason = if is_view {
        Some("views are read-only".to_string())
    } else if row_key.is_empty() {
        Some("table has no primary key or non-null unique key, so rows cannot be identified safely".to_string())
    } else {
        None
    };

    Ok(TableMeta { database: database.into(), table: table.into(), is_view, columns, primary_key, row_key, foreign_keys, read_only_reason })
}
