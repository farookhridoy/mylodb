//! The table designer for a SQLite file: read a table's definition, turn an edited definition into DDL, apply it atomically.
//!
//! SQLite can only add, rename and drop columns in place. Any other change (a column's type, NULL or default, the primary key, a
//! foreign key) is done the way the SQLite documentation prescribes: build the new table, copy the rows, drop the old one, rename
//! the new one, recreate the indexes. `apply_ddl` runs the whole list in one transaction with foreign keys off, and checks them
//! at the end, so a failing change leaves the table as it was.

use crate::backend::open;
use myq_schema::ddl::{parse_column_type, ColumnDef, ColumnDefault, ForeignKeyDef, IndexColumn, IndexDef, IndexKind, RefAction, TableDef};
use rusqlite::Connection;
use std::path::Path;

fn q(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn lit(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn is_number(s: &str) -> bool {
    !s.is_empty() && s.parse::<f64>().is_ok() && !s.eq_ignore_ascii_case("nan") && !s.eq_ignore_ascii_case("inf")
}

// ---------- reading ----------

fn parse_default(raw: Option<String>) -> ColumnDefault {
    let Some(d) = raw else { return ColumnDefault::None };
    let t = d.trim();
    if t.eq_ignore_ascii_case("null") {
        ColumnDefault::Null
    } else if t.len() >= 2 && t.starts_with('\'') && t.ends_with('\'') {
        ColumnDefault::Literal(t[1..t.len() - 1].replace("''", "'"))
    } else if is_number(t) {
        ColumnDefault::Literal(t.to_string())
    } else {
        ColumnDefault::Expression(t.trim_start_matches('(').trim_end_matches(')').to_string())
    }
}

fn ref_action(s: &str) -> RefAction {
    RefAction::parse(s)
}

/// The definition of one table, with every column, index and foreign key carrying its current name as `original_name`.
pub fn read_table_def(path: &Path, table: &str) -> Result<TableDef, String> {
    let conn = open(path, false).map_err(|e| e.to_string())?;
    let e = |x: rusqlite::Error| x.to_string();
    let create_sql: Option<String> = conn.query_row("SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1", [table], |r| r.get(0)).ok();
    let Some(create_sql) = create_sql else { return Err(format!("table {table} does not exist")) };
    let autoinc = create_sql.to_uppercase().contains("AUTOINCREMENT");

    let mut cols: Vec<(ColumnDef, i64)> = Vec::new();
    {
        let mut st = conn.prepare(&format!("PRAGMA table_info({})", q(table))).map_err(e)?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?.unwrap_or_default(), r.get::<_, i64>(3)?, r.get::<_, Option<String>>(4)?, r.get::<_, i64>(5)?))).map_err(e)?;
        for row in rows {
            let (name, ty, notnull, dflt, pk) = row.map_err(e)?;
            let (base, params, _, _) = parse_column_type(&ty);
            cols.push((
                ColumnDef { name: name.clone(), original_name: Some(name), data_type: base, params, nullable: notnull == 0 && pk == 0, default: parse_default(dflt), ..Default::default() },
                pk,
            ));
        }
    }
    let mut pk_cols: Vec<(i64, String)> = cols.iter().filter(|(_, pk)| *pk > 0).map(|(c, pk)| (*pk, c.name.clone())).collect();
    pk_cols.sort();
    if autoinc && pk_cols.len() == 1 {
        if let Some((c, _)) = cols.iter_mut().find(|(c, _)| c.name == pk_cols[0].1) {
            c.auto_increment = true;
        }
    }
    let mut indexes: Vec<IndexDef> = Vec::new();
    if !pk_cols.is_empty() {
        indexes.push(IndexDef { name: "PRIMARY".into(), original_name: Some("PRIMARY".into()), kind: IndexKind::Primary, columns: pk_cols.iter().map(|(_, n)| IndexColumn { name: n.clone(), length: None, desc: false }).collect() });
    }
    {
        let mut st = conn.prepare(&format!("PRAGMA index_list({})", q(table))).map_err(e)?;
        let list: Vec<(String, i64, String)> = st.query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, String>(3)?))).map_err(e)?.collect::<Result<_, _>>().map_err(e)?;
        for (name, unique, origin) in list {
            if origin == "pk" {
                continue;
            }
            let mut xs = conn.prepare(&format!("PRAGMA index_xinfo({})", q(&name))).map_err(e)?;
            // seqno, cid, name, desc, coll, key
            let cs: Vec<(Option<String>, i64, i64)> = xs.query_map([], |r| Ok((r.get::<_, Option<String>>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(5)?))).map_err(e)?.collect::<Result<_, _>>().map_err(e)?;
            let key_cols: Vec<IndexColumn> = cs.iter().filter(|(_, _, key)| *key == 1).filter_map(|(n, d, _)| n.clone().map(|n| IndexColumn { name: n, length: None, desc: *d == 1 })).collect();
            if key_cols.is_empty() {
                continue; // an expression index: not shown, and left alone when only other things change
            }
            indexes.push(IndexDef { name: name.clone(), original_name: Some(name), kind: if unique == 1 { IndexKind::Unique } else { IndexKind::Index }, columns: key_cols });
        }
    }
    let mut fks: Vec<ForeignKeyDef> = Vec::new();
    {
        let mut st = conn.prepare(&format!("PRAGMA foreign_key_list({})", q(table))).map_err(e)?;
        // id, seq, table, from, to, on_update, on_delete
        let rows: Vec<(i64, String, String, Option<String>, String, String)> = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, Option<String>>(4)?, r.get::<_, String>(5)?, r.get::<_, String>(6)?))).map_err(e)?.collect::<Result<_, _>>().map_err(e)?;
        for (id, rt, from, to, on_update, on_delete) in rows {
            let to = match to {
                Some(t) => t,
                None => conn.query_row(&format!("SELECT name FROM pragma_table_info({}) WHERE pk = 1", lit(&rt)), [], |r| r.get::<_, String>(0)).unwrap_or_else(|_| "rowid".into()),
            };
            match fks.iter_mut().find(|f| f.name == format!("fk_{table}_{}", id + 1)) {
                Some(f) => {
                    f.columns.push(from);
                    f.ref_columns.push(to);
                }
                None => fks.push(ForeignKeyDef { name: format!("fk_{table}_{}", id + 1), original_name: Some(format!("fk_{table}_{}", id + 1)), columns: vec![from], ref_schema: None, ref_table: rt, ref_columns: vec![to], on_delete: ref_action(&on_delete), on_update: ref_action(&on_update) }),
            }
        }
    }
    Ok(TableDef { name: table.to_string(), engine: None, charset: None, collation: None, comment: String::new(), columns: cols.into_iter().map(|(c, _)| c).collect(), indexes, foreign_keys: fks })
}

// ---------- validation ----------

fn validate(t: &TableDef) -> Vec<String> {
    let mut e = Vec::new();
    if t.name.trim().is_empty() {
        e.push("table name is empty".into());
    }
    if t.name.to_lowercase().starts_with("sqlite_") {
        e.push("table names starting with sqlite_ are reserved".into());
    }
    if t.columns.is_empty() {
        e.push("a table needs at least one column".into());
    }
    let mut seen = std::collections::HashSet::new();
    for c in &t.columns {
        if c.name.trim().is_empty() {
            e.push("a column has no name".into());
        } else if !seen.insert(c.name.to_lowercase()) {
            e.push(format!("column {} appears twice", c.name));
        }
        if c.data_type.trim().is_empty() {
            e.push(format!("column {} has no type", c.name));
        }
        if c.generated.is_some() {
            e.push(format!("column {}: generated columns are not available in this designer", c.name));
        }
    }
    let has = |n: &str| t.columns.iter().any(|c| c.name == n);
    let pks: Vec<&IndexDef> = t.indexes.iter().filter(|i| i.kind == IndexKind::Primary).collect();
    if pks.len() > 1 {
        e.push("a table has one primary key".into());
    }
    for i in &t.indexes {
        if matches!(i.kind, IndexKind::Fulltext | IndexKind::Spatial) {
            e.push(format!("index {}: SQLite has no {:?} indexes", i.name, i.kind).to_lowercase());
        }
        if i.columns.is_empty() {
            e.push(format!("index {} has no columns", i.name));
        }
        for c in &i.columns {
            if !has(&c.name) {
                e.push(format!("index {} uses the missing column {}", i.name, c.name));
            }
        }
        if i.kind != IndexKind::Primary && i.name.to_lowercase().starts_with("sqlite_") && !i.name.to_lowercase().starts_with("sqlite_autoindex") {
            e.push(format!("index names starting with sqlite_ are reserved ({})", i.name));
        }
    }
    for c in t.columns.iter().filter(|c| c.auto_increment) {
        let sole = pks.first().is_some_and(|p| p.columns.len() == 1 && p.columns[0].name == c.name);
        if !sole || !c.data_type.eq_ignore_ascii_case("integer") {
            e.push(format!("column {}: AUTOINCREMENT needs an INTEGER column that is the whole primary key", c.name));
        }
    }
    for f in &t.foreign_keys {
        if f.columns.is_empty() || f.ref_table.is_empty() || f.columns.len() != f.ref_columns.len() || f.ref_columns.iter().any(|c| c.is_empty()) {
            e.push(format!("foreign key {} is incomplete", f.name));
        }
        for c in &f.columns {
            if !has(c) {
                e.push(format!("foreign key {} uses the missing column {}", f.name, c));
            }
        }
    }
    e
}

// ---------- rendering ----------

fn type_sql(c: &ColumnDef) -> String {
    match c.params.as_deref().filter(|p| !p.is_empty()) {
        Some(p) => format!("{}({p})", c.data_type),
        None => c.data_type.clone(),
    }
}

fn default_sql(d: &ColumnDefault) -> Option<String> {
    match d {
        ColumnDefault::None => None,
        ColumnDefault::Null => Some("DEFAULT NULL".into()),
        ColumnDefault::Literal(v) if is_number(v) => Some(format!("DEFAULT {v}")),
        ColumnDefault::Literal(v) => Some(format!("DEFAULT {}", lit(v))),
        ColumnDefault::Expression(e) => {
            let u = e.trim().to_uppercase();
            Some(if ["CURRENT_TIMESTAMP", "CURRENT_DATE", "CURRENT_TIME"].contains(&u.as_str()) { format!("DEFAULT {u}") } else { format!("DEFAULT ({})", e.trim()) })
        }
    }
}

fn column_sql(c: &ColumnDef, inline_pk: bool) -> String {
    let mut s = format!("{} {}", q(&c.name), type_sql(c));
    if inline_pk {
        s.push_str(" PRIMARY KEY AUTOINCREMENT");
    } else if !c.nullable {
        s.push_str(" NOT NULL");
    }
    if let Some(d) = default_sql(&c.default) {
        s.push(' ');
        s.push_str(&d);
    }
    s
}

fn index_cols(i: &IndexDef) -> String {
    i.columns.iter().map(|c| format!("{}{}", q(&c.name), if c.desc { " DESC" } else { "" })).collect::<Vec<_>>().join(", ")
}

fn fk_sql(f: &ForeignKeyDef) -> String {
    let cols = |v: &[String]| v.iter().map(|c| q(c)).collect::<Vec<_>>().join(", ");
    format!("CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {} ON UPDATE {}", q(&f.name), cols(&f.columns), q(&f.ref_table), cols(&f.ref_columns), f.on_delete.sql(), f.on_update.sql())
}

/// An automatic unique index (`sqlite_autoindex_…`) cannot be created by name; it is a UNIQUE constraint of the table.
fn is_inline_unique(i: &IndexDef) -> bool {
    i.kind == IndexKind::Unique && i.name.to_lowercase().starts_with("sqlite_autoindex")
}

fn create_table_sql(t: &TableDef, as_name: &str) -> String {
    let pk = t.indexes.iter().find(|i| i.kind == IndexKind::Primary);
    let auto_col = t.columns.iter().find(|c| c.auto_increment).map(|c| c.name.clone());
    let mut parts: Vec<String> = t.columns.iter().map(|c| column_sql(c, auto_col.as_deref() == Some(c.name.as_str()))).collect();
    if let Some(p) = pk.filter(|p| !(p.columns.len() == 1 && auto_col.as_deref() == Some(p.columns[0].name.as_str()))) {
        parts.push(format!("PRIMARY KEY ({})", index_cols(p)));
    }
    for i in t.indexes.iter().filter(|i| is_inline_unique(i)) {
        parts.push(format!("UNIQUE ({})", index_cols(i)));
    }
    parts.extend(t.foreign_keys.iter().map(fk_sql));
    format!("CREATE TABLE {} (\n  {}\n)", q(as_name), parts.join(",\n  "))
}

fn create_index_sql(table: &str, i: &IndexDef) -> String {
    format!("CREATE {}INDEX {} ON {} ({})", if i.kind == IndexKind::Unique { "UNIQUE " } else { "" }, q(&i.name), q(table), index_cols(i))
}

fn plain_indexes(t: &TableDef) -> impl Iterator<Item = &IndexDef> {
    t.indexes.iter().filter(|i| matches!(i.kind, IndexKind::Index | IndexKind::Unique) && !is_inline_unique(i))
}

/// The statements for a new table (`original` is `None`) or for turning `original` into `desired`.
pub fn ddl_statements(original: Option<&TableDef>, desired: &TableDef) -> Result<Vec<String>, Vec<String>> {
    let errors = validate(desired);
    if !errors.is_empty() {
        return Err(errors);
    }
    let Some(old) = original else {
        let mut out = vec![create_table_sql(desired, &desired.name)];
        out.extend(plain_indexes(desired).map(|i| create_index_sql(&desired.name, i)));
        return Ok(out);
    };

    let old_col = |name: &str| old.columns.iter().find(|c| c.name == name);
    let same_shape = |a: &ColumnDef, b: &ColumnDef| a.data_type.eq_ignore_ascii_case(&b.data_type) && a.params.as_deref().unwrap_or("") == b.params.as_deref().unwrap_or("") && a.nullable == b.nullable && a.default == b.default && a.auto_increment == b.auto_increment;
    let pk_of = |t: &TableDef| t.indexes.iter().find(|i| i.kind == IndexKind::Primary).map(|p| p.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>()).unwrap_or_default();
    let fk_key = |t: &TableDef| t.foreign_keys.iter().map(|f| (f.columns.clone(), f.ref_table.clone(), f.ref_columns.clone(), f.on_delete, f.on_update)).collect::<Vec<_>>();
    // desired columns paired with the column they were
    let kept: Vec<(&ColumnDef, Option<&ColumnDef>)> = desired.columns.iter().map(|c| (c, c.original_name.as_deref().and_then(old_col))).collect();
    let dropped: Vec<&ColumnDef> = old.columns.iter().filter(|o| !kept.iter().any(|(_, was)| was.is_some_and(|w| w.name == o.name))).collect();
    let added: Vec<&ColumnDef> = kept.iter().filter(|(_, was)| was.is_none()).map(|(c, _)| *c).collect();
    let in_place_ok = kept.iter().all(|(c, was)| was.map_or(true, |w| same_shape(c, w)))
        && pk_of(old) == pk_of(desired).iter().map(|n| kept.iter().find(|(c, _)| &c.name == n).and_then(|(_, w)| w.map(|w| w.name.clone())).unwrap_or_else(|| n.clone())).collect::<Vec<_>>()
        && fk_key(old) == fk_key(desired)
        && added.iter().all(|c| (c.nullable || !matches!(c.default, ColumnDefault::None)) && !pk_of(desired).contains(&c.name) && !desired.indexes.iter().any(|i| is_inline_unique(i) && i.columns.iter().any(|x| x.name == c.name)))
        && dropped.iter().all(|c| !pk_of(old).contains(&c.name) && !old.foreign_keys.iter().any(|f| f.columns.contains(&c.name)) && !old.indexes.iter().any(|i| is_inline_unique(i) && i.columns.iter().any(|x| x.name == c.name)))
        && old.indexes.iter().filter(|i| is_inline_unique(i)).all(|i| desired.indexes.iter().any(|d| is_inline_unique(d) && d.original_name == i.original_name && d.columns == i.columns));

    let mut out = Vec::new();
    if in_place_ok {
        // indexes that were removed or changed go first (a column cannot be dropped while an index uses it)
        let same = |o: &IndexDef, d: &IndexDef| o.name == d.name && o.kind == d.kind && o.columns == d.columns;
        let gone: Vec<&IndexDef> = plain_indexes(old).filter(|o| !plain_indexes(desired).any(|d| d.original_name.as_deref() == Some(o.name.as_str()) && same(o, d))).collect();
        for i in &gone {
            out.push(format!("DROP INDEX {}", q(&i.name)));
        }
        for (c, was) in &kept {
            if let Some(w) = was.filter(|w| w.name != c.name) {
                out.push(format!("ALTER TABLE {} RENAME COLUMN {} TO {}", q(&old.name), q(&w.name), q(&c.name)));
            }
        }
        for c in &added {
            out.push(format!("ALTER TABLE {} ADD COLUMN {}", q(&old.name), column_sql(c, false)));
        }
        for c in &dropped {
            out.push(format!("ALTER TABLE {} DROP COLUMN {}", q(&old.name), q(&c.name)));
        }
        if old.name != desired.name {
            out.push(format!("ALTER TABLE {} RENAME TO {}", q(&old.name), q(&desired.name)));
        }
        for d in plain_indexes(desired).filter(|d| !plain_indexes(old).any(|o| d.original_name.as_deref() == Some(o.name.as_str()) && same(o, d))) {
            out.push(create_index_sql(&desired.name, d));
        }
        return Ok(out);
    }

    // rebuild: new table, copy the rows the columns that survive, drop the old table, rename, recreate the indexes
    let temp = format!("__new_{}", desired.name);
    out.push(create_table_sql(desired, &temp));
    let pairs: Vec<(String, String)> = kept.iter().filter_map(|(c, was)| was.map(|w| (c.name.clone(), w.name.clone()))).collect();
    if !pairs.is_empty() {
        out.push(format!("INSERT INTO {} ({}) SELECT {} FROM {}", q(&temp), pairs.iter().map(|(n, _)| q(n)).collect::<Vec<_>>().join(", "), pairs.iter().map(|(_, o)| q(o)).collect::<Vec<_>>().join(", "), q(&old.name)));
    }
    out.push(format!("DROP TABLE {}", q(&old.name)));
    out.push(format!("ALTER TABLE {} RENAME TO {}", q(&temp), q(&desired.name)));
    out.extend(plain_indexes(desired).map(|i| create_index_sql(&desired.name, i)));
    Ok(out)
}

// ---------- applying ----------

/// Run the statements in one transaction with foreign keys off, then check the foreign keys. Nothing is kept if any step fails.
pub fn apply_ddl(path: &Path, statements: &[String]) -> Result<(), String> {
    let conn: Connection = open(path, true).map_err(|e| e.to_string())?;
    conn.pragma_update(None, "foreign_keys", false).map_err(|e| e.to_string())?;
    let result = (|| -> Result<(), String> {
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        for s in statements {
            tx.execute_batch(s).map_err(|e| format!("{e} (while running: {})", s.lines().next().unwrap_or("")))?;
        }
        let mut st = tx.prepare("PRAGMA foreign_key_check").map_err(|e| e.to_string())?;
        let bad: Vec<String> = st.query_map([], |r| Ok(format!("{} row {:?} points at a missing {}", r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?, r.get::<_, String>(2)?))).map_err(|e| e.to_string())?.filter_map(|r| r.ok()).take(3).collect();
        if !bad.is_empty() {
            return Err(format!("the change was not applied: it would leave rows without their parent ({})", bad.join("; ")));
        }
        drop(st);
        tx.commit().map_err(|e| e.to_string())
    })();
    let _ = conn.pragma_update(None, "foreign_keys", true);
    result
}
