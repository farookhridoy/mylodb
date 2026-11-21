import { Children, Fragment, isValidElement, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactElement, type ReactNode } from "react";
import { createPortal } from "react-dom";
import Icon from "./Icon";

/** The lists shorter than this have no search box (like select2's `minimumResultsForSearch`). */
const SEARCH_FROM = 7;
/** A server can have thousands of databases: only this many matches are drawn, and the box asks for more typing. */
const MAX_SHOWN = 300;

interface Opt { value: string; label: string; disabled: boolean; group: string | null }

/** `<option>` and `<optgroup>` children, however they are nested in arrays and fragments. */
function collect(children: ReactNode, group: string | null = null, out: Opt[] = []): Opt[] {
  Children.forEach(children, (c) => {
    if (!isValidElement(c)) return;
    const el = c as ReactElement<{ value?: string | number; disabled?: boolean; label?: string; children?: ReactNode }>;
    if (el.type === Fragment) collect(el.props.children, group, out);
    else if (el.type === "optgroup") collect(el.props.children, el.props.label ?? null, out);
    else if (el.type === "option") {
      const label = Children.toArray(el.props.children).join("");
      out.push({ value: String(el.props.value ?? label), label, disabled: !!el.props.disabled, group });
    }
  });
  return out;
}

export type SelectChange = { target: { value: string } };

/**
 * A drop-in replacement for `<select>` with a searchable dropdown, like select2: type to filter, arrows and Enter to choose, Esc to close.
 * The original `<select>` stays in the page, hidden and in sync (as select2 does), so forms, autofill and assistive tools still see it.
 * Same props and `<option>` children as a native select; `onChange` receives `{ target: { value } }`.
 */
export default function Select(props: {
  value?: string | number | null;
  onChange?: (e: SelectChange) => void;
  children?: ReactNode;
  disabled?: boolean;
  title?: string;
  className?: string;
  style?: React.CSSProperties;
  id?: string;
  placeholder?: string;
  "aria-label"?: string;
}) {
  const options = useMemo(() => collect(props.children), [props.children]);
  const value = props.value === null || props.value === undefined ? "" : String(props.value);
  const current = options.find((o) => o.value === value);
  const searchable = options.length >= SEARCH_FROM;

  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [box, setBox] = useState<{ left: number; top: number; width: number; up: boolean; maxH: number } | null>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const search = useRef<HTMLInputElement>(null);

  const matches = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? options.filter((o) => o.label.toLowerCase().includes(q)) : options;
  }, [options, query]);
  const shown = matches.length > MAX_SHOWN ? matches.slice(0, MAX_SHOWN) : matches;

  const place = () => {
    const r = trigger.current?.getBoundingClientRect();
    if (!r) return;
    const below = window.innerHeight - r.bottom - 8, above = r.top - 8;
    const up = below < 220 && above > below;
    const width = Math.max(r.width, 170);
    // keep the list inside the window (a dropdown at the right edge opens leftwards)
    const left = Math.max(8, Math.min(r.left, window.innerWidth - width - 8));
    setBox({ left, top: up ? r.top - 2 : r.bottom + 2, width, up, maxH: Math.max(140, Math.min(320, up ? above : below)) });
  };
  useLayoutEffect(() => { if (open) place(); }, [open]);  // eslint-disable-line react-hooks/exhaustive-deps

  const choose = (o: Opt | undefined) => {
    if (!o || o.disabled) return;
    props.onChange?.({ target: { value: o.value } });
    setOpen(false);
    trigger.current?.focus();
  };
  const openList = () => {
    if (props.disabled) return;
    setQuery("");
    setActive(Math.max(0, options.findIndex((o) => o.value === value)));
    setOpen(true);
  };

  // close on outside click, resize and scrolling of anything but the list
  useEffect(() => {
    if (!open) return;
    const away = (e: MouseEvent) => { const t = e.target as Node; if (!list.current?.contains(t) && !trigger.current?.contains(t)) setOpen(false); };
    const scroll = (e: Event) => { if (!list.current?.contains(e.target as Node)) setOpen(false); };
    window.addEventListener("mousedown", away);
    window.addEventListener("resize", place);
    window.addEventListener("scroll", scroll, true);
    return () => { window.removeEventListener("mousedown", away); window.removeEventListener("resize", place); window.removeEventListener("scroll", scroll, true); };
  }, [open]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => { if (open && searchable) search.current?.focus(); else if (open) list.current?.focus(); }, [open, box, searchable]);
  useEffect(() => { setActive(0); }, [query]);
  useEffect(() => { list.current?.querySelector(".ss-opt.active")?.scrollIntoView({ block: "nearest" }); }, [active, open, box]);

  const move = (d: number) => {
    if (shown.length === 0) return;
    let i = active;
    for (let n = 0; n < shown.length; n++) { i = (i + d + shown.length) % shown.length; if (!shown[i].disabled) break; }
    setActive(i);
  };
  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") { e.preventDefault(); move(1); }
    else if (e.key === "ArrowUp") { e.preventDefault(); move(-1); }
    else if (e.key === "Home") { e.preventDefault(); setActive(0); }
    else if (e.key === "End") { e.preventDefault(); setActive(Math.max(0, shown.length - 1)); }
    else if (e.key === "Enter") { e.preventDefault(); choose(shown[active]); }
    else if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); setOpen(false); trigger.current?.focus(); }
    else if (e.key === "Tab") setOpen(false);
  };

  let lastGroup: string | null = null;
  return (
    <span className={`ss ${props.className ?? ""}`} style={props.style}>
      {/* the real select, hidden and kept in sync */}
      <select className="ss-native" tabIndex={-1} aria-hidden="true" value={value} disabled={props.disabled} id={props.id} title={props.title}
              onChange={(e) => props.onChange?.({ target: { value: e.target.value } })}>
        {props.children}
      </select>
      <button type="button" ref={trigger} className="ss-trigger" role="combobox" aria-haspopup="listbox" aria-expanded={open} aria-label={props["aria-label"]}
              title={props.title} disabled={props.disabled} data-value={value}
              onClick={() => (open ? setOpen(false) : openList())}
              onKeyDown={(e) => { if (["ArrowDown", "ArrowUp", "Enter", " "].includes(e.key)) { e.preventDefault(); if (!open) openList(); } }}>
        <span className="ss-label">{current ? current.label : <span className="dim">{props.placeholder ?? ""}</span>}</span>
        <Icon name="chevron" size={10} className="ss-caret" />
      </button>
      {open && box && createPortal(
        <div ref={list} className={`ss-pop ${box.up ? "up" : ""}`} tabIndex={-1} role="presentation" onKeyDown={onKey}
             style={{ left: box.left, minWidth: box.width, maxHeight: box.maxH, ...(box.up ? { bottom: window.innerHeight - box.top } : { top: box.top }) }}>
          {searchable && (
            <div className="ss-search">
              <Icon name="search" size={12} />
              <input ref={search} value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search…" aria-label="Search options" autoComplete="off" spellCheck={false} />
            </div>
          )}
          <div className="ss-list" role="listbox">
            {shown.map((o, i) => {
              const header = o.group !== lastGroup && o.group ? <div key={`g-${o.group}`} className="ss-group">{o.group}</div> : null;
              lastGroup = o.group;
              return (
                <Fragment key={o.value + ":" + i}>
                  {header}
                  <div role="option" aria-selected={o.value === value} aria-disabled={o.disabled}
                       className={`ss-opt ${i === active ? "active" : ""} ${o.value === value ? "chosen" : ""} ${o.disabled ? "disabled" : ""}`}
                       onMouseMove={() => setActive(i)} onClick={() => choose(o)}>
                    <Highlight text={o.label || " "} q={query} />
                  </div>
                </Fragment>
              );
            })}
            {matches.length > shown.length && <div className="ss-none">{(matches.length - shown.length).toLocaleString()} more — type to narrow</div>}
            {shown.length === 0 && <div className="ss-none">No matches</div>}
          </div>
        </div>,
        document.body,
      )}
    </span>
  );
}

function Highlight({ text, q }: { text: string; q: string }) {
  const t = q.trim().toLowerCase();
  const i = t ? text.toLowerCase().indexOf(t) : -1;
  if (i < 0) return <>{text}</>;
  return <>{text.slice(0, i)}<mark>{text.slice(i, i + t.length)}</mark>{text.slice(i + t.length)}</>;
}
