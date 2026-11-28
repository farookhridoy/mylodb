//! Views, functions, procedures and triggers: their entries in the browser, the script that recreates one, and how to drop it.
//! A function is listed with its argument types (`area(numeric, numeric)`), because several may share a name; a trigger with its table
//! (`audit ON orders`), because its name is unique only there.

use crate::backend::{PgBackend, SYSTEM};
use crate::grid::q;
use myq_core::{BackendError, BackendResult, Kind, Node};

/// `name(argument types)` of the function `p`.
const SIGNATURE: &str = "p.proname::text || '(' || pg_catalog.pg_get_function_identity_arguments(p.oid) || ')'";

pub(crate) async fn nodes(b: &PgBackend, include_system: bool) -> BackendResult<Vec<Node>> {
    // functions that belong to an extension are the extension's business
    let routines = b
        .query(
            &format!("SELECT n.nspname::text, {SIGNATURE}, p.prokind = 'p' FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace WHERE p.prokind IN ('f','p') AND ($1 OR NOT {SYSTEM}) AND pg_catalog.has_schema_privilege(n.oid, 'USAGE') AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_depend d WHERE d.classid = 'pg_catalog.pg_proc'::regclass AND d.objid = p.oid AND d.deptype = 'e') ORDER BY 1, 2"),
            &[&include_system],
        )
        .await?;
    let mut out: Vec<Node> = routines.iter().map(|r| Node { database: r.get(0), kind: if r.get(2) { Kind::Procedure } else { Kind::Function }, name: r.get(1), parent: None }).collect();
    let triggers = b
        .query(
            &format!("SELECT n.nspname::text, t.tgname::text, c.relname::text FROM pg_catalog.pg_trigger t JOIN pg_catalog.pg_class c ON c.oid = t.tgrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE NOT t.tgisinternal AND ($1 OR NOT {SYSTEM}) AND pg_catalog.has_schema_privilege(n.oid, 'USAGE') ORDER BY 1, 2, 3"),
            &[&include_system],
        )
        .await?;
    out.extend(triggers.iter().map(|r| {
        let (name, table): (String, String) = (r.get(1), r.get(2));
        Node { database: r.get(0), kind: Kind::Trigger, name: format!("{name} ON {table}"), parent: Some(table) }
    }));
    Ok(out)
}

/// The trigger an entry stands for: (trigger, table). The whole label is matched on the server, because either name may itself
/// contain ` ON `.
async fn trigger(b: &PgBackend, schema: &str, label: &str) -> BackendResult<(u32, String, String)> {
    let rows = b
        .query("SELECT t.oid, t.tgname::text, c.relname::text FROM pg_catalog.pg_trigger t JOIN pg_catalog.pg_class c ON c.oid = t.tgrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1 AND t.tgname::text || ' ON ' || c.relname::text = $2 AND NOT t.tgisinternal", &[&schema, &label])
        .await?;
    match rows.as_slice() {
        [r] => Ok((r.get(0), r.get(1), r.get(2))),
        [] => Err(missing(schema, label)),
        _ => Err(BackendError(format!("two triggers read as `{label}`; change this one with SQL"))),
    }
}

fn missing(schema: &str, name: &str) -> BackendError {
    BackendError(format!("{schema}.{name} was not found (it may have been changed or dropped; press Refresh)"))
}

/// A script that puts the object back as it is now.
pub async fn object_script(b: &PgBackend, kind: Kind, schema: &str, name: &str) -> BackendResult<String> {
    match kind {
        Kind::Function | Kind::Procedure => {
            let rows = b
                .query(&format!("SELECT pg_catalog.pg_get_functiondef(p.oid) FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace WHERE n.nspname = $1 AND {SIGNATURE} = $2 AND p.prokind IN ('f','p')"), &[&schema, &name])
                .await?;
            let def: String = rows.first().ok_or_else(|| missing(schema, name))?.get(0);
            Ok(format!("{};\n", def.trim_end()))
        }
        Kind::Trigger => {
            let (oid, trigger, table) = trigger(b, schema, name).await?;
            let rows = b.query("SELECT pg_catalog.pg_get_triggerdef($1, true)", &[&oid]).await?;
            let def: String = rows.first().ok_or_else(|| missing(schema, name))?.get(0);
            Ok(format!("DROP TRIGGER IF EXISTS {} ON {}.{};\n{def};\n", q(&trigger), q(schema), q(&table)))
        }
        Kind::View => {
            let rows = b
                .query("SELECT c.relkind = 'm', pg_catalog.pg_get_viewdef(c.oid, true) FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('v','m')", &[&schema, &name])
                .await?;
            let row = rows.first().ok_or_else(|| missing(schema, name))?;
            let (materialized, body): (bool, String) = (row.get(0), row.get(1));
            let target = format!("{}.{}", q(schema), q(name));
            Ok(if materialized {
                format!("DROP MATERIALIZED VIEW IF EXISTS {target};\nCREATE MATERIALIZED VIEW {target} AS\n{}\n", body.trim_end())
            } else {
                format!("CREATE OR REPLACE VIEW {target} AS\n{}\n", body.trim_end())
            })
        }
        other => Err(BackendError(format!("there is no script for a {other:?} here"))),
    }
}

/// The statement that drops a view, function, procedure or trigger. Nothing is dropped along with it (no CASCADE).
pub async fn drop_object_sql(b: &PgBackend, kind: Kind, schema: &str, name: &str) -> BackendResult<String> {
    Ok(match kind {
        // only a name the server itself reports is pasted into the statement
        Kind::Function | Kind::Procedure => {
            let rows = b
                .query(&format!("SELECT p.proname::text, pg_catalog.pg_get_function_identity_arguments(p.oid), p.prokind = 'p' FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace WHERE n.nspname = $1 AND {SIGNATURE} = $2 AND p.prokind IN ('f','p')"), &[&schema, &name])
                .await?;
            let row = rows.first().ok_or_else(|| missing(schema, name))?;
            let (function, arguments, procedure): (String, String, bool) = (row.get(0), row.get(1), row.get(2));
            format!("DROP {} {}.{}({arguments})", if procedure { "PROCEDURE" } else { "FUNCTION" }, q(schema), q(&function))
        }
        Kind::Trigger => {
            let (_, trigger, table) = trigger(b, schema, name).await?;
            format!("DROP TRIGGER {} ON {}.{}", q(&trigger), q(schema), q(&table))
        }
        Kind::View => {
            let rows = b.query("SELECT c.relkind = 'm' FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('v','m')", &[&schema, &name]).await?;
            let materialized: bool = rows.first().ok_or_else(|| missing(schema, name))?.get(0);
            format!("DROP {}VIEW {}.{}", if materialized { "MATERIALIZED " } else { "" }, q(schema), q(name))
        }
        other => return Err(BackendError(format!("a {other:?} cannot be dropped here"))),
    })
}
