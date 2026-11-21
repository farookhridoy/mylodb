import { useState } from "react";
import Select from "./Select";
import { AREA_LABEL, DEFAULTS, FAMILIES, SCHEMES, SIZE_RANGE, applyPrefs, savePrefs, type Area, type Prefs } from "./prefs";
import type { ThemePref } from "./theme";

/** Preferences: theme, and font family, size and weight for each part of the window. Changes apply live; Cancel puts the old values back. */
export default function PreferencesDialog(props: {
  initial: Prefs;
  theme: ThemePref;
  onTheme(t: ThemePref): void;
  onChange(p: Prefs): void;
  /** Called on every edit, so the editor can preview the colour scheme. */
  onLive(p: Prefs): void;
  onClose(): void;
}) {
  const [p, setP] = useState<Prefs>(props.initial);
  const update = (next: Prefs) => { setP(next); applyPrefs(next); props.onLive(next); };
  const setArea = (a: Area, patch: Partial<Prefs[Area]>) => update({ ...p, [a]: { ...p[a], ...patch } });
  const areas: Area[] = ["ui", "tree", "grid", "editor"];
  const cancel = () => { applyPrefs(props.initial); props.onClose(); };
  const save = () => { savePrefs(p); props.onChange(p); props.onClose(); };
  return (
    <div className="overlay">
      <div className="dialog prefs" role="dialog" aria-label="Preferences">
        <h3>Preferences</h3>
        <section>
          <h4>Theme</h4>
          <Select value={props.theme} title="Theme" aria-label="Theme" onChange={(e) => props.onTheme(e.target.value as ThemePref)}>
            <option value="system">System</option><option value="light">Light</option><option value="dark">Dark</option>
          </Select>
        </section>
        <section>
          <h4>Fonts</h4>
          <table className="prefs-table">
            <thead><tr><th /><th>Family</th><th>Size</th><th>Bold</th></tr></thead>
            <tbody>
              {areas.map((a) => (
                <tr key={a} data-area={a}>
                  <td>{AREA_LABEL[a]}</td>
                  <td>
                    <Select value={p[a].family} title={`${AREA_LABEL[a]} font`} aria-label={`${AREA_LABEL[a]} font`} onChange={(e) => setArea(a, { family: e.target.value })}>
                      {a === "tree" && <option value="">Same as interface</option>}
                      {FAMILIES.map((f) => <option key={f.label} value={f.value}>{f.label}</option>)}
                    </Select>
                  </td>
                  <td><input type="number" style={{ width: 60 }} min={SIZE_RANGE[a][0]} max={SIZE_RANGE[a][1]} value={p[a].size} aria-label={`${AREA_LABEL[a]} size`}
                             onChange={(e) => setArea(a, { size: Math.min(SIZE_RANGE[a][1], Math.max(SIZE_RANGE[a][0], Number(e.target.value) || DEFAULTS[a].size)) })} /></td>
                  <td><input type="checkbox" checked={p[a].bold} aria-label={`${AREA_LABEL[a]} bold`} onChange={(e) => setArea(a, { bold: e.target.checked })} /></td>
                </tr>
              ))}
            </tbody>
          </table>
          <label className="check" style={{ marginTop: 8 }}>Editor colours
            <Select value={p.editorScheme} title="Editor colours" aria-label="Editor colours" onChange={(e) => update({ ...p, editorScheme: e.target.value as Prefs["editorScheme"] })}>
              {SCHEMES.map((x) => <option key={x.id} value={x.id}>{x.label}</option>)}
            </Select>
          </label>
          <label className="check" style={{ marginTop: 8 }}>Editor line height
            <input type="number" step={0.1} min={1} max={2.2} style={{ width: 60 }} value={p.editorLineHeight} aria-label="Editor line height"
                   onChange={(e) => update({ ...p, editorLineHeight: Math.min(2.2, Math.max(1, Number(e.target.value) || DEFAULTS.editorLineHeight)) })} />
          </label>
          <div className="prefs-sample" aria-label="Sample">
            <div className="sample-grid">id&nbsp;&nbsp;name&nbsp;&nbsp;&nbsp;&nbsp;total<br />1&nbsp;&nbsp;&nbsp;Ann&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;19.50</div>
            <div className="sample-editor">SELECT name, total FROM orders WHERE total &gt; 10;</div>
          </div>
        </section>
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
