//! Pure SQL text tools: statement splitting, formatting, autocomplete. No server access.

mod complete;
mod format;
mod keywords;
mod kind;
mod reader;
mod split;
mod text;

pub use complete::{complete, Catalog, Completions, Suggestion, SuggestionKind, TableInfo};
pub use format::{format_sql, FormatError, FormatOptions, IndentStyle, KeywordCase};
pub use kind::{statement_kind, StatementKind};
pub use reader::{ScriptReader, ScriptStatement};
pub use split::{split_statements, statement_at, Statement};
pub use text::{byte_to_utf16, utf16_to_byte};
