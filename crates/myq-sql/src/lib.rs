//! Pure SQL text tools: statement splitting, formatting, autocomplete. No server access.

mod complete;
mod format;
mod keywords;
mod kind;
mod reader;
mod risk;
mod split;
mod text;

pub use complete::{complete, Catalog, Completions, Suggestion, SuggestionKind, TableInfo};
pub use format::{format_sql, FormatError, FormatOptions, IndentStyle, KeywordCase};
pub use kind::{statement_kind, StatementKind};
pub use risk::{read_only_violation, read_only_violations, review, Finding, RiskKind, Severity, StatementRisk};
pub use reader::{ScriptReader, ScriptStatement};
pub use split::{split_statements, statement_at, Statement};
pub use text::{byte_to_utf16, utf16_to_byte};
