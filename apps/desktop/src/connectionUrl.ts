import type { SslMode } from "./api";

export interface ParsedUrl { engine: "mysql" | "postgres"; host: string; port: number; database: string | null; user: string | null; password: string | null; ssl: SslMode | null }

/**
 * Read a connection URL: `jdbc:mysql://host:3306/db`, `mysql://user:pass@host/db?ssl-mode=REQUIRED`, `mariadb://…`,
 * `postgres://user:pass@host:5432/db?sslmode=verify-full` or `postgresql://…`.
 * Returns null when the text is not a usable URL (so the form can say so instead of guessing).
 */
export function parseConnectionUrl(text: string): ParsedUrl | null {
  let t = text.trim().replace(/^jdbc:/i, "");
  if (!/^(mysql|mariadb|postgres|postgresql):\/\//i.test(t)) return null;
  const engine = /^postgres/i.test(t) ? "postgres" : "mysql";
  // one scheme for the parser: it only needs the parts after it
  t = t.replace(/^[a-z]+:/i, "mysql:");
  let u: URL;
  try { u = new URL(t); } catch { return null; }
  if (!u.hostname) return null;
  const port = u.port ? Number(u.port) : engine === "postgres" ? 5432 : 3306;
  if (!Number.isInteger(port) || port < 1 || port > 65535) return null;
  const q = (k: string) => [...u.searchParams.entries()].find(([n]) => n.toLowerCase() === k)?.[1]?.toLowerCase();
  const mode = q("sslmode") ?? q("ssl-mode") ?? q("usessl") ?? q("requiressl") ?? "";
  // PostgreSQL's words mean exactly what they say: `require` encrypts without checking the certificate
  const ssl: SslMode | null = engine === "postgres"
    ? mode === "require" ? "skipverify" : mode === "verify-ca" ? "verifyca" : mode === "verify-full" ? "required" : null
    : ["required", "verify_ca", "verify_identity", "true"].includes(mode) ? "required" : null;
  const db = decodeURIComponent(u.pathname.replace(/^\//, ""));
  return {
    engine, host: u.hostname.replace(/^\[|\]$/g, ""), port, database: db || null,
    user: u.username ? decodeURIComponent(u.username) : null,
    password: u.password ? decodeURIComponent(u.password) : null,
    ssl,
  };
}

/** The JDBC-style URL shown for the host form (display only; the password is never part of it). */
export const formatConnectionUrl = (host: string, port: number, database: string | null, engine?: string) => `${engine === "postgres" ? "postgresql" : "jdbc:mysql"}://${host}:${port}/${database ?? ""}`;
