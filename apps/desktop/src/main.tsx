import React from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./styles.css";
import { applyTheme, loadPref } from "./theme";

applyTheme(loadPref()); // before the first paint, so there is no flash of the wrong theme

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
