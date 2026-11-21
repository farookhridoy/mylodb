//! Index advisor: findings about the indexes of a set of tables, from their definitions alone (no server needed).
//!
//! * **Duplicate**: two indexes on the same columns in the same order (and the same uniqueness): one of them is wasted.
//! * **Redundant**: a plain index whose columns are the start of another index (or of the primary key): the longer index serves the same lookups.
//! * **Missing foreign key index**: a foreign key whose columns are not the start of any index. The server needs one to enforce the key and to
//!   find child rows when a parent row changes or is deleted (InnoDB creates it automatically; other engines do not).
//!
//! A unique index is never called redundant (it enforces something), full-text and spatial indexes are left alone, and an index with
//! prefix lengths or descending parts is only matched with an identical one.

use crate::ddl::{q, IndexColumn, IndexDef, IndexKind, TableDef};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    Duplicate,
    Redundant,
    MissingForeignKeyIndex,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub table: String,
    pub kind: FindingKind,
    /// The index to drop (duplicate, redundant) or the foreign key without an index (missing).
    pub index: String,
    /// In words: what is wrong and what covers it.
    pub detail: String,
    /// The statement that fixes it (`DROP INDEX`, or `CREATE INDEX` for a missing key index), without the trailing `;`.
    pub sql: String,
}

fn plain(i: &IndexDef) -> bool {
    matches!(i.kind, IndexKind::Index | IndexKind::Unique | IndexKind::Primary)
}

fn simple(c: &IndexColumn) -> bool {
    c.length.is_none() && !c.desc
}

fn names(i: &IndexDef) -> Vec<&str> {
    i.columns.iter().map(|c| c.name.as_str()).collect()
}

fn is_prefix(short: &IndexDef, long: &IndexDef) -> bool {
    short.columns.len() < long.columns.len() && short.columns.iter().zip(&long.columns).all(|(a, b)| a == b)
}

fn drop_sql(db: &str, table: &str, name: &str) -> String {
    format!("DROP INDEX {} ON {}.{}", q(name), q(db), q(table))
}

pub fn analyze(db: &str, tables: &[TableDef]) -> Vec<Finding> {
    let mut out = Vec::new();
    for t in tables {
        let idx: Vec<&IndexDef> = t.indexes.iter().filter(|i| plain(i)).collect();
        let mut flagged: Vec<&str> = Vec::new();
        // duplicates: keep the first (the primary key, else the first declared), flag the rest
        for (n, i) in idx.iter().enumerate() {
            if i.kind == IndexKind::Primary {
                continue;
            }
            if let Some(first) = idx[..n].iter().find(|j| j.columns == i.columns && (j.kind == IndexKind::Unique) == (i.kind == IndexKind::Unique)) {
                // a unique duplicate of a unique index, or a plain duplicate of a plain one; a plain index duplicating a unique/primary one is also wasted
                out.push(Finding { table: t.name.clone(), kind: FindingKind::Duplicate, index: i.name.clone(), detail: format!("`{}` has the same columns ({}) as `{}`", i.name, names(i).join(", "), first.name), sql: drop_sql(db, &t.name, &i.name) });
                flagged.push(&i.name);
            } else if i.kind == IndexKind::Index {
                if let Some(same) = idx.iter().find(|j| j.name != i.name && j.columns == i.columns && matches!(j.kind, IndexKind::Primary | IndexKind::Unique)) {
                    out.push(Finding { table: t.name.clone(), kind: FindingKind::Duplicate, index: i.name.clone(), detail: format!("`{}` repeats the columns ({}) of the {} `{}`", i.name, names(i).join(", "), if same.kind == IndexKind::Primary { "primary key" } else { "unique index" }, same.name), sql: drop_sql(db, &t.name, &i.name) });
                    flagged.push(&i.name);
                }
            }
        }
        // redundant: a plain index that is the start of another index
        for i in idx.iter().filter(|i| i.kind == IndexKind::Index && !flagged.contains(&i.name.as_str()) && i.columns.iter().all(simple)) {
            if let Some(longer) = idx.iter().find(|j| j.name != i.name && is_prefix(i, j) && j.columns.iter().zip(&i.columns).all(|(a, b)| simple(a) && a.name == b.name)) {
                out.push(Finding { table: t.name.clone(), kind: FindingKind::Redundant, index: i.name.clone(), detail: format!("`{}` ({}) is the start of `{}` ({}), which serves the same lookups", i.name, names(i).join(", "), longer.name, names(longer).join(", ")), sql: drop_sql(db, &t.name, &i.name) });
            }
        }
        // foreign keys without an index that starts with their columns
        for f in &t.foreign_keys {
            let covered = t.indexes.iter().filter(|i| plain(i)).any(|i| i.columns.len() >= f.columns.len() && i.columns.iter().zip(&f.columns).all(|(a, b)| &a.name == b && a.length.is_none()));
            if !covered {
                let name = format!("idx_{}_{}", t.name, f.columns.join("_"));
                let cols = f.columns.iter().map(|c| q(c)).collect::<Vec<_>>().join(", ");
                out.push(Finding { table: t.name.clone(), kind: FindingKind::MissingForeignKeyIndex, index: f.name.clone(), detail: format!("foreign key `{}` on ({}) has no index that starts with those columns", f.name, f.columns.join(", ")), sql: format!("CREATE INDEX {} ON {}.{} ({cols})", q(&name), q(db), q(&t.name)) });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ddl::{ColumnDef, ForeignKeyDef, RefAction};

    fn col(n: &str) -> ColumnDef {
        ColumnDef { name: n.into(), data_type: "int".into(), nullable: true, ..Default::default() }
    }
    fn ic(n: &str) -> IndexColumn {
        IndexColumn { name: n.into(), length: None, desc: false }
    }
    fn ix(name: &str, kind: IndexKind, cols: &[&str]) -> IndexDef {
        IndexDef { name: name.into(), original_name: None, kind, columns: cols.iter().map(|c| ic(c)).collect() }
    }
    fn t(indexes: Vec<IndexDef>) -> TableDef {
        TableDef { name: "orders".into(), columns: vec![col("id"), col("customer_id"), col("placed"), col("status")], indexes, ..Default::default() }
    }

    #[test]
    fn a_clean_table_has_no_findings() {
        let tbl = t(vec![ix("PRIMARY", IndexKind::Primary, &["id"]), ix("idx_customer", IndexKind::Index, &["customer_id", "placed"]), ix("idx_status", IndexKind::Index, &["status"])]);
        assert!(analyze("shop", &[tbl]).is_empty());
    }

    #[test]
    fn identical_indexes_are_duplicates_and_the_later_one_is_flagged() {
        let tbl = t(vec![ix("PRIMARY", IndexKind::Primary, &["id"]), ix("a", IndexKind::Index, &["status"]), ix("b", IndexKind::Index, &["status"])]);
        let f = analyze("shop", &[tbl]);
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].kind, f[0].index.as_str()), (FindingKind::Duplicate, "b"));
        assert_eq!(f[0].sql, "DROP INDEX `b` ON `shop`.`orders`");
        assert!(f[0].detail.contains("`a`"));
    }

    #[test]
    fn a_plain_index_that_repeats_the_primary_key_or_a_unique_index_is_a_duplicate() {
        let tbl = t(vec![ix("PRIMARY", IndexKind::Primary, &["id"]), ix("idx_id", IndexKind::Index, &["id"]), ix("uq_status", IndexKind::Unique, &["status"]), ix("idx_status", IndexKind::Index, &["status"])]);
        let f: Vec<(&str, FindingKind)> = analyze("shop", &[tbl]).iter().map(|x| (x.index.as_str(), x.kind)).collect::<Vec<_>>().into_iter().map(|(a, b)| (Box::leak(a.to_string().into_boxed_str()) as &str, b)).collect();
        assert_eq!(f, [("idx_id", FindingKind::Duplicate), ("idx_status", FindingKind::Duplicate)]);
    }

    #[test]
    fn the_start_of_a_longer_index_is_redundant_but_a_unique_index_never_is() {
        let tbl = t(vec![
            ix("PRIMARY", IndexKind::Primary, &["id"]),
            ix("idx_c", IndexKind::Index, &["customer_id"]),
            ix("idx_c_p", IndexKind::Index, &["customer_id", "placed"]),
            ix("uq_s", IndexKind::Unique, &["status"]),
            ix("uq_s_p", IndexKind::Unique, &["status", "placed"]),
            ix("idx_p_c", IndexKind::Index, &["placed", "customer_id"]),
        ]);
        let f = analyze("shop", &[tbl]);
        let redundant: Vec<&str> = f.iter().filter(|x| x.kind == FindingKind::Redundant).map(|x| x.index.as_str()).collect();
        assert_eq!(redundant, ["idx_c"], "{f:?}");
        assert!(f.iter().find(|x| x.index == "idx_c").unwrap().detail.contains("`idx_c_p`"));
        assert!(!f.iter().any(|x| x.index.starts_with("uq_") || x.index == "idx_c_p" || x.index == "idx_p_c"), "column order matters, unique indexes enforce something");
    }

    #[test]
    fn prefix_lengths_and_descending_parts_are_only_matched_with_identical_indexes() {
        let mut short = ix("idx_s", IndexKind::Index, &["status"]);
        short.columns[0].length = Some(4);
        let tbl = t(vec![ix("PRIMARY", IndexKind::Primary, &["id"]), short, ix("idx_s_p", IndexKind::Index, &["status", "placed"])]);
        assert!(analyze("shop", &[tbl]).is_empty(), "a 4-character prefix index is not the same as the whole column");
    }

    #[test]
    fn full_text_indexes_are_left_alone() {
        let tbl = t(vec![ix("PRIMARY", IndexKind::Primary, &["id"]), ix("ft_a", IndexKind::Fulltext, &["status"]), ix("ft_b", IndexKind::Fulltext, &["status"])]);
        assert!(analyze("shop", &[tbl]).is_empty());
    }

    #[test]
    fn a_foreign_key_without_an_index_gets_a_create_index_suggestion() {
        let fk = |cols: &[&str]| ForeignKeyDef { name: "fk_c".into(), original_name: None, columns: cols.iter().map(|c| c.to_string()).collect(), ref_schema: None, ref_table: "customers".into(), ref_columns: vec!["id".into()], on_delete: RefAction::Restrict, on_update: RefAction::Restrict };
        let mut bare = t(vec![ix("PRIMARY", IndexKind::Primary, &["id"])]);
        bare.foreign_keys.push(fk(&["customer_id"]));
        let f = analyze("shop", &[bare]);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].kind, FindingKind::MissingForeignKeyIndex);
        assert_eq!(f[0].sql, "CREATE INDEX `idx_orders_customer_id` ON `shop`.`orders` (`customer_id`)");

        let mut covered = t(vec![ix("PRIMARY", IndexKind::Primary, &["id"]), ix("idx_c_p", IndexKind::Index, &["customer_id", "placed"])]);
        covered.foreign_keys.push(fk(&["customer_id"]));
        assert!(analyze("shop", &[covered]).is_empty(), "an index that starts with the key's columns covers it");
    }
}
