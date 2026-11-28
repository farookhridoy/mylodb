//! The table designer for PostgreSQL: reading a table into the same `TableDef` the other engines use, and turning an edited one into
//! statements. PostgreSQL changes definitions inside a transaction, so a design is applied whole or not at all.
//!
//! What the designer cannot show (indexes on expressions, partial or non-btree indexes, deferrable constraints, check constraints,
//! triggers) is left as it is.

use crate::backend::{explain, PgBackend};
use crate::grid::q;
use myq_core::{Dialect, Postgres};
use myq_schema::ddl::{ColumnDef, ColumnDefault, ForeignKeyDef, Generated, IndexColumn, IndexDef, IndexKind, RefAction, TableDef};
use std::collections::{HashMap, HashSet};

type Result<T> = std::result::Result<T, String>;

fn lit(s: &str) -> String {
    Postgres.quote_string(s)
}

/// `character varying(20)` is the type `character varying` with `20`; in `timestamp(3) without time zone` the number sits inside.
fn parse_type(full: &str) -> (String, Option<String>) {
    if full.contains('"') {
        return (full.to_string(), None);
    }
    let (body, array) = match full.strip_suffix("[]") {
        Some(b) => (b, "[]"),
        None => (full, ""),
    };
    match body.find('(').zip(body.rfind(')')) {
        Some((open, close)) if open < close => (format!("{}{}{array}", &body[..open], &body[close + 1..]), Some(body[open + 1..close].to_string())),
        _ => (full.to_string(), None),
    }
}

fn type_sql(c: &ColumnDef) -> String {
    let Some(p) = c.params.as_deref().filter(|p| !p.is_empty()) else { return c.data_type.clone() };
    let (body, array) = match c.data_type.strip_suffix("[]") {
        Some(b) => (b, "[]"),
        None => (c.data_type.as_str(), ""),
    };
    match body.split_once(' ') {
        Some((first @ ("timestamp" | "time"), rest)) => format!("{first}({p}) {rest}{array}"),
        _ => format!("{body}({p}){array}"),
    }
}

/// The text of `'…'::type` when the whole default is one string literal.
fn literal_default(expr: &str) -> Option<String> {
    let rest = expr.strip_prefix('\'')?;
    let mut out = String::new();
    let mut chars = rest.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c != '\'' {
            out.push(c);
        } else if chars.peek().is_some_and(|(_, n)| *n == '\'') {
            out.push('\'');
            chars.next();
        } else {
            let tail = &rest[i + 1..];
            let cast_only = tail.is_empty() || tail.strip_prefix("::").is_some_and(|t| t.chars().all(|c| c.is_ascii_alphanumeric() || " _.\"[]".contains(c)));
            return cast_only.then_some(out);
        }
    }
    None
}

fn action(code: &str) -> RefAction {
    match code {
        "r" => RefAction::Restrict,
        "c" => RefAction::Cascade,
        "n" => RefAction::SetNull,
        "d" => RefAction::SetDefault,
        _ => RefAction::NoAction,
    }
}

/// A table as the designer edits it, and what was left out.
pub async fn read_table_def(b: &PgBackend, schema: &str, table: &str) -> Result<(TableDef, Vec<String>)> {
    let found = b
        .query("SELECT c.oid, c.relkind::text, COALESCE(pg_catalog.obj_description(c.oid, 'pg_class'), '') FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1 AND c.relname = $2", &[&schema, &table])
        .await
        .map_err(|e| e.0)?;
    let row = found.first().ok_or_else(|| format!("table `{schema}.{table}` not found"))?;
    let (oid, kind, comment): (u32, String, String) = (row.get(0), row.get(1), row.get(2));
    if kind != "r" && kind != "p" {
        return Err("only tables can be designed (a view is changed through its definition)".into());
    }
    let mut warnings = Vec::new();
    if kind == "p" {
        warnings.push("this table is partitioned: how it is partitioned is not shown and is left unchanged".to_string());
    }
    let rows = b
        .query(
            "SELECT a.attname::text, pg_catalog.format_type(a.atttypid, a.atttypmod), a.attnotnull, pg_catalog.pg_get_expr(d.adbin, d.adrelid), a.attidentity::text, a.attgenerated::text,
                CASE WHEN a.attcollation <> t.typcollation THEN (SELECT l.collname::text FROM pg_catalog.pg_collation l WHERE l.oid = a.attcollation) END,
                COALESCE(pg_catalog.col_description(a.attrelid, a.attnum), '')
             FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_type t ON t.oid = a.atttypid LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
             WHERE a.attrelid = $1 AND a.attnum > 0 AND NOT a.attisdropped ORDER BY a.attnum",
            &[&oid],
        )
        .await
        .map_err(|e| e.0)?;
    let columns = rows
        .iter()
        .map(|r| {
            let name: String = r.get(0);
            let (data_type, params) = parse_type(&r.get::<_, String>(1));
            let default: Option<String> = r.get(3);
            let identity: String = r.get(4);
            let generated: String = r.get(5);
            let serial = default.as_deref().is_some_and(|d| d.starts_with("nextval(")) && matches!(data_type.as_str(), "smallint" | "integer" | "bigint");
            let auto = serial || !identity.is_empty();
            ColumnDef {
                original_name: Some(name.clone()),
                name,
                data_type,
                params,
                nullable: !r.get::<_, bool>(2),
                generated: (!generated.is_empty()).then(|| Generated { expression: default.clone().unwrap_or_default(), stored: true }),
                default: match default {
                    Some(d) if !auto && generated.is_empty() => match literal_default(&d) {
                        Some(text) => ColumnDefault::Literal(text),
                        None => ColumnDefault::Expression(d),
                    },
                    _ => ColumnDefault::None,
                },
                auto_increment: auto,
                collation: r.get(6),
                comment: r.get(7),
                ..Default::default()
            }
        })
        .collect();
    let found = b
        .query(
            "SELECT x.relname::text, i.indisprimary, i.indisunique,
                m.amname <> 'btree' OR i.indexprs IS NOT NULL OR i.indpred IS NOT NULL OR i.indnatts <> i.indnkeyatts OR NOT i.indimmediate OR EXISTS (SELECT 1 FROM pg_catalog.pg_constraint e WHERE e.conindid = i.indexrelid AND e.contype = 'x'),
                ARRAY(SELECT a.attname::text FROM generate_series(0, i.indnkeyatts::int - 1) k JOIN pg_catalog.pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = i.indkey[k] ORDER BY k),
                ARRAY(SELECT (i.indoption[k] & 1) = 1 FROM generate_series(0, i.indnkeyatts::int - 1) k ORDER BY k)
             FROM pg_catalog.pg_index i JOIN pg_catalog.pg_class x ON x.oid = i.indexrelid JOIN pg_catalog.pg_am m ON m.oid = x.relam
             WHERE i.indrelid = $1 ORDER BY NOT i.indisprimary, 1",
            &[&oid],
        )
        .await
        .map_err(|e| e.0)?;
    let mut indexes = Vec::new();
    for r in &found {
        let (name, primary, unique, special): (String, bool, bool, bool) = (r.get(0), r.get(1), r.get(2), r.get(3));
        if special {
            warnings.push(format!("index `{name}` is of a kind the designer does not show (expression, partial, non-btree, included columns, deferrable or exclusion); it is left unchanged"));
            continue;
        }
        let (names, desc): (Vec<String>, Vec<bool>) = (r.get(4), r.get(5));
        indexes.push(IndexDef {
            original_name: Some(name.clone()),
            name: if primary { "PRIMARY".into() } else { name },
            kind: if primary { IndexKind::Primary } else if unique { IndexKind::Unique } else { IndexKind::Index },
            columns: names.into_iter().zip(desc).map(|(name, desc)| IndexColumn { name, length: None, desc }).collect(),
        });
    }
    let found = b
        .query(
            "SELECT k.conname::text, n.nspname::text, t.relname::text,
                ARRAY(SELECT a.attname::text FROM unnest(k.conkey) WITH ORDINALITY u(num, ord) JOIN pg_catalog.pg_attribute a ON a.attrelid = k.conrelid AND a.attnum = u.num ORDER BY u.ord),
                ARRAY(SELECT a.attname::text FROM unnest(k.confkey) WITH ORDINALITY u(num, ord) JOIN pg_catalog.pg_attribute a ON a.attrelid = k.confrelid AND a.attnum = u.num ORDER BY u.ord),
                k.confdeltype::text, k.confupdtype::text, k.condeferrable
             FROM pg_catalog.pg_constraint k JOIN pg_catalog.pg_class t ON t.oid = k.confrelid JOIN pg_catalog.pg_namespace n ON n.oid = t.relnamespace
             WHERE k.contype = 'f' AND k.conrelid = $1 ORDER BY 1",
            &[&oid],
        )
        .await
        .map_err(|e| e.0)?;
    let mut foreign_keys = Vec::new();
    for r in &found {
        let name: String = r.get(0);
        if r.get::<_, bool>(7) {
            warnings.push(format!("foreign key `{name}` is deferrable, which the designer does not show; it is left unchanged"));
            continue;
        }
        let ref_schema: String = r.get(1);
        foreign_keys.push(ForeignKeyDef {
            original_name: Some(name.clone()),
            name,
            columns: r.get(3),
            ref_schema: (ref_schema != schema).then_some(ref_schema),
            ref_table: r.get(2),
            ref_columns: r.get(4),
            on_delete: action(r.get::<_, String>(5).as_str()),
            on_update: action(r.get::<_, String>(6).as_str()),
        });
    }
    Ok((TableDef { name: table.to_string(), comment, columns, indexes, foreign_keys, ..Default::default() }, warnings))
}

fn is_integer(t: &str) -> bool {
    matches!(t.to_ascii_lowercase().as_str(), "smallint" | "integer" | "bigint" | "int" | "int2" | "int4" | "int8")
}

fn validate(original: Option<&TableDef>, t: &TableDef) -> Vec<String> {
    let mut e = Vec::new();
    if t.name.trim().is_empty() {
        e.push("table name is empty".to_string());
    }
    if t.columns.is_empty() {
        e.push("a table needs at least one column".to_string());
    }
    let mut seen = HashSet::new();
    for c in &t.columns {
        if c.name.trim().is_empty() {
            e.push("a column has no name".to_string());
        } else if !seen.insert(c.name.as_str()) {
            e.push(format!("duplicate column name `{}`", c.name));
        }
        if c.data_type.trim().is_empty() {
            e.push(format!("column `{}` has no type", c.name));
        }
        if !c.nullable && matches!(c.default, ColumnDefault::Null) {
            e.push(format!("column `{}` is NOT NULL but its default is NULL", c.name));
        }
        if c.auto_increment {
            if !is_integer(&c.data_type) {
                e.push(format!("column `{}`: only smallint, integer and bigint can number themselves", c.name));
            }
            if c.generated.is_some() || !matches!(c.default, ColumnDefault::None) {
                e.push(format!("column `{}` numbers itself, so it cannot also have a default or an expression", c.name));
            }
        }
        if c.generated.as_ref().is_some_and(|g| g.expression.trim().is_empty()) {
            e.push(format!("generated column `{}` has no expression", c.name));
        }
        let before = c.original_name.as_deref().and_then(|n| original.and_then(|o| o.columns.iter().find(|c| c.name == n)));
        if let (Some(old), Some(new)) = (before, &c.generated) {
            if old.generated.as_ref().map(|g| &g.expression) != Some(&new.expression) {
                e.push(format!("PostgreSQL cannot change the expression of an existing column: drop `{}` and add it again", c.name));
            }
        }
    }
    let exists = |n: &str| t.columns.iter().any(|c| c.name == n);
    let mut names = HashSet::new();
    for i in &t.indexes {
        if i.kind != IndexKind::Primary {
            if i.name.trim().is_empty() {
                e.push("an index has no name".to_string());
            } else if !names.insert(i.name.as_str()) {
                e.push(format!("duplicate index name `{}`", i.name));
            }
        }
        if matches!(i.kind, IndexKind::Fulltext | IndexKind::Spatial) {
            e.push(format!("index `{}`: choose primary, unique or index", i.name));
        }
        if i.columns.is_empty() {
            e.push(format!("index `{}` has no columns", i.name));
        }
        for c in &i.columns {
            if !exists(&c.name) {
                e.push(format!("index `{}` uses unknown column `{}`", i.name, c.name));
            }
        }
    }
    if t.indexes.iter().filter(|i| i.kind == IndexKind::Primary).count() > 1 {
        e.push("a table can have only one primary key".to_string());
    }
    let mut names = HashSet::new();
    for f in &t.foreign_keys {
        if f.name.trim().is_empty() {
            e.push("a foreign key has no name".to_string());
        } else if !names.insert(f.name.as_str()) {
            e.push(format!("duplicate foreign key name `{}`", f.name));
        }
        if f.columns.is_empty() || f.columns.len() != f.ref_columns.len() {
            e.push(format!("foreign key `{}` needs the same number of local and referenced columns", f.name));
        }
        if f.ref_table.trim().is_empty() {
            e.push(format!("foreign key `{}` has no referenced table", f.name));
        }
        for c in &f.columns {
            if !exists(c) {
                e.push(format!("foreign key `{}` uses unknown column `{c}`", f.name));
            }
        }
    }
    e
}

/// `"name" type COLLATE … NOT NULL DEFAULT …`, as CREATE TABLE and ADD COLUMN write it.
fn column_sql(c: &ColumnDef) -> String {
    let mut s = format!("{} {}", q(&c.name), type_sql(c));
    if let Some(co) = &c.collation {
        s.push_str(&format!(" COLLATE {}", q(co)));
    }
    if let Some(g) = &c.generated {
        s.push_str(&format!(" GENERATED ALWAYS AS ({}) STORED", g.expression));
    }
    if c.auto_increment {
        s.push_str(" GENERATED BY DEFAULT AS IDENTITY");
    }
    if !c.nullable {
        s.push_str(" NOT NULL");
    }
    if c.generated.is_none() {
        if let Some(d) = default_sql(&c.default) {
            s.push_str(&format!(" DEFAULT {d}"));
        }
    }
    s
}

fn default_sql(d: &ColumnDefault) -> Option<String> {
    match d {
        ColumnDefault::None | ColumnDefault::Null => None,
        ColumnDefault::Literal(v) => Some(lit(v)),
        ColumnDefault::Expression(e) => Some(e.trim().to_string()),
    }
}

fn index_columns(i: &IndexDef, desc_allowed: bool) -> String {
    i.columns.iter().map(|c| format!("{}{}", q(&c.name), if c.desc && desc_allowed { " DESC" } else { "" })).collect::<Vec<_>>().join(", ")
}

/// A primary key or a unique key written inside CREATE TABLE or ADD (a descending column needs an index of its own).
fn constraint_sql(i: &IndexDef) -> Option<String> {
    match i.kind {
        IndexKind::Primary => Some(format!("PRIMARY KEY ({})", index_columns(i, false))),
        IndexKind::Unique if !i.columns.iter().any(|c| c.desc) => Some(format!("CONSTRAINT {} UNIQUE ({})", q(&i.name), index_columns(i, false))),
        _ => None,
    }
}

fn create_index_sql(table: &str, i: &IndexDef) -> String {
    format!("CREATE {}INDEX {} ON {table} ({})", if i.kind == IndexKind::Unique { "UNIQUE " } else { "" }, q(&i.name), index_columns(i, true))
}

fn foreign_key_sql(schema: &str, f: &ForeignKeyDef) -> String {
    let list = |v: &[String]| v.iter().map(|c| q(c)).collect::<Vec<_>>().join(", ");
    format!(
        "CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {}.{} ({}) ON DELETE {} ON UPDATE {}",
        q(&f.name),
        list(&f.columns),
        q(f.ref_schema.as_deref().unwrap_or(schema)),
        q(&f.ref_table),
        list(&f.ref_columns),
        f.on_delete.sql(),
        f.on_update.sql()
    )
}

fn comment_on_column(table: &str, c: &ColumnDef) -> String {
    format!("COMMENT ON COLUMN {table}.{} IS {}", q(&c.name), if c.comment.is_empty() { "NULL".into() } else { lit(&c.comment) })
}

/// After a column starts numbering itself, the numbers continue after the highest one already there.
fn continue_numbers(table: &str, column: &str) -> String {
    format!("SELECT setval(pg_catalog.pg_get_serial_sequence({}, {}), COALESCE(max({}), 0) + 1, false) FROM {table}", lit(table), lit(column), q(column))
}

fn create(schema: &str, t: &TableDef) -> Vec<String> {
    let table = format!("{}.{}", q(schema), q(&t.name));
    let mut parts: Vec<String> = t.columns.iter().map(column_sql).collect();
    parts.extend(t.indexes.iter().filter_map(constraint_sql));
    parts.extend(t.foreign_keys.iter().map(|f| foreign_key_sql(schema, f)));
    let mut out = vec![format!("CREATE TABLE {table} (\n  {}\n)", parts.join(",\n  "))];
    out.extend(t.indexes.iter().filter(|i| constraint_sql(i).is_none()).map(|i| create_index_sql(&table, i)));
    if !t.comment.is_empty() {
        out.push(format!("COMMENT ON TABLE {table} IS {}", lit(&t.comment)));
    }
    out.extend(t.columns.iter().filter(|c| !c.comment.is_empty()).map(|c| comment_on_column(&table, c)));
    out
}

fn alter(schema: &str, old: &TableDef, new: &TableDef) -> Vec<String> {
    let table = format!("{}.{}", q(schema), q(&old.name));
    let mut out = Vec::new();
    // old column name -> new name, for the columns that stay
    let renamed: HashMap<&str, &str> = new.columns.iter().filter_map(|c| c.original_name.as_deref().map(|o| (o, c.name.as_str()))).collect();
    let now = |n: &String| renamed.get(n.as_str()).map_or_else(|| n.clone(), |s| s.to_string());
    let own = |f: &ForeignKeyDef| f.ref_schema.as_deref().unwrap_or(schema) == schema && (f.ref_table == old.name || f.ref_table == new.name);

    let same_index = |o: &IndexDef, n: &IndexDef| o.kind == n.kind && o.columns.len() == n.columns.len() && o.columns.iter().zip(&n.columns).all(|(a, b)| now(&a.name) == b.name && a.desc == b.desc);
    // the new definition of an old index, if it survives (perhaps under another name)
    let survivor = |o: &IndexDef| new.indexes.iter().find(|n| (n.original_name == o.original_name || (o.kind == IndexKind::Primary && n.kind == IndexKind::Primary)) && same_index(o, n));
    let same_fk = |o: &ForeignKeyDef, n: &ForeignKeyDef| {
        o.name == n.name
            && o.columns.iter().map(&now).collect::<Vec<_>>() == n.columns
            && o.ref_schema == n.ref_schema
            && (o.ref_table == n.ref_table || (own(o) && own(n)))
            && o.ref_columns.iter().map(|c| if own(o) { now(c) } else { c.clone() }).collect::<Vec<_>>() == n.ref_columns
            && o.on_delete == n.on_delete
            && o.on_update == n.on_update
    };
    let fk_kept = |o: &ForeignKeyDef| new.foreign_keys.iter().any(|n| n.original_name == o.original_name && same_fk(o, n));

    // 1. foreign keys, then indexes, that go or change
    for f in old.foreign_keys.iter().filter(|f| !fk_kept(f)) {
        out.push(format!("ALTER TABLE {table} DROP CONSTRAINT {}", q(f.original_name.as_deref().unwrap_or(&f.name))));
    }
    for i in old.indexes.iter().filter(|i| survivor(i).is_none()) {
        let real = i.original_name.as_deref().unwrap_or(&i.name);
        if i.kind == IndexKind::Primary {
            out.push(format!("ALTER TABLE {table} DROP CONSTRAINT {}", q(real)));
        } else {
            // a unique key may be a constraint or a plain index: the one that does not apply does nothing
            if i.kind == IndexKind::Unique {
                out.push(format!("ALTER TABLE {table} DROP CONSTRAINT IF EXISTS {}", q(real)));
            }
            out.push(format!("DROP INDEX IF EXISTS {}.{}", q(schema), q(real)));
        }
    }
    // 2. columns that go
    for c in old.columns.iter().filter(|o| !renamed.contains_key(o.name.as_str())) {
        out.push(format!("ALTER TABLE {table} DROP COLUMN {}", q(&c.name)));
    }
    // 3. new names; through temporary ones when names are exchanged
    let moves: Vec<(&str, &str)> = old.columns.iter().filter_map(|o| renamed.get(o.name.as_str()).filter(|n| **n != o.name).map(|n| (o.name.as_str(), *n))).collect();
    let exchange = moves.iter().any(|(_, to)| moves.iter().any(|(from, _)| from == to));
    let rename = |from: &str, to: &str| format!("ALTER TABLE {table} RENAME COLUMN {} TO {}", q(from), q(to));
    if exchange {
        out.extend(moves.iter().enumerate().map(|(i, (from, _))| rename(from, &format!("__renaming_{i}"))));
        out.extend(moves.iter().enumerate().map(|(i, (_, to))| rename(&format!("__renaming_{i}"), to)));
    } else {
        out.extend(moves.iter().map(|(from, to)| rename(from, to)));
    }
    // 4. changes to the columns that stay
    for n in &new.columns {
        let Some(o) = n.original_name.as_deref().and_then(|name| old.columns.iter().find(|c| c.name == name)) else { continue };
        let col = format!("ALTER TABLE {table} ALTER COLUMN {}", q(&n.name));
        let retyped = type_sql(o) != type_sql(n) || o.collation != n.collation;
        let redefault = o.default != n.default;
        if o.auto_increment && !n.auto_increment {
            // an identity column, or an old style serial one with its sequence as the default
            out.push(format!("{col} DROP IDENTITY IF EXISTS"));
            out.push(format!("{col} DROP DEFAULT"));
        }
        if o.generated.is_some() && n.generated.is_none() {
            out.push(format!("{col} DROP EXPRESSION"));
        }
        // a default may not fit the new type, so it is taken off and put back
        let had_default = default_sql(&o.default).is_some();
        if had_default && (retyped || redefault || n.auto_increment) {
            out.push(format!("{col} DROP DEFAULT"));
        }
        if retyped {
            let collate = n.collation.as_ref().map_or(String::new(), |c| format!(" COLLATE {}", q(c)));
            // a generated column is computed again; any other is converted
            let using = if n.generated.is_some() { String::new() } else { format!(" USING {}::{}", q(&n.name), type_sql(n)) };
            out.push(format!("{col} TYPE {}{collate}{using}", type_sql(n)));
        }
        if o.nullable != n.nullable {
            out.push(format!("{col} {} NOT NULL", if n.nullable { "DROP" } else { "SET" }));
        }
        if let Some(d) = default_sql(&n.default).filter(|_| n.generated.is_none() && (redefault || (retyped && had_default))) {
            out.push(format!("{col} SET DEFAULT {d}"));
        }
        if n.auto_increment && !o.auto_increment {
            if o.nullable == n.nullable && n.nullable {
                out.push(format!("{col} SET NOT NULL"));
            }
            out.push(format!("{col} ADD GENERATED BY DEFAULT AS IDENTITY"));
            out.push(continue_numbers(&table, &n.name));
        }
        if o.comment != n.comment {
            out.push(comment_on_column(&table, n));
        }
    }
    // 5. new columns (PostgreSQL adds them at the end)
    for n in new.columns.iter().filter(|c| c.original_name.is_none()) {
        out.push(format!("ALTER TABLE {table} ADD COLUMN {}", column_sql(n)));
        if !n.comment.is_empty() {
            out.push(comment_on_column(&table, n));
        }
    }
    // 6. indexes that are new, changed or renamed
    for o in &old.indexes {
        if let Some(n) = survivor(o).filter(|n| n.kind != IndexKind::Primary) {
            let real = o.original_name.as_deref().unwrap_or(&o.name);
            if n.name != real {
                out.push(format!("ALTER INDEX {}.{} RENAME TO {}", q(schema), q(real), q(&n.name)));
            }
        }
    }
    for n in new.indexes.iter().filter(|n| !old.indexes.iter().any(|o| survivor(o).is_some_and(|s| std::ptr::eq(s, *n)))) {
        out.push(match constraint_sql(n) {
            Some(c) => format!("ALTER TABLE {table} ADD {c}"),
            None => create_index_sql(&table, n),
        });
    }
    // 7. foreign keys that are new or changed
    for f in new.foreign_keys.iter().filter(|n| !old.foreign_keys.iter().any(|o| n.original_name == o.original_name && same_fk(o, n))) {
        // the table still has its old name here
        let mut f = f.clone();
        if own(&f) {
            f.ref_table = old.name.clone();
        }
        out.push(format!("ALTER TABLE {table} ADD {}", foreign_key_sql(schema, &f)));
    }
    if old.comment != new.comment {
        out.push(format!("COMMENT ON TABLE {table} IS {}", if new.comment.is_empty() { "NULL".into() } else { lit(&new.comment) }));
    }
    // 8. the table's own name last, so every statement above uses the current one
    if old.name != new.name {
        out.push(format!("ALTER TABLE {table} RENAME TO {}", q(&new.name)));
    }
    out
}

/// The statements a design would run, or everything that is wrong with it. Pure: needs no server.
pub fn ddl_statements(schema: &str, original: Option<&TableDef>, desired: &TableDef) -> std::result::Result<Vec<String>, Vec<String>> {
    let errors = validate(original, desired);
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(match original {
        Some(o) => alter(schema, o, desired),
        None => create(schema, desired),
    })
}

/// Run the statements in one transaction: all of them, or none.
pub async fn apply_ddl(b: &PgBackend, statements: &[String]) -> Result<()> {
    if b.read_only() {
        return Err("this connection is read-only".into());
    }
    let client = b.checkout().await.map_err(|e| e.0)?;
    let outcome = async {
        client.batch_execute("BEGIN").await.map_err(|e| explain(e).0)?;
        for s in statements {
            client.batch_execute(s).await.map_err(|e| format!("{} (while running: {}). Nothing was changed.", explain(e).0, s.lines().next().unwrap_or("")))?;
        }
        client.batch_execute("COMMIT").await.map_err(|e| format!("{}. Nothing was changed.", explain(e).0))
    }
    .await;
    // rolls back whatever was not committed
    b.give_back(client).await;
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_split_and_join() {
        for (full, base, params) in [
            ("integer", "integer", None),
            ("character varying(20)", "character varying", Some("20")),
            ("numeric(10,2)[]", "numeric[]", Some("10,2")),
            ("timestamp(3) without time zone", "timestamp without time zone", Some("3")),
            ("timestamp with time zone", "timestamp with time zone", None),
            ("\"Odd(type)\"", "\"Odd(type)\"", None),
        ] {
            let (b, p) = parse_type(full);
            assert_eq!((b.as_str(), p.as_deref()), (base, params), "{full}");
            assert_eq!(type_sql(&ColumnDef { data_type: b, params: p, ..Default::default() }), full);
        }
    }

    #[test]
    fn only_a_whole_literal_is_a_literal() {
        assert_eq!(literal_default("'it''s'::character varying"), Some("it's".into()));
        assert_eq!(literal_default("'a'::text || 'b'::text"), None);
        assert_eq!(literal_default("now()"), None);
    }
}
