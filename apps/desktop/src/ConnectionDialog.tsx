import { useState } from "react";
import { api, type SavedConnection } from "./api";

const blank: SavedConnection = {
  id: null, name: "", color: "#4f8cff", host: "127.0.0.1", port: 3306,
  user: "root", database: null, ssl: "disabled", new_password: "",
};

export default function ConnectionDialog(props: {
  initial?: SavedConnection;
  onClose: () => void;
  onSaved: (id: number) => void;
}) {
  const [c, setC] = useState<SavedConnection>(props.initial ?? blank);
  const [error, setError] = useState<string | null>(null);
  const set = <K extends keyof SavedConnection>(k: K, v: SavedConnection[K]) => setC({ ...c, [k]: v });

  const save = async () => {
    try {
      const payload = { ...c, new_password: c.new_password ? c.new_password : null };
      props.onSaved(await api.saveConnection(payload));
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="overlay">
      <div className="dialog">
        <h3>{c.id ? "Edit connection" : "New connection"}</h3>
        <label>Name<input value={c.name} onChange={(e) => set("name", e.target.value)} autoFocus /></label>
        <div className="row">
          <label>Host<input value={c.host} onChange={(e) => set("host", e.target.value)} /></label>
          <label className="narrow">Port<input type="number" value={c.port} onChange={(e) => set("port", Number(e.target.value))} /></label>
        </div>
        <div className="row">
          <label>User<input value={c.user} onChange={(e) => set("user", e.target.value)} /></label>
          <label>Password<input type="password" placeholder={c.id ? "(unchanged)" : ""} value={c.new_password ?? ""} onChange={(e) => set("new_password", e.target.value)} /></label>
        </div>
        <div className="row">
          <label>SSL
            <select value={c.ssl} onChange={(e) => set("ssl", e.target.value as SavedConnection["ssl"])}>
              <option value="disabled">Disabled</option>
              <option value="required">Required (verify)</option>
              <option value="skipverify">Required (skip verify)</option>
            </select>
          </label>
          <label className="narrow">Tab color<input type="color" value={c.color} onChange={(e) => set("color", e.target.value)} /></label>
        </div>
        {error && <div className="error">{error}</div>}
        <div className="actions">
          <button onClick={props.onClose}>Cancel</button>
          <button className="primary" disabled={!c.name || !c.host || !c.user} onClick={save}>Save</button>
        </div>
      </div>
    </div>
  );
}
