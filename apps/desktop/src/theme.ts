import { useEffect, useState } from "react";

export type ThemePref = "system" | "light" | "dark";
const KEY = "mylodb.theme";

export function loadPref(): ThemePref {
  const v = localStorage.getItem(KEY);
  return v === "light" || v === "dark" ? v : "system";
}

export function resolve(pref: ThemePref): "light" | "dark" {
  return pref === "system" ? (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light") : pref;
}

/** Put the resolved theme on <html> so the CSS variables switch. Called before the first paint to avoid a flash. */
export function applyTheme(pref: ThemePref): "light" | "dark" {
  const r = resolve(pref);
  document.documentElement.dataset.theme = r;
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
  return { pref, resolved, setPref };
}
