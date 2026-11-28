//! Backup and restore with PostgreSQL's own programs (`pg_dump`, `pg_restore`, `psql`), found on this computer. Only they know
//! every kind of object a server can hold; a dump written any other way would quietly lose some. They are not shipped with MyLoDB.

use crate::backend::PgBackend;
use myq_driver::{ConnConfig, SslMode};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;

type Result<T> = std::result::Result<T, String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DumpFormat {
    /// A SQL script, restored with `psql`.
    #[default]
    Sql,
    /// PostgreSQL's compressed archive, restored with `pg_restore`.
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DumpContent {
    #[default]
    All,
    Structure,
    Data,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DumpOptions {
    /// Only these schemas; empty means the whole database.
    #[serde(default)]
    pub schemas: Vec<String>,
    #[serde(default)]
    pub format: DumpFormat,
    #[serde(default)]
    pub content: DumpContent,
    /// A SQL script that first drops what it is about to create.
    #[serde(default)]
    pub clean: bool,
    /// Leave out who owns each object and who may use it.
    #[serde(default)]
    pub no_owner: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RestoreOptions {
    /// Drop what is about to be restored first (archives only; a script says so itself).
    #[serde(default)]
    pub clean: bool,
    /// All or nothing.
    #[serde(default)]
    pub single_transaction: bool,
    #[serde(default)]
    pub no_owner: bool,
}

#[derive(Debug, Serialize)]
pub struct DumpResult {
    pub path: PathBuf,
    pub bytes: u64,
    pub elapsed_ms: u64,
    /// The program that did it, for the record.
    pub tool: String,
}

#[derive(Debug, Serialize)]
pub struct RestoreResult {
    pub elapsed_ms: u64,
    pub tool: String,
    /// What the program complained about although it finished.
    pub warnings: Vec<String>,
}

fn executable(dir: &Path, name: &str) -> PathBuf {
    dir.join(if cfg!(windows) { format!("{name}.exe") } else { name.to_string() })
}

/// `…/*/bin` for every entry of `parent`.
fn versions_in(parent: &str, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(parent).into_iter().flatten().flatten() {
        out.push(entry.path().join("bin"));
    }
}

/// Folders that may hold the programs: the one named by `MYLODB_PG_BIN` alone, otherwise the PATH and the usual places.
fn candidates() -> Vec<PathBuf> {
    if let Some(dir) = std::env::var_os("MYLODB_PG_BIN") {
        return vec![dir.into()];
    }
    let mut out: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if cfg!(windows) {
        versions_in("C:\\Program Files\\PostgreSQL", &mut out);
    } else {
        versions_in("/Applications/Postgres.app/Contents/Versions", &mut out);
        versions_in("/Users/Shared/DBngin/postgresql", &mut out);
        versions_in("/usr/lib/postgresql", &mut out);
        for brew in ["/opt/homebrew/opt", "/usr/local/opt"] {
            for entry in std::fs::read_dir(brew).into_iter().flatten().flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name == "libpq" || name.starts_with("postgresql") {
                    out.push(entry.path().join("bin"));
                }
            }
        }
        if let Some(home) = std::env::var_os("HOME") {
            versions_in(&Path::new(&home).join("Library/Application Support/Herd/services/postgresql").to_string_lossy(), &mut out);
        }
    }
    out
}

/// `pg_dump (PostgreSQL) 16.2` is 16.
fn major_of(version: &str) -> Option<u32> {
    version.split("(PostgreSQL)").nth(1)?.trim().split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

fn quiet(mut command: Command) -> Command {
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // no console window
    command.stdin(Stdio::null());
    command
}

/// The newest folder of programs that can work with a server of this major version.
async fn find(server: u32) -> Result<(PathBuf, u32)> {
    let mut found: Vec<(u32, PathBuf)> = Vec::new();
    for dir in candidates() {
        let program = executable(&dir, "pg_dump");
        if !program.is_file() {
            continue;
        }
        let mut command = quiet(Command::new(&program));
        let Ok(out) = command.arg("--version").output().await else { continue };
        if let Some(major) = major_of(&String::from_utf8_lossy(&out.stdout)) {
            found.push((major, dir));
        }
    }
    found.sort_by_key(|(major, _)| *major);
    match found.pop() {
        Some((major, dir)) if major >= server => Ok((dir, major)),
        Some((major, _)) => Err(format!("The newest PostgreSQL programs on this computer are version {major}, older than the server ({server}). Install the PostgreSQL {server} command line tools (for example Postgres.app, `brew install libpq`, or the installer from postgresql.org).")),
        None => Err("Backup and restore use PostgreSQL's own programs (pg_dump, pg_restore, psql), which were not found on this computer. Install them (for example Postgres.app, `brew install libpq`, or the installer from postgresql.org) and try again.".into()),
    }
}

/// The connection, as the environment of a PostgreSQL program. The password is never on the command line.
fn connect_env(command: &mut Command, cfg: &ConnConfig) {
    // nothing inherited may point the program at another server
    for name in ["PGSERVICE", "PGSERVICEFILE", "PGHOSTADDR", "PGOPTIONS", "PGPASSFILE", "PGSSLROOTCERT", "PGSSLCERT", "PGSSLKEY"] {
        command.env_remove(name);
    }
    command.env("PGHOST", &cfg.host).env("PGPORT", cfg.port.to_string()).env("PGUSER", &cfg.user).env("PGCONNECT_TIMEOUT", "10").env("PGAPPNAME", "MyLoDB");
    command.env("PGDATABASE", cfg.database.as_deref().filter(|d| !d.is_empty()).unwrap_or("postgres"));
    if let Some(p) = &cfg.password {
        command.env("PGPASSWORD", p);
    }
    let set = |v: &Option<String>| v.as_deref().filter(|v| !v.is_empty()).map(|v| match (v.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{home}/{rest}"),
        _ => v.to_string(),
    });
    let mode = match cfg.ssl {
        SslMode::Disabled => "disable",
        SslMode::SkipVerify => "require",
        SslMode::VerifyCa => "verify-ca",
        SslMode::Required => "verify-full",
    };
    command.env("PGSSLMODE", mode);
    match set(&cfg.ssl_ca) {
        Some(ca) => {
            command.env("PGSSLROOTCERT", ca);
        }
        // the computer's own list of authorities (PostgreSQL 16 and later)
        None if matches!(cfg.ssl, SslMode::Required) => {
            command.env("PGSSLROOTCERT", "system");
        }
        None => {}
    }
    if let (Some(cert), Some(key)) = (set(&cfg.ssl_cert), set(&cfg.ssl_key)) {
        command.env("PGSSLCERT", cert).env("PGSSLKEY", key);
    }
}

/// Run a program to the end. `line` hears each line it prints; the complaints are returned, or the last of them on failure.
async fn run(mut command: Command, cancel: &AtomicBool, line: &mut (dyn FnMut(&str) + Send)) -> Result<Vec<String>> {
    command.stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = command.spawn().map_err(|e| format!("the program could not be started: {e}"))?;
    let stderr = child.stderr.take().ok_or("no output from the program")?;
    let mut reader = BufReader::new(stderr);
    let mut complaints: Vec<String> = Vec::new();
    let mut buffer = Vec::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill().await;
            return Err("cancelled".into());
        }
        // a partial line stays in the buffer when the wait runs out
        match tokio::time::timeout(Duration::from_millis(200), reader.read_until(b'\n', &mut buffer)).await {
            Err(_) => continue,
            Ok(Ok(0)) if buffer.is_empty() => break,
            Ok(Ok(_)) => {
                let text = String::from_utf8_lossy(&buffer).trim_end().to_string();
                buffer.clear();
                let lower = text.to_ascii_lowercase();
                if ["error:", "fatal:", "warning:", "detail:", "hint:"].iter().any(|k| lower.contains(k)) {
                    complaints.push(text.clone());
                    if complaints.len() > 100 {
                        complaints.remove(0);
                    }
                }
                line(&text);
            }
            Ok(Err(e)) => {
                let mut rest = Vec::new();
                let _ = reader.read_to_end(&mut rest).await;
                return Err(format!("the program's output could not be read: {e}"));
            }
        }
    }
    let status = child.wait().await.map_err(|e| e.to_string())?;
    if status.success() {
        Ok(complaints)
    } else if complaints.is_empty() {
        Err(format!("the program stopped with {status}"))
    } else {
        let tail = complaints.len().saturating_sub(8);
        Err(complaints[tail..].join("\n"))
    }
}

async fn server_major(b: &PgBackend) -> Result<u32> {
    let rows = b.query("SELECT current_setting('server_version_num')::int / 10000", &[]).await.map_err(|e| e.0)?;
    Ok(rows.first().ok_or("the server did not say its version")?.get::<_, i32>(0) as u32)
}

/// A schema name as a `pg_dump` pattern that matches it and nothing else.
fn pattern(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Dump to `target`. A partial file is never left behind: the dump is written beside it and renamed when complete.
pub async fn dump(b: &PgBackend, options: &DumpOptions, target: &Path, cancel: &AtomicBool, line: &mut (dyn FnMut(&str) + Send)) -> Result<DumpResult> {
    if options.clean && options.content == DumpContent::Data {
        return Err("a data-only backup cannot drop the tables first".into());
    }
    if target.exists() {
        return Err(format!("{} already exists", target.display()));
    }
    let started = Instant::now();
    let (dir, major) = find(server_major(b).await?).await?;
    let partial = target.with_extension("partial");
    let mut command = quiet(Command::new(executable(&dir, "pg_dump")));
    connect_env(&mut command, b.config());
    command.args(["--no-password", "--verbose", "--encoding=UTF8"]);
    command.arg(match options.format { DumpFormat::Sql => "--format=plain", DumpFormat::Custom => "--format=custom" });
    match options.content {
        DumpContent::All => {}
        DumpContent::Structure => {
            command.arg("--schema-only");
        }
        DumpContent::Data => {
            command.arg("--data-only");
        }
    }
    // an archive decides these when it is restored
    if options.format == DumpFormat::Sql {
        if options.clean {
            command.args(["--clean", "--if-exists"]);
        }
        if options.no_owner {
            command.args(["--no-owner", "--no-privileges"]);
        }
    }
    for schema in &options.schemas {
        command.arg(format!("--schema={}", pattern(schema)));
    }
    command.arg("--file").arg(&partial);
    let outcome = run(command, cancel, line).await;
    if let Err(e) = outcome {
        let _ = std::fs::remove_file(&partial);
        return Err(e);
    }
    std::fs::rename(&partial, target).map_err(|e| format!("cannot write {}: {e}", target.display()))?;
    let bytes = std::fs::metadata(target).map(|m| m.len()).unwrap_or(0);
    Ok(DumpResult { path: target.to_path_buf(), bytes, elapsed_ms: started.elapsed().as_millis() as u64, tool: format!("pg_dump {major}") })
}

/// Whether the file is an archive for `pg_restore` (otherwise it is run as a script).
pub fn is_archive(path: &Path) -> Result<bool> {
    use std::io::Read;
    let mut head = [0u8; 5];
    let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut head)).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    Ok(n == 5 && &head == b"PGDMP")
}

/// Restore an archive or run a SQL script into the connected database. It stops at the first error.
pub async fn restore(b: &PgBackend, source: &Path, options: &RestoreOptions, cancel: &AtomicBool, line: &mut (dyn FnMut(&str) + Send)) -> Result<RestoreResult> {
    if b.read_only() {
        return Err("this connection is read-only".into());
    }
    let archive = is_archive(source)?;
    let started = Instant::now();
    let (dir, major) = find(server_major(b).await?).await?;
    let database = b.config().database.clone().filter(|d| !d.is_empty()).unwrap_or_else(|| "postgres".into());
    let (mut command, tool) = if archive {
        let mut c = quiet(Command::new(executable(&dir, "pg_restore")));
        c.args(["--no-password", "--verbose", "--exit-on-error"]).arg(format!("--dbname={database}"));
        if options.clean {
            c.args(["--clean", "--if-exists"]);
        }
        if options.no_owner {
            c.args(["--no-owner", "--no-privileges"]);
        }
        if options.single_transaction {
            c.arg("--single-transaction");
        }
        c.arg(source);
        (c, format!("pg_restore {major}"))
    } else {
        let mut c = quiet(Command::new(executable(&dir, "psql")));
        c.args(["--no-psqlrc", "--no-password", "--quiet", "--set=ON_ERROR_STOP=1"]);
        if options.single_transaction {
            c.arg("--single-transaction");
        }
        c.arg("--file").arg(source);
        (c, format!("psql {major}"))
    };
    connect_env(&mut command, b.config());
    let warnings = run(command, cancel, line).await?;
    Ok(RestoreResult { elapsed_ms: started.elapsed().as_millis() as u64, tool, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_and_patterns() {
        assert_eq!(major_of("pg_dump (PostgreSQL) 18.4 (Homebrew)\n"), Some(18));
        assert_eq!(major_of("psql (PostgreSQL) 9.6.3"), Some(9));
        assert_eq!(major_of("something else"), None);
        assert_eq!(pattern("we\"ird*"), "\"we\"\"ird*\"");
    }

    #[test]
    fn the_password_is_in_the_environment_and_ssl_follows_the_connection() {
        let mut c = Command::new("x");
        connect_env(&mut c, &ConnConfig { host: "h".into(), port: 5, user: "u".into(), password: Some("secret".into()), ssl: SslMode::Required, ..Default::default() });
        let env: std::collections::HashMap<_, _> = c.as_std().get_envs().map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned()))).collect();
        assert_eq!((env["PGPASSWORD"].as_deref(), env["PGSSLMODE"].as_deref(), env["PGSSLROOTCERT"].as_deref(), env["PGDATABASE"].as_deref()), (Some("secret"), Some("verify-full"), Some("system"), Some("postgres")));
        assert_eq!(env["PGHOSTADDR"], None, "removed");
        assert_eq!(c.as_std().get_args().count(), 0);
    }
}
