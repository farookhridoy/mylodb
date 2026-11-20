// Fake Tauri bridge: in-memory tables + a call log, so the real React UI can be exercised in a browser.
window.__calls = [];
const meta = {
  customers: { columns: [["id","int",1,0],["name","varchar",0,0],["email","varchar",0,1]], pk: ["id"], fks: [] },
  orders: { columns: [["id","int",1,0],["customer_id","int",0,0],["total","decimal",0,0],["note","text",0,1]], pk: ["id"],
            fks: [{ name: "fk1", columns: ["customer_id"], ref_schema: "shop", ref_table: "customers", ref_columns: ["id"] }] },
};
const data = {
  customers: [["1","Ann","ann@x.io"],["2","Bob",null],["3","Cy","cy@z.io"]],
  orders: [["1","1","10.50","first"],["2","1","99.99",null],["3","2","5.00","third x"]],
};
const metaOf = (t) => ({
  database: "shop", table: t, is_view: false, primary_key: meta[t].pk, row_key: meta[t].pk, read_only_reason: null, foreign_keys: meta[t].fks,
  columns: meta[t].columns.map(([name, data_type, ai, nullable]) => ({ name, data_type, column_type: data_type, nullable: !!nullable, key: meta[t].pk.includes(name) ? "PRI" : "", default: null, extra: ai ? "auto_increment" : "", comment: "", binary: false, generated: false, auto_increment: !!ai })),
});
const colsOf = (t) => meta[t].columns.map(([name, d]) => ({ name, type_name: d === "int" ? "MYSQL_TYPE_LONG" : "MYSQL_TYPE_VAR_STRING", binary: false }));
function filtered(t, spec) {
  let rows = data[t].map((r) => r.slice());
  const names = meta[t].columns.map((c) => c[0]);
  for (const f of spec.filters) {
    const i = names.indexOf(f.column);
    rows = rows.filter((r) => {
      const v = r[i], a = f.values[0];
      switch (f.op) {
        case "contains": return v !== null && v.toLowerCase().includes(a.toLowerCase());
        case "eq": return v === a; case "ne": return v !== a;
        case "gt": return v !== null && parseFloat(v) > parseFloat(a);
        case "is_null": return v === null; case "not_null": return v !== null;
        default: return true;
      }
    });
  }
  for (const s of [...spec.sort].reverse()) {
    const i = names.indexOf(s.column);
    rows.sort((x, y) => (parseFloat(x[i]) - parseFloat(y[i])) * (s.desc ? -1 : 1) || String(x[i]).localeCompare(String(y[i])) * (s.desc ? -1 : 1));
  }
  return rows;
}
const nodes = () => [
  { database: "shop", kind: "database", name: "shop", parent: null },
  { database: "shop", kind: "table", name: "customers", parent: null },
  { database: "shop", kind: "table", name: "orders", parent: null },
  ...Object.keys(meta).flatMap((t) => meta[t].columns.map(([name]) => ({ database: "shop", kind: "column", name, parent: t }))),
];
const colDef = ([name, d, ai, nullable]) => ({
  name, original_name: name, data_type: d === "int" ? "int" : d, params: d === "varchar" ? "40" : d === "decimal" ? "10,2" : null, unsigned: false, zerofill: false,
  nullable: !!nullable, default: { kind: "none" }, auto_increment: !!ai, charset: null, collation: null, comment: "", on_update: null, generated: null, invisible: false,
});
const defs = {};
const defOf = (t) => defs[t] ?? (defs[t] = {
  name: t, engine: "InnoDB", charset: "utf8mb4", collation: "utf8mb4_0900_ai_ci", comment: "",
  columns: meta[t].columns.map(colDef),
  indexes: [{ name: "PRIMARY", original_name: "PRIMARY", kind: "primary", columns: [{ name: "id", length: null, desc: false }] }],
  foreign_keys: meta[t].fks.map((f) => ({ ...f, original_name: f.name, on_delete: "restrict", on_update: "restrict" })),
});
const accounts = [
  { user: "app", host: "%", locked: false, password_expired: false, plugin: "caching_sha2_password" },
  { user: "report", host: "localhost", locked: true, password_expired: false, plugin: "caching_sha2_password" },
];
const grants = { app: { scopes: [
  { level: "database", db: "shop", privileges: ["SELECT", "UPDATE"], columns: {}, grant_option: false },
  { level: "table", db: "shop", table: "orders", privileges: [], columns: { SELECT: ["id"] }, grant_option: false },
], unsupported: ["GRANT `role1`@`%` TO `app`@`%`"] }, report: { scopes: [], unsupported: [] } };
const handlers = {
  server_options: () => ({ engines: ["InnoDB", "MyISAM"], charsets: [["utf8mb4", ["utf8mb4_0900_ai_ci", "utf8mb4_bin"], "utf8mb4_0900_ai_ci"], ["latin1", ["latin1_swedish_ci"], "latin1_swedish_ci"]] }),
  table_def: (a) => ({ def: JSON.parse(JSON.stringify(defOf(a.table))), warnings: [] }),
  ddl_preview: (a) => {
    const errors = a.desired.name.trim() ? [] : ["table name is empty"];
    const changed = JSON.stringify(a.original) !== JSON.stringify(a.desired);
    return { statements: changed && !errors.length ? [`-- ${a.original ? "ALTER" : "CREATE"} ${a.desired.name}`] : [], errors };
  },
  ddl_apply: (a) => {
    window.__applied = JSON.parse(JSON.stringify(a));
    const d = JSON.parse(JSON.stringify(a.desired));
    d.columns.forEach((c) => (c.original_name = c.name)); d.indexes.forEach((i) => (i.original_name = i.name)); d.foreign_keys.forEach((f) => (f.original_name = f.name));
    defs[d.name] = d; meta[d.name] = meta[d.name] ?? { columns: [], pk: [], fks: [] };
    return ["-- applied"];
  },
  table_op: (a) => { window.__tableOps = [...(window.__tableOps || []), { table: a.table, operation: a.operation }]; return []; },
  database_op: (a) => { window.__dbOps = [...(window.__dbOps || []), a.operation]; return "-- ok"; },
  accounts_list: () => accounts,
  account_grants: (a) => JSON.parse(JSON.stringify(grants[a.user] ?? { scopes: [], unsupported: [] })),
  account_preview: (a) => { window.__lastPreview = JSON.parse(JSON.stringify(a)); return [`-- ${a.original ? "ALTER" : "CREATE"} ${a.edit.user}`]; },
  account_apply: (a) => { window.__accountApplied = JSON.parse(JSON.stringify(a)); return 2; },
  account_drop: (a) => { window.__dropped = a; return null; },
  list_connections: () => [{ id: 1, name: "Local", color: "#4f8cff", host: "127.0.0.1", port: 3306, user: "root", database: null, ssl: "disabled" }],
  connect: () => nodes(),
  refresh_index: () => { window.__refreshed = (window.__refreshed || 0) + 1; return nodes(); },
  disconnect: () => null, session_get: () => null, session_set: () => null, history_list: () => [], favorites_list: () => [],
  list_databases: () => ["shop"], filter_index: () => [],
  table_meta: (a) => metaOf(a.table),
  table_page: (a) => {
    const rows = filtered(a.table, a.spec);
    const page = rows.slice(a.spec.offset, a.spec.offset + a.spec.limit);
    return { columns: colsOf(a.table), rows: page, long_cells: [], has_more: a.spec.offset + a.spec.limit < rows.length, elapsed_ms: 1, sql: `SELECT * FROM shop.${a.table}` };
  },
  table_count: (a) => filtered(a.table, { filters: a.filters, sort: [] }).length,
  preview_changes: (a) => a.changes.map((c) => `-- ${c.op}`),
  apply_changes: (a) => {
    for (const c of a.changes) {
      const names = meta[a.table].columns.map((x) => x[0]);
      if (c.op === "update") {
        const id = c.key[0].value.value; const row = data[a.table].find((r) => r[0] === id);
        for (const s of c.set) row[names.indexOf(s.column)] = s.value.kind === "null" ? null : s.value.value;
      } else if (c.op === "delete") data[a.table] = data[a.table].filter((r) => r[0] !== c.key[0].value.value);
      else if (c.op === "insert") { const row = names.map(() => null); row[0] = String(data[a.table].length + 10); for (const v of c.values) row[names.indexOf(v.column)] = v.value.kind === "text" ? v.value.value : null; data[a.table].push(row); }
    }
    return { applied: a.changes.length, insert_ids: [] };
  },
};
window.__TAURI_INTERNALS__ = {
  transformCallback: () => 0,
  invoke: async (cmd, args) => {
    window.__calls.push({ cmd, args: JSON.parse(JSON.stringify(args ?? {})) });
    const h = handlers[cmd];
    if (!h) throw "mock: unhandled command " + cmd;
    return h(args ?? {});
  },
};
