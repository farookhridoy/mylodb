use crate::Result;
use mysql_async::{prelude::Queryable, Conn, Row};

/// How many rows the server expects a `DELETE` or `UPDATE` to touch, from `EXPLAIN` (which does not run the statement).
/// This is the optimizer's estimate, not a count. None when the server cannot explain the statement.
pub async fn estimate_rows(conn: &mut Conn, database: Option<&str>, statement: &str) -> Result<Option<u64>> {
    if let Some(db) = database {
        conn.query_drop(format!("USE `{}`", db.replace('`', "``"))).await?;
    }
    let statement = statement.trim().trim_end_matches(';');
    // MySQL 8.0.32+ may default to a text tree without a `rows` column, so ask for the table form; servers that do not know the
    // keyword (MariaDB, older MySQL) get the plain form, which is the table form there.
    let rows: Vec<Row> = match conn.query(format!("EXPLAIN FORMAT=TRADITIONAL {statement}")).await {
        Ok(r) => r,
        Err(_) => match conn.query(format!("EXPLAIN {statement}")).await {
            Ok(r) => r,
            Err(_) => return Ok(None),
        },
    };
    Ok(rows.first().and_then(|r| r.get_opt::<Option<u64>, _>("rows")).and_then(|v| v.ok()).flatten())
}
