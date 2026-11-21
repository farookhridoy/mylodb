import type { ReactNode } from "react";

/** One flat line-icon set (16 px grid, `currentColor`), used instead of emoji so both themes and all platforms look the same. */
const PATHS: Record<string, ReactNode> = {
  server: <><rect x="2" y="2.5" width="12" height="4.2" rx="1" /><rect x="2" y="9.3" width="12" height="4.2" rx="1" /><path d="M4.6 4.6h.01M4.6 11.4h.01" /></>,
  database: <><ellipse cx="8" cy="4" rx="5" ry="2" /><path d="M3 4v8c0 1.1 2.2 2 5 2s5-.9 5-2V4M3 8c0 1.1 2.2 2 5 2s5-.9 5-2" /></>,
  table: <><rect x="2" y="3" width="12" height="10" rx="1" /><path d="M2 6.4h12M2 9.7h12M6.4 6.4V13" /></>,
  view: <><path d="M1.5 8S4 3.5 8 3.5 14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8z" /><circle cx="8" cy="8" r="2" /></>,
  procedure: <path d="M5.8 2.5C4.3 2.5 4.2 3.6 4.2 5s0 2.2-1.7 3c1.7.8 1.7 1.6 1.7 3s.1 2.5 1.6 2.5M10.2 2.5c1.5 0 1.6 1.1 1.6 2.5s0 2.2 1.7 3c-1.700.8-1.700 1.600-1.700 3s-.1 2.500-1.600 2.500" />,
  function: <path d="M10.200 3.200c-1.600-.5-2.700.3-2.900 1.900L6.600 12c-.2 1.500-1.200 2.100-2.600 1.600M4.600 7.500h5.200" />,
  trigger: <path d="M9 1.800 3.500 9h4L7 14.200 12.500 7h-4z" />,
  event: <><circle cx="8" cy="8" r="5.800" /><path d="M8 4.600V8l2.400 1.500" /></>,
  folder: <path d="M2 4.500v8c0 .6.400 1 1 1h10c.6 0 1-.4 1-1V6c0-.6-.4-1-1-1H8L6.500 3.500H3c-.6 0-1 .4-1 1z" />,
  column: <rect x="3" y="5" width="10" height="6" rx="1" />,
  key: <><circle cx="5.500" cy="8" r="2.500" /><path d="M8 8h6M12 8v2.500" /></>,
  unique: <path d="M8 3l4.500 5L8 13 3.500 8z" />,
  index: <path d="M3 4h10M3 8h7M3 12h4" />,
  plus: <path d="M8 3v10M3 8h10" />,
  edit: <path d="m10.500 3.500 2 2L5 13H3v-2z" />,
  trash: <path d="M3.500 4.500h9M6 4.500V3h4v1.500M4.500 4.500l.6 8.500h5.800l.6-8.500M7 7v4M9 7v4" />,
  user: <><circle cx="8" cy="5.500" r="2.500" /><path d="M3 13.500c0-2.800 2.200-4.500 5-4.500s5 1.700 5 4.500" /></>,
  transfer: <path d="M5 13V3M5 3 2.500 5.500M5 3l2.500 2.500M11 3v10M11 13l-2.500-2.500M11 13l2.500-2.500" />,
  sun: <><circle cx="8" cy="8" r="2.600" /><path d="M8 1.500v1.600M8 12.900v1.600M1.500 8h1.600M12.900 8h1.600M3.400 3.400l1.100 1.100M11.500 11.500l1.100 1.100M12.600 3.400l-1.100 1.100M4.500 11.500l-1.100 1.100" /></>,
  moon: <path d="M12.500 9.500A5 5 0 1 1 6.500 3.500a4 4 0 0 0 6 6z" />,
  auto: <><circle cx="8" cy="8" r="5.500" /><path d="M8 2.500a5.500 5.500 0 0 0 0 11z" fill="currentColor" /></>,
  pulse: <path d="M1.500 8h3l2-5 3 10 2-5h3" />,
  refresh: <path d="M13 8a5 5 0 1 1-1.500-3.500M13 2.500v3h-3" />,
  close: <path d="M4 4l8 8M12 4l-8 8" />,
  lock: <><rect x="3.500" y="7.500" width="9" height="6" rx="1" /><path d="M5.500 7.500v-2a2.500 2.500 0 0 1 5 0v2" /></>,
  clock: <><circle cx="8" cy="8" r="5.800" /><path d="M8 4.600V8l2.400 1.500" /></>,
  star: <path d="m8 2 1.800 3.800 4.200.5-3.100 2.900.8 4.100L8 11.200 4.300 13.300l.8-4.100L2 6.300l4.200-.5z" />,
  chevron: <path d="m6 4 4 4-4 4" />,
  first: <path d="M11 3 6 8l5 5M4.500 3v10" />,
  prev: <path d="M10 3 5 8l5 5" />,
  next: <path d="m6 3 5 5-5 5" />,
  last: <path d="m5 3 5 5-5 5M11.500 3v10" />,
  copy: <><rect x="5.500" y="5.500" width="8" height="8" rx="1" /><path d="M3 10.500V3.500a.5.5 0 0 1 .5-.5h7" /></>,
  warn: <path d="M8 2.500 14.500 13.500h-13zM8 6.500v3.500M8 12h.01" />,
  check: <path d="m3.500 8.500 3 3 6-7" />,
  filter: <path d="M2 3.500h12L9.500 9v4l-3-1.500V9z" />,
  play: <path d="M5 3.500v9l7.500-4.500z" fill="currentColor" />,
  stop: <rect x="4" y="4" width="8" height="8" rx="1" fill="currentColor" />,
  link: <path d="M6.500 9.500l3-3M5 8l-1.200 1.200a2.400 2.400 0 0 0 3.400 3.400L8.500 11.500M11 8l1.200-1.200a2.400 2.400 0 0 0-3.400-3.400L7.500 4.500" />,
  info: <><circle cx="8" cy="8" r="5.800" /><path d="M8 7.200v3.600M8 5h.01" /></>,
  diagram: <><rect x="6" y="2" width="4" height="3.500" rx=".6" /><rect x="1.500" y="10.500" width="4" height="3.500" rx=".6" /><rect x="10.500" y="10.500" width="4" height="3.500" rx=".6" /><path d="M8 5.500V8M3.500 10.500V8h9v2.500" /></>,
  export: <path d="M8 10V2.500M5 5l3-2.500L11 5M2.500 10v3h11v-3" />,
  import: <path d="M8 2.500V10M5 7.500 8 10l3-2.500M2.500 10v3h11v-3" />,
  tools: <path d="M10.500 2.500a3 3 0 0 0-2.800 4L2.500 11.700l1.800 1.800 5.200-5.200a3 3 0 0 0 4-2.800l-1.900 1.900-2-1-1-2z" />,
  sql: <path d="M5.500 4 2 8l3.500 4M10.500 4 14 8l-3.500 4M9 3 7 13" />,
  terminal: <><rect x="1.500" y="3" width="13" height="10" rx="1" /><path d="M4.500 6.500 7 8.500l-2.500 2M8.500 10.500h3" /></>,
  compare: <path d="M3 5h9M9.500 2.500 12 5 9.500 7.500M13 11H4M6.500 8.500 4 11l2.500 2.500" />,
  properties: <><rect x="2" y="2.500" width="12" height="11" rx="1" /><path d="M2 5.800h12M5 8.500h6M5 11h4" /></>,
  grid: <><rect x="2" y="3" width="12" height="10" rx="1" /><path d="M2 6.400h12M2 9.700h12M6 3v10M10 3v10" /></>,
  eraser: <path d="m9.500 3 3.500 3.500-5.500 5.500H4.500L2.500 10zM6 6.500l3.500 3.500M7 13h6.500" />,
  settings: <><path d="M2 4h6M11.500 4H14M2 8h2.500M7.500 8H14M2 12h6.500M11.500 12H14" /><circle cx="9.500" cy="4" r="1.500" /><circle cx="6" cy="8" r="1.500" /><circle cx="10" cy="12" r="1.500" /></>,
  save: <path d="M3 2.500h7.500L13.500 5.500V13a.500.500 0 0 1-.500.500H3a.500.500 0 0 1-.500-.500V3A.500.500 0 0 1 3 2.500zM5.500 2.500v3h4.500v-3M5 13.500V9h6v4.500" />,
  sort: <path d="M4.500 3v10M2.500 11l2 2 2-2M11.500 13V3M9.500 5l2-2 2 2" />,
  home: <path d="M2 8.200 8 2.800l6 5.400M3.600 7v6.500h3.200V10h2.400v3.500h3.200V7" />,
  preview: <><path d="M1.500 8S4 3.500 8 3.500 14.500 8 14.500 8 12 12.500 8 12.500 1.500 8 1.500 8z" /><path d="M6.600 6.200 10 8l-3.400 1.800z" fill="currentColor" /></>,
  search: <><circle cx="7" cy="7" r="4.200" /><path d="m10.200 10.200 3.300 3.300" /></>,
};

export type IconName = keyof typeof PATHS;

export default function Icon({ name, size = 14, className = "" }: { name: IconName; size?: number; className?: string }) {
  return (
    <svg className={`icon icon-${name} ${className}`} width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor"
         strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" focusable="false">
      {PATHS[name]}
    </svg>
  );
}
