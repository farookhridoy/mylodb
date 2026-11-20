//! Left-menu object filter.
//!
//! Query syntax (whitespace-separated terms, all must match):
//! - `orders`        case-insensitive substring
//! - `ord*` `*_log`  glob (`*` any run, `?` one char), anchored
//! - `/^tmp_\d+$/`   regex (case-insensitive)
//! - `type:table`    restrict kind (database, table, view, procedure, function, trigger, event, column)
//! - `db:shop`       restrict to a database (substring)
//! - `col:email`     shorthand for `type:column` + name match

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Database,
    Table,
    View,
    Procedure,
    Function,
    Trigger,
    Event,
    Column,
}

impl Kind {
    fn parse(s: &str) -> Option<Kind> {
        Some(match s.to_ascii_lowercase().as_str() {
            "database" | "db" | "schema" => Kind::Database,
            "table" => Kind::Table,
            "view" => Kind::View,
            "procedure" | "proc" => Kind::Procedure,
            "function" | "func" => Kind::Function,
            "trigger" => Kind::Trigger,
            "event" => Kind::Event,
            "column" | "col" => Kind::Column,
            _ => return None,
        })
    }
}

/// One entry in the cached name index. `parent` is the owning table/routine for columns/triggers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub database: String,
    pub kind: Kind,
    pub name: String,
    pub parent: Option<String>,
}

#[derive(Debug)]
enum Matcher {
    Substring(String),
    Pattern(Regex),
}

impl Matcher {
    fn is_match(&self, name: &str) -> bool {
        match self {
            Matcher::Substring(s) => name.to_lowercase().contains(s),
            Matcher::Pattern(r) => r.is_match(name),
        }
    }
}

/// A parsed filter. Invalid regexes are reported, never silently ignored.
#[derive(Debug, Default)]
pub struct Filter {
    kinds: Vec<Kind>,
    db: Option<String>,
    names: Vec<Matcher>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FilterError {
    InvalidRegex(String),
    UnknownKind(String),
}

fn build(pattern: &str) -> Result<Regex, FilterError> {
    RegexBuilder::new(pattern)
        .case_insensitive(true)
        .build()
        .map_err(|e| FilterError::InvalidRegex(e.to_string()))
}

fn glob_to_regex(glob: &str) -> String {
    let mut out = String::from("^");
    for ch in glob.chars() {
        match ch {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            c => out.push_str(&regex::escape(&c.to_string())),
        }
    }
    out.push('$');
    out
}

impl Filter {
    pub fn parse(query: &str) -> Result<Filter, FilterError> {
        let mut f = Filter::default();
        for term in query.split_whitespace() {
            if let Some(v) = term.strip_prefix("type:") {
                f.kinds.push(Kind::parse(v).ok_or_else(|| FilterError::UnknownKind(v.into()))?);
            } else if let Some(v) = term.strip_prefix("db:") {
                f.db = Some(v.to_lowercase());
            } else if let Some(v) = term.strip_prefix("col:") {
                f.kinds.push(Kind::Column);
                f.names.push(Matcher::Substring(v.to_lowercase()));
            } else if term.len() >= 2 && term.starts_with('/') && term.ends_with('/') {
                f.names.push(Matcher::Pattern(build(&term[1..term.len() - 1])?));
            } else if term.contains('*') || term.contains('?') {
                f.names.push(Matcher::Pattern(build(&glob_to_regex(term))?));
            } else {
                f.names.push(Matcher::Substring(term.to_lowercase()));
            }
        }
        Ok(f)
    }

    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty() && self.db.is_none() && self.names.is_empty()
    }

    pub fn matches(&self, n: &Node) -> bool {
        (self.kinds.is_empty() || self.kinds.contains(&n.kind))
            && self.db.as_ref().map_or(true, |d| n.database.to_lowercase().contains(d))
            && self.names.iter().all(|m| m.is_match(&n.name))
    }

    /// Indices of matching nodes, in index order. The UI expands ancestors of each hit.
    pub fn apply(&self, index: &[Node]) -> Vec<usize> {
        index
            .iter()
            .enumerate()
            .filter(|(_, n)| self.matches(n))
            .map(|(i, _)| i)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(db: &str, kind: Kind, name: &str) -> Node {
        Node { database: db.into(), kind, name: name.into(), parent: None }
    }

    fn index() -> Vec<Node> {
        vec![
            n("shop", Kind::Database, "shop"),
            n("shop", Kind::Table, "orders"),
            n("shop", Kind::Table, "order_items"),
            n("shop", Kind::View, "v_orders"),
            n("shop", Kind::Column, "email"),
            n("blog", Kind::Table, "posts"),
            n("blog", Kind::Table, "tmp_42"),
        ]
    }

    fn run(q: &str) -> Vec<&'static str> {
        let idx = index();
        let hits = Filter::parse(q).unwrap().apply(&idx);
        hits.into_iter().map(|i| ["shop", "orders", "order_items", "v_orders", "email", "posts", "tmp_42"][i]).collect()
    }

    #[test]
    fn substring_is_case_insensitive() {
        assert_eq!(run("ORDER"), ["orders", "order_items", "v_orders"]);
    }

    #[test]
    fn glob_is_anchored() {
        assert_eq!(run("order*"), ["orders", "order_items"]);
        assert_eq!(run("*_orders"), ["v_orders"]);
    }

    #[test]
    fn regex_and_kind_and_db() {
        assert_eq!(run(r"/^tmp_\d+$/"), ["tmp_42"]);
        assert_eq!(run("type:view"), ["v_orders"]);
        assert_eq!(run("db:blog"), ["posts", "tmp_42"]);
        assert_eq!(run("db:shop type:table ord"), ["orders", "order_items"]);
        assert_eq!(run("col:mail"), ["email"]);
    }

    #[test]
    fn errors_are_reported() {
        assert!(matches!(Filter::parse("/(/"), Err(FilterError::InvalidRegex(_))));
        assert_eq!(Filter::parse("type:nope").unwrap_err(), FilterError::UnknownKind("nope".into()));
    }

    #[test]
    fn empty_query_matches_everything() {
        let f = Filter::parse("  ").unwrap();
        assert!(f.is_empty());
        assert_eq!(f.apply(&index()).len(), 7);
    }
}
