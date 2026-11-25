try { if (!window.__tour) localStorage.setItem("mylodb.tourDone", "1"); } catch {}
// Fake Tauri bridge: in-memory tables + a call log, so the real React UI can be exercised in a browser.
window.__calls = [];
window.__vault = window.__vault || { mode: "keychain", unlocked: true, pw: "" };
window.__cbs = {}; window.__cbSeq = 0; window.__listeners = [];
window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
window.__emit = (event, payload) => window.__listeners.filter((l) => l.event === event).forEach((l) => window.__cbs[l.handler]({ event, id: l.id, payload }));
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
const colNodes = (db) => (db === "shop" ? Object.keys(meta).flatMap((t) => meta[t].columns.map(([name]) => ({ database: "shop", kind: "column", name, parent: t }))) : []);
/** The names-only index (what connect returns). `window.__bigServer = { dbs, tables }` adds a huge fake server. */
const nodes = () => {
  const out = [
    { database: "shop", kind: "database", name: "shop", parent: null },
    { database: "shop", kind: "table", name: "customers", parent: null },
    { database: "shop", kind: "table", name: "orders", parent: null },
    ...(window.__droppedObjects || new Set()).has("view:v_orders") ? [] : [{ database: "shop", kind: "view", name: "v_orders", parent: null }],
    { database: "shop", kind: "procedure", name: "p_total", parent: null },
    { database: "shop", kind: "function", name: "f_tax", parent: null },
    { database: "shop", kind: "trigger", name: "trg_orders_bi", parent: "orders" },
    { database: "shop", kind: "event", name: "ev_nightly", parent: null },
    ...(window.__bigRows ? [{ database: "shop", kind: "table", name: "events", parent: null }] : []),
  ];
  const big = window.__bigServer;
  if (big) {
    for (let d = 0; d < big.dbs; d++) {
      const db = "db" + String(d).padStart(4, "0");
      out.push({ database: db, kind: "database", name: db, parent: null });
      for (let t = 0; t < big.tables; t++) out.push({ database: db, kind: "table", name: "t_" + String(t).padStart(5, "0"), parent: null });
    }
    if (big.hugeDb) for (let t = 0; t < big.hugeDb; t++) out.push({ database: "db0000", kind: "table", name: "huge_" + t, parent: null });
  }
  const gone = window.__droppedObjects || new Set();
  return out.filter((n) => !gone.has(`${n.kind}:${n.name}`) && !(window.__droppedDbs || new Set()).has(n.database));
};
window.__loaded = new Set();
const currentIndex = () => [...nodes(), ...[...window.__loaded].flatMap(colNodes)];
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
  open_url: () => null,
  json_prepare: (a) => { window.__jsonPrepared = (window.__jsonPrepared || []).concat([a.path]); return { csv_path: "/tmp/mylodb-json/converted.csv", rows: 3, columns: ["id", "name", "score"], null_text: "\\N" }; },
  search_data: (a) => window.__search ?? { hits: [
    { table: "customers", rows: 2, columns: [["email", 2], ["note", 1]] },
    { table: "orders", rows: 5, columns: [["ref", 5]] },
  ], tables_total: 4, tables_scanned: 4, cancelled: false, skipped: ["blobs: no text column to search"] },
  index_advice: (a) => window.__advice ?? { findings: [
    { table: "orders", kind: "redundant", index: "idx_c", detail: "`idx_c` (customer_id) is the start of `idx_c_p` (customer_id, placed), which serves the same lookups", sql: "DROP INDEX `idx_c` ON `" + a.db + "`.`orders`" },
    { table: "orders", kind: "duplicate", index: "idx_s2", detail: "`idx_s2` has the same columns (status) as `idx_s1`", sql: "DROP INDEX `idx_s2` ON `" + a.db + "`.`orders`" },
    { table: "items", kind: "missing_foreign_key_index", index: "fk_order", detail: "foreign key `fk_order` on (order_id) has no index that starts with those columns", sql: "CREATE INDEX `idx_items_order_id` ON `" + a.db + "`.`items` (`order_id`)" },
  ], unused: [["orders", "idx_old"]], unused_note: "Unused means: no query has used the index since the server started.", tables: 3 },
  er_model: (a) => window.__er ?? { tables: [
    { name: "customers", columns: [{ name: "id", column_type: "bigint unsigned", primary: true, nullable: false }, { name: "name", column_type: "varchar(255)", primary: false, nullable: false }] },
    { name: "orders", columns: [{ name: "id", column_type: "bigint unsigned", primary: true, nullable: false }, { name: "customer_id", column_type: "bigint unsigned", primary: false, nullable: false }] },
    { name: "items", columns: [{ name: "id", column_type: "int", primary: true, nullable: false }, { name: "order_id", column_type: "bigint unsigned", primary: false, nullable: false }] },
    { name: "logs", columns: [{ name: "id", column_type: "int", primary: true, nullable: false }] },
  ], relations: [
    { name: "fk_orders_customer", table: "orders", columns: ["customer_id"], ref_schema: a.db, ref_table: "customers", ref_columns: ["id"], on_delete: "CASCADE", on_update: "RESTRICT" },
    { name: "fk_items_order", table: "items", columns: ["order_id"], ref_schema: a.db, ref_table: "orders", ref_columns: ["id"], on_delete: "RESTRICT", on_update: "RESTRICT" },
  ] },
  build_query: (a) => {
    const s = a.spec;
    if (window.__buildProblem) throw window.__buildProblem;
    const sel = s.select.length ? s.select.map((c) => `${c.agg ? c.agg.toUpperCase() + "(" : ""}${c.alias}.${c.column}${c.agg ? ")" : ""}${c.name ? " AS " + c.name : ""}`).join(", ") : "*";
    const joins = s.joins.map((j) => `\n${j.kind.toUpperCase()} JOIN ${j.table.table} AS ${j.table.alias} ON ${j.on.map(([l, r]) => `${l.alias}.${l.column} = ${j.table.alias}.${r}`).join(" AND ")}`).join("");
    const where = s.filters.length ? `\nWHERE ${s.filters.map((f) => `${f.alias}.${f.column} ${f.op} ${f.values.join(" / ")}`).join(" AND ")}` : "";
    return `SELECT ${sel}\nFROM ${s.from.table} AS ${s.from.alias}${joins}${where}${s.limit ? "\nLIMIT " + s.limit : ""};`;
  },
  schema_compare: (a) => window.__compare ?? { changes: [
    { table: "old_log", kind: "removed", statements: ["DROP TABLE `" + a.targetDb + "`.`old_log`"], deferred: [], destructive: true },
    { table: "posts", kind: "added", statements: ["CREATE TABLE `" + a.targetDb + "`.`posts` (\n  `id` int NOT NULL\n)"], deferred: ["ALTER TABLE `" + a.targetDb + "`.`posts` ADD CONSTRAINT `fk_user` FOREIGN KEY (`id`) REFERENCES `users` (`id`)"], destructive: false },
    { table: "users", kind: "changed", statements: ["ALTER TABLE `" + a.targetDb + "`.`users` ADD COLUMN `email` varchar(255) NULL AFTER `name`"], deferred: [], destructive: false },
  ], identical: 2, warnings: [] },
  capabilities: () => (window.__caps ?? { flavor: "mysql", schemas: { available: false, reason: "MySQL and MariaDB have databases, not schemas" }, routines: { available: true, reason: null }, triggers: { available: true, reason: null }, events: { available: true, reason: null }, users: { available: true, reason: null }, replication: { available: true, reason: null }, explain_analyze: { available: true, reason: null }, read_only_session: { available: true, reason: null }, row_estimates: { available: true, reason: null }, server_tools: { available: true, reason: null }, dump_restore: { available: true, reason: null }, writes: { available: true, reason: null } }),
  list_connections: () => [{ id: 1, name: "Local", color: "#4f8cff", host: "127.0.0.1", port: 3306, user: "root", database: null, ssl: "disabled" }],
  secrets_status: () => ({ mode: window.__vault.mode, unlocked: window.__vault.mode === "keychain" || window.__vault.unlocked }),
  secrets_unlock: (a) => { if (a.password !== window.__vault.pw) throw "vault_wrong_password: that master password is not right"; window.__vault.unlocked = true; return { mode: "file", unlocked: true }; },
  secrets_use_file: (a) => { if (a.password.length < 8) throw "The master password needs at least 8 characters."; Object.assign(window.__vault, { mode: "file", unlocked: true, pw: a.password }); return 2; },
  secrets_use_keychain: () => { if (!window.__vault.unlocked) throw "vault_locked: unlock the saved passwords with your master password"; Object.assign(window.__vault, { mode: "keychain", pw: "" }); return 2; },
  secrets_change_password: (a) => { if (a.old !== window.__vault.pw) throw "vault_wrong_password: that master password is not right"; window.__vault.pw = a.new; },
  secrets_reset: () => { Object.assign(window.__vault, { mode: "keychain", pw: "", unlocked: true }); },
  connect: (a) => {
    if (window.__vault.mode === "file" && !window.__vault.unlocked) throw { kind: "failed", message: "vault_locked: unlock the saved passwords with your master password" };
    window.__loaded = new Set(); window.__index = nodes();
    window.__connectTrust = (window.__connectTrust || []).concat([a.trustFingerprint]);
    if (window.__sshMode === "unknown" && !a.trustFingerprint) throw { kind: "unknown_host", host: "bastion:22", fingerprint: "SHA256:abc123" };
    if (window.__sshMode === "changed" && !window.__forgot) throw { kind: "host_key_changed", host: "bastion:22", expected: "SHA256:old", actual: "SHA256:new" };
    return window.__index;
  },
  forget_ssh_host: () => { window.__forgot = true; return null; },
  test_connection: (a) => {
    window.__tested = (window.__tested || []).concat([JSON.parse(JSON.stringify(a))]);
    if (a.connection.ssh && !a.trustFingerprint) throw { kind: "unknown_host", host: a.connection.ssh.host + ":" + a.connection.ssh.port, fingerprint: "SHA256:abc123" };
    return { version: "9.7.2", ssl_cipher: a.connection.ssl !== "disabled" ? "TLS_AES_256_GCM_SHA384" : null, via_ssh: !!a.connection.ssh };
  },
  refresh_index: () => { window.__refreshed = (window.__refreshed || 0) + 1; window.__index = currentIndex(); return window.__index; },
  open_connections: () => window.__openConnections || [],
  disconnect: () => null, // Other suites start on a query tab (a saved session); home.mjs sets __fresh to see a first connect.
  session_get: () => (window.__fresh ? null : JSON.stringify({ tabs: [{ id: "q1", title: "Query 1", sql: "", database: null }], active: "q1" })), session_set: () => null, history_list: () => [], favorites_list: () => [],
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

// ---- editor intelligence ----
Object.assign(handlers, {
  run_query: (a) => [{ sql: a.sql, sets: window.__runSets ?? [], affected_rows: 0, last_insert_id: null, warnings: 0, elapsed_ms: 1, error: null }],
  complete_sql: (a) => {
    const before = a.sql.slice(0, a.cursor);
    const word = /[\w$`]*$/.exec(before)[0];
    const from = a.cursor - word.length;
    const inFrom = /\bFROM\s+[\w$`]*$/i.test(before);
    const items = inFrom
      ? [{ label: "orders", kind: "table", detail: "shop", insert: "orders" }, { label: "order items", kind: "table", detail: "shop", insert: "`order items`" }, { label: "customers", kind: "table", detail: "shop", insert: "customers" }]
      : [{ label: "SELECT", kind: "keyword", detail: null, insert: "SELECT" }, { label: "COUNT", kind: "function", detail: "COUNT(expr)", insert: "COUNT(" }];
    return { from, to: a.cursor, items: items.filter((i) => i.label.toLowerCase().startsWith(word.toLowerCase())) };
  },
  diagnose_sql: () => window.__diag || [],
  ai_key_status: (a) => !!(window.__aiKeys || {})[a.preset],
  ai_set_key: (a) => { window.__aiKeys = window.__aiKeys || {}; if (a.key) window.__aiKeys[a.preset] = a.key; else delete window.__aiKeys[a.preset]; },
  ai_models: (a) => {
    if (a.preset !== "ollama" && !(window.__aiKeys || {})[a.preset]) throw "The service refused the key (401). Connect again in the AI settings.";
    if (window.__aiBadKey) throw "The service refused the key (401). Connect again in the AI settings.";
    return [{ id: "meta/free-big", name: "big", context: 131072, free: true }, { id: "meta/free-small", name: "small", context: 8192, free: true }, ...(a.all ? [{ id: "paid/model", name: "paid", context: 200000, free: false }] : [])]; // largest window first, as the backend sorts them
  },
  ai_chat: (a) => {
    if (window.__aiFail) throw window.__aiFail;
    const sql = "SELECT c.name, SUM(o.total) AS spent\nFROM customers c\nJOIN orders o ON o.customer_id = c.id\nGROUP BY c.name\nORDER BY spent DESC\nLIMIT 10;";
    return { text: window.__aiText || ("```sql\n" + sql + "\n```\nTop customers by spend."), sql, model: window.__aiAnswerModel || a.settings.model, note: window.__aiNote || null };
  },
  format_sql: (a) => a.sql.trim().replace(/\b(select|from|where)\b/gi, (m) => (a.options.keyword_case === "lower" ? m.toLowerCase() : m.toUpperCase())).replace(/\s+(FROM|WHERE|from|where)\b/g, (_, k) => "\n" + k).replace(/\n/g, a.options.indent.style === "tab" ? "\n\t" : "\n  "),
  statement_at: (a) => { const parts = a.sql.split(";"); let pos = 0; for (const p of parts) { if (a.cursor <= pos + p.length) return p.trim() || null; pos += p.length + 1; } return parts.at(-1).trim() || null; },
  explain_query: (a) => {
    if (a.sql.includes("BAD")) throw "Server error: syntax error near 'BAD'";
    return {
      plan: { columns: [{ name: "id", type_name: "x", binary: false }, { name: "table", type_name: "x", binary: false }, { name: "type", type_name: "x", binary: false }, { name: "rows", type_name: "x", binary: false }],
              rows: [["1", "orders", "ALL", "50000"]], total_rows: 1, truncated: false },
      json: JSON.stringify({ query_block: { select_id: 1, cost_info: { query_cost: "5100.25" }, table: { table_name: "orders", access_type: "ALL", rows_examined_per_scan: 50000 } } }),
      tree: a.analyze ? (window.__tree ?? "-> Table scan on orders  (cost=5100 rows=50000) (actual time=0.1..12 rows=50000 loops=1)") : null,
      hints: [{ severity: "warning", message: "Full table scan on `orders` (about 50000 rows).", table: "orders" }, { severity: "good", message: "`x` is answered from the index alone (covering index).", table: "x" }],
    };
  },
  profile_query: (a) => {
    if (a.sql.includes("DELETE")) throw "only SELECT statements can be profiled";
    return { elapsed_ms: 42, rows_returned: 3, stages: [{ name: "executing", ms: 30 }, { name: "starting", ms: 0.04 }], stages_note: null, counters: [{ name: "Innodb_rows_read", value: 20000 }, { name: "Select_scan", value: 1 }] };
  },
});

// ---- server tools ----
let procs = [
  { id: 11, user: "app", host: "10.0.0.5:5001", db: "shop", command: "Query", time: 12, state: "Sending data", info: "SELECT * FROM orders WHERE note LIKE '%x%'" },
  { id: 12, user: "app", host: "10.0.0.5:5002", db: "shop", command: "Sleep", time: 300, state: "", info: null },
  { id: 13, user: "report", host: "localhost", db: null, command: "Query", time: 1, state: "executing", info: "SELECT COUNT(*) FROM customers" },
  { id: 99, user: "root", host: "localhost", db: null, command: "Query", time: 0, state: "init", info: "SHOW FULL PROCESSLIST" },
];
let statusCalls = 0;
Object.assign(handlers, {
  server_info: () => ({ version: "9.7.2", version_comment: "MySQL Community Server", hostname: "mac", port: 3306, datadir: "/data/", charset: "utf8mb4", collation: "utf8mb4_0900_ai_ci", sql_mode: "STRICT_TRANS_TABLES", time_zone: "SYSTEM", max_connections: 151, uptime_seconds: 93784, ssl_cipher: null }),
  process_list: () => { window.__plCalls = (window.__plCalls || 0) + 1; return { processes: JSON.parse(JSON.stringify(procs)), self_id: 99 }; },
  sqlite_maintain: (a) => ({ integrity: "Integrity check: ok. The file is sound.", foreign_keys: "Foreign key check: no violations.", vacuum: "VACUUM done: 4.00 MB → 3.10 MB (0.90 MB freed)" }[a.op] ?? `${a.op} done.`),
  schedules_list: () => (window.__sched ?? []).map((s) => ({ ...s, next_run: s.enabled ? "2026-11-26 03:00:00" : null, running: false })),
  schedule_save: (a) => {
    const j = { ...a.job };
    if (!j.name.trim()) throw "Give the job a name.";
    if (!j.databases.length) throw "Choose at least one database.";
    window.__sched = window.__sched ?? [];
    const i = window.__sched.findIndex((s) => s.id && s.id === j.id);
    if (i >= 0) window.__sched[i] = { ...window.__sched[i], ...j }; else window.__sched.push({ ...j, id: "j" + (window.__sched.length + 1), created: "2026-11-25 09:00:00", last_run: null, log: [] });
    return window.__sched.map((s) => ({ ...s, next_run: s.enabled ? "2026-11-26 03:00:00" : null, running: false }));
  },
  schedule_delete: (a) => { window.__sched = (window.__sched ?? []).filter((s) => s.id !== a.id); return window.__sched; },
  schedule_run_now: (a) => { const s = window.__sched.find((x) => x.id === a.id); s.last_run = "2026-11-25 10:00:00"; s.log.unshift({ time: "2026-11-25 10:00:00", ok: true, message: "shop-20261125-100000.zip: 12 table(s), 3400 rows, 0.1 MB in 0.4 s; removed 1 old backup(s)" }); return s.log[0].message; },
  find_usages: (a) => {
    const all = [
      { kind: "view", db: "shop", name: "v_big_orders", detail: "select `o`.`total` from `shop`.`orders` o where `o`.`total` > 100" },
      { kind: "trigger", db: "shop", name: "trg_orders_ai", detail: "on orders: INSERT INTO orders_log SELECT * FROM orders" },
      { kind: "foreign key (points here)", db: "shop", name: "order_items.fk_items_order", detail: "order_items.order_id → orders.id" },
    ];
    return a.column ? all.filter((u) => u.detail.includes(a.column)) : all;
  },
  top_statements: (a) => {
    if (window.__noPerfSchema) throw "the Performance Schema statement summary is not available (performance_schema is OFF). Turn it on with performance_schema=ON in the server settings and restart the server";
    const rows = [
      { db: "shop", digest_text: "SELECT * FROM `orders` WHERE `customer_id` = ?", calls: 9000, total_ms: 45000, avg_ms: 5, max_ms: 120, rows_examined: 9000000, rows_sent: 9000, no_index: 9000, last_seen: "2026-11-25 10:00:00" },
      { db: "shop", digest_text: "UPDATE `stock` SET `qty` = `qty` - ? WHERE `id` = ?", calls: 120000, total_ms: 9000, avg_ms: 0.075, max_ms: 4, rows_examined: 120000, rows_sent: 0, no_index: 0, last_seen: "2026-11-25 10:00:01" },
    ].filter((r) => !a.search || r.digest_text.includes(a.search));
    return a.sort === "calls" ? rows.slice().sort((x, y) => y.calls - x.calls) : rows;
  },
  kill_process: (a) => { window.__kills = (window.__kills || []).concat([{ id: a.processId, kind: a.kind }]); if (a.kind === "connection") procs = procs.filter((p) => p.id !== a.processId); return null; },
  server_variables: (a) => a.scope === "global"
    ? [{ name: "max_connections", value: "151", source: "COMPILED" }, { name: "sort_buffer_size", value: "262144", source: "DYNAMIC" }, { name: "version", value: "9.7.2", source: "COMPILED" }]
    : [{ name: "max_connections", value: "151", source: null }],
  server_status: () => { statusCalls++; return [["Uptime", String(1000 + statusCalls * 2)], ["Questions", String(1000 + statusCalls * 200)], ["Threads_connected", "7"], ["Threads_running", "2"], ["Slow_queries", "3"], ["Aborted_connects", "1"], ["Bytes_received", String(statusCalls * 4000)], ["Bytes_sent", String(statusCalls * 9000)], ["Innodb_buffer_pool_reads", "10"], ["Innodb_buffer_pool_read_requests", "1000"], ["Max_used_connections", "20"], ...(window.__statusExtra ? [["Com_select", String(statusCalls * 500)], ["Com_insert", String(statusCalls * 50)]] : [])]; },
  set_global_variable: (a) => { window.__setVars = (window.__setVars || []).concat([{ name: a.name, value: a.value }]); return a.value === "bad" ? ["Warning 1292: Truncated incorrect max_connections value: 'bad'"] : []; },
  maintain_tables: (a) => { window.__maint = (window.__maint || []).concat([JSON.parse(JSON.stringify(a))]); return a.tables.flatMap((t) => [{ table: `${a.db}.${t}`, op: a.operation.op, msg_type: "status", msg_text: "OK" }]).concat(a.operation.op === "repair" ? [{ table: `${a.db}.${a.tables[0]}`, op: "repair", msg_type: "note", msg_text: "The storage engine for the table doesn't support repair" }, { table: `${a.db}.ghost`, op: "repair", msg_type: "Error", msg_text: "Table doesn't exist" }] : []); },
  log_settings: () => ({ general_log: false, slow_query_log: false, log_output: "FILE", long_query_time: "10.000000", log_queries_not_using_indexes: false, log_error: "stderr", slow_query_log_file: "/data/slow.log", general_log_file: "/data/general.log", log_bin: true, error_log_in_table: true }),
  error_log: (a) => { window.__errLogArgs = JSON.parse(JSON.stringify(a)); return [{ time: "2026-11-20 10:00:00", level: "Error", source: "Server", text: "Disk almost full" }, { time: "2026-11-20 09:00:00", level: "Warning", source: "InnoDB", text: "Cannot allocate memory" }].filter((l) => a.levels.length === 0 || a.levels.includes(l.level)); },
  slow_log: () => { throw "slow queries are not written to a table (log_output does not include TABLE); they go to the file named by `slow_query_log_file`"; },
  binary_logs: () => [{ name: "binlog.000001", size: 1500 }, { name: "binlog.000002", size: 3 * 1024 * 1024 }],
  binlog_events: (a) => { window.__binlogArgs = a; return [{ pos: 4, event_type: "Format_desc", end_pos: 126, info: "Server ver: 9.7.2" }, { pos: 126, event_type: "Query", end_pos: 200, info: "BEGIN" }]; },
});

// ---- backup, restore, import, export ----
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
/** A job that reports progress and then either finishes, or stops when the UI cancels it. */
async function job(jobId, phase, message, ms = 700) {
  window.__cancelFlag = false;
  window.__emit("job-progress", { job_id: jobId, phase, done: 50, total: 100, message });
  const until = Date.now() + ms;
  while (Date.now() < until) { if (window.__cancelFlag) throw "cancelled"; await wait(25); }
}
Object.assign(handlers, {
  csv_sniff: () => ";",
  csv_preview: (a) => { window.__previews = (window.__previews || []).concat([JSON.parse(JSON.stringify(a))]); return { headers: ["id", "name", "qty"], rows: [["1", "Ann", "5"], ["2", "Bob", ""]], column_count: 3, more: false }; },
  csv_infer: () => [{ name: "id", column_type: "int" }, { name: "name", column_type: "varchar(255)" }, { name: "qty", column_type: "int" }],
  csv_create_table: (a) => { window.__created = JSON.parse(JSON.stringify(a)); return currentIndex(); },
  import_csv_job: async (a) => { window.__import = JSON.parse(JSON.stringify(a)); await job(a.jobId, "Importing", "1 rows", window.__jobMs ?? 600);
    return { rows_read: 2, affected_rows: 2, errors: window.__importErrors ?? [], ragged_rows: 0, aborted: false, rolled_back: false, warnings: ["the table uses the MyISAM engine, which has no transactions"], elapsed_ms: 12 }; },
  run_sql_source_job: async (a) => { window.__run = JSON.parse(JSON.stringify(a)); await job(a.jobId, "Running script", "10 statements", 300);
    return { files: 1, statements: 10, failed: 2, aborted: false, elapsed_ms: 40, errors: [{ file: "dump.sql", line: 5, statement: "INSERT INTO t VALUES (1,'dup')", message: "Duplicate entry '1' for key 't.PRIMARY' (1062)" }, { file: "dump.sql", line: 8, statement: "SELECT * FROM missing", message: "Table 'x.missing' doesn't exist (1146)" }] }; },
  backup_job: async (a) => { window.__backup = JSON.parse(JSON.stringify(a)); await job(a.jobId, "Backing up", "shop.orders", window.__jobMs ?? 700);
    return { path: a.dir + "/" + a.name + "-20261120-101500.sql", files: 1, databases: a.options.databases.length, tables: 2, rows: 1234, bytes: 5 * 1024 * 1024, warnings: ["shop.legacy uses the MyISAM engine, so its rows are not part of the consistent snapshot"], elapsed_ms: 2500 }; },
  cancel_job: (a) => { window.__cancelled = a.jobId; window.__cancelFlag = true; return null; },
  export_query: (a) => { window.__exportQuery = JSON.parse(JSON.stringify(a)); if (/delete/i.test(a.sql)) throw "only SELECT statements can be exported"; return { rows: 42, warnings: [] }; },
  export_table: (a) => { window.__exportTable = JSON.parse(JSON.stringify(a)); return { rows: 3, warnings: ["1 cell(s) longer than 32767 characters were cut to Excel's limit"] }; },
});

// ---- scoped filter and lazy columns (mirror the backend's behaviour) ----
Object.assign(handlers, {
  load_columns: (a) => {
    window.__loadCalls = (window.__loadCalls || []).concat([a.db]);
    const start = window.__index.length;
    if (window.__loaded.has(a.db)) return { start, nodes: [] };
    const cols = colNodes(a.db);
    window.__loaded.add(a.db);
    window.__index = window.__index.concat(cols);
    return { start, nodes: cols };
  },
  filter_index: (a) => {
    window.__filterCalls = (window.__filterCalls || []).concat([{ query: a.query, scope: a.scope }]);
    window.__filterKinds = (window.__filterKinds || []).concat([a.kinds]);
    const terms = a.query.split(/\s+/).filter((t) => t && !t.includes(":") && !/[*?/]/.test(t)).map((t) => t.toLowerCase());
    const colTerm = /(^|\s)col:(\S+)/i.exec(a.query)?.[2]?.toLowerCase();
    const hits = [];
    window.__index.forEach((n, i) => {
      if (a.scope) {
        if (n.database !== a.scope || n.kind === "database") return;
        if (colTerm) { if (n.kind === "column" && n.name.toLowerCase().includes(colTerm)) hits.push(i); return; }
        if ((a.kinds ? a.kinds.includes(n.kind) : n.kind !== "column") && terms.every((t) => n.name.toLowerCase().includes(t))) hits.push(i);
      } else if (n.kind === "database" && terms.every((t) => n.name.toLowerCase().includes(t))) hits.push(i);
    });
    return hits;
  },
});
window.__TAURI_INTERNALS__ = {
  transformCallback: (cb) => { const id = ++window.__cbSeq; window.__cbs[id] = cb; return id; },
  invoke: async (cmd, args) => {
    window.__calls.push({ cmd, args: JSON.parse(JSON.stringify(args ?? {})) });
    if (cmd === "plugin:event|listen") { const id = window.__listeners.length + 1; window.__listeners.push({ event: args.event, handler: args.handler, id }); return id; }
    if (cmd === "plugin:event|unlisten") { window.__listeners = window.__listeners.filter((l) => l.id !== args.eventId); return null; }
    if (cmd === "plugin:dialog|open") { const v = window.__nextOpen ?? null; window.__nextOpen = null; return v; }
    if (cmd === "plugin:dialog|save") { const v = window.__nextSave ?? null; window.__nextSave = null; return v; }
    const h = handlers[cmd];
    if (!h) throw "mock: unhandled command " + cmd;
    return h(args ?? {});
  },
};

// ---- table sizes and column types for the object browser ----
const SIZES = { orders: 114688, customers: 5452595 };               // 112 KB and 5.2 MB
const typeOf = ([name, d]) => (name === "id" ? "bigint unsigned" : d === "varchar" ? "varchar(255)" : d === "decimal" ? "decimal(10,2)" : d === "text" ? "text" : d);
Object.assign(handlers, {
  db_table_stats: (a) => {
    window.__statCalls = (window.__statCalls || []).concat([a.db]);
    return nodes().filter((n) => n.database === a.db && n.kind === "table")
      .map((n) => ({ name: n.name, bytes: a.db === "shop" ? SIZES[n.name] ?? 16384 : 16384, rows: a.db === "shop" ? 3 : 0 }));
  },
  table_column_details: (a) => {
    window.__detailCalls = (window.__detailCalls || []).concat([`${a.db}.${a.table}`]);
    const m = meta[a.table];
    if (a.db === "shop" && m) {
      return m.columns.map((c) => ({ name: c[0], column_type: typeOf(c), nullable: !!c[3], key: m.pk.includes(c[0]) ? "PRI" : "", extra: c[2] ? "auto_increment" : "" }));
    }
    return [{ name: "id", column_type: "bigint unsigned", nullable: false, key: "PRI", extra: "auto_increment" }, { name: "code", column_type: "char(255)", nullable: true, key: "", extra: "" }];
  },
});

// ---- objects and indexes ----
const DEFS = {
  "view:v_orders": "CREATE ALGORITHM=UNDEFINED DEFINER=`root`@`localhost` SQL SECURITY DEFINER VIEW `v_orders` AS select `orders`.`id` AS `id` from `orders`",
  "procedure:p_total": "CREATE DEFINER=`root`@`localhost` PROCEDURE `p_total`()\nBEGIN SELECT 1; SELECT 2; END",
  "function:f_tax": "CREATE DEFINER=`root`@`localhost` FUNCTION `f_tax`(a INT) RETURNS int DETERMINISTIC RETURN a * 2",
  "trigger:trg_orders_bi": "CREATE TRIGGER `trg_orders_bi` BEFORE INSERT ON `orders` FOR EACH ROW SET NEW.note = 'x'",
  "event:ev_nightly": "CREATE EVENT `ev_nightly` ON SCHEDULE EVERY 1 DAY DO SELECT 1",
};
Object.assign(handlers, {
  table_index_details: (a) => {
    window.__indexCalls = (window.__indexCalls || []).concat([`${a.db}.${a.table}`]);
    return a.table === "orders"
      ? [{ name: "PRIMARY", unique: true, kind: "BTREE", columns: ["id"] }, { name: "idx_customer_total", unique: false, kind: "BTREE", columns: ["customer_id", "total"] }]
      : [{ name: "PRIMARY", unique: true, kind: "BTREE", columns: ["id"] }];
  },
  object_definition: (a) => {
    window.__defCalls = (window.__defCalls || []).concat([`${a.kind}:${a.name}`]);
    const d = a.kind === "table" && meta[a.name] ? `CREATE TABLE \`${a.name}\` (\n  \`id\` bigint unsigned NOT NULL AUTO_INCREMENT,\n  PRIMARY KEY (\`id\`)\n) ENGINE=InnoDB` : DEFS[`${a.kind}:${a.name}`];
    if (!d) throw "no such object";
    return d;
  },
  drop_object: (a) => {
    window.__droppedObjects = window.__droppedObjects || new Set();
    window.__droppedObjects.add(`${a.kind}:${a.name}`);
    window.__dropCalls = (window.__dropCalls || []).concat([`${a.kind}:${a.db}.${a.name}`]);
    return `DROP ${a.kind.toUpperCase()} \`${a.db}\`.\`${a.name}\``;
  },
});

// ---- table overview (Properties and Diagram views) ----
const fkRel = { name: "fk1", schema: "shop", table: "orders", columns: ["customer_id"], ref_schema: "shop", ref_table: "customers", ref_columns: ["id"], on_update: "RESTRICT", on_delete: "CASCADE" };
Object.assign(handlers, {
  table_overview: (a) => {
    window.__overviewCalls = (window.__overviewCalls || []).concat([`${a.db}.${a.table}`]);
    const m = meta[a.table];
    if (a.db !== "shop" || !m) return { info: { is_view: false, engine: null, collation: null, charset: null, auto_increment: null, comment: "", partitioned: false, row_format: null, rows: null, avg_row_length: null, data_length: null, index_length: null, data_free: null, created: null, updated: null }, columns: [], indexes: [], constraints: [], outgoing: [], incoming: [], triggers: [], partitions: [], ddl: null };
    const isOrders = a.table === "orders";
    return {
      info: { is_view: false, engine: "InnoDB", collation: "utf8mb4_unicode_ci", charset: "utf8mb4", auto_increment: 4, comment: isOrders ? "customer orders" : "", partitioned: false, row_format: "Dynamic", rows: 3, avg_row_length: 5461, data_length: 16384, index_length: 32768, data_free: 0, created: "2026-01-02 10:00:00", updated: null },
      columns: m.columns.map((c, i) => ({ name: c[0], ordinal: i + 1, column_type: typeOf(c), nullable: !!c[3], key: m.pk.includes(c[0]) ? "PRI" : c[0] === "customer_id" ? "MUL" : "", default: null, extra: c[2] ? "auto_increment" : "", comment: "" })),
      indexes: [{ name: "PRIMARY", unique: true, kind: "BTREE", columns: ["id"] }, ...(isOrders ? [{ name: "idx_customer_total", unique: false, kind: "BTREE", columns: ["customer_id", "total"] }] : [])],
      constraints: [{ name: "PRIMARY", kind: "PRIMARY KEY", columns: ["id"] }, ...(isOrders ? [{ name: "fk1", kind: "FOREIGN KEY", columns: ["customer_id"] }] : [])],
      outgoing: isOrders ? [fkRel] : [],
      incoming: isOrders ? [] : [fkRel],
      triggers: isOrders ? [{ name: "trg_orders_bi", timing: "BEFORE", event: "INSERT", statement: "SET NEW.note = 'x'" }] : [],
      partitions: [],
      ddl: `CREATE TABLE \`${a.table}\` (\n  \`id\` bigint unsigned NOT NULL AUTO_INCREMENT,\n  PRIMARY KEY (\`id\`)\n) ENGINE=InnoDB`,
    };
  },
});

// ---- connections with safety settings, review and read-only refusal ----
{
  const base = handlers.list_connections;
  handlers.list_connections = () => [...base(), ...(window.__extraConns || [])];
  const connect = handlers.connect;
  handlers.connect = (a) => { window.__activeConnId = a.id; return connect(a); };
  const activeConn = () => handlers.list_connections().find((c) => c.id === window.__activeConnId) || {};
  const run = handlers.run_query;
  handlers.run_query = (a) => {
    if (activeConn().read_only && /^\s*(insert|update|delete|drop|truncate|create|alter)\b/i.test(a.sql)) {
      throw `Not run: this connection is read-only, and statement 1 (${a.sql.slice(0, 60)}) it changes data. Edit the connection to turn read-only off.`;
    }
    return run(a);
  };
  handlers.review_sql = (a) => {
    const c = activeConn();
    const items = [];
    a.sql.split(";").map((x) => x.trim()).filter(Boolean).forEach((sql, index) => {
      const findings = [];
      if (/^drop\s+(table|database)\b/i.test(sql)) findings.push({ kind: "drop_object", severity: "danger", reason: "DROP TABLE deletes the table and all its rows" });
      if (/^truncate\b/i.test(sql)) findings.push({ kind: "truncate", severity: "danger", reason: "TRUNCATE deletes every row and cannot be rolled back" });
      if (/^delete\s+from\b/i.test(sql) && !/\bwhere\b/i.test(sql)) findings.push({ kind: "delete_without_where", severity: "danger", reason: "DELETE without a WHERE condition deletes every row" });
      if (/^update\b/i.test(sql) && !/\bwhere\b/i.test(sql)) findings.push({ kind: "update_without_where", severity: "danger", reason: "UPDATE without a WHERE condition changes every row" });
      if (/^rename\b/i.test(sql)) findings.push({ kind: "rename", severity: "warning", reason: "renaming breaks anything that still uses the old name" });
      if (findings.length) items.push({ index, sql, findings, estimated_rows: /^(delete|update)\b/i.test(sql) ? 5 : null });
    });
    return { items, environment: c.environment || "development", read_only: !!c.read_only, confirm: c.confirm_destructive !== false };
  };
}

// history of what ran in this page (most recent first)
handlers.history_list = () => window.__calls.filter((c) => c.cmd === "run_query").map((c, i) => ({ id: i + 1, sql: c.args.sql, ran_at: "2026-01-02 10:00:00", duration_ms: 1, ok: true })).reverse();


// ---- large tables: `window.__bigRows = N` adds a table `events` with N rows; paging by OFFSET or by keyset cursor ----
if (window.__bigRows) {
  meta.events = { columns: [["id", "int", 1, 0], ["kind", "varchar", 0, 0], ["note", "varchar", 0, 1]], pk: ["id"], fks: [] };
  data.events = Array.from({ length: window.__bigRows }, (_, i) => [String(i + 1), ["click", "view", "buy", "share"][i % 4], "event " + (i + 1)]);
}
Object.assign(handlers, {
  table_page: (a) => {
    window.__pageSpecs = (window.__pageSpecs || []).concat([JSON.parse(JSON.stringify(a.spec))]);
    const rows = filtered(a.table, a.spec), lim = a.spec.limit, c = a.spec.cursor;
    const keyset = a.spec.sort.length === 0 && meta[a.table].pk.length === 1;
    let page, hasMore, hasPrev;
    if (c && c.after) { const rest = rows.filter((r) => Number(r[0]) > Number(c.after[0])); page = rest.slice(0, lim); hasMore = rest.length > lim; hasPrev = true; }
    else if (c && c.before) { const rest = rows.filter((r) => Number(r[0]) < Number(c.before[0])); page = rest.slice(-lim); hasPrev = rest.length > lim; hasMore = true; }
    else if (c === "end") { page = rows.slice(-lim); hasPrev = rows.length > lim; hasMore = false; }
    else { page = rows.slice(a.spec.offset, a.spec.offset + lim); hasMore = a.spec.offset + lim < rows.length; hasPrev = a.spec.offset > 0; }
    return { columns: colsOf(a.table), rows: page, long_cells: window.__longCells ?? [], has_more: hasMore, has_prev: hasPrev, keyset,
             first_key: keyset && page.length ? [page[0][0]] : null, last_key: keyset && page.length ? [page.at(-1)[0]] : null, elapsed_ms: 1, sql: `SELECT * FROM shop.${a.table}` };
  },
  table_count_estimate: (a) => { window.__estimateCalls = (window.__estimateCalls || 0) + 1; return window.__estimate !== undefined ? window.__estimate : filtered(a.table, { filters: a.filters, sort: [] }).length; },
  table_count: async (a) => {
    window.__countCalls = (window.__countCalls || []).concat([a.runId]);
    if (window.__slowCount) { while (!(window.__cancelledRuns || []).includes(a.runId)) await wait(20); throw "Query execution was interrupted"; }
    return filtered(a.table, { filters: a.filters, sort: [] }).length;
  },
  export_table: async (a) => {
    window.__exportTable = JSON.parse(JSON.stringify(a));
    await job(a.jobId, "export", "120,000 rows written", window.__jobMs ?? 400);
    return { rows: window.__bigRows ?? 3, warnings: window.__bigRows ? [] : ["1 cell(s) longer than 32767 characters were cut to Excel's limit"] };
  },
});
{
  const cancel = handlers.cancel_query;
  handlers.cancel_query = (a) => { window.__cancelledRuns = (window.__cancelledRuns || []).concat([a.runId]); return cancel ? cancel(a) : null; };
}


// ---- plans: the SQL of bulk / destructive operations (mirrors myq_schema::plan), and running it ----
{
  const qn = (x) => "`" + x.replace(/`/g, "``") + "`";
  const t = (db, n) => `${qn(db)}.${qn(n)}`;
  handlers.build_plan = (a) => {
    const o = a.op;
    window.__plans = (window.__plans || []).concat([JSON.parse(JSON.stringify(o))]);
    if (window.__planRefuse) throw window.__planRefuse;
    switch (o.op) {
      case "drop_tables": {
        const tb = o.objects.filter((x) => !x[1]).map((x) => t(o.db, x[0])).join(", "), vw = o.objects.filter((x) => x[1]).map((x) => t(o.db, x[0])).join(", ");
        return [tb && `DROP TABLE ${tb};`, vw && `DROP VIEW ${vw};`].filter(Boolean);
      }
      case "empty_tables": return o.objects.map((x) => `TRUNCATE TABLE ${t(o.db, x[0])};`);
      case "copy_tables": return o.copies.flatMap(([f, to]) => [`CREATE TABLE ${t(o.db, to)} LIKE ${t(o.db, f)};`, ...(o.with_data ? [`INSERT INTO ${t(o.db, to)} SELECT * FROM ${t(o.db, f)};`] : [])]);
      case "rename_table": return [`RENAME TABLE ${t(o.db, o.from)} TO ${t(o.db, o.to)};`];
      case "rename_routine": return [o.definition.replace(`${o.kind.toUpperCase()} ${qn(o.from)}`, `${o.kind.toUpperCase()} ${t(o.db, o.to)}`) + ";", `DROP ${o.kind.toUpperCase()} ${t(o.db, o.from)};`];
      case "drop_object": return [`DROP ${o.kind.toUpperCase()} ${t(o.db, o.name)};`];
      case "create_database": return [`CREATE DATABASE ${qn(o.name)}${o.collation ? " COLLATE " + o.collation : ""};`];
      case "drop_databases": return o.names.map((n) => `DROP DATABASE ${qn(n)};`);
      case "add_prefix": return [`RENAME TABLE ${o.tables.map((x) => `${t(o.db, x)} TO ${t(o.db, o.prefix + x)}`).join(", ")};`];
      case "replace_prefix": { const ps = o.tables.filter((x) => x.startsWith(o.old)); if (!ps.length) throw `none of the chosen tables starts with \`${o.old}\``; return [`RENAME TABLE ${ps.map((x) => `${t(o.db, x)} TO ${t(o.db, o.new + x.slice(o.old.length))}`).join(", ")};`]; }
      case "copy_with_prefix": return o.tables.flatMap((x) => [`CREATE TABLE ${t(o.db, o.prefix + x)} LIKE ${t(o.db, x)};`, ...(o.with_data ? [`INSERT INTO ${t(o.db, o.prefix + x)} SELECT * FROM ${t(o.db, x)};`] : [])]);
      case "checksum": return [`CHECKSUM TABLE ${o.tables.map((x) => t(o.db, x)).join(", ")};`];
      case "alter_database": return [`ALTER DATABASE ${qn(o.name)}${o.charset ? " CHARACTER SET " + o.charset : ""}${o.collation ? " COLLATE " + o.collation : ""};`];
      case "copy_database": return [`CREATE DATABASE ${qn(o.dst)};`, `CREATE TABLE ${t(o.dst, "customers")} LIKE ${t(o.src, "customers")};`, `CREATE TABLE ${t(o.dst, "orders")} LIKE ${t(o.src, "orders")};`];
      case "rename_database": return [`CREATE DATABASE ${qn(o.new)};`, `RENAME TABLE ${t(o.old, "customers")} TO ${t(o.new, "customers")}, ${t(o.old, "orders")} TO ${t(o.new, "orders")};`, `DROP DATABASE ${qn(o.old)};`];
      default: throw "mock: unsupported plan " + o.op;
    }
  };
  handlers.run_plan = (a) => {
    window.__runPlans = (window.__runPlans || []).concat([a.statements.slice()]);
    const out = [];
    for (const sql of a.statements) {
      const err = window.__planError && /DROP|TRUNCATE|RENAME/i.test(sql) ? window.__planError : null;
      out.push({ sql, sets: [], affected_rows: 0, last_insert_id: null, warnings: 0, elapsed_ms: 1, error: err });
      if (err) break;
      const m = /^DROP (TABLE|VIEW|PROCEDURE|FUNCTION|TRIGGER|EVENT) (.+);$/.exec(sql);
      if (m) for (const part of m[2].split(", ")) { const name = part.split(".").pop().replace(/`/g, ""); window.__droppedObjects = (window.__droppedObjects || new Set()).add(`${m[1].toLowerCase()}:${name}`); }
      const d = /^DROP DATABASE `(.+)`;$/.exec(sql);
      if (d) window.__droppedDbs = (window.__droppedDbs || new Set()).add(d[1]);
    }
    return out;
  };
}

handlers.database_list = () => [
  { name: "information_schema", collation: "utf8mb3_general_ci", charset: "utf8mb3", system: true },
  { name: "mysql", collation: "utf8mb4_0900_ai_ci", charset: "utf8mb4", system: true },
  { name: "shop", collation: "utf8mb4_0900_ai_ci", charset: "utf8mb4", system: false },
  { name: "blog", collation: "utf8mb4_bin", charset: "utf8mb4", system: false },
  { name: "old_2019", collation: "latin1_swedish_ci", charset: "latin1", system: false },
].filter((d) => !(window.__droppedDbs || new Set()).has(d.name));
handlers.database_sizes = () => [["shop", 5, 3145728], ["blog", 2, 16384], ["old_2019", 12, 52428800], ["mysql", 40, 9000000]];
handlers.database_tables = (a) => a.db !== "shop" ? [] : [
  { name: "customers", is_view: false, engine: "InnoDB", rows: 1200, collation: "utf8mb4_0900_ai_ci", bytes: 114688, overhead: 0, comment: "people" },
  { name: "orders", is_view: false, engine: "InnoDB", rows: 5400, collation: "utf8mb4_0900_ai_ci", bytes: 1048576, overhead: 4194304, comment: "" },
  { name: "v_orders", is_view: true, engine: null, rows: null, collation: null, bytes: null, overhead: null, comment: "" },
].filter((t) => !(window.__droppedObjects || new Set()).has(`${t.is_view ? "view" : "table"}:${t.name}`));
handlers.server_home = () => ({
  info: { version: "9.7.2", version_comment: "MySQL Community Server", hostname: "mac", port: 3306, datadir: "/data", charset: "utf8mb4", collation: "utf8mb4_0900_ai_ci", sql_mode: "", time_zone: "SYSTEM", max_connections: 151, uptime_seconds: 93784, ssl_cipher: "TLS_AES_256_GCM_SHA384" },
  account: "root@localhost", protocol: 10, connections: 4, connection_collation: "utf8mb4_0900_ai_ci", databases: 3,
});
handlers.preview_sql_source = (a) => {
  window.__previewed = a.path;
  return { files: 1, statements: 12, bytes: 4096, kinds: [["INSERT", 8], ["CREATE TABLE", 2], ["DROP TABLE", 2]], destructive: 2, errors: [],
    first: [{ file: "dump.sql", line: 1, text: "DROP TABLE IF EXISTS `a`" }, { file: "dump.sql", line: 2, text: "CREATE TABLE `a` (id INT)" }] };
};
handlers.server_catalog = () => ({
  engines: { columns: ["Engine", "Support", "Comment"], rows: [["InnoDB", "DEFAULT", "Supports transactions"], ["MyISAM", "YES", "MyISAM storage engine"]] },
  charsets: { columns: ["Charset", "Description", "Default collation", "Maxlen"], rows: [["utf8mb4", "UTF-8 Unicode", "utf8mb4_0900_ai_ci", "4"], ["latin1", "cp1252 West European", "latin1_swedish_ci", "1"]] },
  replication: { columns: [], rows: [] }, replication_note: null,
});
handlers.read_text_file = () => window.__fileText ?? "SELECT 'from file';";
handlers.write_text_file = (a) => { window.__written = a; return null; };
handlers.save_connection = (a) => { window.__extraConns = (window.__extraConns || []).concat([{ ...a.connection, id: 100 + (window.__extraConns || []).length }]); return 100 + window.__extraConns.length; };

// ---- in-app updates: window.__update = { version, date, body } offers one; the calls and their order are in window.__updateLog ----
window.__TAURI_INTERNALS__.unregisterCallback = () => {};
window.__updateLog = [];
Object.assign(handlers, {
  "plugin:updater|check": () => (window.__update ? { rid: 77, currentVersion: "0.3.0", rawJson: {}, ...window.__update } : null),
  "plugin:updater|download": async (a) => {
    window.__updateLog.push("download");
    const send = (m) => a.onEvent.onmessage(m);
    send({ event: "Started", data: { contentLength: 4194304 } });
    send({ event: "Progress", data: { chunkLength: 1048576 } });
    await new Promise((r) => setTimeout(r, window.__downloadMs ?? 400));
    send({ event: "Progress", data: { chunkLength: 3145728 } });
    send({ event: "Finished" });
    return 78;
  },
  "plugin:updater|install": () => { window.__updateLog.push("install"); return null; },
  "plugin:process|restart": () => { window.__updateLog.push("restart"); return null; },
  "plugin:resources|close": () => null,
});

// ---- long cells: window.__longCells = [[row, col]] marks cells whose shown text is cut; window.__fullText is what fetch_cell returns ----
Object.assign(handlers, {
  fetch_cell: (a) => {
    window.__cellFetches = (window.__cellFetches || []).concat([JSON.parse(JSON.stringify(a))]);
    const t = window.__fullText ?? "";
    return { is_null: false, len: t.length, truncated: false, base64: btoa(unescape(encodeURIComponent(t))) };
  },
});
window.__meta = meta; window.__data = data; // for suites that add columns
