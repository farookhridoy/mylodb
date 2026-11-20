import type { Kind, Node } from "./api";

export const GROUPS: { kind: Kind; label: string }[] = [
  { kind: "table", label: "Tables" },
  { kind: "view", label: "Views" },
  { kind: "procedure", label: "Stored Procs" },
  { kind: "function", label: "Functions" },
  { kind: "trigger", label: "Triggers" },
  { kind: "event", label: "Events" },
];

export interface GroupNode { kind: Kind; label: string; items: Node[] }
export interface DbNode { name: string; groups: GroupNode[]; columns: Map<string, Node[]> }

/** Build the tree from the flat index, keeping only the entries in `visible` (index positions). */
export function buildTree(index: Node[], visible: Set<number> | null): DbNode[] {
  const dbs = new Map<string, DbNode>();
  const db = (name: string) => {
    let d = dbs.get(name);
    if (!d) {
      d = { name, groups: GROUPS.map((g) => ({ ...g, items: [] })), columns: new Map() };
      dbs.set(name, d);
    }
    return d;
  };
  index.forEach((n, i) => {
    if (visible && !visible.has(i)) return;
    const d = db(n.database);
    if (n.kind === "database") return;
    if (n.kind === "column") {
      const key = n.parent ?? "";
      d.columns.set(key, [...(d.columns.get(key) ?? []), n]);
      return;
    }
    d.groups.find((g) => g.kind === n.kind)?.items.push(n);
  });
  return [...dbs.values()];
}
