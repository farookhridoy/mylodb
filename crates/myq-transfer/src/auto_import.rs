//! Importing a file with nobody to answer questions (a scheduled import): CSV, TSV, JSON or JSON Lines into an existing table, the
//! columns matched to the file's header by name. It stops at the first bad row and undoes everything.

use crate::{import_csv, json_to_csv, preview_csv, sniff_delimiter, CsvFormat, Error, ImportMode, ImportPlan, ImportResult, ProgressFn, Result, NULL_TEXT};
use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::Conn;
use std::path::Path;
use std::sync::atomic::AtomicBool;

/// For each header, the table column with that name (case and surrounding spaces ignored), or `None` when there is none.
pub fn map_by_header(headers: &[String], columns: &[String]) -> Vec<Option<String>> {
    headers.iter().map(|h| columns.iter().find(|c| c.eq_ignore_ascii_case(h.trim())).cloned()).collect()
}

/// The result of the import, and the names in the file's header that no column of the table has (they were not loaded).
pub struct AutoImport {
    pub result: ImportResult,
    pub skipped: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
pub async fn import_file(conn: &mut Conn, path: &Path, database: &str, table: &str, mode: ImportMode, replace_all: bool, progress: ProgressFn<'_>, cancel: &AtomicBool) -> Result<AutoImport> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    // a JSON file goes through the same CSV pipeline; the temporary file is removed whatever happens
    let converted = if matches!(ext.as_str(), "json" | "jsonl" | "ndjson") {
        let tmp = std::env::temp_dir().join(format!("mylodb_auto_{}_{}.csv", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)));
        if let Err(e) = json_to_csv(path, &tmp) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        Some(tmp)
    } else {
        None
    };
    let source = converted.as_deref().unwrap_or(path);
    let outcome = async {
        let format = match &converted {
            Some(_) => CsvFormat { null_text: Some(NULL_TEXT.into()), ..Default::default() },
            None => CsvFormat { delimiter: sniff_delimiter(source)?, ..Default::default() },
        };
        let headers = preview_csv(source, &format, 1)?.headers;
        let columns: Vec<String> = conn
            .exec("SELECT COLUMN_NAME FROM information_schema.COLUMNS WHERE TABLE_SCHEMA=? AND TABLE_NAME=? AND EXTRA NOT IN ('VIRTUAL GENERATED','STORED GENERATED') ORDER BY ORDINAL_POSITION", (database, table))
            .await?;
        if columns.is_empty() {
            return Err(Error::Invalid(format!("table `{database}`.`{table}` does not exist")));
        }
        let mapping = map_by_header(&headers, &columns);
        if mapping.iter().all(Option::is_none) {
            return Err(Error::Invalid(format!("no name in the file's first row matches a column of `{table}`")));
        }
        let skipped = headers.iter().zip(&mapping).filter(|(_, m)| m.is_none()).map(|(h, _)| h.clone()).collect();
        let plan = ImportPlan { database: database.into(), table: table.into(), mapping, mode, batch_rows: 500, stop_on_error: true, truncate_first: false, empty_as_null: true, disable_fk_checks: false, delete_first: replace_all };
        let result = import_csv(conn, source, &format, &plan, progress, cancel).await?;
        Ok(AutoImport { result, skipped })
    }
    .await;
    if let Some(tmp) = &converted {
        let _ = std::fs::remove_file(tmp);
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_find_columns_by_name() {
        let cols = vec!["id".to_string(), "Total".to_string()];
        let m = map_by_header(&[" ID ".into(), "total".into(), "extra".into()], &cols);
        assert_eq!(m, vec![Some("id".into()), Some("Total".into()), None]);
    }
}
