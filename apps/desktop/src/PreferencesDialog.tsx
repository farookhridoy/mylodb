import SecuritySettings from "./SecuritySettings";
import AiSetup from "./AiSetup";
import AuditSettings from "./AuditSettings";
import { useMemo, useState } from "react";
import Select from "./Select";
import { AREA_LABEL, DEFAULTS, FAMILIES, SCHEMES, SIZE_RANGE, applyPrefs, savePrefs, type Area, type Prefs } from "./prefs";
import { THEMES, themeById, type ThemePref } from "./theme";

type PageId = "theme" | "font-ui" | "font-tree" | "font-grid" | "font-editor" | "colours" | "passwords" | "audit" | "ai";
interface Page { id: PageId; group: string; label: string; words: string }
const PAGES: Page[] = [
  { id: "theme", group: "Appearance", label: "Theme", words: "light dark system mode colour" },
  { id: "font-ui", group: "Fonts", label: "Interface", words: "font family size bold menus buttons dialogs" },
  { id: "font-tree", group: "Fonts", label: "Sidebar", words: "font family size bold object tree navigator" },
  { id: "font-grid", group: "Fonts", label: "Data grid", words: "font family size bold results table cells" },
  { id: "font-editor", group: "Fonts", label: "Query editor", words: "font family size bold line height sql code" },
  { id: "passwords", group: "Security", label: "Saved passwords", words: "keychain master password encrypted file prompt unlock security" },
  { id: "audit", group: "Security", label: "Audit log", words: "audit log changes history file who what when compliance record" },
  { id: "ai", group: "AI", label: "Ask AI", words: "ai assistant write query ollama groq openrouter gemini free model key llm" },
  { id: "colours", group: "Editor", label: "Syntax colours", words: "editor colour scheme highlight solarized contrast vivid multi-colour" },
];

/** Settings, laid out like DBeaver's: a filterable list of pages on the left, the chosen page on the right. Changes apply live; Cancel restores the old values. */
export default function PreferencesDialog(props: {
  initial: Prefs;
  theme: ThemePref;
  onTheme(t: ThemePref): void;
  onChange(p: Prefs): void;
  /** Called on every edit, so the editor can preview the colour scheme. */
  onLive(p: Prefs): void;
  onClose(): void;
  /** Open on this page instead of Theme. */
  startPage?: PageId;
}) {
  const [p, setP] = useState<Prefs>(props.initial);
  const [page, setPage] = useState<PageId>(props.startPage ?? "theme");
  const [filter, setFilter] = useState("");
  const update = (next: Prefs) => { setP(next); applyPrefs(next); props.onLive(next); };
  const setArea = (a: Area, patch: Partial<Prefs[Area]>) => update({ ...p, [a]: { ...p[a], ...patch } });
  const cancel = () => { applyPrefs(props.initial); props.onLive(props.initial); props.onClose(); };
  const save = () => { savePrefs(p); props.onChange(p); props.onClose(); };

  const shown = useMemo(() => {
    const words = filter.toLowerCase().split(/\s+/).filter(Boolean);
    return PAGES.filter((x) => words.every((w) => `${x.group} ${x.label} ${x.words}`.toLowerCase().includes(w)));
  }, [filter]);
  const groups = [...new Set(shown.map((x) => x.group))];
  const current = shown.find((x) => x.id === page) ?? shown[0];

  const fontPage = (a: Area) => (
    <div className="prefs-page" data-area={a}>
      <label>Font
        <Select value={p[a].family} title={`${AREA_LABEL[a]} font`} aria-label={`${AREA_LABEL[a]} font`} onChange={(e) => setArea(a, { family: e.target.value })}>
          {a === "tree" && <option value="">Same as interface</option>}
          {FAMILIES.map((f) => <option key={f.label} value={f.value}>{f.label}</option>)}
        </Select>
      </label>
      <label>Size
        <input type="number" style={{ width: 70 }} min={SIZE_RANGE[a][0]} max={SIZE_RANGE[a][1]} value={p[a].size} aria-label={`${AREA_LABEL[a]} size`}
               onChange={(e) => setArea(a, { size: Math.min(SIZE_RANGE[a][1], Math.max(SIZE_RANGE[a][0], Number(e.target.value) || DEFAULTS[a].size)) })} />
      </label>
      <label className="check"><input type="checkbox" checked={p[a].bold} aria-label={`${AREA_LABEL[a]} bold`} onChange={(e) => setArea(a, { bold: e.target.checked })} /> Bold</label>
      {a === "editor" && (
        <label>Line height
          <input type="number" step={0.1} min={1} max={2.2} style={{ width: 70 }} value={p.editorLineHeight} aria-label="Editor line height"
                 onChange={(e) => update({ ...p, editorLineHeight: Math.min(2.2, Math.max(1, Number(e.target.value) || DEFAULTS.editorLineHeight)) })} />
        </label>
      )}
      <div className="prefs-sample" aria-label="Sample">
        {a === "grid" && <div className="sample-grid">id&nbsp;&nbsp;name&nbsp;&nbsp;&nbsp;&nbsp;total<br />1&nbsp;&nbsp;&nbsp;Ann&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;19.50</div>}
        {a === "editor" && <div className="sample-editor">SELECT name, total FROM orders WHERE total &gt; 10;</div>}
        {a === "tree" && <div className="sample-tree">▾ shop &nbsp; 5.31 MB<br />&nbsp;&nbsp;&nbsp;orders</div>}
        {a === "ui" && <div>Run &nbsp; Preview &nbsp; Format &nbsp; Explain</div>}
      </div>
    </div>
  );

  return (
    <div className="overlay">
      <div className="dialog prefs" role="dialog" aria-label="Settings">
        <h3>Settings</h3>
        <div className="prefs-body">
          <nav className="prefs-nav" aria-label="Preference pages">
            <input value={filter} onChange={(e) => setFilter(e.target.value)} placeholder="type filter text" aria-label="Filter preferences" autoFocus />
            {groups.map((g) => (
              <div key={g}>
                <div className="prefs-group">{g}</div>
                {shown.filter((x) => x.group === g).map((x) => (
                  <button key={x.id} className={`prefs-item ${current?.id === x.id ? "active" : ""}`} onClick={() => setPage(x.id)}>{x.label}</button>
                ))}
              </div>
            ))}
            {shown.length === 0 && <div className="dim small" style={{ padding: 8 }}>Nothing matches</div>}
          </nav>
          <section className="prefs-content">
            {current && <h4>{current.group} › {current.label}</h4>}
            {current?.id === "theme" && (
              <div className="prefs-page">
                <label>Theme
                  <Select value={props.theme} title="Theme" aria-label="Theme" onChange={(e) => props.onTheme(e.target.value as ThemePref)}>
                    <optgroup label="Automatic"><option value="system">System</option><option value="light">Light</option><option value="dark">Dark</option></optgroup>
                    <optgroup label="Dark themes">{THEMES.filter((t) => t.base === "dark").map((t) => <option key={t.id} value={t.id}>{t.label}</option>)}</optgroup>
                    <optgroup label="Light themes">{THEMES.filter((t) => t.base === "light").map((t) => <option key={t.id} value={t.id}>{t.label}</option>)}</optgroup>
                  </Select>
                </label>
                <div className="theme-grid" role="radiogroup" aria-label="Colour themes">
                  {THEMES.map((t) => (
                    <button key={t.id} role="radio" aria-checked={props.theme === t.id} className={`theme-card ${props.theme === t.id ? "on" : ""}`} onClick={() => props.onTheme(t.id)} title={`${t.label} (${t.base})`}>
                      <span className="theme-swatch" style={{ background: t.swatch[0], borderColor: t.swatch[1] }}>
                        <i style={{ background: t.swatch[1] }} /><i style={{ background: t.swatch[2] }} /><i style={{ background: t.swatch[3] }} />
                      </span>
                      {t.label}
                    </button>
                  ))}
                </div>
                <div className="dim small">{themeById(props.theme) ? `${themeById(props.theme)!.label}: the SQL editor uses this theme's syntax colours unless you pick a scheme under Editor → Syntax colours.` : "System follows macOS. Pick a colour theme below, or one of the plain Light and Dark."}</div>
              </div>
            )}
            {current?.id === "passwords" && <SecuritySettings />}
            {current?.id === "audit" && <AuditSettings />}
            {current?.id === "ai" && <div className="prefs-page ai-page"><AiSetup /></div>}
            {current?.id === "font-ui" && fontPage("ui")}
            {current?.id === "font-tree" && fontPage("tree")}
            {current?.id === "font-grid" && fontPage("grid")}
            {current?.id === "font-editor" && fontPage("editor")}
            {current?.id === "colours" && (
              <div className="prefs-page">
                <label>Editor colours
                  <Select value={p.editorScheme} title="Editor colours" aria-label="Editor colours" onChange={(e) => update({ ...p, editorScheme: e.target.value as Prefs["editorScheme"] })}>
                    {SCHEMES.map((x) => <option key={x.id} value={x.id}>{x.label}</option>)}
                  </Select>
                </label>
                <div className="dim small">The editor behind this dialog shows the scheme as you choose it.</div>
              </div>
            )}
          </section>
        </div>
        <div className="actions">
          <button onClick={() => update({ ...DEFAULTS })}>Reset to defaults</button>
          <span className="grow" />
          <button onClick={cancel}>Cancel</button>
          <button className="primary" onClick={save}>Save</button>
        </div>
      </div>
    </div>
  );
}
