//! Table definitions and DDL generation.
//!
//! `TableDef` is the single model for the table designer: it is read from the server, edited by the
//! user, and turned into `CREATE TABLE` or a minimal sequence of `ALTER TABLE` statements by diffing
//! the original against the edited definition. Existing objects carry their `original_name`, which is
//! how renames and moves are told apart from drop + add.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ColumnDefault {
    /// No DEFAULT clause. Equivalent to `Null` for a nullable column.
    #[default]
    None,
    Null,
    /// A string/number literal, stored unquoted.
    Literal(String),
    /// A function or expression such as `CURRENT_TIMESTAMP` or `uuid()`.
    Expression(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Generated {
    pub expression: String,
    pub stored: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ColumnDef {
    pub name: String,
    /// The column's name on the server; `None` for a column that is being added.
    #[serde(default)]
    pub original_name: Option<String>,
    /// Lower-case base type, e.g. `varchar`, `int`, `decimal`, `enum`.
    pub data_type: String,
    /// Text inside the parentheses: `40`, `10,2`, `'a','b'`.
    #[serde(default)]
    pub params: Option<String>,
    #[serde(default)]
    pub unsigned: bool,
    #[serde(default)]
    pub zerofill: bool,
    #[serde(default)]
    pub nullable: bool,
    #[serde(default)]
    pub default: ColumnDefault,
    #[serde(default)]
    pub auto_increment: bool,
    /// Set only when it differs from the table's own character set / collation.
    #[serde(default)]
    pub charset: Option<String>,
    #[serde(default)]
    pub collation: Option<String>,
    #[serde(default)]
    pub comment: String,
    /// e.g. `CURRENT_TIMESTAMP` for `ON UPDATE CURRENT_TIMESTAMP`.
    #[serde(default)]
    pub on_update: Option<String>,
    #[serde(default)]
    pub generated: Option<Generated>,
    #[serde(default)]
    pub invisible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexKind {
    Primary,
    Unique,
    Index,
    Fulltext,
    Spatial,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexColumn {
    pub name: String,
    /// Prefix length for string columns.
    #[serde(default)]
    pub length: Option<u32>,
    #[serde(default)]
    pub desc: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexDef {
    pub name: String,
    #[serde(default)]
    pub original_name: Option<String>,
    pub kind: IndexKind,
    pub columns: Vec<IndexColumn>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RefAction {
    #[default]
    Restrict,
    Cascade,
    SetNull,
    NoAction,
    SetDefault,
}

impl RefAction {
    pub fn sql(self) -> &'static str {
        match self {
            RefAction::Restrict => "RESTRICT",
            RefAction::Cascade => "CASCADE",
            RefAction::SetNull => "SET NULL",
            RefAction::NoAction => "NO ACTION",
            RefAction::SetDefault => "SET DEFAULT",
        }
    }

    pub fn parse(s: &str) -> RefAction {
        match s.to_uppercase().as_str() {
            "CASCADE" => RefAction::Cascade,
            "SET NULL" => RefAction::SetNull,
            "NO ACTION" => RefAction::NoAction,
            "SET DEFAULT" => RefAction::SetDefault,
            _ => RefAction::Restrict,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForeignKeyDef {
    pub name: String,
    #[serde(default)]
    pub original_name: Option<String>,
    pub columns: Vec<String>,
    /// `None` means the table's own database.
    #[serde(default)]
    pub ref_schema: Option<String>,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
    #[serde(default)]
    pub on_delete: RefAction,
    #[serde(default)]
    pub on_update: RefAction,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TableDef {
    pub name: String,
    #[serde(default)]
    pub engine: Option<String>,
    #[serde(default)]
    pub charset: Option<String>,
    #[serde(default)]
    pub collation: Option<String>,
    #[serde(default)]
    pub comment: String,
    pub columns: Vec<ColumnDef>,
    #[serde(default)]
    pub indexes: Vec<IndexDef>,
    #[serde(default)]
    pub foreign_keys: Vec<ForeignKeyDef>,
}

/// Everything wrong with a definition, in plain words, so the UI can list it.
#[derive(Debug, PartialEq, Eq)]
pub struct Invalid(pub Vec<String>);

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.join("; "))
    }
}
impl std::error::Error for Invalid {}

// ---------- rendering ----------

pub fn q(ident: &str) -> String {
    format!("`{}`", ident.replace('`', "``"))
}

/// A quoted string literal. Backslashes and quotes are escaped.
pub fn lit(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "''"))
}

fn qualified(db: Option<&str>, table: &str) -> String {
    match db {
        Some(d) => format!("{}.{}", q(d), q(table)),
        None => q(table),
    }
}

fn is_now_like(e: &str) -> bool {
    let u = e.trim().to_uppercase();
    ["CURRENT_TIMESTAMP", "NOW(", "LOCALTIME", "LOCALTIMESTAMP"].iter().any(|p| u.starts_with(p))
}

impl ColumnDef {
    /// `varchar(40)`, `int unsigned zerofill`, `enum('a','b')`.
    pub fn type_sql(&self) -> String {
        let mut s = self.data_type.clone();
        if let Some(p) = self.params.as_deref().filter(|p| !p.is_empty()) {
            s.push_str(&format!("({p})"));
        }
        if self.unsigned {
            s.push_str(" unsigned");
        }
        if self.zerofill {
            s.push_str(" zerofill");
        }
        s
    }

    /// The full column definition used by CREATE/ADD/CHANGE.
    pub fn sql(&self) -> String {
        let mut s = format!("{} {}", q(&self.name), self.type_sql());
        match (&self.charset, &self.collation) {
            (Some(cs), Some(co)) => s.push_str(&format!(" CHARACTER SET {cs} COLLATE {co}")),
            (None, Some(co)) => s.push_str(&format!(" COLLATE {co}")),
            (Some(cs), None) => s.push_str(&format!(" CHARACTER SET {cs}")),
            (None, None) => {}
        }
        if let Some(g) = &self.generated {
            s.push_str(&format!(" GENERATED ALWAYS AS ({}) {}", g.expression, if g.stored { "STORED" } else { "VIRTUAL" }));
        }
        s.push_str(if self.nullable { " NULL" } else { " NOT NULL" });
        if self.generated.is_none() {
            match &self.default {
                ColumnDefault::None | ColumnDefault::Null => {}
                ColumnDefault::Literal(v) => s.push_str(&format!(" DEFAULT {}", lit(v))),
                ColumnDefault::Expression(e) if is_now_like(e) => s.push_str(&format!(" DEFAULT {}", e.trim())),
                ColumnDefault::Expression(e) => s.push_str(&format!(" DEFAULT ({})", e.trim())),
            }
            if self.auto_increment {
                s.push_str(" AUTO_INCREMENT");
            }
            if let Some(u) = self.on_update.as_deref().filter(|u| !u.is_empty()) {
                s.push_str(&format!(" ON UPDATE {u}"));
            }
        }
        if self.invisible {
            s.push_str(" INVISIBLE");
        }
        if !self.comment.is_empty() {
            s.push_str(&format!(" COMMENT {}", lit(&self.comment)));
        }
        s
    }
}

impl IndexDef {
    fn columns_sql(&self) -> String {
        let cols: Vec<String> = self
            .columns
            .iter()
            .map(|c| format!("{}{}{}", q(&c.name), c.length.map_or(String::new(), |l| format!("({l})")), if c.desc { " DESC" } else { "" }))
            .collect();
        format!("({})", cols.join(", "))
    }

    /// Inline form for CREATE TABLE.
    fn create_sql(&self) -> String {
        match self.kind {
            IndexKind::Primary => format!("PRIMARY KEY {}", self.columns_sql()),
            IndexKind::Unique => format!("UNIQUE KEY {} {}", q(&self.name), self.columns_sql()),
            IndexKind::Index => format!("KEY {} {}", q(&self.name), self.columns_sql()),
            IndexKind::Fulltext => format!("FULLTEXT KEY {} {}", q(&self.name), self.columns_sql()),
            IndexKind::Spatial => format!("SPATIAL KEY {} {}", q(&self.name), self.columns_sql()),
        }
    }

    fn add_sql(&self) -> String {
        match self.kind {
            IndexKind::Primary => format!("ADD PRIMARY KEY {}", self.columns_sql()),
            IndexKind::Unique => format!("ADD UNIQUE INDEX {} {}", q(&self.name), self.columns_sql()),
            IndexKind::Index => format!("ADD INDEX {} {}", q(&self.name), self.columns_sql()),
            IndexKind::Fulltext => format!("ADD FULLTEXT INDEX {} {}", q(&self.name), self.columns_sql()),
            IndexKind::Spatial => format!("ADD SPATIAL INDEX {} {}", q(&self.name), self.columns_sql()),
        }
    }

    fn drop_sql(&self) -> String {
        match self.kind {
            IndexKind::Primary => "DROP PRIMARY KEY".to_string(),
            _ => format!("DROP INDEX {}", q(&self.name)),
        }
    }
}

impl ForeignKeyDef {
    fn constraint_sql(&self, own_db: Option<&str>) -> String {
        let cols = |v: &[String]| v.iter().map(|c| q(c)).collect::<Vec<_>>().join(", ");
        let target = qualified(self.ref_schema.as_deref().or(own_db).filter(|_| self.ref_schema.is_some()), &self.ref_table);
        format!(
            "CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) ON DELETE {} ON UPDATE {}",
            q(&self.name),
            cols(&self.columns),
            target,
            cols(&self.ref_columns),
            self.on_delete.sql(),
            self.on_update.sql()
        )
    }
}

fn table_options(t: &TableDef) -> Vec<String> {
    let mut o = Vec::new();
    if let Some(e) = t.engine.as_deref().filter(|e| !e.is_empty()) {
        o.push(format!("ENGINE={e}"));
    }
    match (&t.charset, &t.collation) {
        (Some(cs), Some(co)) => o.push(format!("DEFAULT CHARSET={cs} COLLATE={co}")),
        (Some(cs), None) => o.push(format!("DEFAULT CHARSET={cs}")),
        (None, Some(co)) => o.push(format!("COLLATE={co}")),
        (None, None) => {}
    }
    o
}

// ---------- validation ----------

pub fn validate(t: &TableDef) -> Vec<String> {
    let mut e = Vec::new();
    if t.name.trim().is_empty() {
        e.push("table name is empty".to_string());
    }
    if t.columns.is_empty() {
        e.push("a table needs at least one column".to_string());
    }
    let mut seen = std::collections::HashSet::new();
    for c in &t.columns {
        if c.name.trim().is_empty() {
            e.push("a column has no name".to_string());
        } else if !seen.insert(c.name.to_lowercase()) {
            e.push(format!("duplicate column name `{}`", c.name));
        }
        if c.data_type.trim().is_empty() {
            e.push(format!("column `{}` has no type", c.name));
        }
        if !c.nullable && matches!(c.default, ColumnDefault::Null) {
            e.push(format!("column `{}` is NOT NULL but its default is NULL", c.name));
        }
        if c.generated.as_ref().is_some_and(|g| g.expression.trim().is_empty()) {
            e.push(format!("generated column `{}` has no expression", c.name));
        }
        if c.generated.is_some() && c.auto_increment {
            e.push(format!("generated column `{}` cannot be AUTO_INCREMENT", c.name));
        }
    }
    let col_exists = |n: &str| t.columns.iter().any(|c| c.name == n);
    let mut idx_names = std::collections::HashSet::new();
    let mut primaries = 0;
    for i in &t.indexes {
        if i.kind == IndexKind::Primary {
            primaries += 1;
        } else if i.name.trim().is_empty() {
            e.push("an index has no name".to_string());
        } else if !idx_names.insert(i.name.to_lowercase()) {
            e.push(format!("duplicate index name `{}`", i.name));
        }
        if i.columns.is_empty() {
            e.push(format!("index `{}` has no columns", i.name));
        }
        for c in &i.columns {
            if !col_exists(&c.name) {
                e.push(format!("index `{}` uses unknown column `{}`", i.name, c.name));
            }
        }
    }
    if primaries > 1 {
        e.push("a table can have only one primary key".to_string());
    }
    let autos: Vec<&ColumnDef> = t.columns.iter().filter(|c| c.auto_increment).collect();
    if autos.len() > 1 {
        e.push("only one AUTO_INCREMENT column is allowed".to_string());
    }
    for a in autos {
        let keyed = t.indexes.iter().any(|i| i.kind != IndexKind::Fulltext && i.columns.first().is_some_and(|c| c.name == a.name));
        if !keyed {
            e.push(format!("AUTO_INCREMENT column `{}` must be the first column of an index", a.name));
        }
    }
    let mut fk_names = std::collections::HashSet::new();
    for f in &t.foreign_keys {
        if f.name.trim().is_empty() {
            e.push("a foreign key has no name".to_string());
        } else if !fk_names.insert(f.name.to_lowercase()) {
            e.push(format!("duplicate foreign key name `{}`", f.name));
        }
        if f.columns.is_empty() || f.columns.len() != f.ref_columns.len() {
            e.push(format!("foreign key `{}` needs the same number of local and referenced columns", f.name));
        }
        if f.ref_table.trim().is_empty() {
            e.push(format!("foreign key `{}` has no referenced table", f.name));
        }
        for c in &f.columns {
            if !col_exists(c) {
                e.push(format!("foreign key `{}` uses unknown column `{c}`", f.name));
            }
        }
    }
    e
}

fn check(t: &TableDef) -> Result<(), Invalid> {
    let errs = validate(t);
    if errs.is_empty() { Ok(()) } else { Err(Invalid(errs)) }
}

// ---------- CREATE ----------

pub fn create_table_sql(db: Option<&str>, t: &TableDef) -> Result<String, Invalid> {
    check(t)?;
    let mut lines: Vec<String> = t.columns.iter().map(|c| format!("  {}", c.sql())).collect();
    let mut idx: Vec<&IndexDef> = t.indexes.iter().collect();
    idx.sort_by_key(|i| i.kind != IndexKind::Primary); // PRIMARY KEY first
    lines.extend(idx.iter().map(|i| format!("  {}", i.create_sql())));
    lines.extend(t.foreign_keys.iter().map(|f| format!("  {}", f.constraint_sql(db))));
    let mut sql = format!("CREATE TABLE {} (\n{}\n)", qualified(db, &t.name), lines.join(",\n"));
    let mut opts = table_options(t);
    if !t.comment.is_empty() {
        opts.push(format!("COMMENT={}", lit(&t.comment)));
    }
    if !opts.is_empty() {
        sql.push(' ');
        sql.push_str(&opts.join(" "));
    }
    Ok(sql)
}

// ---------- ALTER (diff) ----------

/// Map old column names to the names they have in `new`, so unchanged indexes and keys that merely
/// follow a renamed column are not rebuilt.
fn rename_map(old: &TableDef, new: &TableDef) -> std::collections::HashMap<String, String> {
    let mut m = std::collections::HashMap::new();
    for c in &new.columns {
        if let Some(o) = &c.original_name {
            if old.columns.iter().any(|x| &x.name == o) {
                m.insert(o.clone(), c.name.clone());
            }
        }
    }
    m
}

fn mapped<'a>(m: &'a std::collections::HashMap<String, String>, n: &'a str) -> &'a str {
    m.get(n).map(String::as_str).unwrap_or(n)
}

fn same_index(old: &IndexDef, new: &IndexDef, m: &std::collections::HashMap<String, String>) -> bool {
    old.kind == new.kind
        && old.name == new.name
        && old.columns.len() == new.columns.len()
        && old.columns.iter().zip(&new.columns).all(|(a, b)| mapped(m, &a.name) == b.name && a.length == b.length && a.desc == b.desc)
}

fn same_fk(old: &ForeignKeyDef, new: &ForeignKeyDef, m: &std::collections::HashMap<String, String>) -> bool {
    old.name == new.name
        && old.ref_schema == new.ref_schema
        && old.ref_table == new.ref_table
        && old.ref_columns == new.ref_columns
        && old.on_delete == new.on_delete
        && old.on_update == new.on_update
        && old.columns.iter().map(|c| mapped(m, c)).eq(new.columns.iter().map(String::as_str))
}

/// Statements that turn `old` (as read from the server) into `new`, in an order that works:
/// drop changed foreign keys, one `ALTER TABLE` for columns/indexes/options, add foreign keys, rename.
/// Empty when nothing changed.
pub fn alter_table_sql(db: Option<&str>, old: &TableDef, new: &TableDef) -> Result<Vec<String>, Invalid> {
    check(new)?;
    let tbl = qualified(db, &old.name);
    let m = rename_map(old, new);
    let mut out = Vec::new();

    // 1. foreign keys that are removed or changed
    let kept_fk = |o: &ForeignKeyDef| new.foreign_keys.iter().any(|n| n.original_name.as_deref() == Some(&o.name) && same_fk(o, n, &m));
    let drop_fks: Vec<String> = old.foreign_keys.iter().filter(|o| !kept_fk(o)).map(|o| format!("DROP FOREIGN KEY {}", q(&o.name))).collect();
    if !drop_fks.is_empty() {
        out.push(format!("ALTER TABLE {tbl} {}", drop_fks.join(", ")));
    }

    let mut clauses: Vec<String> = Vec::new();

    // 2. indexes that are removed or changed
    let kept_idx = |o: &IndexDef| new.indexes.iter().any(|n| (n.original_name.as_deref() == Some(&o.name) || (o.kind == IndexKind::Primary && n.kind == IndexKind::Primary)) && same_index(o, n, &m));
    for o in old.indexes.iter().filter(|o| !kept_idx(o)) {
        clauses.push(o.drop_sql());
    }

    // 3. dropped columns
    for o in &old.columns {
        if !new.columns.iter().any(|n| n.original_name.as_deref() == Some(&o.name)) {
            clauses.push(format!("DROP COLUMN {}", q(&o.name)));
        }
    }

    // 4. add / change / move columns, walking the new order against a simulated current order
    let mut cur: Vec<String> = old.columns.iter().filter(|o| new.columns.iter().any(|n| n.original_name.as_deref() == Some(&o.name))).map(|o| o.name.clone()).collect();
    for (i, nc) in new.columns.iter().enumerate() {
        let position = if i == 0 { " FIRST".to_string() } else { format!(" AFTER {}", q(&new.columns[i - 1].name)) };
        match nc.original_name.as_deref().and_then(|o| old.columns.iter().find(|c| c.name == o)) {
            Some(oc) => {
                let changed = oc.sql() != nc.sql();
                let moved = cur.get(i).map(String::as_str) != Some(oc.name.as_str());
                if changed || moved {
                    clauses.push(format!("CHANGE COLUMN {} {}{}", q(&oc.name), nc.sql(), if moved { position } else { String::new() }));
                }
                if let Some(p) = cur.iter().position(|n| n == &oc.name) {
                    cur.remove(p);
                }
                cur.insert(i.min(cur.len()), nc.name.clone());
            }
            None => {
                clauses.push(format!("ADD COLUMN {}{}", nc.sql(), position));
                cur.insert(i.min(cur.len()), nc.name.clone());
            }
        }
    }

    // 5. indexes that are new or changed
    let existing_idx = |n: &IndexDef| old.indexes.iter().any(|o| (n.original_name.as_deref() == Some(&o.name) || (o.kind == IndexKind::Primary && n.kind == IndexKind::Primary)) && same_index(o, n, &m));
    for n in new.indexes.iter().filter(|n| !existing_idx(n)) {
        clauses.push(n.add_sql());
    }

    // 6. table options
    if old.engine != new.engine && new.engine.is_some() {
        clauses.push(format!("ENGINE={}", new.engine.as_deref().unwrap()));
    }
    if (old.charset != new.charset || old.collation != new.collation) && (new.charset.is_some() || new.collation.is_some()) {
        clauses.push(match (&new.charset, &new.collation) {
            (Some(cs), Some(co)) => format!("DEFAULT CHARSET={cs} COLLATE={co}"),
            (Some(cs), None) => format!("DEFAULT CHARSET={cs}"),
            (None, Some(co)) => format!("COLLATE={co}"),
            (None, None) => unreachable!(),
        });
    }
    if old.comment != new.comment {
        clauses.push(format!("COMMENT={}", lit(&new.comment)));
    }
    if !clauses.is_empty() {
        out.push(format!("ALTER TABLE {tbl} {}", clauses.join(", ")));
    }

    // 7. foreign keys that are new or changed
    let existing_fk = |n: &ForeignKeyDef| old.foreign_keys.iter().any(|o| n.original_name.as_deref() == Some(&o.name) && same_fk(o, n, &m));
    let add_fks: Vec<String> = new.foreign_keys.iter().filter(|n| !existing_fk(n)).map(|n| format!("ADD {}", n.constraint_sql(db))).collect();
    if !add_fks.is_empty() {
        out.push(format!("ALTER TABLE {tbl} {}", add_fks.join(", ")));
    }

    // 8. rename last, so every statement above refers to the table by its current name
    if old.name != new.name {
        out.push(format!("RENAME TABLE {tbl} TO {}", qualified(db, &new.name)));
    }
    Ok(out)
}

// ---------- column type parsing ----------

/// Split `COLUMN_TYPE` text such as `decimal(10,2) unsigned zerofill` or `enum('a','b')`
/// into (base type, parameters, unsigned, zerofill).
pub fn parse_column_type(s: &str) -> (String, Option<String>, bool, bool) {
    let s = s.trim();
    let base_end = s.find(|c: char| c == '(' || c.is_whitespace()).unwrap_or(s.len());
    let base = s[..base_end].to_lowercase();
    let mut rest = &s[base_end..];
    let mut params = None;
    if rest.starts_with('(') {
        // the closing parenthesis, skipping any inside quoted enum/set values
        let (mut depth, mut quote, mut end) = (0i32, None::<char>, None);
        let mut chars = rest.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            match quote {
                Some(_) if c == '\\' => { chars.next(); }
                Some(qc) if c == qc => {
                    if chars.peek().is_some_and(|(_, n)| *n == qc) { chars.next(); } else { quote = None; }
                }
                Some(_) => {}
                None if c == '\'' || c == '"' => quote = Some(c),
                None if c == '(' => depth += 1,
                None if c == ')' => {
                    depth -= 1;
                    if depth == 0 { end = Some(i); break; }
                }
                None => {}
            }
        }
        if let Some(e) = end {
            params = Some(rest[1..e].to_string());
            rest = &rest[e + 1..];
        }
    }
    let flags = rest.to_lowercase();
    (base, params, flags.contains("unsigned"), flags.contains("zerofill"))
}

// ---------- table-level statements ----------

pub fn drop_table_sql(db: &str, table: &str) -> String {
    format!("DROP TABLE {}", qualified(Some(db), table))
}

pub fn truncate_table_sql(db: &str, table: &str) -> String {
    format!("TRUNCATE TABLE {}", qualified(Some(db), table))
}

pub fn rename_table_sql(db: &str, from: &str, to: &str) -> String {
    format!("RENAME TABLE {} TO {}", qualified(Some(db), from), qualified(Some(db), to))
}

/// Copy a table: always the structure (including indexes), and the rows too when `copy_columns` is given.
/// The caller passes the columns to copy because generated columns must be left out of the INSERT.
pub fn duplicate_table_sql(db: &str, from: &str, to: &str, copy_columns: Option<&[String]>) -> Vec<String> {
    let mut v = vec![format!("CREATE TABLE {} LIKE {}", qualified(Some(db), to), qualified(Some(db), from))];
    if let Some(cols) = copy_columns.filter(|c| !c.is_empty()) {
        let list = cols.iter().map(|c| q(c)).collect::<Vec<_>>().join(", ");
        v.push(format!("INSERT INTO {} ({list}) SELECT {list} FROM {}", qualified(Some(db), to), qualified(Some(db), from)));
    }
    v
}

pub fn create_database_sql(name: &str, charset: Option<&str>, collation: Option<&str>) -> String {
    let mut s = format!("CREATE DATABASE {}", q(name));
    if let Some(c) = charset.filter(|c| !c.is_empty()) {
        s.push_str(&format!(" CHARACTER SET {c}"));
    }
    if let Some(c) = collation.filter(|c| !c.is_empty()) {
        s.push_str(&format!(" COLLATE {c}"));
    }
    s
}

pub fn drop_database_sql(name: &str) -> String {
    format!("DROP DATABASE {}", q(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, ty: &str) -> ColumnDef {
        ColumnDef { name: name.into(), original_name: Some(name.into()), data_type: ty.into(), nullable: true, ..Default::default() }
    }
    fn pk(cols: &[&str]) -> IndexDef {
        IndexDef { name: "PRIMARY".into(), original_name: Some("PRIMARY".into()), kind: IndexKind::Primary, columns: cols.iter().map(|c| IndexColumn { name: c.to_string(), length: None, desc: false }).collect() }
    }
    fn idx(name: &str, kind: IndexKind, cols: &[&str]) -> IndexDef {
        IndexDef { name: name.into(), original_name: Some(name.into()), kind, columns: cols.iter().map(|c| IndexColumn { name: c.to_string(), length: None, desc: false }).collect() }
    }
    fn base() -> TableDef {
        let mut id = col("id", "int");
        id.nullable = false;
        id.auto_increment = true;
        let mut name = col("name", "varchar");
        name.params = Some("40".into());
        name.nullable = false;
        TableDef { name: "t".into(), engine: Some("InnoDB".into()), columns: vec![id, name, col("note", "text")], indexes: vec![pk(&["id"])], ..Default::default() }
    }
    /// Mark every object as existing (what reading from the server produces).
    fn as_new(mut t: TableDef) -> TableDef {
        for c in &mut t.columns { c.original_name = None; }
        for i in &mut t.indexes { i.original_name = None; }
        for f in &mut t.foreign_keys { f.original_name = None; }
        t
    }

    #[test]
    fn create_table_renders_everything() {
        let mut t = base();
        t.columns[1].charset = Some("latin1".into());
        t.columns[1].collation = Some("latin1_swedish_ci".into());
        t.columns[1].comment = "it's".into();
        t.columns[1].default = ColumnDefault::Literal("a\\b".into());
        let mut created = col("created", "datetime");
        created.nullable = false;
        created.default = ColumnDefault::Expression("CURRENT_TIMESTAMP".into());
        created.on_update = Some("CURRENT_TIMESTAMP".into());
        let mut uid = col("uid", "char");
        uid.params = Some("36".into());
        uid.default = ColumnDefault::Expression("uuid()".into());
        let mut g = col("double_id", "int");
        g.generated = Some(Generated { expression: "`id` * 2".into(), stored: true });
        t.columns.extend([created, uid, g]);
        t.indexes.push(idx("n", IndexKind::Unique, &["name"]));
        t.indexes[1].columns[0].length = Some(10);
        t.indexes[1].columns[0].desc = true;
        t.foreign_keys.push(ForeignKeyDef { name: "fk".into(), original_name: None, columns: vec!["id".into()], ref_schema: Some("other".into()), ref_table: "p".into(), ref_columns: vec!["id".into()], on_delete: RefAction::Cascade, on_update: RefAction::SetNull });
        t.charset = Some("utf8mb4".into());
        t.collation = Some("utf8mb4_bin".into());
        t.comment = "c".into();
        let sql = create_table_sql(Some("d"), &t).unwrap();
        assert_eq!(sql, "CREATE TABLE `d`.`t` (
  `id` int NOT NULL AUTO_INCREMENT,
  `name` varchar(40) CHARACTER SET latin1 COLLATE latin1_swedish_ci NOT NULL DEFAULT 'a\\\\b' COMMENT 'it''s',
  `note` text NULL,
  `created` datetime NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  `uid` char(36) NULL DEFAULT (uuid()),
  `double_id` int GENERATED ALWAYS AS (`id` * 2) STORED NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `n` (`name`(10) DESC),
  CONSTRAINT `fk` FOREIGN KEY (`id`) REFERENCES `other`.`p` (`id`) ON DELETE CASCADE ON UPDATE SET NULL
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin COMMENT='c'");
    }

    #[test]
    fn nothing_changed_means_no_statements() {
        let t = base();
        assert_eq!(alter_table_sql(Some("d"), &t, &t).unwrap(), Vec::<String>::new());
        // NULL default and no default are the same for a nullable column
        let mut n = t.clone();
        n.columns[2].default = ColumnDefault::Null;
        assert!(alter_table_sql(Some("d"), &t, &n).unwrap().is_empty());
    }

    #[test]
    fn add_drop_rename_move_and_change_columns() {
        let old = base();
        let mut new = old.clone();
        new.columns.remove(2);                                     // drop note
        new.columns[1].name = "full_name".into();                  // rename name
        new.columns[1].params = Some("80".into());                 // and widen it
        new.columns.insert(0, as_new(TableDef { columns: vec![col("tenant", "int")], ..Default::default() }).columns.remove(0)); // new first column
        let s = alter_table_sql(Some("d"), &old, &new).unwrap();
        assert_eq!(s, vec![
            "ALTER TABLE `d`.`t` DROP COLUMN `note`, ADD COLUMN `tenant` int NULL FIRST, CHANGE COLUMN `name` `full_name` varchar(80) NOT NULL".to_string(),
        ]);
    }

    #[test]
    fn moving_a_column_emits_one_change_with_position() {
        let old = base();
        let mut new = old.clone();
        let note = new.columns.remove(2);
        new.columns.insert(0, note);                               // note, id, name
        let s = alter_table_sql(None, &old, &new).unwrap();
        assert_eq!(s, vec!["ALTER TABLE `t` CHANGE COLUMN `note` `note` text NULL FIRST".to_string()]);
    }

    #[test]
    fn index_changes_follow_renamed_columns_without_rebuilding() {
        let mut old = base();
        old.indexes.push(idx("by_name", IndexKind::Index, &["name"]));
        let mut new = old.clone();
        new.columns[1].name = "title".into();
        new.indexes[1].columns[0].name = "title".into();           // the index just follows the rename
        let s = alter_table_sql(None, &old, &new).unwrap();
        assert_eq!(s, vec!["ALTER TABLE `t` CHANGE COLUMN `name` `title` varchar(40) NOT NULL".to_string()]);

        let mut changed = old.clone();
        changed.indexes[1].kind = IndexKind::Unique;               // a real change: drop + add
        assert_eq!(alter_table_sql(None, &old, &changed).unwrap(), vec!["ALTER TABLE `t` DROP INDEX `by_name`, ADD UNIQUE INDEX `by_name` (`name`)".to_string()]);
    }

    #[test]
    fn primary_key_swap_and_new_index() {
        let old = base();
        let mut new = old.clone();
        new.indexes[0].columns.push(IndexColumn { name: "name".into(), length: None, desc: false });
        new.indexes.push(as_new(TableDef { indexes: vec![idx("ft", IndexKind::Fulltext, &["note"])], ..Default::default() }).indexes.remove(0));
        let s = alter_table_sql(None, &old, &new).unwrap();
        assert_eq!(s, vec!["ALTER TABLE `t` DROP PRIMARY KEY, ADD PRIMARY KEY (`id`, `name`), ADD FULLTEXT INDEX `ft` (`note`)".to_string()]);
    }

    #[test]
    fn foreign_keys_are_dropped_first_added_last_and_rename_is_final() {
        let fk = |name: &str, action: RefAction| ForeignKeyDef { name: name.into(), original_name: Some(name.into()), columns: vec!["id".into()], ref_schema: None, ref_table: "p".into(), ref_columns: vec!["id".into()], on_delete: action, on_update: RefAction::Restrict };
        let mut old = base();
        old.foreign_keys = vec![fk("a", RefAction::Restrict), fk("b", RefAction::Restrict)];
        let mut new = old.clone();
        new.foreign_keys = vec![fk("a", RefAction::Cascade)];       // a changed, b removed
        new.foreign_keys.push(ForeignKeyDef { original_name: None, ..fk("c", RefAction::Restrict) });
        new.name = "t2".into();
        new.comment = "x".into();
        let s = alter_table_sql(Some("d"), &old, &new).unwrap();
        assert_eq!(s, vec![
            "ALTER TABLE `d`.`t` DROP FOREIGN KEY `a`, DROP FOREIGN KEY `b`".to_string(),
            "ALTER TABLE `d`.`t` COMMENT='x'".to_string(),
            "ALTER TABLE `d`.`t` ADD CONSTRAINT `a` FOREIGN KEY (`id`) REFERENCES `p` (`id`) ON DELETE CASCADE ON UPDATE RESTRICT, ADD CONSTRAINT `c` FOREIGN KEY (`id`) REFERENCES `p` (`id`) ON DELETE RESTRICT ON UPDATE RESTRICT".to_string(),
            "RENAME TABLE `d`.`t` TO `d`.`t2`".to_string(),
        ]);
    }

    #[test]
    fn validation_reports_every_problem() {
        let mut t = base();
        t.name = " ".into();
        t.columns.push(col("ID", "int"));                           // duplicate (case-insensitive)
        t.columns[1].nullable = false;
        t.columns[1].default = ColumnDefault::Null;
        t.columns[2].auto_increment = true;                         // second auto column, not keyed
        t.indexes.push(idx("bad", IndexKind::Index, &["ghost"]));
        t.indexes.push(pk(&["id"]));
        t.foreign_keys.push(ForeignKeyDef { name: "f".into(), original_name: None, columns: vec!["nope".into()], ref_schema: None, ref_table: "".into(), ref_columns: vec![], on_delete: RefAction::Restrict, on_update: RefAction::Restrict });
        let errs = validate(&t);
        for needle in ["table name is empty", "duplicate column name `ID`", "NOT NULL but its default is NULL", "only one AUTO_INCREMENT", "must be the first column of an index", "unknown column `ghost`", "only one primary key", "same number of local and referenced", "no referenced table", "unknown column `nope`"] {
            assert!(errs.iter().any(|e| e.contains(needle)), "missing: {needle}\n{errs:#?}");
        }
        assert!(create_table_sql(None, &t).is_err() && alter_table_sql(None, &base(), &t).is_err());
    }

    #[test]
    fn parses_column_types() {
        assert_eq!(parse_column_type("varchar(40)"), ("varchar".into(), Some("40".into()), false, false));
        assert_eq!(parse_column_type("int unsigned zerofill"), ("int".into(), None, true, true));
        assert_eq!(parse_column_type("decimal(10,2) unsigned"), ("decimal".into(), Some("10,2".into()), true, false));
        assert_eq!(parse_column_type("enum('a','b)c','it''s')"), ("enum".into(), Some("'a','b)c','it''s'".into()), false, false));
        assert_eq!(parse_column_type("datetime(3)"), ("datetime".into(), Some("3".into()), false, false));
        assert_eq!(parse_column_type("LONGTEXT"), ("longtext".into(), None, false, false));
    }

    #[test]
    fn table_level_statements() {
        assert_eq!(rename_table_sql("d", "a`b", "c"), "RENAME TABLE `d`.`a``b` TO `d`.`c`");
        let cols = vec!["id".to_string(), "n`x".to_string()];
        assert_eq!(duplicate_table_sql("d", "a", "b", Some(&cols)), vec!["CREATE TABLE `d`.`b` LIKE `d`.`a`", "INSERT INTO `d`.`b` (`id`, `n``x`) SELECT `id`, `n``x` FROM `d`.`a`"]);
        assert_eq!(duplicate_table_sql("d", "a", "b", None), vec!["CREATE TABLE `d`.`b` LIKE `d`.`a`"]);
        assert_eq!(create_database_sql("x", Some("utf8mb4"), Some("utf8mb4_bin")), "CREATE DATABASE `x` CHARACTER SET utf8mb4 COLLATE utf8mb4_bin");
        assert_eq!(drop_table_sql("d", "t"), "DROP TABLE `d`.`t`");
        assert_eq!(truncate_table_sql("d", "t"), "TRUNCATE TABLE `d`.`t`");
    }
}
