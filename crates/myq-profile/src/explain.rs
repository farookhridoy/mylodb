use crate::{single_statement, statement_kind, Error, Result, StatementKind};
use myq_driver::{execute_script, Conn, ResultSet};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Likely a performance problem.
    Warning,
    /// Worth knowing.
    Info,
    /// Good news (e.g. a covering index).
    Good,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Hint {
    pub severity: Severity,
    pub message: String,
    pub table: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ExplainResult {
    /// The classic tabular plan (always present).
    pub plan: ResultSet,
    /// `EXPLAIN FORMAT=JSON` document, when the server supports it.
    pub json: Option<String>,
    /// `EXPLAIN ANALYZE` iterator tree with actual timings (only when requested).
    pub tree: Option<String>,
    pub hints: Vec<Hint>,
}

fn col<'a>(columns: &[String], row: &'a [Option<String>], name: &str) -> Option<&'a str> {
    columns.iter().position(|c| c.eq_ignore_ascii_case(name)).and_then(|i| row.get(i)).and_then(|v| v.as_deref())
}

/// Plain-language findings from a classic EXPLAIN table.
pub fn hints_from_rows(columns: &[String], rows: &[Vec<Option<String>>]) -> Vec<Hint> {
    let mut out = Vec::new();
    for row in rows {
        let table = col(columns, row, "table").map(str::to_string);
        let name = table.clone().unwrap_or_else(|| "(derived)".into());
        let ty = col(columns, row, "type").unwrap_or("").to_uppercase();
        let est_rows: f64 = col(columns, row, "rows").and_then(|r| r.parse().ok()).unwrap_or(0.0);
        let extra = col(columns, row, "Extra").unwrap_or("");
        let key = col(columns, row, "key");
        let possible = col(columns, row, "possible_keys");
        let filtered: f64 = col(columns, row, "filtered").and_then(|r| r.parse().ok()).unwrap_or(100.0);
        let mut push = |severity, message: String| out.push(Hint { severity, message, table: table.clone() });

        if ty == "ALL" && table.is_some() {
            if est_rows >= 1000.0 {
                push(Severity::Warning, format!("Full table scan on `{name}` (about {} rows). An index on the columns used to filter or join would avoid reading every row.", est_rows as u64));
            } else {
                push(Severity::Info, format!("Full table scan on `{name}`, but it only has about {} rows.", est_rows as u64));
            }
        } else if ty == "INDEX" {
            push(Severity::Info, format!("Full index scan on `{name}` (every index entry is read)."));
        }
        if key.is_none() && possible.is_some() {
            push(Severity::Warning, format!("`{name}` has possible keys ({}) but none was chosen.", possible.unwrap()));
        }
        if extra.contains("Using filesort") {
            push(Severity::Warning, format!("Sorting `{name}` needs an extra pass (filesort). An index matching the ORDER BY can avoid it."));
        }
        if extra.contains("Using temporary") {
            push(Severity::Warning, format!("A temporary table is built for `{name}` (GROUP BY / DISTINCT / UNION)."));
        }
        if extra.contains("Using join buffer") {
            push(Severity::Warning, format!("Joining `{name}` uses a join buffer, which means there is no usable index for the join condition."));
        }
        if extra.contains("Using index") && !extra.contains("Using index condition") && !extra.contains("Using index for") {
            push(Severity::Good, format!("`{name}` is answered from the index alone (covering index)."));
        }
        if extra.contains("Impossible WHERE") || extra.contains("no matching row") {
            push(Severity::Info, format!("The server found `{name}` can return no rows ({}).", extra));
        }
        if filtered < 10.0 && est_rows >= 1000.0 && ty != "ALL" {
            push(Severity::Info, format!("Only about {filtered}% of the rows examined in `{name}` are kept."));
        }
    }
    out
}

async fn run_one(conn: &mut Conn, database: Option<&str>, sql: &str) -> Result<ResultSet> {
    let mut results = execute_script(conn, database, sql).await;
    let r = results.pop().ok_or_else(|| Error::Server("no result".into()))?;
    if let Some(e) = r.error {
        return Err(Error::Server(e));
    }
    r.sets.into_iter().next().ok_or_else(|| Error::Server("the server returned no plan".into()))
}

/// EXPLAIN one statement. `analyze` runs it for real (EXPLAIN ANALYZE) and is limited to reads.
pub async fn explain(conn: &mut Conn, database: Option<&str>, sql: &str, analyze: bool) -> Result<ExplainResult> {
    let stmt = single_statement(sql)?;
    let kind = statement_kind(&stmt);
    if kind == StatementKind::Other {
        return Err(Error::Rejected("EXPLAIN works on SELECT, INSERT, UPDATE, DELETE and REPLACE statements".into()));
    }
    if analyze && kind != StatementKind::Read {
        return Err(Error::Rejected("EXPLAIN ANALYZE executes the statement, so it is only allowed for SELECT".into()));
    }
    // MySQL 9 returns a tree for a bare EXPLAIN (the `explain_format` default), so ask for the table
    // explicitly; servers without the keyword (MariaDB) already return the table by default.
    let plan = match run_one(conn, database, &format!("EXPLAIN FORMAT=TRADITIONAL {stmt}")).await {
        Ok(p) => p,
        Err(_) => run_one(conn, database, &format!("EXPLAIN {stmt}")).await?,
    };
    let columns: Vec<String> = plan.columns.iter().map(|c| c.name.clone()).collect();
    let hints = hints_from_rows(&columns, &plan.rows);
    // The JSON form and the analyzed tree are extras: a server that lacks one still gives the plan.
    let json = run_one(conn, database, &format!("EXPLAIN FORMAT=JSON {stmt}")).await.ok().and_then(|s| s.rows.into_iter().next()).and_then(|r| r.into_iter().next().flatten());
    let tree = if analyze {
        // MySQL: EXPLAIN ANALYZE (an iterator tree). MariaDB spells it ANALYZE, and FORMAT=JSON carries the measured times (r_total_time_ms, r_rows).
        let first_cell = |s: ResultSet| s.rows.into_iter().next().and_then(|r| r.into_iter().next().flatten()).unwrap_or_default();
        Some(match run_one(conn, database, &format!("EXPLAIN ANALYZE {stmt}")).await {
            Ok(s) => first_cell(s),
            Err(_) => first_cell(run_one(conn, database, &format!("ANALYZE FORMAT=JSON {stmt}")).await?),
        })
    } else {
        None
    };
    Ok(ExplainResult { plan, json, tree, hints })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols() -> Vec<String> {
        ["id", "select_type", "table", "type", "possible_keys", "key", "rows", "filtered", "Extra"].iter().map(|s| s.to_string()).collect()
    }
    fn row(table: &str, ty: &str, possible: Option<&str>, key: Option<&str>, rows: &str, filtered: &str, extra: &str) -> Vec<Option<String>> {
        vec![Some("1".into()), Some("SIMPLE".into()), Some(table.into()), Some(ty.into()), possible.map(String::from), key.map(String::from), Some(rows.into()), Some(filtered.into()), Some(extra.into())]
    }
    fn msgs(h: &[Hint], sev: Severity) -> Vec<String> {
        h.iter().filter(|x| x.severity == sev).map(|x| x.message.clone()).collect()
    }

    #[test]
    fn flags_scans_sorts_temporaries_and_unused_keys() {
        let h = hints_from_rows(&cols(), &[
            row("big", "ALL", Some("idx_a"), None, "50000", "10.00", "Using where; Using filesort; Using temporary; Using join buffer (hash join)"),
            row("tiny", "ALL", None, None, "20", "100.00", ""),
            row("ix", "index", None, Some("PRIMARY"), "500", "100.00", "Using index"),
        ]);
        let w = msgs(&h, Severity::Warning);
        assert!(w.iter().any(|m| m.contains("Full table scan on `big`") && m.contains("50000")), "{w:?}");
        assert!(w.iter().any(|m| m.contains("possible keys (idx_a)")));
        assert!(w.iter().any(|m| m.contains("filesort")) && w.iter().any(|m| m.contains("temporary")) && w.iter().any(|m| m.contains("join buffer")));
        let i = msgs(&h, Severity::Info);
        assert!(i.iter().any(|m| m.contains("`tiny`") && m.contains("only has about 20")), "small tables are not alarming: {i:?}");
        assert!(i.iter().any(|m| m.contains("Full index scan on `ix`")));
        assert!(msgs(&h, Severity::Good).iter().any(|m| m.contains("covering index")));
    }

    #[test]
    fn a_good_plan_has_no_warnings() {
        let h = hints_from_rows(&cols(), &[row("t", "const", Some("PRIMARY"), Some("PRIMARY"), "1", "100.00", "")]);
        assert!(h.is_empty(), "{h:?}");
        let h = hints_from_rows(&cols(), &[row("t", "ref", Some("i"), Some("i"), "5", "100.00", "Using index condition")]);
        assert!(msgs(&h, Severity::Good).is_empty(), "index condition pushdown is not a covering index");
        let h = hints_from_rows(&cols(), &[vec![None; 9]]);
        assert!(h.is_empty(), "NULL-filled rows (e.g. 'no tables used') are ignored");
    }
}
