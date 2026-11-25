//! Context-aware autocomplete.
//!
//! The engine tokenizes the statement around the cursor (no full parser), works out which clause the
//! cursor is in, resolves the tables and aliases the statement refers to, and returns ranked candidates
//! from the object catalog plus static keyword and function lists. It never touches the server: the
//! catalog is built from the object index the app already holds.

use crate::keywords::{ends_table_ref, is_reserved, AFTER_TABLE, FUNCTIONS, KEYWORDS, STATEMENT_STARTERS};
use crate::text::{byte_to_utf16, utf16_to_byte};
use myq_schema::filter::{Kind, Node};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct TableInfo {
    pub name: String,
    pub is_view: bool,
    pub columns: Vec<String>,
}

/// A foreign key: `db.table(columns)` points at `ref_db.ref_table(ref_columns)`.
#[derive(Debug, Clone)]
pub struct Fk {
    pub name: String,
    pub db: String,
    pub table: String,
    pub columns: Vec<String>,
    pub ref_db: String,
    pub ref_table: String,
    pub ref_columns: Vec<String>,
}

/// Databases, their tables/views and each table's columns.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub databases: Vec<String>,
    pub tables: HashMap<String, Vec<TableInfo>>,
    /// Foreign keys of the databases that were loaded; they drive the join suggestions.
    pub fks: Vec<Fk>,
}

impl Catalog {
    pub fn from_nodes(nodes: &[Node]) -> Catalog {
        let mut c = Catalog::default();
        for n in nodes {
            match n.kind {
                Kind::Database => {
                    c.databases.push(n.name.clone());
                    c.tables.entry(n.name.clone()).or_default();
                }
                Kind::Table | Kind::View => c.tables.entry(n.database.clone()).or_default().push(TableInfo {
                    name: n.name.clone(),
                    is_view: n.kind == Kind::View,
                    columns: vec![],
                }),
                _ => {}
            }
        }
        // columns arrive after their tables in the index, but do not rely on it
        let mut pos: HashMap<(&str, &str), usize> = HashMap::new();
        for (db, ts) in &c.tables {
            for (i, t) in ts.iter().enumerate() {
                pos.insert((db.as_str(), t.name.as_str()), i);
            }
        }
        let mut cols: Vec<(String, usize, String)> = Vec::new();
        for n in nodes.iter().filter(|n| n.kind == Kind::Column) {
            if let Some(t) = &n.parent {
                if let Some(i) = pos.get(&(n.database.as_str(), t.as_str())) {
                    cols.push((n.database.clone(), *i, n.name.clone()));
                }
            }
        }
        for (db, i, col) in cols {
            if let Some(ts) = c.tables.get_mut(&db) {
                ts[i].columns.push(col);
            }
        }
        c
    }

    fn find_table(&self, db: Option<&str>, table: &str, current_db: Option<&str>) -> Option<(&str, &TableInfo)> {
        let eq = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
        let search = |d: &str| self.tables.get_key_value(d).and_then(|(k, ts)| ts.iter().find(|t| eq(&t.name, table)).map(|t| (k.as_str(), t)));
        match db.or(current_db) {
            Some(d) => {
                let key = self.tables.keys().find(|k| eq(k, d))?;
                search(key)
            }
            // no database known: accept the table only when the name is unambiguous
            None => {
                let mut found = self.tables.keys().filter_map(|d| search(d));
                let first = found.next()?;
                found.next().is_none().then_some(first)
            }
        }
    }

    fn find_database(&self, name: &str) -> Option<&str> {
        self.databases.iter().find(|d| d.eq_ignore_ascii_case(name)).map(String::as_str)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SuggestionKind {
    Column,
    Table,
    View,
    Database,
    Alias,
    Function,
    Keyword,
    /// A ready join condition built from a foreign key (`o.customer_id = c.id`).
    Join,
    /// A table linked by a foreign key to a table already in the statement.
    Related,
}

#[derive(Debug, Clone, Serialize)]
pub struct Suggestion {
    pub label: String,
    pub kind: SuggestionKind,
    pub detail: Option<String>,
    /// What replaces the typed word (quoted, or qualified when a column name is ambiguous).
    pub insert: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Completions {
    /// UTF-16 range of the text the chosen suggestion replaces.
    pub from: usize,
    pub to: usize,
    pub items: Vec<Suggestion>,
}

// ---------- tokenizer ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum T {
    Word,
    Quoted,
    Str,
    Num,
    Punct,
}

#[derive(Clone, Debug)]
struct Tok {
    kind: T,
    start: usize,
    end: usize,
    /// Identifier text (unquoted for `Quoted`), the punctuation character, or the raw text.
    text: String,
    /// A quoted identifier or string that is not closed yet.
    open: bool,
}

/// A comment's byte range; `closed` is false for an unterminated `/* …` (and always true for `--`/`#`).
#[derive(Debug, Clone, Copy)]
struct Comment {
    start: usize,
    end: usize,
    closed: bool,
    line: bool,
}

impl Comment {
    /// Is a cursor at `pos` inside this comment (so typing there is typing comment text)?
    fn contains(&self, pos: usize) -> bool {
        pos > self.start + 1 && if self.line || !self.closed { pos <= self.end } else { pos < self.end }
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$' || !c.is_ascii()
}

fn tokenize(s: &str) -> (Vec<Tok>, Vec<Comment>) {
    let b = s.as_bytes();
    let (mut i, mut out, mut comments) = (0usize, Vec::new(), Vec::new());
    while i < b.len() {
        let c = s[i..].chars().next().unwrap();
        if c.is_whitespace() {
            i += c.len_utf8();
        } else if s[i..].starts_with("--") && b.get(i + 2).map_or(true, |x| x.is_ascii_whitespace()) || c == '#' {
            let start = i;
            i = s[i..].find('\n').map_or(s.len(), |p| i + p);
            comments.push(Comment { start, end: i, closed: true, line: true });
        } else if s[i..].starts_with("/*") {
            let start = i;
            match s[i + 2..].find("*/") {
                Some(p) => {
                    i += 2 + p + 2;
                    comments.push(Comment { start, end: i, closed: true, line: false });
                }
                None => {
                    comments.push(Comment { start, end: s.len(), closed: false, line: false });
                    return (out, comments);
                }
            }
        } else if c == '`' {
            let start = i;
            i += 1;
            let mut text = String::new();
            let mut open = true;
            while i < b.len() {
                let ch = s[i..].chars().next().unwrap();
                i += ch.len_utf8();
                if ch == '`' {
                    if s[i..].starts_with('`') {
                        text.push('`');
                        i += 1;
                    } else {
                        open = false;
                        break;
                    }
                } else {
                    text.push(ch);
                }
            }
            out.push(Tok { kind: T::Quoted, start, end: i, text, open });
        } else if c == '\'' || c == '"' {
            let start = i;
            i += 1;
            let mut open = true;
            while i < b.len() {
                let ch = s[i..].chars().next().unwrap();
                i += ch.len_utf8();
                if ch == '\\' {
                    i += s[i..].chars().next().map_or(0, |n| n.len_utf8());
                } else if ch == c {
                    if s[i..].starts_with(c) {
                        i += 1;
                    } else {
                        open = false;
                        break;
                    }
                }
            }
            out.push(Tok { kind: T::Str, start, end: i, text: s[start..i].to_string(), open });
        } else if c.is_ascii_digit() {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'.') {
                i += 1;
            }
            out.push(Tok { kind: T::Num, start, end: i, text: s[start..i].to_string(), open: false });
        } else if is_word(c) {
            let start = i;
            while i < b.len() {
                let ch = s[i..].chars().next().unwrap();
                if !is_word(ch) {
                    break;
                }
                i += ch.len_utf8();
            }
            out.push(Tok { kind: T::Word, start, end: i, text: s[start..i].to_string(), open: false });
        } else {
            out.push(Tok { kind: T::Punct, start: i, end: i + c.len_utf8(), text: c.to_string(), open: false });
            i += c.len_utf8();
        }
    }
    (out, comments)
}

fn is_punct(t: &Tok, c: &str) -> bool {
    t.kind == T::Punct && t.text == c
}

fn is_kw(t: &Tok, kw: &str) -> bool {
    t.kind == T::Word && t.text.eq_ignore_ascii_case(kw)
}

fn is_ident(t: &Tok) -> bool {
    matches!(t.kind, T::Word | T::Quoted)
}

// ---------- table references ----------

#[derive(Debug, Clone)]
struct TableRef {
    db: Option<String>,
    table: String,
    alias: Option<String>,
    /// `FROM (subquery) alias`: the alias exists but its columns are unknown.
    derived: bool,
}

/// Tables and aliases a statement refers to (`FROM a x, db.b AS y JOIN c ...`, `UPDATE t`, `INSERT INTO t`).
fn table_refs(toks: &[Tok]) -> Vec<TableRef> {
    let mut refs = Vec::new();
    let mut i = 0;
    let intro = |t: &Tok| ["FROM", "JOIN", "UPDATE", "INTO", "STRAIGHT_JOIN", "TABLE"].iter().any(|k| is_kw(t, k));
    while i < toks.len() {
        if !intro(&toks[i]) {
            i += 1;
            continue;
        }
        let from_clause = is_kw(&toks[i], "FROM");
        i += 1;
        loop {
            // a derived table: FROM ( ... ) alias
            if i < toks.len() && is_punct(&toks[i], "(") {
                let mut depth = 0;
                while i < toks.len() {
                    if is_punct(&toks[i], "(") { depth += 1; }
                    if is_punct(&toks[i], ")") { depth -= 1; if depth == 0 { i += 1; break; } }
                    i += 1;
                }
                if i < toks.len() && is_kw(&toks[i], "AS") { i += 1; }
                if i < toks.len() && is_ident(&toks[i]) && !(toks[i].kind == T::Word && ends_table_ref(&toks[i].text)) {
                    refs.push(TableRef { db: None, table: String::new(), alias: Some(toks[i].text.clone()), derived: true });
                    i += 1;
                }
            } else if i < toks.len() && is_ident(&toks[i]) && !ends_table_ref(&toks[i].text) {
                let (mut db, mut table) = (None, toks[i].text.clone());
                i += 1;
                if i + 1 < toks.len() && is_punct(&toks[i], ".") && is_ident(&toks[i + 1]) {
                    db = Some(table);
                    table = toks[i + 1].text.clone();
                    i += 2;
                }
                let mut alias = None;
                if i < toks.len() && is_kw(&toks[i], "AS") {
                    i += 1;
                }
                if i < toks.len() && is_ident(&toks[i]) && !(toks[i].kind == T::Word && ends_table_ref(&toks[i].text)) {
                    alias = Some(toks[i].text.clone());
                    i += 1;
                }
                refs.push(TableRef { db, table, alias, derived: false });
            } else {
                break;
            }
            if from_clause && i < toks.len() && is_punct(&toks[i], ",") {
                i += 1;
                continue;
            }
            break;
        }
    }
    refs
}

// ---------- context ----------

#[derive(Debug, PartialEq, Eq, Clone)]
enum Ctx {
    /// Beginning of a statement.
    Start,
    /// A table (or database) name is expected.
    Tables,
    /// A complete table reference was typed; clauses follow.
    AfterTable,
    /// A value/column expression is expected.
    Operand,
    /// An expression was just completed (operators and clauses follow); the bool says it is a SELECT list.
    AfterExpr { select_list: bool },
    /// A database name (USE).
    Database,
    /// Column list of `INSERT INTO t (`.
    InsertColumns { db: Option<String>, table: String },
    /// SHOW ...
    Show,
    /// Nothing sensible to offer (CREATE TABLE column definitions, VALUES lists).
    Nothing,
}

const OPERAND_AFTER_KW: &[&str] = &[
    "SELECT", "DISTINCT", "WHERE", "AND", "OR", "NOT", "ON", "BY", "HAVING", "SET", "WHEN", "THEN", "ELSE", "CASE", "IN", "LIKE", "BETWEEN", "IS", "XOR", "ALL", "ANY",
];

fn expects_operand(last: Option<&Tok>) -> bool {
    match last {
        None => true,
        Some(t) if t.kind == T::Punct => !matches!(t.text.as_str(), ")" | ";"),
        Some(t) if t.kind == T::Word => OPERAND_AFTER_KW.iter().any(|k| t.text.eq_ignore_ascii_case(k)),
        _ => false,
    }
}

fn context(toks: &[Tok]) -> Ctx {
    let Some(first) = toks.first() else { return Ctx::Start };
    let last = toks.last();
    let upper_first = first.text.to_uppercase();
    if first.kind == T::Word {
        match upper_first.as_str() {
            "USE" if toks.len() == 1 => return Ctx::Database,
            "SHOW" => return Ctx::Show,
            "CREATE" | "ALTER" if !toks.iter().any(|t| is_kw(t, "SELECT") || is_kw(t, "AS")) && toks.iter().all(|t| !is_kw(t, "ON")) => {
                // `CREATE TABLE |` names a new object; `ALTER TABLE |` an existing one.
                if upper_first == "ALTER" && toks.len() >= 2 && toks.last().is_some_and(|t| is_kw(t, "TABLE")) {
                    return Ctx::Tables;
                }
                if toks.iter().any(|t| is_punct(t, "(")) { return Ctx::Nothing; }
                return Ctx::Nothing;
            }
            "DESCRIBE" | "DESC" | "EXPLAIN" if toks.len() == 1 => return Ctx::Tables,
            _ => {}
        }
    }

    let table_intro = |t: &Tok| ["FROM", "JOIN", "UPDATE", "INTO", "TABLE", "VIEW", "STRAIGHT_JOIN", "DESCRIBE", "TRUNCATE", "EXISTS"].iter().any(|k| is_kw(t, k));
    let mut depth = 0i32;
    let mut forced_operand = false;
    let mut i = toks.len();
    while i > 0 {
        i -= 1;
        let t = &toks[i];
        if is_punct(t, ")") {
            depth += 1;
            continue;
        }
        if is_punct(t, "(") {
            if depth > 0 {
                depth -= 1;
                continue;
            }
            // leaving the parenthesis our cursor is in
            let prev = i.checked_sub(1).map(|p| &toks[p]);
            let prev2 = i.checked_sub(2).map(|p| &toks[p]);
            match (prev, prev2) {
                (Some(p), Some(pp)) if is_ident(p) && (is_kw(pp, "INTO") || is_kw(pp, "REPLACE")) => {
                    return Ctx::InsertColumns { db: None, table: p.text.clone() };
                }
                (Some(p), Some(pp)) if is_ident(p) && is_punct(pp, ".") && i >= 4 && is_ident(&toks[i - 3]) && is_kw(&toks[i - 4], "INTO") => {
                    return Ctx::InsertColumns { db: Some(toks[i - 3].text.clone()), table: p.text.clone() };
                }
                (Some(p), _) if is_kw(p, "VALUES") => return Ctx::Nothing,
                (Some(p), _) if is_kw(p, "FROM") || is_kw(p, "JOIN") => return Ctx::Start, // subquery: SELECT …
                (Some(p), Some(pp)) if is_ident(p) && (is_kw(pp, "TABLE") || is_kw(pp, "EXISTS")) => return Ctx::Nothing, // CREATE TABLE t (
                _ => {
                    forced_operand = true; // function call arguments, IN (...), grouping
                    continue;
                }
            }
        }
        if depth > 0 || t.kind != T::Word {
            continue;
        }
        let up = t.text.to_uppercase();
        if table_intro(t) {
            let after_intro = last.is_some_and(|l| std::ptr::eq(l, t) || is_punct(l, ",") || (table_intro(l) && !is_kw(l, "EXISTS")) || is_kw(l, "EXISTS"));
            return if after_intro && !forced_operand { Ctx::Tables } else if forced_operand { Ctx::Operand } else { Ctx::AfterTable };
        }
        if up == "SELECT" {
            let operand = forced_operand || expects_operand(last);
            return if operand { Ctx::Operand } else { Ctx::AfterExpr { select_list: true } };
        }
        if matches!(up.as_str(), "ON" | "WHERE" | "HAVING" | "SET" | "BY" | "WHEN" | "THEN" | "ELSE" | "CASE" | "LIMIT") {
            let operand = forced_operand || expects_operand(last);
            return if operand { Ctx::Operand } else { Ctx::AfterExpr { select_list: false } };
        }
        if up == "VALUES" {
            return Ctx::Nothing;
        }
    }
    Ctx::Start
}

// ---------- building suggestions ----------

fn needs_quotes(name: &str) -> bool {
    let mut chars = name.chars();
    let ok_first = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || !c.is_ascii());
    !(ok_first && name.chars().all(is_word)) || is_reserved(name)
}

use myq_core::q as quote;

fn ident(name: &str, force_quote: bool) -> String {
    if force_quote || needs_quotes(name) { quote(name) } else { name.to_string() }
}

struct Builder {
    items: Vec<Suggestion>,
}

impl Builder {
    fn add(&mut self, label: &str, kind: SuggestionKind, detail: Option<String>, insert: String) {
        self.items.push(Suggestion { label: label.to_string(), kind, detail, insert });
    }
}

fn kind_weight(k: SuggestionKind) -> u8 {
    match k {
        SuggestionKind::Join | SuggestionKind::Related => 0,
        SuggestionKind::Column => 1,
        SuggestionKind::Alias | SuggestionKind::Table | SuggestionKind::View => 2,
        SuggestionKind::Database => 3,
        SuggestionKind::Function => 4,
        SuggestionKind::Keyword => 5,
    }
}

fn rank_and_cut(mut items: Vec<Suggestion>, prefix: &str, limit: usize) -> Vec<Suggestion> {
    let p = prefix.to_lowercase();
    let mut scored: Vec<(u8, u8, Suggestion)> = items
        .drain(..)
        .filter_map(|s| {
            let l = s.label.to_lowercase();
            let rank = if p.is_empty() || l.starts_with(&p) { 0 } else if l.contains(&p) { 1 } else { return None };
            Some((rank, kind_weight(s.kind), s))
        })
        .collect();
    scored.sort_by(|a, b| (a.0, a.1, a.2.label.to_lowercase()).cmp(&(b.0, b.1, b.2.label.to_lowercase())));
    let mut seen = std::collections::HashSet::new();
    scored.into_iter().map(|x| x.2).filter(|s| seen.insert((s.label.clone(), s.insert.clone(), s.kind as u8))).take(limit).collect()
}

fn keyword_items(list: &[&str]) -> Vec<Suggestion> {
    list.iter().map(|k| Suggestion { label: k.to_string(), kind: SuggestionKind::Keyword, detail: None, insert: k.to_string() }).collect()
}

fn function_items() -> Vec<Suggestion> {
    FUNCTIONS
        .iter()
        .map(|(n, sig)| Suggestion { label: n.to_string(), kind: SuggestionKind::Function, detail: Some(sig.to_string()), insert: format!("{n}(") })
        .collect()
}

fn table_items(cat: &Catalog, db: &str, force_quote: bool) -> Vec<Suggestion> {
    let Some(key) = cat.databases.iter().chain(cat.tables.keys()).find(|d| d.eq_ignore_ascii_case(db)) else { return vec![] };
    cat.tables.get(key).into_iter().flatten().map(|t| Suggestion {
        label: t.name.clone(),
        kind: if t.is_view { SuggestionKind::View } else { SuggestionKind::Table },
        detail: Some(key.clone()),
        insert: ident(&t.name, force_quote),
    }).collect()
}

fn column_items(t: &TableInfo, owner: &str, force_quote: bool, qualifier: Option<&str>) -> Vec<Suggestion> {
    t.columns.iter().map(|c| Suggestion {
        label: c.clone(),
        kind: SuggestionKind::Column,
        detail: Some(owner.to_string()),
        insert: match qualifier {
            Some(q) => format!("{}.{}", ident(q, false), ident(c, force_quote)),
            None => ident(c, force_quote),
        },
    }).collect()
}

/// Complete at `cursor_utf16` of `sql`. `current_db` resolves unqualified table names.
/// The table a `JOIN … ON` condition is being written for: the reference just before the last `ON` of this clause
/// (`AND` / `OR` continue the same condition). `None` when the cursor is not in an ON condition.
fn on_target<'a>(toks: &[Tok], refs: &'a [TableRef]) -> Option<&'a TableRef> {
    let mut depth = 0i32;
    for i in (0..toks.len()).rev() {
        let t = &toks[i];
        if is_punct(t, ")") { depth += 1; continue; }
        if is_punct(t, "(") { if depth == 0 { return None; } depth -= 1; continue; }
        if depth > 0 { continue; }
        if is_kw(t, "ON") {
            let name = toks.get(i.checked_sub(1)?)?;
            if !is_ident(name) { return None; }
            let n = name.text.trim_matches('`');
            return refs.iter().find(|r| r.alias.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(n)) || (r.alias.is_none() && r.table.eq_ignore_ascii_case(n)));
        }
        if ["WHERE", "SELECT", "GROUP", "ORDER", "HAVING", "LIMIT", "FROM", "JOIN", "SET", "UNION"].iter().any(|k| is_kw(t, k)) { return None; }
    }
    None
}

/// Join conditions from the foreign keys between `new` and each other table of the statement.
fn join_conditions(cat: &Catalog, new: &TableRef, refs: &[TableRef], current_db: Option<&str>) -> Vec<Suggestion> {
    let eq = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    let Some((ndb, nt)) = cat.find_table(new.db.as_deref(), &new.table, current_db) else { return vec![] };
    let owner = |r: &TableRef| r.alias.clone().unwrap_or_else(|| r.table.clone());
    let mut out = Vec::new();
    for other in refs.iter().filter(|r| !std::ptr::eq(*r, new)) {
        let Some((odb, ot)) = cat.find_table(other.db.as_deref(), &other.table, current_db) else { continue };
        for fk in &cat.fks {
            // the new table holds the key, or the other one does
            let (new_cols, other_cols) = if eq(&fk.db, ndb) && eq(&fk.table, &nt.name) && eq(&fk.ref_db, odb) && eq(&fk.ref_table, &ot.name) {
                (&fk.columns, &fk.ref_columns)
            } else if eq(&fk.db, odb) && eq(&fk.table, &ot.name) && eq(&fk.ref_db, ndb) && eq(&fk.ref_table, &nt.name) {
                (&fk.ref_columns, &fk.columns)
            } else {
                continue;
            };
            let (no, oo) = (ident(&owner(new), false), ident(&owner(other), false));
            let text = new_cols.iter().zip(other_cols).map(|(a, b)| format!("{no}.{} = {oo}.{}", ident(a, false), ident(b, false))).collect::<Vec<_>>().join(" AND ");
            out.push(Suggestion { label: text.clone(), kind: SuggestionKind::Join, detail: Some(format!("foreign key {}", fk.name)), insert: text });
        }
    }
    out
}

/// Tables linked by a foreign key to a table already in the statement, for the table name after `JOIN`.
fn related_tables(cat: &Catalog, refs: &[TableRef], current_db: Option<&str>, force_quote: bool) -> Vec<Suggestion> {
    let eq = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    let mut out: Vec<Suggestion> = Vec::new();
    for r in refs {
        let Some((db, t)) = cat.find_table(r.db.as_deref(), &r.table, current_db) else { continue };
        for fk in &cat.fks {
            let (rdb, rt) = if eq(&fk.db, db) && eq(&fk.table, &t.name) { (&fk.ref_db, &fk.ref_table) } else if eq(&fk.ref_db, db) && eq(&fk.ref_table, &t.name) { (&fk.db, &fk.table) } else { continue };
            if refs.iter().any(|x| eq(&x.table, rt)) || out.iter().any(|s| eq(&s.label, rt)) { continue; }
            let qualified = !current_db.is_some_and(|d| eq(d, rdb));
            let insert = if qualified { format!("{}.{}", ident(rdb, false), ident(rt, force_quote)) } else { ident(rt, force_quote) };
            out.push(Suggestion { label: rt.clone(), kind: SuggestionKind::Related, detail: Some(format!("linked to {}", r.table)), insert });
        }
    }
    out
}

pub fn complete(sql: &str, cursor_utf16: usize, cat: &Catalog, current_db: Option<&str>, limit: usize) -> Completions {
    let cursor = utf16_to_byte(sql, cursor_utf16);
    let empty = |at: usize| Completions { from: byte_to_utf16(sql, at), to: byte_to_utf16(sql, at), items: vec![] };

    let (all, comments) = tokenize(sql);
    // inside a comment, or inside a string: nothing to complete
    if comments.iter().any(|c| c.contains(cursor)) {
        return empty(cursor);
    }
    // typing a number
    if all.iter().any(|t| t.kind == T::Num && t.start < cursor && cursor <= t.end) {
        return empty(cursor);
    }
    if all.iter().any(|t| t.kind == T::Str && t.start < cursor && (cursor < t.end || (t.open && cursor <= t.end))) {
        return empty(cursor);
    }

    // statement boundaries: the last `;` before the cursor, the first one at or after it
    let stmt_start = all.iter().rev().find(|t| is_punct(t, ";") && t.end <= cursor).map_or(0, |t| t.end);
    let stmt_end = all.iter().find(|t| is_punct(t, ";") && t.start >= cursor).map_or(sql.len(), |t| t.start);
    let in_stmt: Vec<Tok> = all.iter().filter(|t| t.start >= stmt_start && t.end <= stmt_end).cloned().collect();
    // The word being typed: an identifier the cursor is in or touches. A backticked name counts while the
    // cursor is inside the backticks (including the closing one the editor inserted automatically).
    let typed: Option<Tok> = in_stmt
        .iter()
        .find(|t| match t.kind {
            T::Word => t.start < cursor && cursor <= t.end,
            T::Quoted => t.start < cursor && if t.open { cursor <= t.end } else { cursor < t.end },
            _ => false,
        })
        .cloned();
    let (from_byte, to_byte, prefix, quoted_prefix) = match &typed {
        Some(t) if t.kind == T::Quoted => {
            let raw = &sql[t.start + 1..cursor.min(t.end)];
            // the closing backtick right after the cursor is part of what gets replaced
            let to = if !t.open && cursor == t.end - 1 { t.end } else { cursor };
            (t.start, to, raw.replace("``", "`"), true)
        }
        Some(t) => (t.start, cursor, sql[t.start..cursor].to_string(), false),
        None => (cursor, cursor, String::new(), false),
    };
    // tokens before the word being typed (`from_byte` is the cursor itself when no word is being typed)
    let before: Vec<Tok> = in_stmt.iter().filter(|t| t.end <= from_byte).cloned().collect();
    let done = |items: Vec<Suggestion>| Completions {
        from: byte_to_utf16(sql, from_byte),
        to: byte_to_utf16(sql, to_byte),
        items: rank_and_cut(items, &prefix, limit),
    };
    if typed.as_ref().is_some_and(|t| t.kind == T::Word && t.text.chars().all(|c| c.is_ascii_digit())) {
        return empty(cursor);
    }
    let toks: Vec<Tok> = before;
    let refs = table_refs(&in_stmt);
    let resolve = |r: &TableRef| cat.find_table(r.db.as_deref(), &r.table, current_db);

    // ---- member access: qualifier.<here> ----
    if toks.last().is_some_and(|t| is_punct(t, ".")) && toks.len() >= 2 && is_ident(&toks[toks.len() - 2]) {
        let q = toks[toks.len() - 2].text.clone();
        let q2 = (toks.len() >= 4 && is_punct(&toks[toks.len() - 3], ".") && is_ident(&toks[toks.len() - 4])).then(|| toks[toks.len() - 4].text.clone());
        let mut items = Vec::new();
        match q2 {
            // db.table.<column>
            Some(db) => {
                if let Some((_, t)) = cat.find_table(Some(&db), &q, current_db) {
                    items.extend(column_items(t, &q, quoted_prefix, None));
                }
            }
            None => {
                // alias.<column> or table.<column>
                let by_alias = refs.iter().find(|r| r.alias.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(&q)));
                let by_name = refs.iter().find(|r| r.alias.is_none() && r.table.eq_ignore_ascii_case(&q));
                if let Some(r) = by_alias.or(by_name) {
                    if let Some((_, t)) = resolve(r) {
                        items.extend(column_items(t, &t.name, quoted_prefix, None));
                    }
                } else if let Some((_, t)) = cat.find_table(None, &q, current_db) {
                    items.extend(column_items(t, &t.name, quoted_prefix, None)); // table not in FROM yet
                }
                // db.<table>
                if items.is_empty() {
                    if let Some(db) = cat.find_database(&q) {
                        items.extend(table_items(cat, db, quoted_prefix));
                    }
                }
            }
        }
        return done(items);
    }

    let ctx = context(&toks);
    let mut b = Builder { items: vec![] };
    match &ctx {
        Ctx::Start => {
            b.items.extend(keyword_items(STATEMENT_STARTERS));
        }
        Ctx::Show => b.items.extend(keyword_items(&[
            "TABLES", "DATABASES", "COLUMNS FROM", "INDEX FROM", "CREATE TABLE", "PROCESSLIST", "FULL PROCESSLIST", "STATUS", "VARIABLES", "GRANTS", "WARNINGS",
            "ERRORS", "ENGINES", "TABLE STATUS", "TRIGGERS", "EVENTS", "PROCEDURE STATUS", "FUNCTION STATUS", "MASTER STATUS", "BINARY LOGS",
        ])),
        Ctx::Database => {
            for d in &cat.databases {
                b.add(d, SuggestionKind::Database, None, ident(d, quoted_prefix));
            }
        }
        Ctx::Tables => {
            // after JOIN: the tables linked to the ones already in the statement come first
            if toks.last().is_some_and(|t| is_kw(t, "JOIN")) {
                b.items.extend(related_tables(cat, &refs, current_db, quoted_prefix));
            }
            if let Some(db) = current_db {
                let related: Vec<String> = b.items.iter().map(|s| s.label.to_lowercase()).collect();
                b.items.extend(table_items(cat, db, quoted_prefix).into_iter().filter(|s| !related.contains(&s.label.to_lowercase())));
            }
            for d in &cat.databases {
                b.add(d, SuggestionKind::Database, None, ident(d, quoted_prefix));
            }
        }
        Ctx::AfterTable => b.items.extend(keyword_items(AFTER_TABLE)),
        Ctx::InsertColumns { db, table } => {
            if let Some((_, t)) = cat.find_table(db.as_deref(), table, current_db) {
                b.items.extend(column_items(t, &t.name, quoted_prefix, None));
            }
        }
        Ctx::Nothing => {}
        Ctx::AfterExpr { select_list } => {
            let list: &[&str] = if *select_list {
                &["FROM", "AS", "INTO"]
            } else {
                &["AND", "OR", "NOT", "IN", "LIKE", "BETWEEN", "IS NULL", "IS NOT NULL", "REGEXP", "ASC", "DESC", "GROUP BY", "ORDER BY", "HAVING", "LIMIT", "UNION", "FOR UPDATE"]
            };
            b.items.extend(keyword_items(list));
        }
        Ctx::Operand => {
            // `JOIN t ON |`: the conditions the foreign keys suggest, then the usual columns
            if let Some(new) = on_target(&toks, &refs) {
                b.items.extend(join_conditions(cat, new, &refs, current_db));
            }
            let resolved: Vec<(&TableRef, Option<&TableInfo>)> = refs.iter().map(|r| (r, resolve(r).map(|x| x.1))).collect();
            let known: Vec<&(&TableRef, Option<&TableInfo>)> = resolved.iter().filter(|(_, t)| t.is_some()).collect();
            // a column name that exists in several referenced tables is inserted qualified
            let mut seen: HashMap<String, usize> = HashMap::new();
            for (_, t) in &known {
                for c in &t.unwrap().columns {
                    *seen.entry(c.to_lowercase()).or_default() += 1;
                }
            }
            for (r, t) in &known {
                let t = t.unwrap();
                let owner = r.alias.clone().unwrap_or_else(|| t.name.clone());
                for c in column_items(t, &owner, quoted_prefix, None) {
                    let ambiguous = seen.get(&c.label.to_lowercase()).is_some_and(|n| *n > 1);
                    b.items.push(if ambiguous {
                        Suggestion { insert: format!("{}.{}", ident(&owner, false), c.insert), ..c }
                    } else {
                        c
                    });
                }
                b.add(&owner, SuggestionKind::Alias, Some(format!("table {}", t.name)), ident(&owner, quoted_prefix));
            }
            for (r, t) in &resolved {
                if t.is_none() && r.derived {
                    if let Some(a) = &r.alias {
                        b.add(a, SuggestionKind::Alias, Some("derived table".into()), ident(a, quoted_prefix));
                    }
                }
            }
            // No FROM written yet (`SELECT na| FROM …` is covered above): offer columns of every table
            // in the current database, but only once the user has typed something to narrow them down.
            if known.is_empty() && !prefix.is_empty() {
                if let Some(db) = current_db.and_then(|d| cat.tables.get_key_value(d)) {
                    for t in db.1 {
                        b.items.extend(column_items(t, &t.name, quoted_prefix, None));
                    }
                }
            }
            b.items.extend(function_items());
            b.items.extend(keyword_items(&["DISTINCT", "CASE", "NULL", "TRUE", "FALSE", "NOT", "EXISTS", "INTERVAL"]));
        }
    }
    let mut c = done(b.items);
    // Nothing matched in a structured context: fall back to any keyword so Ctrl-Space is never empty.
    if c.items.is_empty() && !matches!(ctx, Ctx::Nothing | Ctx::InsertColumns { .. }) && !prefix.is_empty() {
        c.items = rank_and_cut(keyword_items(KEYWORDS), &prefix, limit);
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(db: &str, kind: Kind, name: &str, parent: Option<&str>) -> Node {
        Node { database: db.into(), kind, name: name.into(), parent: parent.map(String::from) }
    }

    fn catalog() -> Catalog {
        let mut n = vec![node("shop", Kind::Database, "shop", None), node("other", Kind::Database, "other", None), node("mysql", Kind::Database, "mysql", None)];
        for (t, cols) in [("customers", vec!["id", "name", "email"]), ("orders", vec!["id", "customer_id", "total", "placed", "order"]), ("order items", vec!["qty"])] {
            n.push(node("shop", Kind::Table, t, None));
            n.extend(cols.into_iter().map(|c| node("shop", Kind::Column, c, Some(t))));
        }
        n.push(node("shop", Kind::View, "v_orders", None));
        n.push(node("shop", Kind::Column, "total", Some("v_orders")));
        n.push(node("other", Kind::Table, "logs", None));
        n.push(node("other", Kind::Column, "msg", Some("logs")));
        Catalog::from_nodes(&n)
    }

    /// Complete at the `|` marker; returns (labels, the full result).
    fn at(sql: &str) -> (Vec<String>, Completions) {
        let cursor = sql.find('|').expect("marker");
        let text = sql.replacen('|', "", 1);
        let c = complete(&text, text[..cursor].encode_utf16().count(), &catalog(), Some("shop"), 200);
        (c.items.iter().map(|i| i.label.clone()).collect(), c)
    }
    fn labels(sql: &str) -> Vec<String> {
        at(sql).0
    }
    fn has(v: &[String], s: &str) -> bool {
        v.iter().any(|x| x == s)
    }

    fn catalog_fk() -> Catalog {
        let mut c = catalog();
        let fk = |name: &str, t: &str, col: &str, rt: &str, rc: &str| Fk { name: name.into(), db: "shop".into(), table: t.into(), columns: vec![col.into()], ref_db: "shop".into(), ref_table: rt.into(), ref_columns: vec![rc.into()] };
        c.fks = vec![fk("fk_orders_customer", "orders", "customer_id", "customers", "id")];
        c
    }
    fn fk_labels(sql: &str) -> Vec<String> {
        let cursor = sql.find('|').expect("marker");
        let text = sql.replacen('|', "", 1);
        complete(&text, text[..cursor].encode_utf16().count(), &catalog_fk(), Some("shop"), 200).items.into_iter().map(|i| i.label).collect()
    }

    #[test]
    fn join_on_suggests_the_foreign_key_condition() {
        let l = fk_labels("SELECT * FROM orders o JOIN customers c ON |");
        assert_eq!(l[0], "c.id = o.customer_id", "{l:?}");
        // the key can be on either side, and the condition can be extended with AND
        let l = fk_labels("SELECT * FROM customers c JOIN orders o ON c.id = o.customer_id AND |");
        assert_eq!(l[0], "o.customer_id = c.id", "{l:?}");
        // no suggestion in a WHERE, and none without a key between the tables
        assert!(!fk_labels("SELECT * FROM orders o JOIN customers c ON c.id = o.customer_id WHERE |").iter().any(|x| x.contains(" = ")));
        assert!(!fk_labels("SELECT * FROM orders o JOIN v_orders v ON |").iter().any(|x| x.contains(" = ")));
    }

    #[test]
    fn join_offers_linked_tables_first() {
        let l = fk_labels("SELECT * FROM orders o JOIN |");
        assert_eq!(l[0], "customers", "{l:?}");
        assert_eq!(l.iter().filter(|x| *x == "customers").count(), 1, "not listed twice");
        // a table with no key to the others (a view) links nothing: the plain alphabetical list
        assert_eq!(fk_labels("SELECT * FROM v_orders v JOIN |")[0], "customers");
    }

    #[test]
    fn statement_start_and_keyword_prefix() {
        let l = labels("|");
        assert!(has(&l, "SELECT") && has(&l, "INSERT INTO") && !has(&l, "customers"));
        assert_eq!(labels("sel|")[0], "SELECT");
        assert_eq!(labels("SELECT 1; upd|")[0], "UPDATE");
    }

    #[test]
    fn table_names_after_from_join_update_into() {
        for q in ["SELECT * FROM |", "SELECT * FROM orders o JOIN |", "UPDATE |", "INSERT INTO |", "DELETE FROM |", "DESCRIBE |", "TRUNCATE TABLE |", "DROP TABLE IF EXISTS |"] {
            let l = labels(q);
            assert!(has(&l, "customers") && has(&l, "orders") && has(&l, "v_orders") && has(&l, "other"), "{q}: {l:?}");
            assert!(!has(&l, "SELECT") && !has(&l, "id"), "{q}: {l:?}");
        }
        assert_eq!(labels("SELECT * FROM cu|")[0], "customers");
        assert_eq!(labels("SELECT * FROM a, cu|")[0], "customers", "second table of a comma list");
    }

    #[test]
    fn after_a_table_come_clauses_not_tables() {
        let l = labels("SELECT * FROM orders |");
        assert!(has(&l, "WHERE") && has(&l, "LEFT JOIN") && has(&l, "GROUP BY") && !has(&l, "customers"), "{l:?}");
        let l = labels("SELECT * FROM orders o |");
        assert!(has(&l, "WHERE"), "after an alias too: {l:?}");
    }

    #[test]
    fn database_qualified_tables_and_columns() {
        let l = labels("SELECT * FROM other.|");
        assert_eq!(l, vec!["logs"]);
        let l = labels("SELECT * FROM shop.or|");
        assert!(has(&l, "orders") && has(&l, "order items") && !has(&l, "customers"), "{l:?}");
        let l = labels("SELECT shop.orders.| FROM shop.orders");
        assert!(has(&l, "total") && has(&l, "customer_id") && !has(&l, "name"), "{l:?}");
    }

    #[test]
    fn alias_and_table_member_completion() {
        let l = labels("SELECT o.| FROM orders o");
        assert!(has(&l, "total") && has(&l, "placed") && !has(&l, "email"), "{l:?}");
        assert_eq!(labels("SELECT o.to| FROM orders AS o JOIN customers c ON c.id = o.customer_id")[0], "total");
        let l = labels("SELECT * FROM orders o JOIN customers c ON c.| = o.customer_id");
        assert!(has(&l, "email") && !has(&l, "total"), "{l:?}");
        let l = labels("SELECT * FROM customers WHERE customers.|");
        assert!(has(&l, "name"), "{l:?}");
        let l = labels("SELECT customers.| FROM orders");        // table not in FROM yet: still resolved by name
        assert!(has(&l, "email"), "{l:?}");
    }

    #[test]
    fn columns_in_every_expression_clause() {
        for q in ["SELECT * FROM orders WHERE |", "SELECT * FROM orders WHERE total > 1 AND |", "SELECT * FROM orders ORDER BY |", "SELECT * FROM orders GROUP BY |",
                  "SELECT | FROM orders", "SELECT id, | FROM orders", "UPDATE orders SET |", "SELECT * FROM orders o JOIN customers c ON |", "SELECT COUNT(|) FROM orders",
                  "SELECT * FROM orders WHERE id IN (|", "SELECT * FROM orders HAVING |"] {
            let l = labels(q);
            assert!(has(&l, "total") && has(&l, "placed"), "{q}: {l:?}");
            assert!(has(&l, "COUNT") || has(&l, "NOW"), "functions are offered too: {q}");
        }
        let l = labels("SELECT * FROM orders WHERE |");
        assert!(!has(&l, "email"), "only referenced tables' columns: {l:?}");
    }

    #[test]
    fn columns_come_before_functions_and_keywords() {
        let l = labels("SELECT * FROM orders WHERE to|");
        assert_eq!(l[0], "total");
        let l = labels("SELECT * FROM orders WHERE c|");
        assert!(l.iter().position(|x| x == "customer_id").unwrap() < l.iter().position(|x| x == "COUNT").unwrap(), "columns rank above functions: {l:?}");
    }

    #[test]
    fn columns_before_from_is_written_use_the_whole_statement() {
        assert_eq!(labels("SELECT na| FROM customers")[0], "name");
        // nothing referenced anywhere: search every table of the current database once something is typed
        let l = labels("SELECT em|");
        assert_eq!(l[0], "email");
        assert!(!has(&labels("SELECT |"), "email"), "no prefix and no tables: do not dump every column");
    }

    #[test]
    fn ambiguous_columns_are_inserted_qualified() {
        let (_, c) = at("SELECT i| FROM orders o JOIN customers c ON c.id = o.customer_id");
        let ids: Vec<&Suggestion> = c.items.iter().filter(|i| i.label == "id").collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.iter().any(|i| i.insert == "o.id") && ids.iter().any(|i| i.insert == "c.id"));
        let (_, c) = at("SELECT t| FROM orders o JOIN customers c ON 1");
        assert_eq!(c.items.iter().find(|i| i.label == "total").unwrap().insert, "total", "unique names stay bare");
    }

    #[test]
    fn quoting_of_special_and_reserved_names() {
        let (_, c) = at("SELECT * FROM shop.ord|");
        assert_eq!(c.items.iter().find(|i| i.label == "order items").unwrap().insert, "`order items`");
        let (_, c) = at("SELECT * FROM orders WHERE ord|");
        assert_eq!(c.items.iter().find(|i| i.label == "order" && i.kind == SuggestionKind::Column).unwrap().insert, "`order`", "reserved word as a column");
    }

    #[test]
    fn open_backtick_replaces_the_backtick_and_an_auto_closed_one() {
        let sql = "SELECT * FROM `or`";
        let text_cursor = sql.find("`or").unwrap() + 3;
        let c = complete(sql, text_cursor, &catalog(), Some("shop"), 50);
        assert_eq!(c.from, sql.find('`').unwrap());
        assert_eq!(c.to, sql.len(), "the closing backtick typed by the editor is consumed");
        let insert = |c: &Completions, l: &str| c.items.iter().find(|i| i.label == l).unwrap().insert.clone();
        assert_eq!(insert(&c, "orders"), "`orders`");
        assert_eq!(insert(&c, "order items"), "`order items`");
        let c = complete("SELECT * FROM `or", 17, &catalog(), Some("shop"), 50);
        assert_eq!((c.from, c.to), (14, 17));
        assert_eq!(insert(&c, "orders"), "`orders`");
    }

    #[test]
    fn insert_column_list_and_values() {
        let l = labels("INSERT INTO orders (|");
        assert!(has(&l, "customer_id") && has(&l, "total") && !has(&l, "COUNT"), "{l:?}");
        let l = labels("INSERT INTO shop.customers (id, |");
        assert!(has(&l, "email"), "{l:?}");
        assert!(labels("INSERT INTO orders VALUES (|").is_empty());
        assert!(labels("CREATE TABLE t (|").is_empty());
    }

    #[test]
    fn use_show_and_after_expression_keywords() {
        assert_eq!(labels("USE |"), vec!["mysql", "other", "shop"]);
        assert!(has(&labels("SHOW |"), "TABLES"));
        let l = labels("SELECT id |");
        assert!(has(&l, "FROM") && has(&l, "AS"), "{l:?}");
        let l = labels("SELECT * FROM orders WHERE total > 1 |");
        assert!(has(&l, "AND") && has(&l, "ORDER BY") && has(&l, "LIMIT"), "{l:?}");
    }

    #[test]
    fn nothing_inside_strings_or_comments_and_other_statements_are_ignored() {
        assert!(labels("SELECT 'abc|").is_empty());
        assert!(labels("SELECT 'a|b' FROM orders").is_empty());
        assert!(labels("SELECT 1 -- note |").is_empty());
        assert!(labels("SELECT /* hint |").is_empty());
        assert!(labels("SELECT 12|").is_empty(), "numbers");
        // a string that has been closed does not block completion after it
        assert!(!labels("SELECT 'x' FROM cu|").is_empty());
        // the tables of an earlier statement do not leak into the next one
        let l = labels("SELECT * FROM orders; SELECT | FROM customers");
        assert!(has(&l, "email") && !has(&l, "total"), "{l:?}");
        // …and tokens after the cursor in the same statement are still used for aliases
        assert!(has(&labels("SELECT c.| FROM customers c WHERE 1"), "email"));
    }

    #[test]
    fn utf16_offsets_with_astral_characters() {
        let sql = "SELECT '😀' AS e, cu";
        let utf16 = sql.encode_utf16().count();
        let c = complete(sql, utf16, &catalog(), Some("shop"), 20);
        assert_eq!(c.to, utf16);
        assert_eq!(c.from, utf16 - 2);
        assert_eq!(c.items[0].label, "customer_id");
    }

    #[test]
    fn derived_tables_offer_their_alias_and_views_are_tables() {
        let l = labels("SELECT d.| FROM (SELECT 1 AS x) d");
        assert!(l.is_empty(), "columns of a derived table are unknown: {l:?}");
        let l = labels("SELECT * FROM v_|");
        assert_eq!(l[0], "v_orders");
        let (_, c) = at("SELECT * FROM v_|");
        assert_eq!(c.items[0].kind, SuggestionKind::View);
    }

    #[test]
    fn results_are_capped_and_ranked_prefix_first() {
        let c = complete("SELECT * FROM ", 14, &catalog(), Some("shop"), 3);
        assert_eq!(c.items.len(), 3);
        let l = labels("SELECT * FROM o|");
        assert!(l.iter().position(|x| x == "orders").unwrap() < l.iter().position(|x| x == "customers").unwrap(), "prefix matches first: {l:?}");
    }
}
