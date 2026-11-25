//! The AI query writer: turns a plain-language request into SQL by asking a chat model that speaks the common
//! "OpenAI chat completions" protocol (Ollama on this computer, Groq, OpenRouter, Google Gemini and others, several with free tiers).
//!
//! What is sent: the request text and the *names* of the tables and columns (and foreign keys) of the one database in use.
//! Never rows or values. Nothing is sent until the user has set a service up and confirmed it, and the answer is only ever placed
//! in the editor; it is not run.

use myq_sql::Catalog;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct AiSettings {
    /// Base URL, for example `http://localhost:11434/v1`; `/chat/completions` is added.
    pub endpoint: String,
    pub model: String,
}

/// The endpoint must be https, except on this computer (a key sent over plain http to another machine would be readable on the way).
pub fn check_endpoint(endpoint: &str) -> Result<String, String> {
    let e = endpoint.trim().trim_end_matches('/');
    let rest = e.strip_prefix("https://").map(|r| (true, r)).or_else(|| e.strip_prefix("http://").map(|r| (false, r)));
    let Some((secure, rest)) = rest else { return Err("The address must start with https:// (or http:// for a model on this computer).".into()) };
    let host = rest.split(['/', ':']).next().unwrap_or("");
    if host.is_empty() {
        return Err("The address has no host name.".into());
    }
    if !secure && !matches!(host, "localhost" | "127.0.0.1" | "[::1]") {
        return Err("Plain http is only allowed for a model on this computer (localhost). Use https:// for other services.".into());
    }
    Ok(e.to_string())
}

/// The schema as compact text: `table(col, col, …)` per line, `a.x -> b.y` for foreign keys. Capped so a huge database still fits.
pub fn schema_text(cat: &Catalog, db: &str) -> String {
    const MAX_TABLES: usize = 80;
    const MAX_COLUMNS: usize = 40;
    let Some(tables) = cat.tables.iter().find(|(k, _)| k.eq_ignore_ascii_case(db)).map(|(_, v)| v) else { return String::new() };
    let mut out = String::new();
    for t in tables.iter().take(MAX_TABLES) {
        let cols: Vec<&str> = t.columns.iter().take(MAX_COLUMNS).map(String::as_str).collect();
        let more = if t.columns.len() > MAX_COLUMNS { ", …" } else { "" };
        out.push_str(&format!("{}{}({}{})\n", t.name, if t.is_view { " [view]" } else { "" }, cols.join(", "), more));
    }
    if tables.len() > MAX_TABLES {
        out.push_str(&format!("(… and {} more tables)\n", tables.len() - MAX_TABLES));
    }
    let fks: Vec<String> = cat
        .fks
        .iter()
        .filter(|f| f.db.eq_ignore_ascii_case(db))
        .map(|f| format!("{}({}) -> {}({})", f.table, f.columns.join(", "), f.ref_table, f.ref_columns.join(", ")))
        .collect();
    if !fks.is_empty() {
        out.push_str("Foreign keys:\n");
        for f in fks.iter().take(150) {
            out.push_str(f);
            out.push('\n');
        }
    }
    out
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'a str,
    content: String,
}

pub fn messages(dialect: &str, db: &str, schema: &str, request: &str) -> Vec<(&'static str, String)> {
    let system = format!(
        "You write {dialect} SQL for the database `{db}`. Use only the tables and columns listed. Answer with the SQL only, in one code block, \
         no explanation. Prefer a SELECT with a LIMIT unless the request clearly asks to change data. Never invent names that are not listed."
    );
    let user = format!("Schema of `{db}`:\n{schema}\nRequest: {request}");
    vec![("system", system), ("user", user)]
}

/// The SQL inside the model's answer: the first code block if there is one, otherwise the whole text.
pub fn extract_sql(answer: &str) -> String {
    let a = answer.trim();
    if let Some(start) = a.find("```") {
        let after = &a[start + 3..];
        // skip the language tag on the first line (```sql)
        let body = after.split_once('\n').map_or(after, |(first, rest)| if first.trim().chars().all(|c| c.is_ascii_alphabetic()) { rest } else { after });
        let end = body.find("```").unwrap_or(body.len());
        return body[..end].trim().to_string();
    }
    a.to_string()
}

pub async fn ask(settings: &AiSettings, key: Option<&str>, msgs: Vec<(&'static str, String)>) -> Result<String, String> {
    let base = check_endpoint(&settings.endpoint)?;
    if settings.model.trim().is_empty() {
        return Err("Choose a model in Preferences, AI assistant.".into());
    }
    let body = serde_json::json!({
        "model": settings.model.trim(),
        "temperature": 0.1,
        "stream": false,
        "messages": msgs.iter().map(|(r, c)| Message { role: r, content: c.clone() }).collect::<Vec<_>>(),
    });
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(90)).build().map_err(|e| e.to_string())?;
    let mut req = client.post(format!("{base}/chat/completions")).json(&body);
    if let Some(k) = key.filter(|k| !k.is_empty()) {
        req = req.bearer_auth(k);
    }
    let resp = req.send().await.map_err(|e| {
        if e.is_connect() { format!("Could not reach {base}. Is the service running? ({e})") } else if e.is_timeout() { "The model took too long to answer (90 seconds).".to_string() } else { e.to_string() }
    })?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let detail = serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| v["error"]["message"].as_str().or(v["error"].as_str()).map(String::from)).unwrap_or_else(|| text.chars().take(300).collect());
        return Err(match status.as_u16() {
            401 | 403 => format!("The service refused the key ({status}). Check the key in Preferences, AI assistant. {detail}"),
            404 => format!("Not found ({status}): check the address and the model name. {detail}"),
            429 => format!("Too many requests or the free limit is used up ({status}). Wait a bit, or pick another model. {detail}"),
            _ => format!("The service answered {status}: {detail}"),
        });
    }
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| "The service answered with something that is not JSON.".to_string())?;
    let content = v["choices"][0]["message"]["content"].as_str().ok_or("The answer had no text in it.")?;
    let sql = extract_sql(content);
    if sql.is_empty() { Err("The model answered with nothing. Try again, or word the request differently.".into()) } else { Ok(sql) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use myq_schema::filter::{Kind, Node};

    #[test]
    fn endpoints_must_be_https_or_local() {
        assert_eq!(check_endpoint("https://api.groq.com/openai/v1/").unwrap(), "https://api.groq.com/openai/v1");
        assert!(check_endpoint("http://localhost:11434/v1").is_ok());
        assert!(check_endpoint("http://127.0.0.1:1234/v1").is_ok());
        assert!(check_endpoint("http://example.com/v1").is_err());
        assert!(check_endpoint("ftp://x").is_err());
        assert!(check_endpoint("https://").is_err());
    }

    #[test]
    fn the_sql_is_taken_out_of_the_answer() {
        assert_eq!(extract_sql("```sql\nSELECT 1;\n```"), "SELECT 1;");
        assert_eq!(extract_sql("Here you go:\n```\nSELECT 2\n```\nHope it helps"), "SELECT 2");
        assert_eq!(extract_sql("SELECT 3"), "SELECT 3");
        assert_eq!(extract_sql("```mysql\nSELECT * FROM t\nWHERE a = 1\n```"), "SELECT * FROM t\nWHERE a = 1");
    }

    #[test]
    fn the_schema_text_lists_tables_columns_and_keys_only() {
        let n = |k, name: &str, parent: Option<&str>| Node { database: "shop".into(), kind: k, name: name.into(), parent: parent.map(String::from) };
        let mut c = Catalog::from_nodes(&[n(Kind::Database, "shop", None), n(Kind::Table, "orders", None), n(Kind::Column, "id", Some("orders")), n(Kind::Column, "total", Some("orders"))]);
        c.fks.push(myq_sql::Fk { name: "f".into(), db: "shop".into(), table: "orders".into(), columns: vec!["customer_id".into()], ref_db: "shop".into(), ref_table: "customers".into(), ref_columns: vec!["id".into()] });
        let t = schema_text(&c, "shop");
        assert!(t.contains("orders(id, total)") && t.contains("orders(customer_id) -> customers(id)"), "{t}");
        assert_eq!(schema_text(&c, "nope"), "");
    }
}
