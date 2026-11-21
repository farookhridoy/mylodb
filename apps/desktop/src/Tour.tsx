import { useEffect, useLayoutEffect, useState } from "react";

export interface TourStep { selector: string; title: string; text: string }

/** The first-run guide. Each step points at a real control; the tour is remembered as seen and can be replayed from Help → Quick Tour. */
export const TOUR_KEY = "mylodb.tourDone";
export const tourSeen = () => localStorage.getItem(TOUR_KEY) === "1";

export const TOUR_STEPS: TourStep[] = [
  { selector: "button[title='New connection']", title: "1. Add a connection", text: "Start here. Enter the host, user and password, or paste a mysql:// URL, then press Test. Passwords go to the macOS Keychain." },
  { selector: ".conn-bar", title: "2. Pick a connection", text: "Saved connections live in this list. You can group them into folders and keep several open at once; each opens as a tab." },
  { selector: ".filter", title: "3. Find anything fast", text: "Type part of a name to filter the tree. Try db:shop, type:view or col:email. Press ⇧⌘B to jump here." },
  { selector: ".side-tabs", title: "4. Objects, History, Favorites", text: "Browse objects, replay anything you ran before, or keep your favorite queries." },
  { selector: ".main", title: "5. Write and run SQL", text: "Press ⌘T for a new script and ⌘↵ to run it. Destructive statements are reviewed first. ⌘K opens the command palette, which also holds your snippets." },
  { selector: ".sidebar", title: "6. You are ready", text: "Right-click databases and tables for more. Open Help → User Manual any time, and Help → Quick Tour to see this again." },
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
