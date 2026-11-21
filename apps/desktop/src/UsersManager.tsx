import { useCallback, useEffect, useMemo, useState } from "react";
import Icon from "./Icon";
import { userApi, type Account, type AccountEdit, type Node, type Scope, type ScopeGrant } from "./api";
import { ConfirmDialog } from "./SidePanels";

const GLOBAL_PRIVS = ["SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "DROP", "RELOAD", "SHUTDOWN", "PROCESS", "FILE", "REFERENCES", "INDEX", "ALTER", "SHOW DATABASES", "SUPER", "CREATE TEMPORARY TABLES", "LOCK TABLES", "EXECUTE", "REPLICATION SLAVE", "REPLICATION CLIENT", "CREATE VIEW", "SHOW VIEW", "CREATE ROUTINE", "ALTER ROUTINE", "CREATE USER", "EVENT", "TRIGGER", "CREATE TABLESPACE"];
const DB_PRIVS = ["SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "DROP", "REFERENCES", "INDEX", "ALTER", "CREATE TEMPORARY TABLES", "LOCK TABLES", "EXECUTE", "CREATE VIEW", "SHOW VIEW", "CREATE ROUTINE", "ALTER ROUTINE", "EVENT", "TRIGGER"];
const TABLE_PRIVS = ["SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "DROP", "REFERENCES", "INDEX", "ALTER", "CREATE VIEW", "SHOW VIEW", "TRIGGER"];
const COLUMN_PRIVS = ["SELECT", "INSERT", "UPDATE", "REFERENCES"];

const scopeKey = (s: Scope) => (s.level === "global" ? "global" : s.level === "database" ? `db:${s.db}` : `tbl:${s.db}.${s.table}`);
const scopeLabel = (s: Scope) => (s.level === "global" ? "Global (*.*)" : s.level === "database" ? `Database ${s.db}` : `Table ${s.db}.${s.table}`);
const privsFor = (s: Scope) => (s.level === "global" ? GLOBAL_PRIVS : s.level === "database" ? DB_PRIVS : TABLE_PRIVS);

interface Draft { original: Account | null; edit: AccountEdit; scopes: ScopeGrant[]; unsupported: string[]; grantsLoaded: boolean }

const blank = (): Draft => ({
  original: null, grantsLoaded: true, unsupported: [], scopes: [],
  edit: { user: "", host: "localhost", password: "", plugin: null, locked: false, expire_password: false },
});

export default function UsersManager(props: { connectionId: number; nodes: Node[]; ensureColumns(db: string): Promise<unknown> }) {
  const cid = props.connectionId;
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [selScope, setSelScope] = useState<string>("");
  const [sql, setSql] = useState<string[]>([]);
  const [sqlError, setSqlError] = useState<string | null>(null);
  const [tab, setTab] = useState<"account" | "privileges" | "sql">("account");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [confirmDrop, setConfirmDrop] = useState(false);
  const [busy, setBusy] = useState(false);
  const [adding, setAdding] = useState<{ level: Scope["level"]; db: string; table: string } | null>(null);

  const dbs = useMemo(() => props.nodes.filter((n) => n.kind === "database").map((n) => n.name), [props.nodes]);
  const tablesOf = (d: string) => props.nodes.filter((n) => n.database === d && n.kind === "table").map((n) => n.name);
  const columnsOf = (d: string, t: string) => props.nodes.filter((n) => n.database === d && n.kind === "column" && n.parent === t).map((n) => n.name);

  const reload = useCallback(async () => {
    try { setAccounts(await userApi.list(cid)); setError(null); } catch (e) { setError(String(e)); }
  }, [cid]);
  useEffect(() => { reload(); }, [reload]);

  const choose = async (a: Account) => {
    setNotice(null);
    setTab("account");
    try {
      const g = await userApi.grants(cid, a.user, a.host);
      setDraft({
        original: a, grantsLoaded: true, unsupported: g.unsupported, scopes: structuredClone(g.scopes),
        edit: { user: a.user, host: a.host, password: "", plugin: null, locked: a.locked, expire_password: false },
      });
      setSelScope(g.scopes[0] ? scopeKey(g.scopes[0]) : "");
      setError(null);
    } catch (e) { setError(String(e)); }
  };

  // Preview the statements for the current edit (passwords are masked by the server side).
  useEffect(() => {
    if (!draft) return;
    let live = true;
    const t = setTimeout(() => {
      const edit = { ...draft.edit, password: draft.edit.password || null };
      userApi.preview(cid, draft.original, edit, draft.grantsLoaded ? draft.scopes : null)
        .then((s) => live && (setSql(s), setSqlError(null)))
        .catch((e) => live && (setSql([]), setSqlError(String(e))));
    }, 200);
    return () => { live = false; clearTimeout(t); };
  }, [draft, cid]);

  const setEdit = (p: Partial<AccountEdit>) => draft && setDraft({ ...draft, edit: { ...draft.edit, ...p } });
  const scope = draft?.scopes.find((s) => scopeKey(s) === selScope);
  // column privileges list the table's columns, which are loaded per database
  useEffect(() => { if (scope?.level === "table") props.ensureColumns(scope.db); }, [scope?.level, scope && "db" in scope ? scope.db : null]); // eslint-disable-line react-hooks/exhaustive-deps
  const updateScope = (patch: Partial<ScopeGrant>) =>
    draft && setDraft({ ...draft, scopes: draft.scopes.map((s) => (scopeKey(s) === selScope ? ({ ...s, ...patch } as ScopeGrant) : s)) });

  const togglePriv = (p: string) => {
    if (!scope) return;
    const has = scope.privileges.includes(p);
    updateScope({ privileges: has ? scope.privileges.filter((x) => x !== p) : [...scope.privileges, p] });
  };
  const toggleAll = () => {
    if (!scope) return;
    updateScope({ privileges: scope.privileges.includes("ALL PRIVILEGES") ? [] : ["ALL PRIVILEGES"] });
  };
  const toggleColumn = (priv: string, col: string) => {
    if (!scope) return;
    const cur = scope.columns[priv] ?? [];
    const next = cur.includes(col) ? cur.filter((c) => c !== col) : [...cur, col];
    const columns = { ...scope.columns, [priv]: next };
    if (next.length === 0) delete columns[priv];
    updateScope({ columns });
  };

  const addScope = () => {
    if (!draft || !adding) return;
    const s: Scope = adding.level === "global" ? { level: "global" } : adding.level === "database" ? { level: "database", db: adding.db } : { level: "table", db: adding.db, table: adding.table };
    if (!draft.scopes.some((x) => scopeKey(x) === scopeKey(s))) {
      setDraft({ ...draft, scopes: [...draft.scopes, { ...s, privileges: [], columns: {}, grant_option: false } as ScopeGrant] });
    }
    setSelScope(scopeKey(s));
    setAdding(null);
  };

  const apply = async () => {
    if (!draft) return;
    setBusy(true);
    try {
      const edit = { ...draft.edit, password: draft.edit.password || null };
      const n = await userApi.apply(cid, draft.original, edit, draft.grantsLoaded ? draft.scopes : null);
      setNotice(`Applied ${n} statement(s)`);
      setError(null);
      await reload();
      const fresh = (await userApi.list(cid)).find((a) => a.user === edit.user && a.host === edit.host);
      if (fresh) await choose(fresh);
    } catch (e) { setError(String(e)); }
    setBusy(false);
  };

  const canApply = !!draft && draft.edit.user.trim() !== "" && sql.length > 0 && !sqlError && !busy;

  return (
    <div className="users">
      <aside className="u-list">
        <div className="panel-bar">
          <button className="primary" onClick={() => { setDraft(blank()); setSelScope(""); setTab("account"); setNotice(null); }}>＋ New user</button>
          <button onClick={reload} title="Reload">⟳</button>
        </div>
        <ul className="list">
          {accounts.map((a) => (
            <li key={`${a.user}@${a.host}`} className={draft?.original && draft.original.user === a.user && draft.original.host === a.host ? "sel" : ""} onClick={() => choose(a)}>
              <span><b>{a.user || "(anonymous)"}</b><span className="dim">@{a.host}</span> {a.locked && <span title="Locked"><Icon name="lock" size={12} /></span>}{a.password_expired && <span title="Password expired"> <Icon name="clock" size={12} /></span>}</span>
            </li>
          ))}
        </ul>
      </aside>

      <section className="u-main">
        {error && <div className="banner bad">{error} <button className="mini" onClick={() => setError(null)}><Icon name="close" size={10} /></button></div>}
        {notice && <div className="banner ok">{notice}</div>}
        {!draft ? <div className="empty">Select a user, or create a new one</div> : (
          <>
            <div className="dz-head">
              <b>{draft.original ? `${draft.original.user}@${draft.original.host}` : "New user"}</b>
              <span className="grow" />
              {draft.original && <button className="danger" onClick={() => setConfirmDrop(true)}>Drop user</button>}
              <button className="primary" disabled={!canApply} onClick={apply}>{busy ? "Applying…" : draft.original ? "Apply changes" : "Create user"}</button>
            </div>
            <div className="dz-tabs">
              {(["account", "privileges", "sql"] as const).map((t) => (
                <button key={t} className={tab === t ? "active" : ""} onClick={() => setTab(t)}>{t === "sql" ? `SQL${sql.length ? " •" : ""}` : t[0].toUpperCase() + t.slice(1)}</button>
              ))}
            </div>
            <div className="dz-body">
              {tab === "account" && (
                <div className="dz-form">
                  <label>User name<input value={draft.edit.user} onChange={(e) => setEdit({ user: e.target.value })} /></label>
                  <label>Host<input value={draft.edit.host} onChange={(e) => setEdit({ host: e.target.value })} placeholder="localhost, 192.168.%, %" /></label>
                  <label>{draft.original ? "New password" : "Password"}
                    <input type="password" value={draft.edit.password ?? ""} placeholder={draft.original ? "(unchanged)" : ""} onChange={(e) => setEdit({ password: e.target.value })} /></label>
                  <label>Authentication plugin
                    <select value={draft.edit.plugin ?? ""} onChange={(e) => setEdit({ plugin: e.target.value || null })}>
                      <option value="">(server default)</option><option value="caching_sha2_password">caching_sha2_password</option><option value="mysql_native_password">mysql_native_password</option>
                    </select></label>
                  <label className="check"><input type="checkbox" checked={draft.edit.locked} onChange={(e) => setEdit({ locked: e.target.checked })} /> Account locked</label>
                  <label className="check"><input type="checkbox" checked={draft.edit.expire_password} onChange={(e) => setEdit({ expire_password: e.target.checked })} /> Expire password (must change at next login)</label>
                </div>
              )}

              {tab === "privileges" && (
                <div className="dz-split">
                  <div className="dz-list">
                    <div className="dz-actions">
                      <button onClick={() => setAdding({ level: "database", db: dbs[0] ?? "", table: "" })}>＋ Add scope</button>
                      <button disabled={!scope} onClick={() => { setDraft({ ...draft, scopes: draft.scopes.filter((s) => scopeKey(s) !== selScope) }); setSelScope(""); }}>－ Remove</button>
                    </div>
                    {draft.scopes.map((s) => (
                      <div key={scopeKey(s)} className={`dz-item ${scopeKey(s) === selScope ? "active" : ""}`} onClick={() => setSelScope(scopeKey(s))}>
                        {scopeLabel(s)}
                        <div className="dim small">{s.privileges.includes("ALL PRIVILEGES") ? "ALL PRIVILEGES" : `${s.privileges.length} privilege(s)${Object.keys(s.columns).length ? " + column grants" : ""}`}{s.grant_option ? " · grant option" : ""}</div>
                      </div>
                    ))}
                    {draft.scopes.length === 0 && <div className="empty">No privileges</div>}
                    {draft.unsupported.length > 0 && (
                      <details className="small"><summary className="dim">{draft.unsupported.length} grant(s) not editable here</summary>
                        {draft.unsupported.map((u) => <code key={u} className="preview">{u}</code>)}</details>
                    )}
                  </div>
                  <div className="dz-edit">
                    {adding && (
                      <div className="dz-form add-scope">
                        <label>Level
                          <select value={adding.level} onChange={(e) => setAdding({ ...adding, level: e.target.value as Scope["level"], table: "" })}>
                            <option value="global">Global</option><option value="database">Database</option><option value="table">Table</option>
                          </select></label>
                        {adding.level !== "global" && (
                          <label>Database<input list="u-dbs" value={adding.db} onChange={(e) => setAdding({ ...adding, db: e.target.value })} />
                            <datalist id="u-dbs">{dbs.map((d) => <option key={d} value={d} />)}</datalist></label>
                        )}
                        {adding.level === "table" && (
                          <label>Table<input list="u-tables" value={adding.table} onChange={(e) => setAdding({ ...adding, table: e.target.value })} />
                            <datalist id="u-tables">{tablesOf(adding.db).map((t) => <option key={t} value={t} />)}</datalist></label>
                        )}
                        <div className="actions"><button onClick={() => setAdding(null)}>Cancel</button>
                          <button className="primary" disabled={adding.level !== "global" && !adding.db || adding.level === "table" && !adding.table} onClick={addScope}>Add</button></div>
                      </div>
                    )}
                    {!adding && scope && (
                      <div className="privs">
                        <label className="check all"><input type="checkbox" checked={scope.privileges.includes("ALL PRIVILEGES")} onChange={toggleAll} /> <b>ALL PRIVILEGES</b></label>
                        <div className="priv-grid">
                          {privsFor(scope).map((p) => (
                            <label key={p} className="check"><input type="checkbox" disabled={scope.privileges.includes("ALL PRIVILEGES")} checked={scope.privileges.includes(p) || scope.privileges.includes("ALL PRIVILEGES")} onChange={() => togglePriv(p)} /> {p}</label>
                          ))}
                          {scope.privileges.filter((p) => !privsFor(scope).includes(p) && p !== "ALL PRIVILEGES").map((p) => (
                            <label key={p} className="check"><input type="checkbox" checked onChange={() => togglePriv(p)} /> {p}</label>
                          ))}
                        </div>
                        {scope.level !== "global" && (
                          <label className="check"><input type="checkbox" checked={scope.grant_option} onChange={(e) => updateScope({ grant_option: e.target.checked })} /> GRANT OPTION (may grant these privileges to others)</label>
                        )}
                        {scope.level === "table" && (
                          <div className="col-privs">
                            <div className="dim">Column privileges (limit a privilege to specific columns)</div>
                            {COLUMN_PRIVS.map((p) => (
                              <details key={p} open={(scope.columns[p] ?? []).length > 0}>
                                <summary>{p} {(scope.columns[p] ?? []).length > 0 && <span className="dim">({scope.columns[p].join(", ")})</span>}</summary>
                                <div className="priv-grid">
                                  {columnsOf(scope.db, scope.table).map((c) => (
                                    <label key={c} className="check"><input type="checkbox" checked={(scope.columns[p] ?? []).includes(c)} onChange={() => toggleColumn(p, c)} /> {c}</label>
                                  ))}
                                </div>
                              </details>
                            ))}
                          </div>
                        )}
                      </div>
                    )}
                    {!adding && !scope && <div className="empty">Select a scope, or add one</div>}
                  </div>
                </div>
              )}

              {tab === "sql" && (
                <div className="dz-sql">
                  {sqlError && <div className="error">{sqlError}</div>}
                  {sql.length === 0 && !sqlError ? <div className="empty">No changes</div> : (
                    <>
                      <code className="preview">{sql.join(";\n") + ";"}</code>
                      <p className="dim small">Passwords are never shown here.</p>
                    </>
                  )}
                </div>
              )}
            </div>
          </>
        )}
      </section>

      {confirmDrop && draft?.original && (
        <ConfirmDialog message={`Drop user ${draft.original.user}@${draft.original.host}? Their privileges are removed too.`} confirmLabel="Drop user"
          onCancel={() => setConfirmDrop(false)}
          onConfirm={async () => {
            setConfirmDrop(false);
            try { await userApi.drop(cid, draft.original!.user, draft.original!.host); setDraft(null); setNotice("User dropped"); await reload(); }
            catch (e) { setError(String(e)); }
          }} />
      )}
    </div>
  );
}
