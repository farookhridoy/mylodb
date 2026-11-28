use mysql_async::{ClientIdentity, Opts, OptsBuilder, SslOpts};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SslMode {
    #[default]
    Disabled,
    /// TLS with certificate and hostname verification.
    Required,
    /// TLS that verifies the certificate chain but not the host name (certificates issued for a name,
    /// reached by IP address or through an SSH tunnel).
    VerifyCa,
    /// TLS without verifying the server certificate (self-signed servers).
    SkipVerify,
}

/// Connection settings. The password is supplied by the caller (Keychain), never stored here.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ConnConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    #[serde(skip)]
    pub password: Option<String>,
    pub database: Option<String>,
    #[serde(default)]
    pub ssl: SslMode,
    /// PEM file with the certificate authority that signed the server certificate.
    #[serde(default)]
    pub ssl_ca: Option<String>,
    /// PEM client certificate chain and its private key, for servers that require client certificates.
    #[serde(default)]
    pub ssl_cert: Option<String>,
    #[serde(default)]
    pub ssl_key: Option<String>,
    /// Make every session of this connection read-only on the server (`SET SESSION TRANSACTION READ ONLY` when it is opened).
    /// The server then refuses writes and schema changes, whatever the client sends.
    #[serde(default)]
    pub read_only: bool,
    /// Collation of the connection (`SET NAMES <charset> COLLATE <collation>`); the server default when None.
    #[serde(default)]
    pub collation: Option<String>,
}

/// Other parts of the application bring a second TLS crypto library, and rustls refuses to guess between two: without a choice
/// made here, opening an encrypted connection panics. Choosing twice does nothing.
pub fn choose_tls_provider() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

/// A collation name is letters, digits and underscores: anything else is never pasted into SQL.
pub fn valid_collation(c: &str) -> bool {
    !c.is_empty() && c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn path(p: &str) -> PathBuf {
    match (p.strip_prefix("~/"), std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"))) {
        (Some(rest), Ok(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(p),
    }
}

impl ConnConfig {
    pub fn to_opts(&self) -> Opts {
        // `setup` (not `init`): it runs again whenever the pool resets a recycled connection, which wipes session state.
        let mut setup = vec![match self.collation.as_deref().filter(|c| valid_collation(c)) {
            // the character set is the part of the collation name before the first underscore
            Some(c) => format!("SET NAMES {} COLLATE {c}", c.split('_').next().unwrap_or("utf8mb4")),
            None => "SET NAMES utf8mb4".to_string(),
        }];
        if self.read_only {
            setup.push("SET SESSION TRANSACTION READ ONLY".to_string());
        }
        let mut b = OptsBuilder::default()
            .ip_or_hostname(self.host.clone())
            .tcp_port(self.port)
            .user(Some(self.user.clone()))
            .pass(self.password.clone())
            .db_name(self.database.clone())
            // Default charset: utf8mb4 for full Unicode.
            .setup(setup);
        if self.ssl != SslMode::Disabled {
            choose_tls_provider();
            let mut ssl = SslOpts::default();
            if let Some(ca) = self.ssl_ca.as_deref().filter(|c| !c.is_empty()) {
                ssl = ssl.with_root_certs(vec![path(ca).into()]);
            }
            if let (Some(cert), Some(key)) = (self.ssl_cert.as_deref().filter(|c| !c.is_empty()), self.ssl_key.as_deref().filter(|k| !k.is_empty())) {
                ssl = ssl.with_client_identity(Some(ClientIdentity::new(path(cert).into(), path(key).into())));
            }
            ssl = match self.ssl {
                SslMode::VerifyCa => ssl.with_danger_skip_domain_validation(true),
                SslMode::SkipVerify => ssl.with_danger_accept_invalid_certs(true).with_danger_skip_domain_validation(true),
                _ => ssl,
            };
            b = b.ssl_opts(ssl);
        }
        b.into()
    }
}
