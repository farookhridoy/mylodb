//! Local app store: saved connections in SQLite, passwords in the OS keychain.

use myq_driver::{ConnConfig, SslMode};
use myq_tunnel::{SshConfig, SshSecrets};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("keychain: {0}")]
    Keychain(#[from] keyring::Error),
    #[error("stored SSH settings are unreadable: {0}")]
    Json(#[from] serde_json::Error),
}
pub type Result<T> = std::result::Result<T, Error>;

/// Which secret of a connection. Each lives under its own keychain entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    DbPassword,
    SshPassword,
    SshPassphrase,
}

impl Slot {
    /// The database password keeps its original entry name so existing keychain items stay valid.
    fn account(self, id: i64) -> String {
        match self {
            Slot::DbPassword => id.to_string(),
            Slot::SshPassword => format!("{id}:ssh-password"),
            Slot::SshPassphrase => format!("{id}:ssh-passphrase"),
        }
    }
}

/// Where secrets live. The keychain in production; an in-memory map in tests.
pub trait SecretStore: Send + Sync {
    fn set(&self, id: i64, slot: Slot, secret: &str) -> Result<()>;
    fn get(&self, id: i64, slot: Slot) -> Result<Option<String>>;
    fn delete(&self, id: i64, slot: Slot) -> Result<()>;
}

pub struct Keychain;
const SERVICE: &str = "dev.mylodb.connection";

impl SecretStore for Keychain {
    fn set(&self, id: i64, slot: Slot, secret: &str) -> Result<()> {
        Ok(keyring::Entry::new(SERVICE, &slot.account(id))?.set_password(secret)?)
    }
    fn get(&self, id: i64, slot: Slot) -> Result<Option<String>> {
        match keyring::Entry::new(SERVICE, &slot.account(id))?.get_password() {
            Ok(p) => Ok(Some(p)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    fn delete(&self, id: i64, slot: Slot) -> Result<()> {
        match keyring::Entry::new(SERVICE, &slot.account(id))?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

/// What a connection points at. It decides how loudly the app warns, not what the server allows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    #[default]
    Development,
    Staging,
    Production,
}

impl Environment {
    fn as_str(self) -> &'static str {
        match self {
            Environment::Development => "development",
            Environment::Staging => "staging",
            Environment::Production => "production",
        }
    }

    fn parse(s: &str) -> Environment {
        match s {
            "staging" => Environment::Staging,
            "production" => Environment::Production,
            _ => Environment::Development,
        }
    }
}

fn yes() -> bool {
    true
}

/// A saved connection. `color` tags the connection's tab (e.g. red for production).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedConnection {
    pub id: Option<i64>,
    pub name: String,
    pub color: String,
    #[serde(flatten)]
    pub config: ConnConfig,
    #[serde(default)]
    pub environment: Environment,
    /// Stop a run that takes longer than this many seconds (the running statement is killed). None: no limit.
    #[serde(default)]
    pub query_timeout_secs: Option<u32>,
    /// List every database the account can see. When off and a default database is set, the browser shows only that database
    /// (what shared hosting needs: one database per account, and the rest are not yours to browse).
    #[serde(default = "yes")]
    pub show_all_databases: bool,
    /// Ask before running statements that can destroy data (DROP, TRUNCATE, DELETE/UPDATE without WHERE, ...).
    #[serde(default = "yes")]
    pub confirm_destructive: bool,
    /// Reach the server through this SSH server.
    #[serde(default)]
    pub ssh: Option<SshConfig>,
    /// Only used on save: `Some` replaces the stored secret, `None` keeps it.
    #[serde(default)]
    pub new_password: Option<String>,
    #[serde(default)]
    pub new_ssh_password: Option<String>,
    #[serde(default)]
    pub new_ssh_passphrase: Option<String>,
}

pub struct Store {
    db: Connection,
    secrets: Box<dyn SecretStore>,
}

const ALL_SLOTS: [Slot; 3] = [Slot::DbPassword, Slot::SshPassword, Slot::SshPassphrase];

impl Store {
    pub fn open(path: &Path, secrets: Box<dyn SecretStore>) -> Result<Store> {
        Self::init(Connection::open(path)?, secrets)
    }

    pub fn in_memory(secrets: Box<dyn SecretStore>) -> Result<Store> {
        Self::init(Connection::open_in_memory()?, secrets)
    }

    /// Add a column to an existing table when an older database lacks it.
    fn ensure_column(db: &Connection, table: &str, column: &str, decl: &str) -> Result<()> {
        let have: Vec<String> = db.prepare(&format!("PRAGMA table_info({table})"))?.query_map([], |r| r.get::<_, String>(1))?.collect::<std::result::Result<_, _>>()?;
        if !have.iter().any(|c| c == column) {
            db.execute(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"), [])?;
        }
        Ok(())
    }

    fn init(db: Connection, secrets: Box<dyn SecretStore>) -> Result<Store> {
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS connections (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                color TEXT NOT NULL DEFAULT '#4f8cff',
                host TEXT NOT NULL,
                port INTEGER NOT NULL,
                user TEXT NOT NULL,
                database TEXT,
                ssl TEXT NOT NULL DEFAULT 'disabled'
            );
            CREATE TABLE IF NOT EXISTS ssh_hosts (
                host_port TEXT PRIMARY KEY,
                fingerprint TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                connection_id INTEGER NOT NULL,
                sql TEXT NOT NULL,
                ran_at TEXT NOT NULL DEFAULT (datetime('now')),
                duration_ms INTEGER NOT NULL,
                ok INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS history_conn ON history (connection_id, id DESC);
            CREATE TABLE IF NOT EXISTS favorites (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                folder TEXT NOT NULL DEFAULT '',
                name TEXT NOT NULL,
                sql TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS session (
                connection_id INTEGER PRIMARY KEY,
                state TEXT NOT NULL
            );",
        )?;
        for col in ["ssl_ca", "ssl_cert", "ssl_key", "ssh"] {
            Self::ensure_column(&db, "connections", col, "TEXT")?;
        }
        Self::ensure_column(&db, "connections", "environment", "TEXT NOT NULL DEFAULT 'development'")?;
        Self::ensure_column(&db, "connections", "read_only", "INTEGER NOT NULL DEFAULT 0")?;
        Self::ensure_column(&db, "connections", "query_timeout", "INTEGER")?;
        Self::ensure_column(&db, "connections", "confirm_destructive", "INTEGER NOT NULL DEFAULT 1")?;
        Self::ensure_column(&db, "connections", "show_all_databases", "INTEGER NOT NULL DEFAULT 1")?;
        Ok(Store { db, secrets })
    }

    fn ssl_str(m: SslMode) -> &'static str {
        match m {
            SslMode::Disabled => "disabled",
            SslMode::Required => "required",
            SslMode::VerifyCa => "verifyca",
            SslMode::SkipVerify => "skipverify",
        }
    }

    fn ssl_parse(s: &str) -> SslMode {
        match s {
            "required" => SslMode::Required,
            "verifyca" => SslMode::VerifyCa,
            "skipverify" => SslMode::SkipVerify,
            _ => SslMode::Disabled,
        }
    }

    fn set_secret(&self, id: i64, slot: Slot, value: &Option<String>) -> Result<()> {
        match value {
            Some(v) if !v.is_empty() => self.secrets.set(id, slot, v),
            _ => Ok(()),
        }
    }

    /// Insert or update; returns the id.
    pub fn save(&self, c: &SavedConnection) -> Result<i64> {
        let ssl = Self::ssl_str(c.config.ssl);
        let ssh = c.ssh.as_ref().map(serde_json::to_string).transpose()?;
        let (ca, cert, key) = (&c.config.ssl_ca, &c.config.ssl_cert, &c.config.ssl_key);
        let id = match c.id {
            Some(id) => {
                self.db.execute(
                    "UPDATE connections SET name=?1,color=?2,host=?3,port=?4,user=?5,database=?6,ssl=?7,ssl_ca=?8,ssl_cert=?9,ssl_key=?10,ssh=?11,environment=?12,read_only=?13,query_timeout=?14,confirm_destructive=?15,show_all_databases=?16 WHERE id=?17",
                    params![c.name, c.color, c.config.host, c.config.port, c.config.user, c.config.database, ssl, ca, cert, key, ssh, c.environment.as_str(), c.config.read_only, c.query_timeout_secs, c.confirm_destructive, c.show_all_databases, id],
                )?;
                id
            }
            None => {
                self.db.execute(
                    "INSERT INTO connections (name,color,host,port,user,database,ssl,ssl_ca,ssl_cert,ssl_key,ssh,environment,read_only,query_timeout,confirm_destructive,show_all_databases) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
                    params![c.name, c.color, c.config.host, c.config.port, c.config.user, c.config.database, ssl, ca, cert, key, ssh, c.environment.as_str(), c.config.read_only, c.query_timeout_secs, c.confirm_destructive, c.show_all_databases],
                )?;
                self.db.last_insert_rowid()
            }
        };
        self.set_secret(id, Slot::DbPassword, &c.new_password)?;
        match &c.ssh {
            Some(_) => {
                self.set_secret(id, Slot::SshPassword, &c.new_ssh_password)?;
                self.set_secret(id, Slot::SshPassphrase, &c.new_ssh_passphrase)?;
            }
            // SSH switched off: its secrets are no longer needed, so they are not kept around
            None => {
                self.secrets.delete(id, Slot::SshPassword)?;
                self.secrets.delete(id, Slot::SshPassphrase)?;
            }
        }
        Ok(id)
    }

    pub fn list(&self) -> Result<Vec<SavedConnection>> {
        let mut st = self.db.prepare(
            "SELECT id,name,color,host,port,user,database,ssl,ssl_ca,ssl_cert,ssl_key,ssh,environment,read_only,query_timeout,confirm_destructive,show_all_databases FROM connections ORDER BY name COLLATE NOCASE",
        )?;
        let rows = st.query_map([], |r| {
            let ssh: Option<String> = r.get(11)?;
            Ok((
                SavedConnection {
                    id: Some(r.get(0)?),
                    name: r.get(1)?,
                    color: r.get(2)?,
                    config: ConnConfig {
                        host: r.get(3)?,
                        port: r.get(4)?,
                        user: r.get(5)?,
                        password: None,
                        database: r.get(6)?,
                        ssl: Self::ssl_parse(&r.get::<_, String>(7)?),
                        ssl_ca: r.get(8)?,
                        ssl_cert: r.get(9)?,
                        ssl_key: r.get(10)?,
                        read_only: r.get::<_, i64>(13)? != 0,
                    },
                    environment: Environment::parse(&r.get::<_, String>(12)?),
                    query_timeout_secs: r.get(14)?,
                    confirm_destructive: r.get::<_, i64>(15)? != 0,
                    show_all_databases: r.get::<_, i64>(16)? != 0,
                    ssh: None,
                    new_password: None,
                    new_ssh_password: None,
                    new_ssh_passphrase: None,
                },
                ssh,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (mut c, ssh) = row?;
            c.ssh = ssh.map(|j| serde_json::from_str(&j)).transpose()?;
            out.push(c);
        }
        Ok(out)
    }

    /// A saved connection (without secrets) by id.
    pub fn connection(&self, id: i64) -> Result<Option<SavedConnection>> {
        Ok(self.list()?.into_iter().find(|c| c.id == Some(id)))
    }

    /// Config with the password filled from the secret store, ready to connect.
    pub fn config_for(&self, id: i64) -> Result<Option<ConnConfig>> {
        let found = self.list()?.into_iter().find(|c| c.id == Some(id));
        match found {
            None => Ok(None),
            Some(mut c) => {
                c.config.password = self.secrets.get(id, Slot::DbPassword)?;
                Ok(Some(c.config))
            }
        }
    }

    /// The SSH settings of a connection with their secrets, or `None` when it connects directly.
    pub fn ssh_for(&self, id: i64) -> Result<Option<(SshConfig, SshSecrets)>> {
        let Some(c) = self.list()?.into_iter().find(|c| c.id == Some(id)) else { return Ok(None) };
        let Some(ssh) = c.ssh else { return Ok(None) };
        let secrets = SshSecrets { password: self.secrets.get(id, Slot::SshPassword)?, key_passphrase: self.secrets.get(id, Slot::SshPassphrase)? };
        Ok(Some((ssh, secrets)))
    }

    /// A stored secret of an existing connection (used to test an edited, not yet saved, connection).
    pub fn secret(&self, id: i64, slot: Slot) -> Result<Option<String>> {
        self.secrets.get(id, slot)
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        self.db.execute("DELETE FROM connections WHERE id=?1", params![id])?;
        self.db.execute("DELETE FROM history WHERE connection_id=?1", params![id])?;
        self.db.execute("DELETE FROM session WHERE connection_id=?1", params![id])?;
        for slot in ALL_SLOTS {
            self.secrets.delete(id, slot)?;
        }
        Ok(())
    }

    // ---- trusted SSH host keys ----

    pub fn ssh_known(&self, host: &str, port: u16) -> Result<Option<String>> {
        Ok(self.db.query_row("SELECT fingerprint FROM ssh_hosts WHERE host_port=?1", params![format!("{host}:{port}")], |r| r.get(0)).optional()?)
    }

    pub fn ssh_trust(&self, host: &str, port: u16, fingerprint: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO ssh_hosts (host_port, fingerprint) VALUES (?1, ?2) ON CONFLICT(host_port) DO UPDATE SET fingerprint=excluded.fingerprint",
            params![format!("{host}:{port}"), fingerprint],
        )?;
        Ok(())
    }

    pub fn ssh_forget(&self, host: &str, port: u16) -> Result<()> {
        self.db.execute("DELETE FROM ssh_hosts WHERE host_port=?1", params![format!("{host}:{port}")])?;
        Ok(())
    }

    // ---- history ----

    /// Log an executed script. Consecutive identical entries are collapsed into one.
    pub fn add_history(&self, connection_id: i64, sql: &str, duration_ms: u64, ok: bool) -> Result<()> {
        let last: Option<String> = self
            .db
            .query_row("SELECT sql FROM history WHERE connection_id=?1 ORDER BY id DESC LIMIT 1", params![connection_id], |r| r.get(0))
            .ok();
        if last.as_deref() == Some(sql) {
            self.db.execute(
                "UPDATE history SET ran_at=datetime('now'), duration_ms=?2, ok=?3 WHERE id=(SELECT MAX(id) FROM history WHERE connection_id=?1)",
                params![connection_id, duration_ms as i64, ok],
            )?;
        } else {
            self.db.execute(
                "INSERT INTO history (connection_id, sql, duration_ms, ok) VALUES (?1,?2,?3,?4)",
                params![connection_id, sql, duration_ms as i64, ok],
            )?;
        }
        Ok(())
    }

    /// Newest first; `search` is a case-insensitive substring match.
    pub fn list_history(&self, connection_id: i64, search: &str, limit: u32) -> Result<Vec<HistoryEntry>> {
        let mut st = self.db.prepare(
            "SELECT id, sql, ran_at, duration_ms, ok FROM history
             WHERE connection_id=?1 AND sql LIKE '%' || ?2 || '%' ESCAPE '\\'
             ORDER BY id DESC LIMIT ?3",
        )?;
        let escaped = search.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        let rows = st.query_map(params![connection_id, escaped, limit], |r| {
            Ok(HistoryEntry { id: r.get(0)?, sql: r.get(1)?, ran_at: r.get(2)?, duration_ms: r.get(3)?, ok: r.get(4)? })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn clear_history(&self, connection_id: i64) -> Result<()> {
        self.db.execute("DELETE FROM history WHERE connection_id=?1", params![connection_id])?;
        Ok(())
    }

    // ---- favorites (global, grouped in folders) ----

    pub fn save_favorite(&self, f: &Favorite) -> Result<i64> {
        match f.id {
            Some(id) => {
                self.db.execute("UPDATE favorites SET folder=?1,name=?2,sql=?3 WHERE id=?4", params![f.folder, f.name, f.sql, id])?;
                Ok(id)
            }
            None => {
                self.db.execute("INSERT INTO favorites (folder,name,sql) VALUES (?1,?2,?3)", params![f.folder, f.name, f.sql])?;
                Ok(self.db.last_insert_rowid())
            }
        }
    }

    pub fn list_favorites(&self) -> Result<Vec<Favorite>> {
        let mut st = self.db.prepare("SELECT id,folder,name,sql FROM favorites ORDER BY folder COLLATE NOCASE, name COLLATE NOCASE")?;
        let rows = st.query_map([], |r| Ok(Favorite { id: Some(r.get(0)?), folder: r.get(1)?, name: r.get(2)?, sql: r.get(3)? }))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn delete_favorite(&self, id: i64) -> Result<()> {
        self.db.execute("DELETE FROM favorites WHERE id=?1", params![id])?;
        Ok(())
    }

    // ---- session restore ----

    /// Opaque UI state (open tabs etc.) for a connection; the UI owns the format.
    pub fn set_session(&self, connection_id: i64, state: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO session (connection_id,state) VALUES (?1,?2) ON CONFLICT(connection_id) DO UPDATE SET state=excluded.state",
            params![connection_id, state],
        )?;
        Ok(())
    }

    pub fn get_session(&self, connection_id: i64) -> Result<Option<String>> {
        Ok(self
            .db
            .query_row("SELECT state FROM session WHERE connection_id=?1", params![connection_id], |r| r.get(0))
            .optional()?)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub sql: String,
    pub ran_at: String,
    pub duration_ms: i64,
    pub ok: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Favorite {
    pub id: Option<i64>,
    pub folder: String,
    pub name: String,
    pub sql: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashMap, sync::Mutex};

    #[derive(Default, Clone)]
    struct Mem(std::sync::Arc<Mutex<HashMap<(i64, String), String>>>);
    impl SecretStore for Mem {
        fn set(&self, id: i64, slot: Slot, s: &str) -> Result<()> {
            self.0.lock().unwrap().insert((id, slot.account(id)), s.into());
            Ok(())
        }
        fn get(&self, id: i64, slot: Slot) -> Result<Option<String>> {
            Ok(self.0.lock().unwrap().get(&(id, slot.account(id))).cloned())
        }
        fn delete(&self, id: i64, slot: Slot) -> Result<()> {
            self.0.lock().unwrap().remove(&(id, slot.account(id)));
            Ok(())
        }
    }

    fn conn(name: &str) -> SavedConnection {
        SavedConnection {
            id: None,
            name: name.into(),
            color: "#e5484d".into(),
            config: ConnConfig {
                host: "h".into(),
                port: 3306,
                user: "u".into(),
                password: None,
                database: None,
                ssl: SslMode::SkipVerify,
        ..Default::default()
            },
            environment: Environment::Development,
            query_timeout_secs: None,
            confirm_destructive: true,
            show_all_databases: true,
            ssh: None,
            new_password: Some("s3cret".into()),
            new_ssh_password: None,
            new_ssh_passphrase: None,
        }
    }

    #[test]
    fn roundtrip_keeps_password_out_of_sqlite() {
        let s = Store::in_memory(Box::new(Mem::default())).unwrap();
        let id = s.save(&conn("prod")).unwrap();
        let listed = s.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].config.ssl, SslMode::SkipVerify);
        assert!(listed[0].config.password.is_none());
        assert_eq!(s.config_for(id).unwrap().unwrap().password.as_deref(), Some("s3cret"));
        let n: i64 = s.db.query_row("SELECT COUNT(*) FROM connections WHERE name LIKE '%s3cret%' OR host LIKE '%s3cret%'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn safety_settings_are_saved_and_default_safely_for_older_rows() {
        let s = Store::in_memory(Box::new(Mem::default())).unwrap();
        let mut c = conn("prod");
        c.environment = Environment::Production;
        c.config.read_only = true;
        c.query_timeout_secs = Some(30);
        c.confirm_destructive = false;
        c.show_all_databases = false;
        let id = s.save(&c).unwrap();
        assert!(!s.connection(id).unwrap().unwrap().show_all_databases);
        let got = s.connection(id).unwrap().unwrap();
        assert_eq!((got.environment, got.config.read_only, got.query_timeout_secs, got.confirm_destructive), (Environment::Production, true, Some(30), false));
        assert!(s.config_for(id).unwrap().unwrap().read_only, "the driver config carries read-only");
        // editing it back
        let mut back = got.clone();
        back.environment = Environment::Staging;
        back.config.read_only = false;
        back.query_timeout_secs = None;
        s.save(&back).unwrap();
        let again = s.connection(id).unwrap().unwrap();
        assert_eq!((again.environment, again.config.read_only, again.query_timeout_secs), (Environment::Staging, false, None));
    }

    #[test]
    fn a_json_connection_without_the_new_fields_means_development_with_warnings_on() {
        let c: SavedConnection = serde_json::from_str(r##"{"id":null,"name":"x","color":"#fff","host":"h","port":3306,"user":"u","database":null}"##).unwrap();
        assert_eq!((c.environment, c.config.read_only, c.query_timeout_secs, c.confirm_destructive), (Environment::Development, false, None, true));
        assert!(c.show_all_databases, "older connections keep listing every database");
    }

    fn ssh_conn() -> SavedConnection {
        let mut c = conn("tunnelled");
        c.ssh = Some(SshConfig { host: "bastion".into(), port: 2222, user: "deploy".into(), auth: myq_tunnel::SshAuth::Key { path: "~/.ssh/id_ed25519".into() } });
        c.new_ssh_passphrase = Some("phrase".into());
        c.config.ssl = SslMode::VerifyCa;
        c.config.ssl_ca = Some("/etc/ssl/ca.pem".into());
        c.config.ssl_cert = Some("/etc/ssl/client.pem".into());
        c.config.ssl_key = Some("/etc/ssl/client.key".into());
        c
    }

    #[test]
    fn ssh_and_ssl_settings_roundtrip_with_secrets_in_their_own_slots() {
        let mem = Mem::default();
        let s = Store::in_memory(Box::new(mem.clone())).unwrap();
        let id = s.save(&ssh_conn()).unwrap();
        let back = &s.list().unwrap()[0];
        assert_eq!(back.ssh.as_ref().unwrap().host, "bastion");
        assert_eq!(back.config.ssl, SslMode::VerifyCa);
        assert_eq!((back.config.ssl_ca.as_deref(), back.config.ssl_key.as_deref()), (Some("/etc/ssl/ca.pem"), Some("/etc/ssl/client.key")));
        let (ssh, secrets) = s.ssh_for(id).unwrap().unwrap();
        assert_eq!(ssh.port, 2222);
        assert_eq!((secrets.password, secrets.key_passphrase.as_deref()), (None, Some("phrase")));
        assert_eq!(s.secret(id, Slot::DbPassword).unwrap().as_deref(), Some("s3cret"), "database password is a separate entry");
        // none of the secrets are in the SQLite file
        let n: i64 = s.db.query_row("SELECT COUNT(*) FROM connections WHERE ssh LIKE '%phrase%' OR ssh LIKE '%s3cret%'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
        // turning SSH off removes its secrets
        let mut c = ssh_conn();
        c.id = Some(id);
        c.ssh = None;
        c.new_ssh_passphrase = None;
        s.save(&c).unwrap();
        assert!(s.ssh_for(id).unwrap().is_none());
        assert_eq!(s.secret(id, Slot::SshPassphrase).unwrap(), None);
        assert_eq!(s.secret(id, Slot::DbPassword).unwrap().as_deref(), Some("s3cret"), "the database password stays");
        // deleting removes every slot
        let id2 = s.save(&ssh_conn()).unwrap();
        s.delete(id2).unwrap();
        assert_eq!(mem.0.lock().unwrap().keys().filter(|(i, _)| *i == id2).count(), 0);
    }

    #[test]
    fn the_connection_payload_the_ui_sends() {
        let c: SavedConnection = serde_json::from_str(
            r##"{"id":null,"name":"prod","color":"#4f8cff","host":"db.internal","port":3306,"user":"app","database":null,"ssl":"verifyca",
                "ssl_ca":"/etc/ca.pem","ssl_cert":null,"ssl_key":null,
                "ssh":{"host":"bastion","port":22,"user":"deploy","auth":{"method":"key","path":"~/.ssh/id_ed25519"}},
                "new_password":null,"new_ssh_password":null,"new_ssh_passphrase":"p"}"##,
        )
        .unwrap();
        assert_eq!(c.config.ssl, SslMode::VerifyCa);
        assert_eq!(c.ssh.as_ref().unwrap().auth, myq_tunnel::SshAuth::Key { path: "~/.ssh/id_ed25519".into() });
        let pw: SavedConnection = serde_json::from_str(
            r##"{"id":3,"name":"x","color":"#000","host":"h","port":1,"user":"u","database":null,"ssl":"disabled","ssh":{"host":"b","port":22,"user":"d","auth":{"method":"password"}}}"##,
        )
        .unwrap();
        assert_eq!(pw.ssh.unwrap().auth, myq_tunnel::SshAuth::Password);
        assert!(pw.new_ssh_password.is_none() && pw.config.ssl_ca.is_none(), "optional fields may be absent");
        // an older UI payload without any SSH/SSL file fields still loads
        let old: SavedConnection = serde_json::from_str(r##"{"id":null,"name":"n","color":"#fff","host":"h","port":3306,"user":"u","database":null,"ssl":"skipverify"}"##).unwrap();
        assert!(old.ssh.is_none());
    }

    #[test]
    fn an_old_database_gains_the_new_columns_and_keeps_its_rows() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE connections (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, color TEXT NOT NULL DEFAULT '#4f8cff',
               host TEXT NOT NULL, port INTEGER NOT NULL, user TEXT NOT NULL, database TEXT, ssl TEXT NOT NULL DEFAULT 'disabled');
             INSERT INTO connections (name, host, port, user, ssl) VALUES ('legacy', 'db.example', 3306, 'root', 'required');",
        )
        .unwrap();
        let s = Store::init(db, Box::new(Mem::default())).unwrap();
        let c = &s.list().unwrap()[0];
        assert_eq!((c.name.as_str(), c.config.ssl, c.ssh.is_none(), c.config.ssl_ca.is_none()), ("legacy", SslMode::Required, true, true));
        assert_eq!((c.environment, c.config.read_only, c.query_timeout_secs, c.confirm_destructive), (Environment::Development, false, None, true), "old rows get safe defaults");
        // opening it again is a no-op (the columns already exist)
        Store::ensure_column(&s.db, "connections", "ssh", "TEXT").unwrap();
    }

    #[test]
    fn trusted_ssh_hosts_are_per_host_and_port() {
        let s = Store::in_memory(Box::new(Mem::default())).unwrap();
        assert_eq!(s.ssh_known("h", 22).unwrap(), None);
        s.ssh_trust("h", 22, "SHA256:a").unwrap();
        s.ssh_trust("h", 2222, "SHA256:b").unwrap();
        assert_eq!(s.ssh_known("h", 22).unwrap().as_deref(), Some("SHA256:a"));
        s.ssh_trust("h", 22, "SHA256:c").unwrap();
        assert_eq!(s.ssh_known("h", 22).unwrap().as_deref(), Some("SHA256:c"));
        s.ssh_forget("h", 22).unwrap();
        assert_eq!((s.ssh_known("h", 22).unwrap(), s.ssh_known("h", 2222).unwrap().as_deref()), (None, Some("SHA256:b")));
    }

    #[test]
    fn history_collapses_repeats_searches_literally_and_is_per_connection() {
        let s = Store::in_memory(Box::new(Mem::default())).unwrap();
        s.add_history(1, "SELECT 1", 5, true).unwrap();
        s.add_history(1, "SELECT 1", 7, true).unwrap();
        s.add_history(1, "SELECT 100%_x", 9, false).unwrap();
        s.add_history(2, "SELECT other", 1, true).unwrap();
        let all = s.list_history(1, "", 10).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].sql, "SELECT 100%_x");
        assert!(!all[0].ok);
        assert_eq!(all[1].duration_ms, 7);
        assert_eq!(s.list_history(1, "100%_", 10).unwrap().len(), 1, "% and _ are literal in search");
        assert_eq!(s.list_history(1, "select 1", 10).unwrap().len(), 2, "case-insensitive substring");
        assert_eq!(s.list_history(1, "select 100", 10).unwrap().len(), 1);
        s.clear_history(1).unwrap();
        assert!(s.list_history(1, "", 10).unwrap().is_empty());
        assert_eq!(s.list_history(2, "", 10).unwrap().len(), 1);
    }

    #[test]
    fn favorites_and_session_roundtrip_and_cascade_on_delete() {
        let s = Store::in_memory(Box::new(Mem::default())).unwrap();
        let id = s.save_favorite(&Favorite { id: None, folder: "Reports".into(), name: "top".into(), sql: "SELECT 1".into() }).unwrap();
        s.save_favorite(&Favorite { id: Some(id), folder: "Reports".into(), name: "top2".into(), sql: "SELECT 2".into() }).unwrap();
        let favs = s.list_favorites().unwrap();
        assert_eq!((favs.len(), favs[0].name.as_str()), (1, "top2"));
        s.delete_favorite(id).unwrap();
        assert!(s.list_favorites().unwrap().is_empty());

        let cid = s.save(&conn("c")).unwrap();
        assert_eq!(s.get_session(cid).unwrap(), None);
        s.set_session(cid, "{\"a\":1}").unwrap();
        s.set_session(cid, "{\"a\":2}").unwrap();
        assert_eq!(s.get_session(cid).unwrap().as_deref(), Some("{\"a\":2}"));
        s.add_history(cid, "SELECT 1", 1, true).unwrap();
        s.delete(cid).unwrap();
        assert_eq!(s.get_session(cid).unwrap(), None);
        assert!(s.list_history(cid, "", 10).unwrap().is_empty());
    }

    #[test]
    fn update_without_password_keeps_old_one_and_delete_removes_it() {
        let s = Store::in_memory(Box::new(Mem::default())).unwrap();
        let id = s.save(&conn("a")).unwrap();
        let mut c = conn("renamed");
        c.id = Some(id);
        c.new_password = None;
        s.save(&c).unwrap();
        assert_eq!(s.list().unwrap()[0].name, "renamed");
        assert_eq!(s.config_for(id).unwrap().unwrap().password.as_deref(), Some("s3cret"));
        s.delete(id).unwrap();
        assert!(s.list().unwrap().is_empty());
        assert!(s.config_for(id).unwrap().is_none());
    }
}
