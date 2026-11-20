//! Local app store: saved connections in SQLite, passwords in the OS keychain.

use myq_driver::{ConnConfig, SslMode};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("keychain: {0}")]
    Keychain(#[from] keyring::Error),
}
pub type Result<T> = std::result::Result<T, Error>;

/// Where passwords live. The keychain in production; an in-memory map in tests.
pub trait SecretStore: Send + Sync {
    fn set(&self, id: i64, secret: &str) -> Result<()>;
    fn get(&self, id: i64) -> Result<Option<String>>;
    fn delete(&self, id: i64) -> Result<()>;
}

pub struct Keychain;
const SERVICE: &str = "dev.mylodb.connection";

impl SecretStore for Keychain {
    fn set(&self, id: i64, secret: &str) -> Result<()> {
        Ok(keyring::Entry::new(SERVICE, &id.to_string())?.set_password(secret)?)
    }
    fn get(&self, id: i64) -> Result<Option<String>> {
        match keyring::Entry::new(SERVICE, &id.to_string())?.get_password() {
            Ok(p) => Ok(Some(p)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    fn delete(&self, id: i64) -> Result<()> {
        match keyring::Entry::new(SERVICE, &id.to_string())?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

/// A saved connection. `color` tags the connection's tab (e.g. red for production).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedConnection {
    pub id: Option<i64>,
    pub name: String,
    pub color: String,
    #[serde(flatten)]
    pub config: ConnConfig,
    /// Only used on save: `Some` replaces the stored password, `None` keeps it.
    #[serde(default)]
    pub new_password: Option<String>,
}

pub struct Store {
    db: Connection,
    secrets: Box<dyn SecretStore>,
}

impl Store {
    pub fn open(path: &Path, secrets: Box<dyn SecretStore>) -> Result<Store> {
        Self::init(Connection::open(path)?, secrets)
    }

    pub fn in_memory(secrets: Box<dyn SecretStore>) -> Result<Store> {
        Self::init(Connection::open_in_memory()?, secrets)
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
            );",
        )?;
        Ok(Store { db, secrets })
    }

    fn ssl_str(m: SslMode) -> &'static str {
        match m {
            SslMode::Disabled => "disabled",
            SslMode::Required => "required",
            SslMode::SkipVerify => "skipverify",
        }
    }

    fn ssl_parse(s: &str) -> SslMode {
        match s {
            "required" => SslMode::Required,
            "skipverify" => SslMode::SkipVerify,
            _ => SslMode::Disabled,
        }
    }

    /// Insert or update; returns the id.
    pub fn save(&self, c: &SavedConnection) -> Result<i64> {
        let ssl = Self::ssl_str(c.config.ssl);
        let id = match c.id {
            Some(id) => {
                self.db.execute(
                    "UPDATE connections SET name=?1,color=?2,host=?3,port=?4,user=?5,database=?6,ssl=?7 WHERE id=?8",
                    params![c.name, c.color, c.config.host, c.config.port, c.config.user, c.config.database, ssl, id],
                )?;
                id
            }
            None => {
                self.db.execute(
                    "INSERT INTO connections (name,color,host,port,user,database,ssl) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    params![c.name, c.color, c.config.host, c.config.port, c.config.user, c.config.database, ssl],
                )?;
                self.db.last_insert_rowid()
            }
        };
        if let Some(p) = &c.new_password {
            self.secrets.set(id, p)?;
        }
        Ok(id)
    }

    pub fn list(&self) -> Result<Vec<SavedConnection>> {
        let mut st = self
            .db
            .prepare("SELECT id,name,color,host,port,user,database,ssl FROM connections ORDER BY name COLLATE NOCASE")?;
        let rows = st.query_map([], |r| {
            Ok(SavedConnection {
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
                },
                new_password: None,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Config with the password filled from the secret store, ready to connect.
    pub fn config_for(&self, id: i64) -> Result<Option<ConnConfig>> {
        let found = self.list()?.into_iter().find(|c| c.id == Some(id));
        match found {
            None => Ok(None),
            Some(mut c) => {
                c.config.password = self.secrets.get(id)?;
                Ok(Some(c.config))
            }
        }
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        self.db.execute("DELETE FROM connections WHERE id=?1", params![id])?;
        self.secrets.delete(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashMap, sync::Mutex};

    #[derive(Default)]
    struct Mem(Mutex<HashMap<i64, String>>);
    impl SecretStore for Mem {
        fn set(&self, id: i64, s: &str) -> Result<()> {
            self.0.lock().unwrap().insert(id, s.into());
            Ok(())
        }
        fn get(&self, id: i64) -> Result<Option<String>> {
            Ok(self.0.lock().unwrap().get(&id).cloned())
        }
        fn delete(&self, id: i64) -> Result<()> {
            self.0.lock().unwrap().remove(&id);
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
            },
            new_password: Some("s3cret".into()),
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
