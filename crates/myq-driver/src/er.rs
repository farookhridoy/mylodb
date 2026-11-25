//! The data behind the whole-database ER diagram: every base table with its columns, and every foreign key, read in three queries
//! (not one per table), so a database with hundreds of tables loads at once.

use crate::Result;
use mysql_async::{prelude::Queryable, Pool};
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ErColumn {
    pub name: String,
    pub column_type: String,
    pub primary: bool,
    pub nullable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ErTable {
    pub name: String,
    pub columns: Vec<ErColumn>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ErRelation {
    pub name: String,
    pub table: String,
    pub columns: Vec<String>,
    /// The referenced database (may differ from this one).
    pub ref_schema: String,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
    pub on_delete: String,
    pub on_update: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ErModel {
    pub tables: Vec<ErTable>,
    pub relations: Vec<ErRelation>,
}

/// Only the foreign keys of one database (what autocomplete needs for join suggestions); the rules are left empty.
pub async fn foreign_keys(pool: &Pool, db: &str) -> Result<Vec<ErRelation>> {
    let mut conn = pool.get_conn().await?;
    let usage: Vec<(String, String, String, String, String, String)> = conn
        .exec(
            "SELECT TABLE_NAME, CONSTRAINT_NAME, COLUMN_NAME, REFERENCED_TABLE_SCHEMA, REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME FROM information_schema.KEY_COLUMN_USAGE
             WHERE TABLE_SCHEMA = ? AND REFERENCED_TABLE_NAME IS NOT NULL ORDER BY TABLE_NAME, CONSTRAINT_NAME, ORDINAL_POSITION",
            (db,),
        )
        .await?;
    let mut relations: Vec<ErRelation> = Vec::new();
    for (table, name, col, ref_schema, ref_table, ref_col) in usage {
        match relations.iter_mut().find(|r| r.table == table && r.name == name) {
            Some(r) => {
                r.columns.push(col);
                r.ref_columns.push(ref_col);
            }
            None => relations.push(ErRelation { name, table, columns: vec![col], ref_schema, ref_table, ref_columns: vec![ref_col], on_delete: String::new(), on_update: String::new() }),
        }
    }
    Ok(relations)
}

pub async fn er_model(pool: &Pool, db: &str) -> Result<ErModel> {
    let mut conn = pool.get_conn().await?;
    let cols: Vec<(String, String, String, String, String)> = conn
        .exec(
            "SELECT c.TABLE_NAME, c.COLUMN_NAME, c.COLUMN_TYPE, c.COLUMN_KEY, c.IS_NULLABLE FROM information_schema.COLUMNS c
             JOIN information_schema.TABLES t ON t.TABLE_SCHEMA = c.TABLE_SCHEMA AND t.TABLE_NAME = c.TABLE_NAME
             WHERE c.TABLE_SCHEMA = ? AND t.TABLE_TYPE = 'BASE TABLE' ORDER BY c.TABLE_NAME, c.ORDINAL_POSITION",
            (db,),
        )
        .await?;
    let mut tables: Vec<ErTable> = Vec::new();
    for (t, name, column_type, key, nullable) in cols {
        let col = ErColumn { name, column_type, primary: key == "PRI", nullable: nullable == "YES" };
        match tables.last_mut() {
            Some(last) if last.name == t => last.columns.push(col),
            _ => tables.push(ErTable { name: t, columns: vec![col] }),
        }
    }
    let rules: Vec<(String, String, String, String)> = conn
        .exec("SELECT TABLE_NAME, CONSTRAINT_NAME, UPDATE_RULE, DELETE_RULE FROM information_schema.REFERENTIAL_CONSTRAINTS WHERE CONSTRAINT_SCHEMA = ?", (db,))
        .await?;
    let usage: Vec<(String, String, String, String, String, String)> = conn
        .exec(
            "SELECT TABLE_NAME, CONSTRAINT_NAME, COLUMN_NAME, REFERENCED_TABLE_SCHEMA, REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME FROM information_schema.KEY_COLUMN_USAGE
             WHERE TABLE_SCHEMA = ? AND REFERENCED_TABLE_NAME IS NOT NULL ORDER BY TABLE_NAME, CONSTRAINT_NAME, ORDINAL_POSITION",
            (db,),
        )
        .await?;
    let mut relations: Vec<ErRelation> = Vec::new();
    for (table, name, col, ref_schema, ref_table, ref_col) in usage {
        match relations.iter_mut().find(|r| r.table == table && r.name == name) {
            Some(r) => {
                r.columns.push(col);
                r.ref_columns.push(ref_col);
            }
            None => {
                let (on_update, on_delete) = rules.iter().find(|r| r.0 == table && r.1 == name).map(|r| (r.2.clone(), r.3.clone())).unwrap_or_default();
                relations.push(ErRelation { name, table, columns: vec![col], ref_schema, ref_table, ref_columns: vec![ref_col], on_delete, on_update });
            }
        }
    }
    Ok(ErModel { tables, relations })
}
