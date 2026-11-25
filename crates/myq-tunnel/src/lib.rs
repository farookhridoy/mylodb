//! SSH local port forwarding for database connections.
//!
//! `open` connects to an SSH server, authenticates, and listens on `127.0.0.1:<port>`; every connection
//! accepted there is carried over a `direct-tcpip` channel to `remote_host:remote_port` as seen from
//! the SSH server. Host keys are never accepted silently: an unknown key is reported with its
//! fingerprint so the user can confirm it, and a changed key is a hard error.

use russh::client::{self, Handle};
use russh::keys::{load_secret_key, HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum SshAuth {
    /// The password itself comes from the keychain; it is not part of the saved settings.
    Password,
    /// A private key file (OpenSSH or PEM). The passphrase, if any, comes from the keychain.
    Key { path: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SshConfig {
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub user: String,
    pub auth: SshAuth,
}

fn default_port() -> u16 {
    22
}

/// Secrets for one connection attempt.
#[derive(Debug, Clone, Default)]
pub struct SshSecrets {
    pub password: Option<String>,
    pub key_passphrase: Option<String>,
}

/// What the caller already knows about the server's host key.
#[derive(Debug, Clone, Default)]
pub struct HostTrust {
    /// The fingerprint saved from an earlier, confirmed connection to this server.
    pub known: Option<String>,
    /// A fingerprint the user has just confirmed for a first connection.
    pub accept_new: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum TunnelError {
    #[error("the SSH server {host} is not known yet; its host key fingerprint is {fingerprint}")]
    UnknownHost { host: String, fingerprint: String },
    #[error("the host key of {host} CHANGED (expected {expected}, got {actual}). Someone may be intercepting the connection; it was not used")]
    HostKeyChanged { host: String, expected: String, actual: String },
    #[error("SSH authentication failed: {0}")]
    Auth(String),
    #[error("could not use the SSH key: {0}")]
    Key(String),
    #[error("SSH: {0}")]
    Ssh(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, TunnelError>;

#[derive(Debug, Default)]
struct Observed {
    fingerprint: Option<String>,
    changed_from: Option<String>,
}

struct Client {
    trust: HostTrust,
    observed: Arc<Mutex<Observed>>,
}

impl client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> std::result::Result<bool, Self::Error> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = key else { return Ok(false) };
        let fp = key.fingerprint(HashAlg::Sha256).to_string();
        let mut seen = self.observed.lock().unwrap();
        seen.fingerprint = Some(fp.clone());
        Ok(match (&self.trust.known, &self.trust.accept_new) {
            (Some(known), _) if *known == fp => true,
            (Some(known), _) => {
                seen.changed_from = Some(known.clone());
                false
            }
            (None, Some(accepted)) => *accepted == fp,
            (None, None) => false,
        })
    }
}

/// A running forward. Dropping it closes the listener and the SSH session.
pub struct Tunnel {
    pub local_port: u16,
    /// The host key fingerprint that was verified (save it to trust this server next time).
    pub fingerprint: String,
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

fn expand_home(path: &str) -> String {
    match (path.strip_prefix("~/"), std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"))) {
        (Some(rest), Ok(home)) => format!("{home}/{rest}"),
        _ => path.to_string(),
    }
}

pub async fn open(cfg: &SshConfig, secrets: &SshSecrets, trust: HostTrust, remote_host: &str, remote_port: u16) -> Result<Tunnel> {
    let observed = Arc::new(Mutex::new(Observed::default()));
    let config = Arc::new(client::Config {
        keepalive_interval: Some(Duration::from_secs(30)),
        keepalive_max: 3,
        ..Default::default()
    });
    let label = format!("{}:{}", cfg.host, cfg.port);
    let connect = client::connect(config, (cfg.host.as_str(), cfg.port), Client { trust, observed: observed.clone() });
    let mut handle = match tokio::time::timeout(Duration::from_secs(15), connect).await {
        Err(_) => return Err(TunnelError::Ssh(format!("timed out connecting to {label}"))),
        Ok(Ok(h)) => h,
        Ok(Err(e)) => {
            // a rejected host key surfaces as a generic failure: explain which kind
            let seen = observed.lock().unwrap();
            return Err(match (&seen.fingerprint, &seen.changed_from) {
                (Some(actual), Some(expected)) => TunnelError::HostKeyChanged { host: label, expected: expected.clone(), actual: actual.clone() },
                (Some(fp), None) => TunnelError::UnknownHost { host: label, fingerprint: fp.clone() },
                _ => TunnelError::Ssh(e.to_string()),
            });
        }
    };
    let fingerprint = observed.lock().unwrap().fingerprint.clone().unwrap_or_default();

    let authed = match &cfg.auth {
        SshAuth::Password => {
            let pw = secrets.password.clone().ok_or_else(|| TunnelError::Auth("no password was provided".into()))?;
            handle.authenticate_password(cfg.user.clone(), pw).await.map_err(|e| TunnelError::Auth(e.to_string()))?
        }
        SshAuth::Key { path } => {
            let key = load_secret_key(expand_home(path), secrets.key_passphrase.as_deref()).map_err(|e| TunnelError::Key(e.to_string()))?;
            let hash = handle.best_supported_rsa_hash().await.ok().flatten().flatten();
            handle
                .authenticate_publickey(cfg.user.clone(), PrivateKeyWithHashAlg::new(Arc::new(key), hash))
                .await
                .map_err(|e| TunnelError::Auth(e.to_string()))?
        }
    };
    if !authed.success() {
        return Err(TunnelError::Auth(format!("{label} rejected the credentials for user {}", cfg.user)));
    }

    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let local_port = listener.local_addr()?.port();
    let handle: Arc<Handle<Client>> = Arc::new(handle);
    let (rhost, rport) = (remote_host.to_string(), remote_port);

    let accept = tokio::spawn(async move {
        // every forwarded connection lives in this set: when the tunnel is dropped the accept task is aborted, the set is dropped with
        // it, and so every open connection is closed too (not only the listener)
        let mut connections = tokio::task::JoinSet::new();
        loop {
            let accepted = tokio::select! {
                a = listener.accept() => a,
                Some(_) = connections.join_next(), if !connections.is_empty() => continue, // finished ones are cleaned up
            };
            let Ok((mut tcp, peer)) = accepted else { break };
            let handle = handle.clone();
            let rhost = rhost.clone();
            connections.spawn(async move {
                match handle.channel_open_direct_tcpip(rhost, rport as u32, peer.ip().to_string(), peer.port() as u32).await {
                    Ok(channel) => {
                        let mut stream = channel.into_stream();
                        let _ = tokio::io::copy_bidirectional(&mut tcp, &mut stream).await;
                    }
                    Err(_) => { /* the remote refused or the session is gone: closing `tcp` tells the client */ }
                }
            });
        }
    });
    Ok(Tunnel { local_port, fingerprint, tasks: vec![accept] })
}
