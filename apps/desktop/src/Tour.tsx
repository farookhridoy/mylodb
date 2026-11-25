import { useEffect, useLayoutEffect, useState } from "react";

export interface TourStep { selector: string; title: string; text: string }

/** The first-run guide. Each step points at a real control; the tour is remembered as seen and can be replayed from Help → Quick Tour. */
export const TOUR_KEY = "mylodb.tourDone";
export const tourSeen = () => localStorage.getItem(TOUR_KEY) === "1";

export const TOUR_STEPS: TourStep[] = [
  { selector: "button[title='New connection']", title: "1. Add a connection", text: "Start here. Enter the host, user and password, or paste a mysql:// URL, then press Test. Choose Development, Staging or Production on the Safety tab. For a SQLite file, set Database type to SQLite file. Passwords go to the macOS Keychain." },
  { selector: ".conn-bar", title: "2. Pick a connection", text: "Saved connections live in this list. You can group them into folders and keep several open at once; each opens as a tab, and switching back does not reconnect." },
  { selector: ".sidebar-toggle", title: "3. Hide or show the sidebar", text: "This button (or ⌘B) hides the object tree so a wide table gets the whole window." },
  { selector: ".filter", title: "4. Find anything fast", text: "Type part of a name to filter the tree. Try db:shop, type:view or col:email, or /regex/. The funnel icon chooses what is searched. Press ⇧⌘B to jump here." },
  { selector: ".side-tabs", title: "5. Objects, History, Favorites", text: "Browse objects, replay anything you ran before, or keep your favorite queries. Double-click a table to open its data; right-click for its menu: properties, sample SQL, design, export, dump, rename, duplicate, drop." },
  { selector: ".main", title: "6. Write and run SQL", text: "Press ⌘T for a new script and ⌘↵ to run the selection. Preview shows a risk review first, and Explain shows how the server runs the query. Results, Profile, Messages, Info and History appear here after the first run. ⇧⌘F formats the SQL." },
  { selector: ".main", title: "7. Work with table data", text: "Open a table to get a grid. Double-click a cell to edit; changes are staged and shown as SQL before Submit. Use the filter row (contains, between, in, regexp, is NULL…) or a WHERE box, click headers to sort, and Count for an exact total. Long text and JSON open in an editor window; BLOB cells have Upload and Save." },
  { selector: ".main", title: "8. Import and export", text: "File → Import Data loads CSV, TSV, JSON or JSON Lines into a table, with a preview and column mapping. File → Export Data writes any SELECT as CSV, JSON, JSON Lines, SQL INSERT, XML, HTML or Excel. Copy as… on a result gives TSV, CSV, JSON, Markdown or INSERT." },
  { selector: ".main", title: "9. Backup, compare and tune", text: "File → Backup / Restore makes a consistent SQL dump and runs one back. The Database menu has Compare Schemas, Index Advisor, Users and Privileges, Server Tools and the table designer. Search → Search Data in Database finds a value in any table." },
  { selector: ".tasks-btn", title: "10. Background jobs", text: "Exports, imports, backups and scripts run here with progress and a Cancel button, so you can keep working." },
  { selector: ".ask-ai-btn", title: "11. Ask AI to write a query", text: "Describe what you need in plain words and a model writes the SQL in a panel on the right. Connect Ollama (free, on your computer), OpenRouter, Groq or Gemini (free tiers) or Claude from the gear in the panel. Only table and column names are sent, and nothing is run for you." },
  { selector: ".sidebar", title: "12. You are ready", text: "Settings (⌘,) has colour themes such as Monokai, GitHub Dark and Nord. ⌘/ lists every shortcut. Help → User Manual explains each menu, and Help → Quick Tour shows this again." },
];

/** A spotlight on one control with a short explanation beside it. Steps whose control is not on screen are skipped. */
export default function Tour(props: { steps?: TourStep[]; onClose(): void }) {
  const steps = props.steps ?? TOUR_STEPS;
  const [i, setI] = useState(0);
  const [box, setBox] = useState<DOMRect | null>(null);
  const step = steps[i];

  useLayoutEffect(() => {
    const measure = () => setBox(document.querySelector(step.selector)?.getBoundingClientRect() ?? null);
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, [step.selector]);

  const finish = () => { localStorage.setItem(TOUR_KEY, "1"); props.onClose(); };
  const next = () => (i + 1 < steps.length ? setI(i + 1) : finish());
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") finish(); else if (e.key === "ArrowRight" || e.key === "Enter") next(); else if (e.key === "ArrowLeft" && i > 0) setI(i - 1); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  // the card sits to the right of the control, or below it when there is no room
  const W = 340;
  const card: React.CSSProperties = box
    ? { left: Math.min(window.innerWidth - W - 12, Math.max(12, box.right + 12 > window.innerWidth - W ? box.left : box.right + 12)), top: Math.min(window.innerHeight - 220, Math.max(12, box.right + 12 > window.innerWidth - W ? box.bottom + 12 : box.top)) }
    : { left: window.innerWidth / 2 - W / 2, top: window.innerHeight / 3 };
  return (
    <div className="tour" role="dialog" aria-label="Quick tour">
      {box && <div className="tour-spot" style={{ left: box.left - 4, top: box.top - 4, width: box.width + 8, height: box.height + 8 }} />}
      <div className="tour-card" style={{ ...card, width: W }}>
        <h4>{step.title}</h4>
        <p>{step.text}</p>
        <div className="tour-actions">
          <span className="dim small">{i + 1} of {steps.length}</span>
          <span>
            <button onClick={finish}>Skip</button>
            {i > 0 && <button onClick={() => setI(i - 1)}>Back</button>}
            <button className="primary" onClick={next}>{i + 1 < steps.length ? "Next" : "Done"}</button>
          </span>
        </div>
      </div>
    </div>
  );
}
