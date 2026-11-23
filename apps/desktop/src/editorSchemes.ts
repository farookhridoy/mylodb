import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";
import type { Extension } from "@codemirror/state";
import type { EditorScheme } from "./prefs";

type Palette = { keyword: string; string: string; number: string; comment: string; type: string; name: string; operator: string; bool: string; punct: string };

const PALETTES: Record<Exclude<EditorScheme, "default">, { light: Palette; dark: Palette }> = {
  // every kind of token its own hue
  vivid: {
    light: { keyword: "#a626a4", string: "#0a8a3a", number: "#c2410c", comment: "#8a8f98", type: "#0e7490", name: "#1d4ed8", operator: "#be123c", bool: "#b45309", punct: "#6b7280" },
    dark: { keyword: "#d58cf0", string: "#7ee787", number: "#ffa657", comment: "#7d8590", type: "#56d4dd", name: "#79b8ff", operator: "#ff7b9c", bool: "#f2cc60", punct: "#9ca3af" },
  },
  solarized: {
    light: { keyword: "#859900", string: "#2aa198", number: "#d33682", comment: "#93a1a1", type: "#b58900", name: "#268bd2", operator: "#cb4b16", bool: "#6c71c4", punct: "#657b83" },
    dark: { keyword: "#859900", string: "#2aa198", number: "#d33682", comment: "#657b83", type: "#b58900", name: "#268bd2", operator: "#cb4b16", bool: "#6c71c4", punct: "#93a1a1" },
  },
  contrast: {
    light: { keyword: "#0000cc", string: "#006400", number: "#8b0000", comment: "#555555", type: "#000000", name: "#000000", operator: "#000000", bool: "#8b0000", punct: "#000000" },
    dark: { keyword: "#7aa2ff", string: "#9fef9f", number: "#ffb3b3", comment: "#b0b0b0", type: "#ffffff", name: "#ffffff", operator: "#ffffff", bool: "#ffb3b3", punct: "#ffffff" },
  },
};

/** Extra syntax colours for the chosen scheme; "default" adds nothing (the app's own colours, One Dark in the dark theme). */
export function schemeExtension(scheme: EditorScheme, dark: boolean, theme?: string | null): Extension {
  // "default" follows the colour theme when there is one; a scheme picked in Preferences overrides it
  const themed = scheme === "default" && theme ? THEME_SYNTAX[theme] : undefined;
  if (scheme === "default" && !themed) return [];
  const c = themed ?? PALETTES[scheme as Exclude<EditorScheme, "default">][dark ? "dark" : "light"];
  const bold = !themed && scheme === "contrast";
  return syntaxHighlighting(HighlightStyle.define([
    { tag: [t.keyword, t.operatorKeyword, t.modifier, t.controlKeyword], color: c.keyword, fontWeight: bold ? "700" : undefined },
    { tag: [t.string, t.special(t.string)], color: c.string },
    { tag: [t.number, t.integer, t.float], color: c.number },
    { tag: [t.bool, t.null, t.atom], color: c.bool },
    { tag: [t.comment, t.lineComment, t.blockComment], color: c.comment, fontStyle: "italic" },
    { tag: [t.typeName, t.className, t.standard(t.name)], color: c.type },
    { tag: [t.name, t.variableName, t.propertyName, t.labelName], color: c.name },
    { tag: [t.operator, t.compareOperator, t.logicOperator, t.arithmeticOperator], color: c.operator },
    { tag: [t.punctuation, t.separator, t.paren, t.squareBracket, t.brace], color: c.punct },
  ]), { fallback: false });
}

/** Syntax colours of the colour themes (One Dark keeps the editor's own). */
export const THEME_SYNTAX: Record<string, Palette> = {
  "monokai": {"keyword": "#f92672", "string": "#e6db74", "number": "#ae81ff", "comment": "#75715e", "type": "#66d9ef", "name": "#f8f8f2", "operator": "#f92672", "bool": "#ae81ff", "punct": "#f8f8f2"},
  "github-dark": {"keyword": "#ff7b72", "string": "#a5d6ff", "number": "#79c0ff", "comment": "#8b949e", "type": "#ffa657", "name": "#c9d1d9", "operator": "#ff7b72", "bool": "#79c0ff", "punct": "#c9d1d9"},
  "github-light": {"keyword": "#cf222e", "string": "#0a3069", "number": "#0550ae", "comment": "#6e7781", "type": "#953800", "name": "#24292f", "operator": "#cf222e", "bool": "#0550ae", "punct": "#24292f"},
  "dracula": {"keyword": "#ff79c6", "string": "#f1fa8c", "number": "#bd93f9", "comment": "#6272a4", "type": "#8be9fd", "name": "#f8f8f2", "operator": "#ff79c6", "bool": "#bd93f9", "punct": "#f8f8f2"},
  "nord": {"keyword": "#81a1c1", "string": "#a3be8c", "number": "#b48ead", "comment": "#616e88", "type": "#8fbcbb", "name": "#d8dee9", "operator": "#81a1c1", "bool": "#b48ead", "punct": "#eceff4"},
  "solarized-dark": {"keyword": "#859900", "string": "#2aa198", "number": "#d33682", "comment": "#657b83", "type": "#b58900", "name": "#268bd2", "operator": "#cb4b16", "bool": "#6c71c4", "punct": "#93a1a1"},
  "solarized-light": {"keyword": "#859900", "string": "#2aa198", "number": "#d33682", "comment": "#93a1a1", "type": "#b58900", "name": "#268bd2", "operator": "#cb4b16", "bool": "#6c71c4", "punct": "#657b83"},
  "tokyo-night": {"keyword": "#bb9af7", "string": "#9ece6a", "number": "#ff9e64", "comment": "#565f89", "type": "#2ac3de", "name": "#c0caf5", "operator": "#89ddff", "bool": "#ff9e64", "punct": "#a9b1d6"},
};
