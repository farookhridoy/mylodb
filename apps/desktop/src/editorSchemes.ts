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
export function schemeExtension(scheme: EditorScheme, dark: boolean): Extension {
  if (scheme === "default") return [];
  const c = PALETTES[scheme][dark ? "dark" : "light"];
  const bold = scheme === "contrast";
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
