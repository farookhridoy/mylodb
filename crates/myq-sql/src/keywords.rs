//! Static catalogs for autocomplete: keywords, functions, reserved words.

/// Keywords offered while typing (ordered roughly by how often they are needed).
pub const KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "AND", "OR", "NOT", "IN", "EXISTS", "BETWEEN", "LIKE", "REGEXP", "IS", "NULL", "AS", "DISTINCT",
    "JOIN", "INNER JOIN", "LEFT JOIN", "RIGHT JOIN", "CROSS JOIN", "ON", "USING", "GROUP BY", "ORDER BY", "HAVING", "LIMIT", "OFFSET",
    "ASC", "DESC", "UNION", "UNION ALL", "INSERT INTO", "VALUES", "UPDATE", "SET", "DELETE FROM", "REPLACE INTO", "ON DUPLICATE KEY UPDATE",
    "CASE", "WHEN", "THEN", "ELSE", "END", "WITH", "OVER", "PARTITION BY", "CREATE TABLE", "CREATE DATABASE", "CREATE INDEX", "CREATE VIEW",
    "ALTER TABLE", "DROP TABLE", "DROP DATABASE", "TRUNCATE TABLE", "RENAME TABLE", "SHOW TABLES", "SHOW DATABASES", "SHOW COLUMNS FROM",
    "DESCRIBE", "EXPLAIN", "USE", "CALL", "BEGIN", "START TRANSACTION", "COMMIT", "ROLLBACK", "GRANT", "REVOKE", "TRUE", "FALSE", "FOR UPDATE",
    "IF", "IF EXISTS", "IF NOT EXISTS", "PRIMARY KEY", "FOREIGN KEY", "REFERENCES", "UNIQUE", "DEFAULT", "AUTO_INCREMENT", "CONSTRAINT",
    "ADD COLUMN", "DROP COLUMN", "MODIFY COLUMN", "ANALYZE TABLE", "OPTIMIZE TABLE", "CHECK TABLE", "REPAIR TABLE", "FLUSH", "KILL",
];

/// Statement starters, offered at the beginning of a statement.
pub const STATEMENT_STARTERS: &[&str] = &[
    "SELECT", "INSERT INTO", "UPDATE", "DELETE FROM", "REPLACE INTO", "CREATE TABLE", "CREATE DATABASE", "CREATE INDEX", "CREATE VIEW",
    "ALTER TABLE", "DROP TABLE", "DROP DATABASE", "TRUNCATE TABLE", "RENAME TABLE", "SHOW", "DESCRIBE", "EXPLAIN", "USE", "SET", "WITH",
    "CALL", "START TRANSACTION", "BEGIN", "COMMIT", "ROLLBACK", "GRANT", "REVOKE", "LOCK TABLES", "UNLOCK TABLES", "ANALYZE TABLE",
    "OPTIMIZE TABLE", "CHECK TABLE", "REPAIR TABLE", "FLUSH", "KILL",
];

/// Offered after a complete table reference (`FROM t |`).
pub const AFTER_TABLE: &[&str] = &[
    "WHERE", "JOIN", "INNER JOIN", "LEFT JOIN", "RIGHT JOIN", "CROSS JOIN", "ON", "USING", "GROUP BY", "ORDER BY", "HAVING", "LIMIT", "UNION", "AS",
    "SET", "VALUES", "FOR UPDATE",
];

/// (name, signature hint). Inserted as `NAME(`.
pub const FUNCTIONS: &[(&str, &str)] = &[
    ("COUNT", "COUNT(expr)"), ("SUM", "SUM(expr)"), ("AVG", "AVG(expr)"), ("MIN", "MIN(expr)"), ("MAX", "MAX(expr)"),
    ("GROUP_CONCAT", "GROUP_CONCAT(expr SEPARATOR sep)"), ("JSON_ARRAYAGG", "JSON_ARRAYAGG(expr)"), ("JSON_OBJECTAGG", "JSON_OBJECTAGG(key, value)"),
    ("CONCAT", "CONCAT(a, b, …)"), ("CONCAT_WS", "CONCAT_WS(sep, a, b, …)"), ("SUBSTRING", "SUBSTRING(str, pos, len)"), ("SUBSTRING_INDEX", "SUBSTRING_INDEX(str, delim, count)"),
    ("LENGTH", "LENGTH(str)"), ("CHAR_LENGTH", "CHAR_LENGTH(str)"), ("UPPER", "UPPER(str)"), ("LOWER", "LOWER(str)"), ("TRIM", "TRIM(str)"),
    ("LTRIM", "LTRIM(str)"), ("RTRIM", "RTRIM(str)"), ("REPLACE", "REPLACE(str, from, to)"), ("REVERSE", "REVERSE(str)"), ("LEFT", "LEFT(str, len)"),
    ("RIGHT", "RIGHT(str, len)"), ("LPAD", "LPAD(str, len, pad)"), ("RPAD", "RPAD(str, len, pad)"), ("INSTR", "INSTR(str, sub)"), ("LOCATE", "LOCATE(sub, str)"),
    ("FORMAT", "FORMAT(n, decimals)"), ("REPEAT", "REPEAT(str, count)"), ("SPACE", "SPACE(n)"), ("HEX", "HEX(x)"), ("UNHEX", "UNHEX(str)"),
    ("MD5", "MD5(str)"), ("SHA1", "SHA1(str)"), ("SHA2", "SHA2(str, bits)"), ("UUID", "UUID()"), ("UUID_TO_BIN", "UUID_TO_BIN(uuid)"), ("BIN_TO_UUID", "BIN_TO_UUID(bin)"),
    ("ABS", "ABS(n)"), ("CEIL", "CEIL(n)"), ("FLOOR", "FLOOR(n)"), ("ROUND", "ROUND(n, decimals)"), ("TRUNCATE", "TRUNCATE(n, decimals)"), ("MOD", "MOD(a, b)"),
    ("POW", "POW(x, y)"), ("SQRT", "SQRT(n)"), ("RAND", "RAND()"), ("SIGN", "SIGN(n)"), ("GREATEST", "GREATEST(a, b, …)"), ("LEAST", "LEAST(a, b, …)"),
    ("NOW", "NOW()"), ("CURDATE", "CURDATE()"), ("CURTIME", "CURTIME()"), ("CURRENT_TIMESTAMP", "CURRENT_TIMESTAMP"), ("UTC_TIMESTAMP", "UTC_TIMESTAMP()"),
    ("DATE", "DATE(expr)"), ("TIME", "TIME(expr)"), ("YEAR", "YEAR(date)"), ("MONTH", "MONTH(date)"), ("DAY", "DAY(date)"), ("HOUR", "HOUR(time)"),
    ("MINUTE", "MINUTE(time)"), ("SECOND", "SECOND(time)"), ("DAYOFWEEK", "DAYOFWEEK(date)"), ("WEEK", "WEEK(date)"), ("LAST_DAY", "LAST_DAY(date)"),
    ("DATE_ADD", "DATE_ADD(date, INTERVAL n unit)"), ("DATE_SUB", "DATE_SUB(date, INTERVAL n unit)"), ("DATE_FORMAT", "DATE_FORMAT(date, format)"),
    ("DATEDIFF", "DATEDIFF(a, b)"), ("TIMESTAMPDIFF", "TIMESTAMPDIFF(unit, a, b)"), ("TIMESTAMPADD", "TIMESTAMPADD(unit, n, ts)"), ("STR_TO_DATE", "STR_TO_DATE(str, format)"),
    ("UNIX_TIMESTAMP", "UNIX_TIMESTAMP(date)"), ("FROM_UNIXTIME", "FROM_UNIXTIME(ts)"), ("EXTRACT", "EXTRACT(unit FROM date)"),
    ("IF", "IF(cond, a, b)"), ("IFNULL", "IFNULL(a, b)"), ("NULLIF", "NULLIF(a, b)"), ("COALESCE", "COALESCE(a, b, …)"), ("CAST", "CAST(expr AS type)"), ("CONVERT", "CONVERT(expr, type)"),
    ("JSON_EXTRACT", "JSON_EXTRACT(doc, path)"), ("JSON_UNQUOTE", "JSON_UNQUOTE(json)"), ("JSON_OBJECT", "JSON_OBJECT(key, value, …)"), ("JSON_ARRAY", "JSON_ARRAY(a, b, …)"),
    ("JSON_SET", "JSON_SET(doc, path, val)"), ("JSON_CONTAINS", "JSON_CONTAINS(doc, val)"), ("JSON_LENGTH", "JSON_LENGTH(doc)"), ("JSON_TABLE", "JSON_TABLE(doc, path COLUMNS (…))"),
    ("ROW_NUMBER", "ROW_NUMBER() OVER (…)"), ("RANK", "RANK() OVER (…)"), ("DENSE_RANK", "DENSE_RANK() OVER (…)"), ("LAG", "LAG(expr, n) OVER (…)"), ("LEAD", "LEAD(expr, n) OVER (…)"),
    ("FIRST_VALUE", "FIRST_VALUE(expr) OVER (…)"), ("NTILE", "NTILE(n) OVER (…)"),
    ("DATABASE", "DATABASE()"), ("USER", "USER()"), ("VERSION", "VERSION()"), ("CONNECTION_ID", "CONNECTION_ID()"), ("LAST_INSERT_ID", "LAST_INSERT_ID()"), ("ROW_COUNT", "ROW_COUNT()"),
    ("FOUND_ROWS", "FOUND_ROWS()"), ("SLEEP", "SLEEP(seconds)"), ("INET_ATON", "INET_ATON(ip)"), ("INET_NTOA", "INET_NTOA(n)"), ("ST_DISTANCE_SPHERE", "ST_DISTANCE_SPHERE(a, b)"),
];

/// Words that must be backtick-quoted when used as identifiers.
const RESERVED: &[&str] = &[
    "ADD", "ALL", "ALTER", "ANALYZE", "AND", "AS", "ASC", "BEFORE", "BETWEEN", "BY", "CASE", "CHECK", "COLUMN", "CONDITION", "CONSTRAINT", "CREATE", "CROSS",
    "CURRENT_DATE", "CURRENT_TIME", "CURRENT_TIMESTAMP", "CURRENT_USER", "DATABASE", "DATABASES", "DEFAULT", "DELETE", "DESC", "DESCRIBE", "DISTINCT", "DROP",
    "EACH", "ELSE", "EXISTS", "EXPLAIN", "FALSE", "FOR", "FOREIGN", "FROM", "FULLTEXT", "GRANT", "GROUP", "HAVING", "IF", "IGNORE", "IN", "INDEX", "INNER",
    "INSERT", "INTERVAL", "INTO", "IS", "JOIN", "KEY", "KEYS", "KILL", "LEFT", "LIKE", "LIMIT", "LOCK", "MATCH", "NATURAL", "NOT", "NULL", "ON", "OPTION",
    "OR", "ORDER", "OUTER", "PRIMARY", "PROCEDURE", "RANGE", "READ", "REFERENCES", "REGEXP", "RENAME", "REPLACE", "REQUIRE", "RIGHT", "SCHEMA", "SELECT",
    "SET", "SHOW", "TABLE", "THEN", "TO", "TRIGGER", "TRUE", "UNION", "UNIQUE", "UNLOCK", "UPDATE", "USAGE", "USE", "USING", "VALUES", "WHEN", "WHERE", "WITH",
    "WRITE", "XOR",
];

pub fn is_reserved(word: &str) -> bool {
    RESERVED.binary_search(&word.to_uppercase().as_str()).is_ok()
}

/// Words that can follow a table name without being its alias.
pub fn ends_table_ref(word: &str) -> bool {
    matches!(
        word.to_uppercase().as_str(),
        "ON" | "USING" | "WHERE" | "SET" | "LEFT" | "RIGHT" | "INNER" | "OUTER" | "CROSS" | "JOIN" | "NATURAL" | "GROUP" | "ORDER" | "HAVING" | "LIMIT"
            | "UNION" | "VALUES" | "SELECT" | "STRAIGHT_JOIN" | "FORCE" | "USE" | "IGNORE" | "PARTITION" | "FOR" | "WINDOW" | "FROM" | "INTO" | "AS"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_list_is_sorted_for_binary_search() {
        let mut sorted = RESERVED.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, RESERVED);
        assert!(is_reserved("select") && is_reserved("Order") && !is_reserved("name"));
    }
}
