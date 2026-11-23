import { useEffect, useState } from "react";

/** A colour theme beside System / Light / Dark. Its colours are the `[data-palette]` blocks in styles.css; `swatch` is bg, panel, accent, text for the preview. */
export const THEMES = [
  { id: "monokai", label: "Monokai", base: "dark", swatch: ["#272822", "#1e1f1c", "#f92672", "#f8f8f2"] },
  { id: "github-dark", label: "GitHub Dark", base: "dark", swatch: ["#0d1117", "#161b22", "#1f6feb", "#c9d1d9"] },
  { id: "github-light", label: "GitHub Light", base: "light", swatch: ["#ffffff", "#f6f8fa", "#0969da", "#24292f"] },
  { id: "dracula", label: "Dracula", base: "dark", swatch: ["#282a36", "#21222c", "#9a6dea", "#f8f8f2"] },
  { id: "one-dark", label: "One Dark", base: "dark", swatch: ["#282c34", "#21252b", "#528bff", "#abb2bf"] },
  { id: "nord", label: "Nord", base: "dark", swatch: ["#2e3440", "#272c36", "#5e81ac", "#d8dee9"] },
  { id: "solarized-dark", label: "Solarized Dark", base: "dark", swatch: ["#002b36", "#073642", "#268bd2", "#93a1a1"] },
  { id: "solarized-light", label: "Solarized Light", base: "light", swatch: ["#fdf6e3", "#eee8d5", "#268bd2", "#586e75"] },
  { id: "tokyo-night", label: "Tokyo Night", base: "dark", swatch: ["#1a1b26", "#16161e", "#5a7bd6", "#c0caf5"] },
] as const;
export type ThemeId = (typeof THEMES)[number]["id"];
export type ThemePref = "system" | "light" | "dark" | ThemeId;
const KEY = "mylodb.theme";

export const themeById = (p: ThemePref) => THEMES.find((t) => t.id === p);

export function loadPref(): ThemePref {
  const v = localStorage.getItem(KEY);
  return v === "light" || v === "dark" || THEMES.some((t) => t.id === v) ? (v as ThemePref) : "system";
}

/** Light or dark: what the rest of the app (editor, diagrams) needs to know. A colour theme says which it is. */
export function resolve(pref: ThemePref): "light" | "dark" {
  const named = themeById(pref);
  if (named) return named.base;
  return pref === "system" ? (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light") : (pref as "light" | "dark");
}

/** Put the resolved base on <html data-theme> and the colour theme (if any) on <html data-palette>. Called before the first paint to avoid a flash. */
export function applyTheme(pref: ThemePref): "light" | "dark" {
  const r = resolve(pref);
  const root = document.documentElement;
  root.dataset.theme = r;
  if (themeById(pref)) root.dataset.palette = pref; else delete root.dataset.palette;
  return r;
}

/** The user's choice (System follows macOS) and the theme actually in effect. */
export function useTheme() {
  const [pref, setPrefState] = useState<ThemePref>(loadPref);
  const [resolved, setResolved] = useState<"light" | "dark">(() => applyTheme(loadPref()));
  useEffect(() => {
    setResolved(applyTheme(pref));
    if (pref !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const on = () => setResolved(applyTheme("system"));
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, [pref]);
  const setPref = (p: ThemePref) => { localStorage.setItem(KEY, p); setPrefState(p); };
  /** The colour theme in effect, or null for System / Light / Dark. */
  const palette: ThemeId | null = themeById(pref)?.id ?? null;
  return { pref, resolved, palette, setPref };
}
