/** Starting points for common statements, inserted into the editor from the command palette ("Snippet: …"). */
export const SNIPPETS: { label: string; sql: string }[] = [
  { label: "SELECT with filter and order", sql: "SELECT *\nFROM table_name\nWHERE column_name = 'value'\nORDER BY id DESC\nLIMIT 100;\n" },
  { label: "INNER JOIN", sql: "SELECT a.*, b.*\nFROM table_a a\nJOIN table_b b ON b.a_id = a.id\nWHERE a.id = 1;\n" },
  { label: "GROUP BY with count", sql: "SELECT column_name, COUNT(*) AS total\nFROM table_name\nGROUP BY column_name\nORDER BY total DESC;\n" },
  { label: "INSERT", sql: "INSERT INTO table_name (column_a, column_b)\nVALUES ('a', 'b');\n" },
  { label: "UPDATE (with WHERE)", sql: "UPDATE table_name\nSET column_a = 'value'\nWHERE id = 1;\n" },
  { label: "DELETE (with WHERE)", sql: "DELETE FROM table_name\nWHERE id = 1;\n" },
  { label: "CREATE TABLE", sql: "CREATE TABLE table_name (\n  id BIGINT UNSIGNED NOT NULL AUTO_INCREMENT,\n  name VARCHAR(255) NOT NULL,\n  created_at TIMESTAMP NULL DEFAULT CURRENT_TIMESTAMP,\n  PRIMARY KEY (id)\n) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;\n" },
  { label: "ALTER TABLE add column", sql: "ALTER TABLE table_name\n  ADD COLUMN column_name VARCHAR(255) NULL AFTER id;\n" },
  { label: "CREATE INDEX", sql: "CREATE INDEX idx_name ON table_name (column_name);\n" },
  { label: "WITH (common table expression)", sql: "WITH recent AS (\n  SELECT * FROM table_name WHERE created_at > NOW() - INTERVAL 7 DAY\n)\nSELECT COUNT(*) FROM recent;\n" },
  { label: "EXPLAIN a query", sql: "EXPLAIN FORMAT=TRADITIONAL\nSELECT * FROM table_name WHERE id = 1;\n" },
  { label: "Table sizes in a database", sql: "SELECT TABLE_NAME, TABLE_ROWS, ROUND((DATA_LENGTH + INDEX_LENGTH) / 1024 / 1024, 2) AS size_mb\nFROM information_schema.TABLES\nWHERE TABLE_SCHEMA = DATABASE()\nORDER BY size_mb DESC;\n" },
  { label: "Duplicate rows by column", sql: "SELECT column_name, COUNT(*) AS n\nFROM table_name\nGROUP BY column_name\nHAVING n > 1;\n" },
];
