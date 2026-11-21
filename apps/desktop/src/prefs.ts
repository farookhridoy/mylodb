/** Appearance preferences: font family, size and weight for each area, kept in localStorage and applied as CSS variables. */
export type Area = "ui" | "tree" | "grid" | "editor";
export interface FontPref { family: string; size: number; bold: boolean }
export type EditorScheme = "default" | "vivid" | "solarized" | "contrast";
export const SCHEMES: { id: EditorScheme; label: string }[] = [
  { id: "default", label: "Default" }, { id: "vivid", label: "Multi-colour (vivid)" }, { id: "solarized", label: "Solarized" }, { id: "contrast", label: "High contrast" },
];
export type Prefs = Record<Area, FontPref> & { editorLineHeight: number; editorScheme: EditorScheme };

const SANS = '-apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif';
const MONO = '"SF Mono", Menlo, Monaco, monospace';
export const DEFAULTS: Prefs = {
  ui: { family: SANS, size: 12, bold: false },
  tree: { family: "", size: 12, bold: false },       // "" = same family as the interface
  grid: { family: MONO, size: 12, bold: false },
  editor: { family: MONO, size: 13, bold: false },
  editorLineHeight: 1.5,
  editorScheme: "default",
};
export const SIZE_RANGE: Record<Area, [number, number]> = { ui: [10, 18], tree: [10, 18], grid: [9, 16], editor: [10, 24] };
export const AREA_LABEL: Record<Area, string> = { ui: "Interface", tree: "Sidebar (object tree)", grid: "Data grid", editor: "Query editor" };
export const FAMILIES: { label: string; value: string }[] = [
  { label: "System (San Francisco)", value: SANS },
  { label: "Helvetica Neue", value: '"Helvetica Neue", Helvetica, Arial, sans-serif' },
  { label: "SF Mono / Menlo", value: MONO },
  { label: "Menlo", value: "Menlo, monospace" },
  { label: "Monaco", value: "Monaco, monospace" },
  { label: "Courier New", value: '"Courier New", monospace' },
  { label: "Georgia (serif)", value: "Georgia, serif" },
];
const KEY = "mylodb.prefs";

const clamp = (n: unknown, [lo, hi]: [number, number], dflt: number) => (typeof n === "number" && isFinite(n) ? Math.min(hi, Math.max(lo, Math.round(n))) : dflt);

/** Read the saved preferences; anything missing or out of range falls back to the default. */
export function loadPrefs(): Prefs {
  let raw: Partial<Prefs> = {};
  try { raw = JSON.parse(localStorage.getItem(KEY) ?? "{}") ?? {}; } catch { /* a damaged entry means defaults */ }
  const area = (a: Area): FontPref => {
    const r = (raw[a] ?? {}) as Partial<FontPref>;
    return { family: typeof r.family === "string" ? r.family : DEFAULTS[a].family, size: clamp(r.size, SIZE_RANGE[a], DEFAULTS[a].size), bold: r.bold === true };
  };
  const lh = typeof raw.editorLineHeight === "number" && raw.editorLineHeight >= 1 && raw.editorLineHeight <= 2.2 ? raw.editorLineHeight : DEFAULTS.editorLineHeight;
  const scheme = SCHEMES.some((x) => x.id === raw.editorScheme) ? (raw.editorScheme as EditorScheme) : "default";
  return { ui: area("ui"), tree: area("tree"), grid: area("grid"), editor: area("editor"), editorLineHeight: lh, editorScheme: scheme };
}

export function savePrefs(p: Prefs) { localStorage.setItem(KEY, JSON.stringify(p)); }

/** Put the preferences on <html> as CSS variables; the stylesheet reads them. */
export function applyPrefs(p: Prefs) {
  const s = document.documentElement.style;
  for (const a of ["ui", "tree", "grid", "editor"] as Area[]) {
    s.setProperty(`--font-${a}`, p[a].family || p.ui.family);
    s.setProperty(`--fs-${a}`, `${p[a].size}px`);
    s.setProperty(`--fw-${a}`, p[a].bold ? "600" : "400");
  }
  s.setProperty("--lh-editor", String(p.editorLineHeight));
}
