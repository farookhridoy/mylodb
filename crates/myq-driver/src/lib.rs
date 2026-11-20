//! MySQL/MariaDB connectivity and schema introspection.

mod config;
mod introspect;

pub use config::{ConnConfig, SslMode};
pub use introspect::{introspect, IntrospectOptions};
pub use mysql_async::Pool;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("mysql: {0}")]
    Mysql(#[from] mysql_async::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Open a pool for the given configuration. No network I/O happens until the first query.
pub fn pool(cfg: &ConnConfig) -> Pool {
    Pool::new(cfg.to_opts())
}
