use mysql_async::{Opts, OptsBuilder, SslOpts};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SslMode {
    #[default]
    Disabled,
    /// TLS with certificate and hostname verification.
    Required,
    /// TLS without verifying the server certificate (self-signed servers).
    SkipVerify,
}

/// Connection settings. The password is supplied by the caller (Keychain), never stored here.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    #[serde(skip)]
    pub password: Option<String>,
    pub database: Option<String>,
    #[serde(default)]
    pub ssl: SslMode,
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
        b = match self.ssl {
            SslMode::Disabled => b,
            SslMode::Required => b.ssl_opts(SslOpts::default()),
            SslMode::SkipVerify => b.ssl_opts(
                SslOpts::default()
                    .with_danger_accept_invalid_certs(true)
                    .with_danger_skip_domain_validation(true),
            ),
        };
        b.into()
    }
}
