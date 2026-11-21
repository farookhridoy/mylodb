//! The objects an engine exposes in the browser tree.

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
    /// A kind from the words the filter accepts (`db`, `proc`, `col` ...).
    pub fn parse(s: &str) -> Option<Kind> {
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

