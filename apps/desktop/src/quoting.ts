// How the active connection quotes names: backticks (MySQL, MariaDB, SQLite) or double quotes (PostgreSQL).
let double = false;
export const setDoubleQuotes = (on: boolean) => { double = on; };
export const quoteIdent = (n: string) => (double ? `"${n.replace(/"/g, '""')}"` : "`" + n.replace(/`/g, "``") + "`");
