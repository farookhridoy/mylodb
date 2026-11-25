import type { SavedConnection } from "./api";
import type { MySnippet } from "./mySnippets";

/**
 * Share connections and snippets as one file (team sharing without a server). Passwords, SSH passwords and key passphrases are
 * never written: whoever imports the file types them once. What does travel: names, hosts, users, colours, environments and snippets.
 */
export interface ShareFile { app: "MyLoDB"; kind: "share"; version: 1; connections: SavedConnection[]; snippets: MySnippet[] }

const identity = (c: Pick<SavedConnection, "engine" | "file" | "host" | "port" | "user" | "database">) =>
  [c.engine ?? "mysql", c.engine === "sqlite" ? c.file ?? "" : `${c.host}:${c.port}`, c.user, c.database ?? ""].join("|").toLowerCase();

export function buildShare(connections: SavedConnection[], snippets: MySnippet[]): string {
  const clean = connections.map((c) => {
    const { id: _id, new_password: _pw, ...rest } = c;
    void _id; void _pw;
    // a password is asked for again; an SSH password or key passphrase is not in the list anyway
    return { id: null, ...rest } as SavedConnection;
  });
  const file: ShareFile = { app: "MyLoDB", kind: "share", version: 1, connections: clean, snippets };
  return JSON.stringify(file, null, 2) + "\n";
}

export function parseShare(text: string): ShareFile {
  let v: unknown;
  try { v = JSON.parse(text); } catch { throw new Error("This is not a MyLoDB share file (it is not valid JSON)."); }
  const f = v as Partial<ShareFile> | null;
  if (!f || f.app !== "MyLoDB" || f.kind !== "share") throw new Error("This is not a MyLoDB share file.");
  if (f.version !== 1) throw new Error(`This file was made by a newer MyLoDB (format ${String(f.version)}). Update MyLoDB to read it.`);
  const connections = (Array.isArray(f.connections) ? f.connections : []).filter((c): c is SavedConnection => typeof c?.name === "string" && typeof c?.host === "string" && typeof c?.user === "string");
  const snippets = (Array.isArray(f.snippets) ? f.snippets : []).filter((s): s is MySnippet => typeof s?.name === "string" && typeof s?.sql === "string" && s.name.trim() !== "");
  return { app: "MyLoDB", kind: "share", version: 1, connections, snippets };
}

export interface ImportPlan { addConnections: SavedConnection[]; skippedConnections: number; addSnippets: MySnippet[]; skippedSnippets: number }

/** What importing would add: a connection that already exists (same server, user and database) or a snippet with the same text is skipped; a name clash gets "(imported)". */
export function planImport(file: ShareFile, haveConnections: SavedConnection[], haveSnippets: MySnippet[]): ImportPlan {
  const ids = new Set(haveConnections.map(identity));
  const names = new Set(haveConnections.map((c) => c.name.toLowerCase()));
  const addConnections: SavedConnection[] = [];
  let skippedConnections = 0;
  for (const c of file.connections) {
    if (ids.has(identity(c))) { skippedConnections++; continue; }
    let name = c.name;
    while (names.has(name.toLowerCase())) name = `${name} (imported)`;
    names.add(name.toLowerCase());
    ids.add(identity(c));
    addConnections.push({ ...c, id: null, name, new_password: null });
  }
  const sn = new Map(haveSnippets.map((s) => [s.name.toLowerCase(), s.sql]));
  const addSnippets: MySnippet[] = [];
  let skippedSnippets = 0;
  for (const s of file.snippets) {
    const have = sn.get(s.name.toLowerCase());
    if (have === s.sql) { skippedSnippets++; continue; }
    let name = s.name;
    while (sn.has(name.toLowerCase())) name = `${name} (imported)`;
    sn.set(name.toLowerCase(), s.sql);
    addSnippets.push({ name, sql: s.sql });
  }
  return { addConnections, skippedConnections, addSnippets, skippedSnippets };
}
