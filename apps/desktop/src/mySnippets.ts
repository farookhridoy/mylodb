/** The user's own snippet library, kept in localStorage; the built-in starting points stay in snippets.ts. */
export interface MySnippet { name: string; sql: string }
const KEY = "mylodb.snippets";

export function loadMySnippets(): MySnippet[] {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    return Array.isArray(raw) ? raw.filter((s): s is MySnippet => typeof s?.name === "string" && typeof s?.sql === "string" && s.name.trim() !== "") : [];
  } catch { return []; }
}

/** Add a snippet, or replace the one with the same name (case-insensitive). Returns the new list. */
export function saveMySnippet(list: MySnippet[], name: string, sql: string): MySnippet[] {
  const n = name.trim();
  const next = [...list.filter((s) => s.name.toLowerCase() !== n.toLowerCase()), { name: n, sql }].sort((a, b) => a.name.localeCompare(b.name));
  localStorage.setItem(KEY, JSON.stringify(next));
  return next;
}

export function deleteMySnippet(list: MySnippet[], name: string): MySnippet[] {
  const next = list.filter((s) => s.name !== name);
  localStorage.setItem(KEY, JSON.stringify(next));
  return next;
}
