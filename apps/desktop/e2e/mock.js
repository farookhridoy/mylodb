// Fake Tauri bridge: in-memory tables + a call log, so the real React UI can be exercised in a browser.
window.__calls = [];
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
  return out;
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
  list_connections: () => [{ id: 1, name: "Local", color: "#4f8cff", host: "127.0.0.1", port: 3306, user: "root", database: null, ssl: "disabled" }],
  connect: (a) => {
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

// ---- editor intelligence ----
Object.assign(handlers, {
  run_query: (a) => [{ sql: a.sql, sets: [], affected_rows: 0, last_insert_id: null, warnings: 0, elapsed_ms: 1, error: null }],
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
  format_sql: (a) => a.sql.trim().replace(/\b(select|from|where)\b/gi, (m) => (a.options.keyword_case === "lower" ? m.toLowerCase() : m.toUpperCase())).replace(/\s+(FROM|WHERE|from|where)\b/g, (_, k) => "\n" + k).replace(/\n/g, a.options.indent.style === "tab" ? "\n\t" : "\n  "),
  statement_at: (a) => { const parts = a.sql.split(";"); let pos = 0; for (const p of parts) { if (a.cursor <= pos + p.length) return p.trim() || null; pos += p.length + 1; } return parts.at(-1).trim() || null; },
  explain_query: (a) => {
    if (a.sql.includes("BAD")) throw "Server error: syntax error near 'BAD'";
    return {
      plan: { columns: [{ name: "id", type_name: "x", binary: false }, { name: "table", type_name: "x", binary: false }, { name: "type", type_name: "x", binary: false }, { name: "rows", type_name: "x", binary: false }],
              rows: [["1", "orders", "ALL", "50000"]], total_rows: 1, truncated: false },
      json: JSON.stringify({ query_block: { select_id: 1, cost_info: { query_cost: "5100.25" }, table: { table_name: "orders", access_type: "ALL", rows_examined_per_scan: 50000 } } }),
      tree: a.analyze ? "-> Table scan on orders  (cost=5100 rows=50000) (actual time=0.1..12 rows=50000 loops=1)" : null,
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
  kill_process: (a) => { window.__kills = (window.__kills || []).concat([{ id: a.processId, kind: a.kind }]); if (a.kind === "connection") procs = procs.filter((p) => p.id !== a.processId); return null; },
  server_variables: (a) => a.scope === "global"
    ? [{ name: "max_connections", value: "151", source: "COMPILED" }, { name: "sort_buffer_size", value: "262144", source: "DYNAMIC" }, { name: "version", value: "9.7.2", source: "COMPILED" }]
    : [{ name: "max_connections", value: "151", source: null }],
  server_status: () => { statusCalls++; return [["Uptime", String(1000 + statusCalls * 2)], ["Questions", String(1000 + statusCalls * 200)], ["Threads_connected", "7"], ["Threads_running", "2"], ["Slow_queries", "3"], ["Aborted_connects", "1"], ["Bytes_received", String(statusCalls * 4000)], ["Bytes_sent", String(statusCalls * 9000)], ["Innodb_buffer_pool_reads", "10"], ["Innodb_buffer_pool_read_requests", "1000"], ["Max_used_connections", "20"]]; },
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
    const terms = a.query.split(/\s+/).filter((t) => t && !t.includes(":") && !/[*?/]/.test(t)).map((t) => t.toLowerCase());
    const colTerm = /(^|\s)col:(\S+)/i.exec(a.query)?.[2]?.toLowerCase();
    const hits = [];
    window.__index.forEach((n, i) => {
      if (a.scope) {
        if (n.database !== a.scope || n.kind === "database") return;
        if (colTerm) { if (n.kind === "column" && n.name.toLowerCase().includes(colTerm)) hits.push(i); return; }
        if (n.kind !== "column" && terms.every((t) => n.name.toLowerCase().includes(t))) hits.push(i);
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
