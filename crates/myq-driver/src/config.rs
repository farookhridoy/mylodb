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
}

fn path(p: &str) -> PathBuf {
    match (p.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(p),
    }
}

impl ConnConfig {
    pub fn to_opts(&self) -> Opts {
        let mut b = OptsBuilder::default()
            .ip_or_hostname(self.host.clone())
            .tcp_port(self.port)
            .user(Some(self.user.clone()))
            .pass(self.password.clone())
            .db_name(self.database.clone())
            // Default charset: utf8mb4 for full Unicode.
            .init(vec!["SET NAMES utf8mb4".to_string()]);
        if self.ssl != SslMode::Disabled {
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
