//! The AI query writer: turns a plain-language request into SQL by asking a chat model that speaks the common
//! "OpenAI chat completions" protocol (Ollama on this computer, Groq, OpenRouter, Google Gemini and others, several with free tiers).
//!
//! What is sent: the request text and the *names* of the tables and columns (and foreign keys) of the one database in use.
//! Never rows or values. Nothing is sent until the user has set a service up and confirmed it, and the answer is only ever placed
//! in the editor; it is not run.

use myq_sql::Catalog;
use serde::{Deserialize, Serialize};

/// A model the user may use, with its context window (tokens) when the service tells us.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelRef {
    pub id: String,
    #[serde(default)]
    pub context: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AiSettings {
    /// Which service: `groq`, `openrouter`, `gemini`, `ollama`, `anthropic` or `custom`. Decides how the key is kept and the protocol.
    pub preset: String,
    /// Base URL, for example `http://localhost:11434/v1`; `/chat/completions` is added (Claude: `https://api.anthropic.com`).
    pub endpoint: String,
    pub model: String,
    /// Other models to try, in order, when the chosen one is too small for the question, busy or out of its free quota.
    #[serde(default)]
    pub fallbacks: Vec<ModelRef>,
    /// The chosen model's own context window, when known.
    #[serde(default)]
    pub context: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub context: Option<u64>,
    pub free: bool,
}

/// Where each service's key is kept (the id is only a place in the secret store).
pub fn key_id(preset: &str) -> i64 {
    match preset { "groq" => 1, "openrouter" => 2, "gemini" => 3, "anthropic" => 4, "custom" => 5, _ => 0 }
}

pub fn is_anthropic(preset: &str) -> bool { preset == "anthropic" }

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

/// The conversation for the model: the rules and the schema first, then the recent turns, then the new request.
pub fn messages(dialect: &str, db: &str, schema: &str, history: &[Turn], request: &str) -> Vec<(String, String)> {
    let system = format!(
        "You write {dialect} SQL for the database `{db}`. Use only the tables and columns listed below. Put the SQL in one code block, followed by at most two short \
         sentences of explanation. Prefer a SELECT with a LIMIT unless the request clearly asks to change data. Never invent names that are not listed. \
         If the request cannot be answered with these tables, say so instead of guessing.\n\nSchema of `{db}`:\n{schema}"
    );
    let mut out = vec![("system".to_string(), system)];
    for t in history.iter().rev().take(6).collect::<Vec<_>>().into_iter().rev() {
        if t.role == "user" || t.role == "assistant" { out.push((t.role.clone(), t.content.clone())); }
    }
    out.push(("user".to_string(), request.to_string()));
    out
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

/// One step of the conversation sent to the model.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Turn {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Answer {
    pub text: String,
    pub sql: Option<String>,
    /// The model that actually answered.
    pub model: String,
    /// Set when another model than the chosen one answered, and why.
    pub note: Option<String>,
}

/// A failure that another model may not have: too much text for the window, a busy or limited service, a model that is gone.
#[derive(Debug)]
struct Failure {
    message: String,
    switchable: bool,
}

fn rough_tokens(text: &str) -> u64 { (text.chars().count() as u64) / 3 + 1 }

fn classify(status: u16, body: &str) -> bool {
    let b = body.to_lowercase();
    matches!(status, 408 | 413 | 429 | 500 | 502 | 503 | 504 | 529)
        || (matches!(status, 400 | 404 | 422) && (b.contains("context") || b.contains("too long") || b.contains("maximum") || b.contains("token") || b.contains("not found") || b.contains("no endpoints") || b.contains("does not exist") || b.contains("unavailable")))
}

fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder().timeout(std::time::Duration::from_secs(90)).build().map_err(|e| e.to_string())
}

fn explain_status(status: reqwest::StatusCode, text: &str, base: &str) -> Failure {
    let detail = serde_json::from_str::<serde_json::Value>(text).ok().and_then(|v| v["error"]["message"].as_str().or(v["error"].as_str()).map(String::from)).unwrap_or_else(|| text.chars().take(300).collect());
    let message = match status.as_u16() {
        401 | 403 => format!("The service refused the key ({status}). Connect again in the AI settings. {detail}"),
        404 => format!("Not found ({status}): check the address and the model name. {detail}"),
        429 => format!("Too many requests, or the free limit is used up ({status}). {detail}"),
        _ => format!("{base} answered {status}: {detail}"),
    };
    Failure { message, switchable: classify(status.as_u16(), text) }
}

fn net_failure(e: reqwest::Error, base: &str) -> Failure {
    let (message, switchable) = if e.is_connect() { (format!("Could not reach {base}. Is the service running? ({e})"), false) } else if e.is_timeout() { ("The model took too long to answer (90 seconds).".to_string(), true) } else { (e.to_string(), false) };
    Failure { message, switchable }
}

async fn call_one(settings: &AiSettings, model: &str, key: Option<&str>, turns: &[(String, String)]) -> Result<String, Failure> {
    let base = check_endpoint(&settings.endpoint).map_err(|m| Failure { message: m, switchable: false })?;
    let client = http().map_err(|m| Failure { message: m, switchable: false })?;
    if is_anthropic(&settings.preset) {
        let system = turns.iter().find(|(r, _)| r == "system").map(|(_, c)| c.clone()).unwrap_or_default();
        let msgs: Vec<serde_json::Value> = turns.iter().filter(|(r, _)| r != "system").map(|(r, c)| serde_json::json!({ "role": r, "content": c })).collect();
        let body = serde_json::json!({ "model": model, "max_tokens": 2000, "temperature": 0.1, "system": system, "messages": msgs });
        let mut req = client.post(format!("{base}/v1/messages")).header("anthropic-version", "2023-06-01").json(&body);
        if let Some(k) = key { req = req.header("x-api-key", k); }
        let resp = req.send().await.map_err(|e| net_failure(e, &base))?;
        let status = resp.status();
        let text = resp.text().await.map_err(|e| Failure { message: e.to_string(), switchable: false })?;
        if !status.is_success() { return Err(explain_status(status, &text, &base)); }
        let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| Failure { message: "The service answered with something that is not JSON.".into(), switchable: false })?;
        return v["content"][0]["text"].as_str().map(String::from).ok_or(Failure { message: "The answer had no text in it.".into(), switchable: true });
    }
    let body = serde_json::json!({
        "model": model, "temperature": 0.1, "stream": false,
        "messages": turns.iter().map(|(r, c)| Message { role: r, content: c.clone() }).collect::<Vec<_>>(),
    });
    let mut req = client.post(format!("{base}/chat/completions")).json(&body);
    if let Some(k) = key.filter(|k| !k.is_empty()) { req = req.bearer_auth(k); }
    let resp = req.send().await.map_err(|e| net_failure(e, &base))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| Failure { message: e.to_string(), switchable: false })?;
    if !status.is_success() { return Err(explain_status(status, &text, &base)); }
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| Failure { message: "The service answered with something that is not JSON.".into(), switchable: false })?;
    v["choices"][0]["message"]["content"].as_str().map(String::from).ok_or(Failure { message: "The answer had no text in it.".into(), switchable: true })
}

/// Ask the chosen model; when it cannot take the question (the text is longer than its window, the service is busy or its free
/// quota is gone) go on to the next model of the list, and say which one answered.
pub async fn chat(settings: &AiSettings, key: Option<&str>, turns: Vec<(String, String)>) -> Result<Answer, String> {
    if settings.model.trim().is_empty() {
        return Err("Choose a model in the AI settings.".into());
    }
    let need = turns.iter().map(|(_, c)| rough_tokens(c)).sum::<u64>() + 1200; // the question plus room for the answer
    let mut candidates: Vec<ModelRef> = vec![ModelRef { id: settings.model.trim().to_string(), context: settings.context }];
    for m in &settings.fallbacks {
        if !candidates.iter().any(|c| c.id == m.id) { candidates.push(m.clone()); }
    }
    let (mut skipped, mut last): (Vec<String>, Option<String>) = (vec![], None);
    for (i, m) in candidates.iter().enumerate() {
        if let Some(ctx) = m.context.filter(|c| *c < need) {
            skipped.push(format!("{} (window {} tokens, the question needs about {need})", m.id, ctx));
            continue;
        }
        match call_one(settings, &m.id, key, &turns).await {
            Ok(text) => {
                let note = (i > 0).then(|| match (&last, skipped.first()) {
                    (Some(why), _) => format!("Switched to {} because the first model failed: {why}", m.id),
                    (None, Some(_)) => format!("Switched to {} because the chosen model's window is too small for this question.", m.id),
                    _ => format!("Answered by {}.", m.id),
                });
                let sql = extract_sql(&text);
                let has_code = text.contains("```");
                let sql = if sql.is_empty() { None } else if has_code || looks_like_sql(&sql) { Some(sql) } else { None };
                return Ok(Answer { text: text.trim().to_string(), sql, model: m.id.clone(), note });
            }
            Err(f) if f.switchable && i + 1 < candidates.len() => last = Some(f.message),
            Err(f) => return Err(match (&last, skipped.is_empty()) { (Some(prev), _) => format!("{} (an earlier model failed too: {prev})", f.message), (None, false) => format!("{} (skipped: {})", f.message, skipped.join("; ")), _ => f.message }),
        }
    }
    Err(format!("No model could take this question. Skipped: {}. Pick a model with a larger window in the AI settings, or select a smaller database.", skipped.join("; ")))
}

fn looks_like_sql(t: &str) -> bool {
    let first = t.trim_start().split_whitespace().next().unwrap_or("").to_uppercase();
    ["SELECT", "WITH", "INSERT", "UPDATE", "DELETE", "REPLACE", "CREATE", "ALTER", "DROP", "SHOW", "EXPLAIN", "DESCRIBE"].contains(&first.as_str())
}

/// The models a service offers. For OpenRouter only the free ones unless `all` is set; chat-only models (no embeddings, speech, images).
pub async fn list_models(preset: &str, endpoint: &str, key: Option<&str>, all: bool) -> Result<Vec<ModelInfo>, String> {
    let base = check_endpoint(endpoint)?;
    let client = http()?;
    let v: serde_json::Value = if is_anthropic(preset) {
        let mut req = client.get(format!("{base}/v1/models?limit=100")).header("anthropic-version", "2023-06-01");
        if let Some(k) = key { req = req.header("x-api-key", k); }
        send_json(req, &base).await?
    } else {
        let mut req = client.get(format!("{base}/models"));
        if let Some(k) = key.filter(|k| !k.is_empty()) { req = req.bearer_auth(k); }
        send_json(req, &base).await?
    };
    let items = v["data"].as_array().or_else(|| v["models"].as_array()).ok_or("The service did not return a list of models.")?;
    let skip = ["embed", "aqa", "imagen", "veo", "tts", "whisper", "guard", "moderation", "transcribe", "audio", "image", "rerank"];
    let mut out: Vec<ModelInfo> = items
        .iter()
        .filter_map(|m| {
            let raw = m["id"].as_str().or_else(|| m["name"].as_str())?;
            let id = raw.strip_prefix("models/").unwrap_or(raw).to_string();
            let lower = id.to_lowercase();
            if skip.iter().any(|s| lower.contains(s)) { return None; }
            let context = m["context_length"].as_u64().or_else(|| m["context_window"].as_u64()).or_else(|| m["inputTokenLimit"].as_u64()).or_else(|| m["top_provider"]["context_length"].as_u64());
            let price0 = |k: &str| m["pricing"][k].as_str().map(|p| p.parse::<f64>().unwrap_or(1.0) == 0.0);
            let free = if preset == "openrouter" { id.ends_with(":free") || (price0("prompt") == Some(true) && price0("completion") == Some(true)) } else { !is_anthropic(preset) };
            let name = m["display_name"].as_str().or_else(|| m["name"].as_str()).unwrap_or(&id).to_string();
            Some(ModelInfo { id, name, context, free })
        })
        .collect();
    if preset == "openrouter" && !all { out.retain(|m| m.free); }
    out.sort_by(|a, b| (b.free, b.context.unwrap_or(0)).cmp(&(a.free, a.context.unwrap_or(0))).then(a.id.cmp(&b.id)));
    out.dedup_by(|a, b| a.id == b.id);
    Ok(out)
}

async fn send_json(req: reqwest::RequestBuilder, base: &str) -> Result<serde_json::Value, String> {
    let resp = req.send().await.map_err(|e| net_failure(e, base).message)?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() { return Err(explain_status(status, &text, base).message); }
    serde_json::from_str(&text).map_err(|_| "The service answered with something that is not JSON.".to_string())
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
    fn switchable_failures_are_the_ones_another_model_may_not_have() {
        assert!(classify(429, "rate limit"));
        assert!(classify(503, ""));
        assert!(classify(400, "This model's maximum context length is 8192 tokens"));
        assert!(classify(404, "No endpoints found for model x"));
        assert!(!classify(401, "bad key"));
        assert!(!classify(400, "invalid request"));
    }

    #[test]
    fn the_conversation_keeps_the_rules_the_schema_and_recent_turns() {
        let hist: Vec<Turn> = (0..10).map(|i| Turn { role: if i % 2 == 0 { "user".into() } else { "assistant".into() }, content: format!("t{i}") }).collect();
        let m = messages("MySQL", "shop", "orders(id)\n", &hist, "now only 2024");
        assert_eq!(m[0].0, "system");
        assert!(m[0].1.contains("orders(id)") && m[0].1.contains("`shop`"));
        assert_eq!(m.len(), 1 + 6 + 1, "the last six turns and the new request");
        assert_eq!(m.last().unwrap(), &("user".to_string(), "now only 2024".to_string()));
        assert_eq!(m[1].1, "t4");
    }

    #[test]
    fn sql_is_recognised_without_a_code_block() {
        assert!(looks_like_sql("select 1"));
        assert!(!looks_like_sql("I cannot answer that"));
    }

    #[test]
    fn each_service_keeps_its_own_key() {
        let ids: Vec<i64> = ["ollama", "groq", "openrouter", "gemini", "anthropic", "custom"].iter().map(|p| key_id(p)).collect();
        assert_eq!(ids, vec![0, 1, 2, 3, 4, 5]);
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
