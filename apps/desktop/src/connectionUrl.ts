import type { SslMode } from "./api";

export interface ParsedUrl { host: string; port: number; database: string | null; user: string | null; password: string | null; ssl: SslMode | null }

/**
 * Read a MySQL connection URL: `jdbc:mysql://host:3306/db`, `mysql://user:pass@host/db?ssl-mode=REQUIRED`, `mariadb://…`.
 * Returns null when the text is not a usable URL (so the form can say so instead of guessing).
 */
export function parseConnectionUrl(text: string): ParsedUrl | null {
  let t = text.trim().replace(/^jdbc:/i, "");
  if (!/^(mysql|mariadb):\/\//i.test(t)) return null;
  t = t.replace(/^mariadb:/i, "mysql:");
  let u: URL;
  try { u = new URL(t); } catch { return null; }
  if (!u.hostname) return null;
  const port = u.port ? Number(u.port) : 3306;
  if (!Number.isInteger(port) || port < 1 || port > 65535) return null;
  const q = (k: string) => [...u.searchParams.entries()].find(([n]) => n.toLowerCase() === k)?.[1]?.toLowerCase();
  const wantsSsl = ["required", "verify_ca", "verify_identity", "true"].includes(q("sslmode") ?? q("ssl-mode") ?? q("usessl") ?? q("requiressl") ?? "");
  const db = decodeURIComponent(u.pathname.replace(/^\//, ""));
  return {
    host: u.hostname.replace(/^\[|\]$/g, ""), port, database: db || null,
    user: u.username ? decodeURIComponent(u.username) : null,
    password: u.password ? decodeURIComponent(u.password) : null,
    ssl: wantsSsl ? "required" : null,
  };
}

/** The JDBC-style URL shown for the host form (display only; the password is never part of it). */
export const formatConnectionUrl = (host: string, port: number, database: string | null) => `jdbc:mysql://${host}:${port}/${database ?? ""}`;
