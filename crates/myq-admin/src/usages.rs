//! "Where is this used?": the objects that mention a table or one of its columns. A rename does not update them, so this is the list
//! to look through first. Views, routines, triggers and events are searched by their text; foreign keys by their definition.

use crate::Result;
use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::Conn;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Usage {
    /// `view`, `procedure`, `function`, `trigger`, `event`, `foreign key (points here)`, `foreign key (from here)` or `index`.
    pub kind: String,
    pub db: String,
    pub name: String,
    /// What it says: the column pair of a key, or the line of the definition that mentions the name.
    pub detail: String,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Does `text` use `name` as a whole identifier (not as part of a longer one, and with backticks or quotes around it counted)?
pub fn mentions(text: &str, name: &str) -> bool {
    first_mention(text, name).is_some()
}

/// The line of `text` that mentions `name`, trimmed and shortened.
pub fn first_mention(text: &str, name: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    let (lt, ln) = (text.to_lowercase(), name.to_lowercase());
    let mut from = 0;
    while let Some(pos) = lt[from..].find(&ln) {
        let start = from + pos;
        let end = start + ln.len();
        let before = lt[..start].chars().next_back();
        let after = lt[end..].chars().next();
        if !before.is_some_and(is_word) && !after.is_some_and(is_word) {
            let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
            let line_end = text[end..].find('\n').map_or(text.len(), |i| end + i);
            let line = text[line_start..line_end].trim();
            return Some(if line.chars().count() > 140 { format!("{}…", line.chars().take(140).collect::<String>()) } else { line.to_string() });
        }
        from = end;
    }
    None
}

/// Filter a list of (kind, db, name, definition) rows down to those that mention the table (and the column, when asked).
pub fn filter_definitions(rows: Vec<(String, String, String, String)>, db: &str, table: &str, column: Option<&str>) -> Vec<Usage> {
    rows.into_iter()
        .filter(|(_, d, _, text)| d.eq_ignore_ascii_case(db) || mentions(text, db))
        .filter_map(|(kind, d, name, text)| {
            if !mentions(&text, table) {
                return None;
            }
            let detail = match column {
                Some(c) => first_mention(&text, c)?,
                None => first_mention(&text, table)?,
            };
            Some(Usage { kind, db: d, name, detail })
        })
        .collect()
}

pub async fn find_usages(conn: &mut Conn, db: &str, table: &str, column: Option<&str>) -> Result<Vec<Usage>> {
    let mut out: Vec<Usage> = Vec::new();
    // keys that point at this table, and keys this table has
    let col_here = column.map(|c| format!(" AND REFERENCED_COLUMN_NAME = '{}'", c.replace('\'', "''"))).unwrap_or_default();
    let pointing: Vec<(String, String, String, String, String)> = conn
        .exec(
            format!("SELECT TABLE_SCHEMA, TABLE_NAME, CONSTRAINT_NAME, COLUMN_NAME, REFERENCED_COLUMN_NAME FROM information_schema.KEY_COLUMN_USAGE WHERE REFERENCED_TABLE_SCHEMA = ? AND REFERENCED_TABLE_NAME = ?{col_here} ORDER BY TABLE_SCHEMA, TABLE_NAME, CONSTRAINT_NAME, ORDINAL_POSITION"),
            (db, table),
        )
        .await?;
    for (d, t, c, col, rcol) in pointing {
        out.push(Usage { kind: "foreign key (points here)".into(), db: d, name: format!("{t}.{c}"), detail: format!("{t}.{col} → {table}.{rcol}") });
    }
    let col_own = column.map(|c| format!(" AND COLUMN_NAME = '{}'", c.replace('\'', "''"))).unwrap_or_default();
    let own: Vec<(String, String, String, String)> = conn
        .exec(
            format!("SELECT CONSTRAINT_NAME, COLUMN_NAME, REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME FROM information_schema.KEY_COLUMN_USAGE WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND REFERENCED_TABLE_NAME IS NOT NULL{col_own} ORDER BY CONSTRAINT_NAME, ORDINAL_POSITION"),
            (db, table),
        )
        .await?;
    for (c, col, rt, rc) in own {
        out.push(Usage { kind: "foreign key (from here)".into(), db: db.into(), name: format!("{table}.{c}"), detail: format!("{table}.{col} → {rt}.{rc}") });
    }
    // objects with a text: only those whose text contains the name are fetched
    let like = format!("%{}%", table.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
    let mut defs: Vec<(String, String, String, String)> = Vec::new();
    let views: Vec<(String, String, String)> = conn.exec("SELECT TABLE_SCHEMA, TABLE_NAME, VIEW_DEFINITION FROM information_schema.VIEWS WHERE VIEW_DEFINITION LIKE ? LIMIT 500", (&like,)).await?;
    defs.extend(views.into_iter().map(|(d, n, t)| ("view".to_string(), d, n, t)));
    let routines: Vec<(String, String, String, String)> = conn.exec("SELECT ROUTINE_SCHEMA, ROUTINE_NAME, LOWER(ROUTINE_TYPE), ROUTINE_DEFINITION FROM information_schema.ROUTINES WHERE ROUTINE_DEFINITION LIKE ? LIMIT 500", (&like,)).await.unwrap_or_default();
    defs.extend(routines.into_iter().map(|(d, n, k, t)| (k, d, n, t)));
    let triggers: Vec<(String, String, String, String)> = conn.exec("SELECT TRIGGER_SCHEMA, TRIGGER_NAME, EVENT_OBJECT_TABLE, ACTION_STATEMENT FROM information_schema.TRIGGERS WHERE ACTION_STATEMENT LIKE ? OR (EVENT_OBJECT_SCHEMA = ? AND EVENT_OBJECT_TABLE = ?) LIMIT 500", (&like, db, table)).await.unwrap_or_default();
    // a trigger on the table counts even when its body does not repeat the name
    for (d, n, on, body) in triggers {
        if d.eq_ignore_ascii_case(db) && on.eq_ignore_ascii_case(table) && column.is_none() {
            out.push(Usage { kind: "trigger".into(), db: d, name: n, detail: format!("on {on}: {}", first_mention(&body, table).unwrap_or_else(|| body.lines().next().unwrap_or("").trim().to_string())) });
        } else {
            defs.push(("trigger".to_string(), d, n, body));
        }
    }
    let events: Vec<(String, String, String)> = conn.exec("SELECT EVENT_SCHEMA, EVENT_NAME, EVENT_DEFINITION FROM information_schema.EVENTS WHERE EVENT_DEFINITION LIKE ? LIMIT 500", (&like,)).await.unwrap_or_default();
    defs.extend(events.into_iter().map(|(d, n, t)| ("event".to_string(), d, n, t)));
    out.extend(filter_definitions(defs, db, table, column));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_counts_only_as_a_whole_identifier() {
        assert!(mentions("SELECT * FROM `orders` o", "orders"));
        assert!(mentions("select * from shop.Orders", "orders"));
        assert!(!mentions("SELECT * FROM orders_archive", "orders"));
        assert!(!mentions("SELECT * FROM old_orders", "orders"));
        assert!(!mentions("SELECT reorders FROM t", "orders"));
        assert!(mentions("UPDATE x SET a = 1 WHERE b IN (SELECT id FROM orders)", "orders"));
        assert!(!mentions("", "orders") && !mentions("x", ""));
    }

    #[test]
    fn the_line_that_mentions_it_is_returned() {
        let text = "BEGIN\n  UPDATE stock SET qty = qty - 1;\n  INSERT INTO orders_log SELECT * FROM orders WHERE id = 1;\nEND";
        assert_eq!(first_mention(text, "orders").unwrap(), "INSERT INTO orders_log SELECT * FROM orders WHERE id = 1;");
        assert_eq!(first_mention("a b c", "zzz"), None);
    }

    #[test]
    fn definitions_are_kept_when_they_mention_the_table_and_the_column() {
        let rows = vec![
            ("view".to_string(), "shop".to_string(), "v_big".to_string(), "select `o`.`total` from `shop`.`orders` o where `o`.`total` > 100".to_string()),
            ("view".to_string(), "shop".to_string(), "v_other".to_string(), "select id from customers".to_string()),
            ("view".to_string(), "shop".to_string(), "v_logs".to_string(), "select id from orders_archive".to_string()),
            ("procedure".to_string(), "other".to_string(), "p_elsewhere".to_string(), "select * from orders".to_string()),
            ("procedure".to_string(), "other".to_string(), "p_qualified".to_string(), "select * from shop.orders".to_string()),
        ];
        let all = filter_definitions(rows.clone(), "shop", "orders", None);
        assert_eq!(all.iter().map(|u| u.name.as_str()).collect::<Vec<_>>(), vec!["v_big", "p_qualified"], "another database's unqualified `orders` is not this table");
        let by_col = filter_definitions(rows, "shop", "orders", Some("total"));
        assert_eq!(by_col.iter().map(|u| u.name.as_str()).collect::<Vec<_>>(), vec!["v_big"]);
    }
}
