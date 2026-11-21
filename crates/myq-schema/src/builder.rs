//! Visual query builder: turns the choices made on the builder page (tables, joins, columns, filters, order, limit) into one `SELECT`.
//! Pure. Every name goes through the dialect's identifier quoting and every value through its string quoting, and every name is checked
//! against the tables the spec itself declares, so the page can never produce SQL that refers to something it did not offer.

use myq_core::{Dialect, MySql};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct TableRef {
    pub db: String,
    pub table: String,
    /// Short name used in the query; unique within the spec.
    pub alias: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinKind {
    Inner,
    Left,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Join {
    pub kind: JoinKind,
    pub table: TableRef,
    /// `(alias.column of an earlier table, column of this table)` pairs, joined with AND.
    pub on: Vec<(Side, String)>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Side {
    pub alias: String,
    pub column: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Agg {
    Count,
    Sum,
    Avg,
    Min,
    Max,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Select {
    pub alias: String,
    pub column: String,
    /// Output name (`AS`).
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub agg: Option<Agg>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Contains,
    StartsWith,
    EndsWith,
    In,
    Between,
    IsNull,
    NotNull,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Filter {
    pub alias: String,
    pub column: String,
    pub op: Op,
    /// One value (two for `between`, a comma-separated list for `in`, none for the NULL tests).
    #[serde(default)]
    pub values: Vec<String>,
    /// Compare as a number (no quotes) when every value looks like one.
    #[serde(default)]
    pub numeric: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Order {
    pub alias: String,
    pub column: String,
    #[serde(default)]
    pub desc: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct QuerySpec {
    pub from: TableRef,
    #[serde(default)]
    pub joins: Vec<Join>,
    /// Empty means every column of every table (`*`).
    #[serde(default)]
    pub select: Vec<Select>,
    #[serde(default)]
    pub distinct: bool,
    #[serde(default)]
    pub filters: Vec<Filter>,
    #[serde(default)]
    pub order: Vec<Order>,
    #[serde(default)]
    pub limit: Option<u64>,
}

fn looks_numeric(s: &str) -> bool {
    let t = s.trim();
    let t = t.strip_prefix('-').unwrap_or(t);
    !t.is_empty() && t.chars().filter(|c| *c == '.').count() <= 1 && t.chars().all(|c| c.is_ascii_digit() || c == '.') && t != "."
}

fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// The statement, or the first thing wrong with the spec.
pub fn build(spec: &QuerySpec) -> Result<String, String> {
    build_with(spec, &MySql)
}

pub fn build_with(spec: &QuerySpec, d: &dyn Dialect) -> Result<String, String> {
    let q = |s: &str| d.quote_ident(s);
    let mut aliases: Vec<&str> = vec![spec.from.alias.as_str()];
    for j in &spec.joins {
        if aliases.contains(&j.table.alias.as_str()) {
            return Err(format!("the name `{}` is used by two tables", j.table.alias));
        }
        aliases.push(&j.table.alias);
    }
    let known = |a: &str| -> Result<String, String> { if aliases.contains(&a) { Ok(q(a)) } else { Err(format!("`{a}` is not one of the tables in the query")) } };
    let col = |a: &str, c: &str| -> Result<String, String> {
        if c.is_empty() { return Err("a column name is empty".into()); }
        Ok(format!("{}.{}", known(a)?, q(c)))
    };
    let tref = |t: &TableRef| format!("{}.{} AS {}", q(&t.db), q(&t.table), q(&t.alias));

    let mut select = Vec::new();
    for s in &spec.select {
        let c = if s.column == "*" { format!("{}.*", known(&s.alias)?) } else { col(&s.alias, &s.column)? };
        let expr = match s.agg {
            Some(Agg::Count) if s.column == "*" => "COUNT(*)".to_string(),
            Some(a) => format!("{}({c})", match a { Agg::Count => "COUNT", Agg::Sum => "SUM", Agg::Avg => "AVG", Agg::Min => "MIN", Agg::Max => "MAX" }),
            None => c,
        };
        select.push(match s.name.as_deref().filter(|n| !n.is_empty()) {
            Some(n) => format!("{expr} AS {}", q(n)),
            None => expr,
        });
    }
    // an aggregate next to plain columns needs GROUP BY on the plain ones
    let aggregated = spec.select.iter().any(|s| s.agg.is_some());
    let group: Vec<String> = if aggregated { spec.select.iter().filter(|s| s.agg.is_none() && s.column != "*").map(|s| col(&s.alias, &s.column)).collect::<Result<_, _>>()? } else { vec![] };
    if aggregated && spec.select.iter().any(|s| s.agg.is_none() && s.column == "*") {
        return Err("`*` cannot be combined with an aggregate: choose the columns".into());
    }

    let mut sql = format!("SELECT {}{}\nFROM {}", if spec.distinct { "DISTINCT " } else { "" }, if select.is_empty() { "*".to_string() } else { select.join(", ") }, tref(&spec.from));
    let mut visible: Vec<&str> = vec![spec.from.alias.as_str()];
    for j in &spec.joins {
        if j.on.is_empty() {
            return Err(format!("join `{}` needs at least one matching column pair", j.table.alias));
        }
        let mut on = Vec::new();
        for (left, right) in &j.on {
            if !visible.contains(&left.alias.as_str()) {
                return Err(format!("join `{}` refers to `{}`, which comes later or does not exist", j.table.alias, left.alias));
            }
            on.push(format!("{} = {}", col(&left.alias, &left.column)?, col(&j.table.alias, right)?));
        }
        sql.push_str(&format!("\n{} JOIN {} ON {}", if j.kind == JoinKind::Left { "LEFT" } else { "INNER" }, tref(&j.table), on.join(" AND ")));
        visible.push(&j.table.alias);
    }

    let mut conds = Vec::new();
    for f in &spec.filters {
        let c = col(&f.alias, &f.column)?;
        let val = |s: &str| if f.numeric && looks_numeric(s) { s.trim().to_string() } else { d.quote_string(s) };
        let one = || f.values.first().map(String::as_str).ok_or_else(|| format!("the filter on `{}` needs a value", f.column));
        conds.push(match f.op {
            Op::IsNull => format!("{c} IS NULL"),
            Op::NotNull => format!("{c} IS NOT NULL"),
            Op::Eq => format!("{c} = {}", val(one()?)),
            Op::Ne => format!("{c} <> {}", val(one()?)),
            Op::Lt => format!("{c} < {}", val(one()?)),
            Op::Le => format!("{c} <= {}", val(one()?)),
            Op::Gt => format!("{c} > {}", val(one()?)),
            Op::Ge => format!("{c} >= {}", val(one()?)),
            Op::Contains => format!("{c} LIKE {}", d.quote_string(&format!("%{}%", like_escape(one()?)))),
            Op::StartsWith => format!("{c} LIKE {}", d.quote_string(&format!("{}%", like_escape(one()?)))),
            Op::EndsWith => format!("{c} LIKE {}", d.quote_string(&format!("%{}", like_escape(one()?)))),
            Op::Between => {
                if f.values.len() != 2 { return Err(format!("the filter on `{}` needs two values", f.column)); }
                format!("{c} BETWEEN {} AND {}", val(&f.values[0]), val(&f.values[1]))
            }
            Op::In => {
                let list: Vec<String> = f.values.iter().flat_map(|v| v.split(',')).map(str::trim).filter(|v| !v.is_empty()).map(|v| val(v)).collect();
                if list.is_empty() { return Err(format!("the filter on `{}` needs at least one value", f.column)); }
                format!("{c} IN ({})", list.join(", "))
            }
        });
    }
    if !conds.is_empty() {
        sql.push_str(&format!("\nWHERE {}", conds.join("\n  AND ")));
    }
    if !group.is_empty() {
        sql.push_str(&format!("\nGROUP BY {}", group.join(", ")));
    }
    if spec.distinct && !spec.select.is_empty() {
        // the server refuses `SELECT DISTINCT a ... ORDER BY b`: say so before sending it
        if let Some(o) = spec.order.iter().find(|o| !spec.select.iter().any(|s| s.alias == o.alias && (s.column == o.column || s.column == "*") && s.agg.is_none())) {
            return Err(format!("with DISTINCT you can only order by columns that are selected: add `{}` to the columns or turn DISTINCT off", o.column));
        }
    }
    if !spec.order.is_empty() {
        let o: Vec<String> = spec.order.iter().map(|o| Ok(format!("{}{}", col(&o.alias, &o.column)?, if o.desc { " DESC" } else { "" }))).collect::<Result<_, String>>()?;
        sql.push_str(&format!("\nORDER BY {}", o.join(", ")));
    }
    if let Some(n) = spec.limit {
        sql.push_str(&format!("\n{}", d.limit_offset(n, 0).replace(" OFFSET 0", "")));
    }
    sql.push(';');
    Ok(sql)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(db: &str, table: &str, alias: &str) -> TableRef {
        TableRef { db: db.into(), table: table.into(), alias: alias.into() }
    }
    fn sel(a: &str, c: &str) -> Select {
        Select { alias: a.into(), column: c.into(), name: None, agg: None }
    }
    fn spec() -> QuerySpec {
        QuerySpec { from: t("shop", "customers", "c"), joins: vec![], select: vec![], distinct: false, filters: vec![], order: vec![], limit: None }
    }
    fn f(a: &str, c: &str, op: Op, v: &[&str]) -> Filter {
        Filter { alias: a.into(), column: c.into(), op, values: v.iter().map(|x| x.to_string()).collect(), numeric: false }
    }

    #[test]
    fn a_bare_table_selects_everything() {
        assert_eq!(build(&spec()).unwrap(), "SELECT *\nFROM `shop`.`customers` AS `c`;");
    }

    #[test]
    fn columns_filters_order_and_limit() {
        let mut s = spec();
        s.select = vec![sel("c", "id"), Select { name: Some("who".into()), ..sel("c", "name") }];
        s.filters = vec![f("c", "name", Op::Contains, &["50%_off"]), Filter { numeric: true, ..f("c", "id", Op::Gt, &["10"]) }, f("c", "note", Op::IsNull, &[])];
        s.order = vec![Order { alias: "c".into(), column: "id".into(), desc: true }];
        s.limit = Some(100);
        assert_eq!(
            build(&s).unwrap(),
            "SELECT `c`.`id`, `c`.`name` AS `who`\nFROM `shop`.`customers` AS `c`\nWHERE `c`.`name` LIKE '%50\\\\%\\\\_off%'\n  AND `c`.`id` > 10\n  AND `c`.`note` IS NULL\nORDER BY `c`.`id` DESC\nLIMIT 100;"
        );
    }

    #[test]
    fn joins_follow_the_matching_columns_and_a_later_table_cannot_be_referenced_early() {
        let mut s = spec();
        s.joins = vec![
            Join { kind: JoinKind::Inner, table: t("shop", "orders", "o"), on: vec![(Side { alias: "c".into(), column: "id".into() }, "customer_id".into())] },
            Join { kind: JoinKind::Left, table: t("shop", "items", "i"), on: vec![(Side { alias: "o".into(), column: "id".into() }, "order_id".into())] },
        ];
        s.select = vec![sel("c", "name"), sel("o", "total"), sel("i", "sku")];
        let sql = build(&s).unwrap();
        assert!(sql.contains("INNER JOIN `shop`.`orders` AS `o` ON `c`.`id` = `o`.`customer_id`") && sql.contains("LEFT JOIN `shop`.`items` AS `i` ON `o`.`id` = `i`.`order_id`"), "{sql}");
        s.joins.swap(0, 1);
        assert!(build(&s).unwrap_err().contains("comes later"));
    }

    #[test]
    fn aggregates_group_by_the_plain_columns() {
        let mut s = spec();
        s.select = vec![sel("c", "country"), Select { agg: Some(Agg::Count), name: Some("n".into()), ..sel("c", "*") }, Select { agg: Some(Agg::Sum), ..sel("c", "balance") }];
        let sql = build(&s).unwrap();
        assert!(sql.starts_with("SELECT `c`.`country`, COUNT(*) AS `n`, SUM(`c`.`balance`)\n"), "{sql}");
        assert!(sql.contains("\nGROUP BY `c`.`country`"), "{sql}");
        s.select.push(sel("c", "*"));
        assert!(build(&s).is_err(), "`*` with an aggregate is refused");
    }

    #[test]
    fn values_are_quoted_and_names_are_checked() {
        let mut s = spec();
        s.filters = vec![f("c", "name", Op::Eq, &["O'Neil"]), f("c", "id", Op::In, &["1, 2,3"]), f("c", "age", Op::Between, &["18", "65"])];
        let sql = build(&s).unwrap();
        assert!(sql.contains("`c`.`name` = 'O''Neil'") && sql.contains("`c`.`id` IN ('1', '2', '3')") && sql.contains("`c`.`age` BETWEEN '18' AND '65'"), "{sql}");
        let mut bad = spec();
        bad.filters = vec![f("zz", "id", Op::Eq, &["1"])];
        assert!(build(&bad).unwrap_err().contains("not one of the tables"));
        bad.filters = vec![f("c", "id", Op::Between, &["1"])];
        assert!(build(&bad).is_err());
        bad.filters = vec![f("c", "id", Op::Eq, &[])];
        assert!(build(&bad).is_err());
        let mut dup = spec();
        dup.joins = vec![Join { kind: JoinKind::Inner, table: t("shop", "orders", "c"), on: vec![(Side { alias: "c".into(), column: "id".into() }, "x".into())] }];
        assert!(build(&dup).unwrap_err().contains("used by two tables"));
        let mut noon = spec();
        noon.joins = vec![Join { kind: JoinKind::Inner, table: t("shop", "orders", "o"), on: vec![] }];
        assert!(build(&noon).is_err());
    }

    #[test]
    fn hostile_names_stay_inside_their_quotes() {
        let mut s = spec();
        s.from = t("sh`op", "cust`omers", "c");
        s.select = vec![sel("c", "na`me")];
        assert_eq!(build(&s).unwrap(), "SELECT `c`.`na``me`\nFROM `sh``op`.`cust``omers` AS `c`;");
    }

    #[test]
    fn distinct_can_only_order_by_selected_columns() {
        let mut s = spec();
        s.distinct = true;
        s.select = vec![sel("c", "country")];
        s.order = vec![Order { alias: "c".into(), column: "id".into(), desc: false }];
        assert!(build(&s).unwrap_err().contains("only order by columns that are selected"));
        s.order[0].column = "country".into();
        assert!(build(&s).is_ok());
        s.distinct = false;
        s.order[0].column = "id".into();
        assert!(build(&s).is_ok(), "without DISTINCT any column may order the rows");
    }

    #[test]
    fn distinct_and_the_sqlite_dialect() {
        let mut s = spec();
        s.distinct = true;
        s.select = vec![sel("c", "country")];
        s.limit = Some(5);
        let sql = build_with(&s, &myq_core::Sqlite).unwrap();
        assert_eq!(sql, "SELECT DISTINCT \"c\".\"country\"\nFROM \"shop\".\"customers\" AS \"c\"\nLIMIT 5;");
    }
}
