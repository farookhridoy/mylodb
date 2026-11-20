use crate::{Error, Result};
use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::Conn;
use myq_schema::ddl::{parse_column_type, ColumnDef, ColumnDefault, ForeignKeyDef, Generated, IndexColumn, IndexDef, IndexKind, RefAction, TableDef};
use serde::Serialize;

fn charset_of(collation: &str) -> String {
    collation.split('_').next().unwrap_or(collation).to_string()
}

fn unquote(v: &str) -> String {
    v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')).map_or_else(|| v.to_string(), |s| s.replace("''", "'").replace("\\'", "'"))
}

/// Read a base table's full definition from `information_schema`. Every object is marked as existing
/// (`original_name` set), so the designer can tell edits from additions.
/// Returns the definition and warnings for things the designer cannot represent (kept untouched).
pub async fn read_table_def(conn: &mut Conn, db: &str, table: &str) -> Result<(TableDef, Vec<String>)> {
    let head: Option<(String, Option<String>, Option<String>, String)> = conn
        .exec_first(
            "SELECT TABLE_TYPE, ENGINE, TABLE_COLLATION, TABLE_COMMENT FROM information_schema.TABLES WHERE TABLE_SCHEMA=? AND TABLE_NAME=?",
            (db, table),
        )
        .await?;
    let (kind, engine, collation, comment) = head.ok_or_else(|| Error::NotFound(format!("{db}.{table}")))?;
    if kind != "BASE TABLE" {
        return Err(Error::NotATable(format!("{db}.{table}")));
    }
    let version: Option<String> = conn.query_first("SELECT VERSION()").await?;
    let mariadb = version.unwrap_or_default().to_lowercase().contains("mariadb");
    let table_cs = collation.as_deref().map(charset_of);

    let cols: Vec<(String, String, String, Option<String>, String, Option<String>, Option<String>, String, Option<String>)> = conn
        .exec(
            "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_DEFAULT, EXTRA, CHARACTER_SET_NAME, COLLATION_NAME, COLUMN_COMMENT, GENERATION_EXPRESSION
             FROM information_schema.COLUMNS WHERE TABLE_SCHEMA=? AND TABLE_NAME=? ORDER BY ORDINAL_POSITION",
            (db, table),
        )
        .await?;
    let columns: Vec<ColumnDef> = cols
        .into_iter()
        .map(|(name, column_type, nullable, default, extra, cs, co, comment, gen_expr)| {
            let (data_type, params, unsigned, zerofill) = parse_column_type(&column_type);
            let extra_l = extra.to_lowercase();
            let generated = if extra_l.contains("virtual generated") || extra_l.contains("stored generated") || extra_l.contains("persistent") {
                Some(Generated { expression: gen_expr.unwrap_or_default(), stored: !extra_l.contains("virtual") })
            } else {
                None
            };
            let default = match (&default, generated.is_some()) {
                (_, true) | (None, _) => ColumnDefault::None,
                (Some(v), _) if extra_l.contains("default_generated") => ColumnDefault::Expression(v.clone()),
                (Some(v), _) if mariadb && v.eq_ignore_ascii_case("NULL") => ColumnDefault::Null,
                (Some(v), _) if mariadb && v.starts_with('\'') => ColumnDefault::Literal(unquote(v)),
                (Some(v), _) if mariadb && v.contains('(') => ColumnDefault::Expression(v.clone()),
                (Some(v), _) => ColumnDefault::Literal(v.clone()),
            };
            let on_update = extra_l.find("on update ").map(|i| extra[i + "on update ".len()..].trim().to_string());
            // A column that merely inherits the table's collation carries no charset of its own.
            let (charset, collation_c) = match (cs, co) {
                (Some(_), Some(c)) if Some(&c) == collation.as_ref() => (None, None),
                (cs, co) => (cs, co),
            };
            ColumnDef {
                original_name: Some(name.clone()),
                name,
                data_type,
                params,
                unsigned,
                zerofill,
                nullable: nullable == "YES",
                default,
                auto_increment: extra_l.contains("auto_increment"),
                charset,
                collation: collation_c,
                comment,
                on_update,
                generated,
                invisible: extra_l.contains("invisible"),
            }
        })
        .collect();

    let mut warnings = Vec::new();
    let stats: Vec<(String, i64, Option<String>, Option<i64>, Option<String>, String)> = conn
        .exec(
            "SELECT INDEX_NAME, NON_UNIQUE, COLUMN_NAME, SUB_PART, COLLATION, INDEX_TYPE FROM information_schema.STATISTICS
             WHERE TABLE_SCHEMA=? AND TABLE_NAME=? ORDER BY INDEX_NAME, SEQ_IN_INDEX",
            (db, table),
        )
        .await?;
    let mut indexes: Vec<IndexDef> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    for (name, non_unique, col, sub, coll, itype) in stats {
        let Some(col) = col else {
            if !skipped.contains(&name) {
                skipped.push(name.clone());
            }
            continue;
        };
        let column = IndexColumn { name: col, length: sub.map(|s| s as u32), desc: coll.as_deref() == Some("D") };
        match indexes.iter_mut().find(|i| i.name == name) {
            Some(i) => i.columns.push(column),
            None => {
                let kind = match (name.as_str(), itype.as_str(), non_unique) {
                    ("PRIMARY", _, _) => IndexKind::Primary,
                    (_, "FULLTEXT", _) => IndexKind::Fulltext,
                    (_, "SPATIAL", _) => IndexKind::Spatial,
                    (_, _, 0) => IndexKind::Unique,
                    _ => IndexKind::Index,
                };
                indexes.push(IndexDef { original_name: Some(name.clone()), name, kind, columns: vec![column] });
            }
        }
    }
    for s in skipped {
        indexes.retain(|i| i.name != s);
        warnings.push(format!("index `{s}` uses an expression and is not shown in the designer; it is left unchanged"));
    }

    let fk_rows: Vec<(String, String, String, String, String, String, String)> = conn
        .exec(
            "SELECT k.CONSTRAINT_NAME, k.COLUMN_NAME, k.REFERENCED_TABLE_SCHEMA, k.REFERENCED_TABLE_NAME, k.REFERENCED_COLUMN_NAME, r.UPDATE_RULE, r.DELETE_RULE
             FROM information_schema.KEY_COLUMN_USAGE k
             JOIN information_schema.REFERENTIAL_CONSTRAINTS r
               ON r.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA AND r.CONSTRAINT_NAME = k.CONSTRAINT_NAME AND r.TABLE_NAME = k.TABLE_NAME
             WHERE k.TABLE_SCHEMA=? AND k.TABLE_NAME=? AND k.REFERENCED_TABLE_NAME IS NOT NULL
             ORDER BY k.CONSTRAINT_NAME, k.ORDINAL_POSITION",
            (db, table),
        )
        .await?;
    let mut foreign_keys: Vec<ForeignKeyDef> = Vec::new();
    for (name, col, rs, rt, rc, upd, del) in fk_rows {
        match foreign_keys.iter_mut().find(|f| f.name == name) {
            Some(f) => {
                f.columns.push(col);
                f.ref_columns.push(rc);
            }
            None => foreign_keys.push(ForeignKeyDef {
                original_name: Some(name.clone()),
                name,
                columns: vec![col],
                ref_schema: (rs != db).then_some(rs),
                ref_table: rt,
                ref_columns: vec![rc],
                on_delete: RefAction::parse(&del),
                on_update: RefAction::parse(&upd),
            }),
        }
    }

    let def = TableDef { name: table.to_string(), engine, charset: table_cs, collation, comment, columns, indexes, foreign_keys };
    Ok((def, warnings))
}

#[derive(Debug, Serialize)]
pub struct ApplyError {
    /// Statements before this index ran successfully and are NOT rolled back (MySQL DDL is not transactional).
    pub index: usize,
    pub sql: String,
    pub message: String,
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "statement {} failed ({}): {}", self.index + 1, self.sql, self.message)?;
        if self.index > 0 {
            write!(f, " — the {} earlier statement(s) were already applied", self.index)?;
        }
        Ok(())
    }
}

/// Run statements in order, stopping at the first failure. Returns the number executed.
pub async fn run_statements(conn: &mut Conn, statements: &[String]) -> std::result::Result<usize, ApplyError> {
    // String literals in generated DDL escape backslashes, which NO_BACKSLASH_ESCAPES would misread.
    let fail = |index: usize, sql: &str, e: myq_driver::mysql_async::Error| ApplyError { index, sql: sql.to_string(), message: e.to_string() };
    conn.query_drop("SET SESSION sql_mode = REPLACE(@@SESSION.sql_mode, 'NO_BACKSLASH_ESCAPES', '')").await.map_err(|e| fail(0, "SET sql_mode", e))?;
    for (i, s) in statements.iter().enumerate() {
        conn.query_drop(s).await.map_err(|e| fail(i, s, e))?;
    }
    Ok(statements.len())
}
