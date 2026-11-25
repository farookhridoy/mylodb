//! Tunnel tests against a throwaway `sshd` started on a random port with its own host and client keys.
//! Skipped when /usr/sbin/sshd or ssh-keygen is missing. Password auth cannot be tested this way
//! (a non-root sshd authenticates against the real system password), so only key auth is covered here.

use myq_tunnel::*;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

struct Sshd {
    dir: PathBuf,
    child: Child,
    port: u16,
    host_fp: String,
}

impl Drop for Sshd {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn keygen(path: &Path) -> String {
    let ok = Command::new("ssh-keygen").args(["-q", "-t", "ed25519", "-N", "", "-f"]).arg(path).status().unwrap().success();
    assert!(ok, "ssh-keygen failed");
    // `SHA256:xxxx` as printed by ssh-keygen -lf
    let out = Command::new("ssh-keygen").args(["-l", "-E", "sha256", "-f"]).arg(path.with_extension("pub")).output().unwrap();
    String::from_utf8(out.stdout).unwrap().split_whitespace().nth(1).unwrap().to_string()
}

fn start_sshd() -> Option<Sshd> {
    if !Path::new("/usr/sbin/sshd").exists() || Command::new("ssh-keygen").arg("-?").stderr(Stdio::null()).stdout(Stdio::null()).status().is_err() {
        eprintln!("SKIP: sshd or ssh-keygen not available");
        return None;
    }
    // tests run in parallel: every server gets its own directory
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("mylodb_sshd_{}_{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let host_fp = keygen(&dir.join("host"));
    keygen(&dir.join("client"));
    std::fs::copy(dir.join("client.pub"), dir.join("authorized_keys")).unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let cfg = format!(
        "Port {port}\nListenAddress 127.0.0.1\nHostKey {d}/host\nPidFile {d}/sshd.pid\nAuthorizedKeysFile {d}/authorized_keys\n\
         PubkeyAuthentication yes\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nStrictModes no\nAllowTcpForwarding yes\nLogLevel ERROR\n",
        d = dir.display()
    );
    std::fs::write(dir.join("sshd_config"), cfg).unwrap();
    let child = Command::new("/usr/sbin/sshd").args(["-D", "-e", "-f"]).arg(dir.join("sshd_config")).stdout(Stdio::null()).stderr(Stdio::null()).spawn().ok()?;
    let sshd = Sshd { dir, child, port, host_fp };
    for _ in 0..50 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return Some(sshd);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    eprintln!("SKIP: sshd did not start");
    None
}

fn config(s: &Sshd, key: &str) -> SshConfig {
    SshConfig { host: "127.0.0.1".into(), port: s.port, user: std::env::var("USER").unwrap_or_else(|_| "root".into()), auth: SshAuth::Key { path: s.dir.join(key).to_string_lossy().into() } }
}

/// A TCP echo server standing in for the database; returns its port.
async fn echo_server() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else { break };
            tokio::spawn(async move {
                let (mut r, mut w) = s.split();
                let _ = tokio::io::copy(&mut r, &mut w).await;
            });
        }
    });
    port
}

#[tokio::test]
async fn host_key_policy_is_fail_closed() {
    let Some(sshd) = start_sshd() else { return };
    let echo = echo_server().await;
    let cfg = config(&sshd, "client");
    let secrets = SshSecrets::default();

    // unknown server: refused, with the fingerprint to show the user
    match open(&cfg, &secrets, HostTrust::default(), "127.0.0.1", echo).await {
        Err(TunnelError::UnknownHost { fingerprint, .. }) => assert_eq!(fingerprint, sshd.host_fp, "fingerprint matches ssh-keygen's"),
        other => panic!("expected UnknownHost, got {:?}", other.map(|t| t.local_port)),
    }
    // confirming a DIFFERENT fingerprint does not help
    let wrong = HostTrust { known: None, accept_new: Some("SHA256:notTheRealOne".into()) };
    assert!(matches!(open(&cfg, &secrets, wrong, "127.0.0.1", echo).await, Err(TunnelError::UnknownHost { .. })));
    // a saved fingerprint that no longer matches is a hard error naming both
    let changed = HostTrust { known: Some("SHA256:oldKey".into()), accept_new: Some(sshd.host_fp.clone()) };
    match open(&cfg, &secrets, changed, "127.0.0.1", echo).await {
        Err(TunnelError::HostKeyChanged { expected, actual, .. }) => assert_eq!((expected.as_str(), actual.as_str()), ("SHA256:oldKey", sshd.host_fp.as_str())),
        other => panic!("expected HostKeyChanged, got {:?}", other.map(|t| t.local_port)),
    }
    // confirmed first use works, and so does a matching saved fingerprint
    let t = open(&cfg, &secrets, HostTrust { known: None, accept_new: Some(sshd.host_fp.clone()) }, "127.0.0.1", echo).await.unwrap();
    assert_eq!(t.fingerprint, sshd.host_fp);
    let t2 = open(&cfg, &secrets, HostTrust { known: Some(sshd.host_fp.clone()), accept_new: None }, "127.0.0.1", echo).await.unwrap();
    assert_ne!(t.local_port, t2.local_port);
}

#[tokio::test]
async fn forwards_data_concurrently_and_closes_with_the_tunnel() {
    let Some(sshd) = start_sshd() else { return };
    let echo = echo_server().await;
    let trust = HostTrust { known: Some(sshd.host_fp.clone()), accept_new: None };
    let tunnel = open(&config(&sshd, "client"), &SshSecrets::default(), trust, "127.0.0.1", echo).await.unwrap();
    let port = tunnel.local_port;

    // 1 MB through the tunnel, from 5 connections at once
    let payload: Vec<u8> = (0..1_000_000u32).map(|i| (i % 251) as u8).collect();
    let mut joins = Vec::new();
    for _ in 0..5 {
        let payload = payload.clone();
        joins.push(tokio::spawn(async move {
            let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            let (mut r, mut w) = s.split();
            let write = async { w.write_all(&payload).await.unwrap(); w.shutdown().await.unwrap(); };
            let read = async { let mut out = Vec::new(); r.read_to_end(&mut out).await.unwrap(); out };
            let (_, out) = tokio::join!(write, read);
            assert_eq!(out.len(), payload.len());
            assert!(out == payload, "data was corrupted in transit");
        }));
    }
    for j in joins { j.await.unwrap(); }

    // a connection that is still open when the tunnel is dropped is closed with it
    let mut open_one = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    open_one.write_all(b"hello").await.unwrap();
    let mut buf = [0u8; 5];
    open_one.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"hello");

    // dropping the tunnel stops listening. Aborting is asynchronous, so wait for it (up to 10 s) instead of guessing a delay.
    drop(tunnel);
    let mut gone = false;
    for _ in 0..100 {
        if TcpStream::connect(("127.0.0.1", port)).await.is_err() {
            gone = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(gone, "the listener must be gone");
    let mut rest = Vec::new();
    let closed = tokio::time::timeout(std::time::Duration::from_secs(10), open_one.read_to_end(&mut rest)).await;
    assert!(closed.is_ok(), "the open connection must be closed when the tunnel is dropped");
}

#[tokio::test]
async fn authentication_and_connection_errors_are_specific() {
    let Some(sshd) = start_sshd() else { return };
    let trust = || HostTrust { known: Some(sshd.host_fp.clone()), accept_new: None };
    let secrets = SshSecrets::default();

    // a valid key the server does not know
    let stranger = sshd.dir.join("stranger");
    keygen(&stranger);
    let r = open(&config(&sshd, "stranger"), &secrets, trust(), "127.0.0.1", 1).await;
    assert!(matches!(r, Err(TunnelError::Auth(_))), "{:?}", r.map(|t| t.local_port));
    // key file problems
    let r = open(&config(&sshd, "does-not-exist"), &secrets, trust(), "127.0.0.1", 1).await;
    assert!(matches!(r, Err(TunnelError::Key(_))), "{:?}", r.map(|t| t.local_port));
    // password method without a password
    let mut pw = config(&sshd, "client");
    pw.auth = SshAuth::Password;
    assert!(matches!(open(&pw, &secrets, trust(), "127.0.0.1", 1).await, Err(TunnelError::Auth(m)) if m.contains("no password")));
    // nothing listening
    let mut dead = config(&sshd, "client");
    dead.port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    assert!(matches!(open(&dead, &secrets, trust(), "127.0.0.1", 1).await, Err(TunnelError::Ssh(_) | TunnelError::Io(_))));

    // the remote end refuses: the client connection is closed instead of hanging
    let closed_port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let t = open(&config(&sshd, "client"), &secrets, trust(), "127.0.0.1", closed_port).await.unwrap();
    let mut s = TcpStream::connect(("127.0.0.1", t.local_port)).await.unwrap();
    let mut buf = [0u8; 8];
    let n = tokio::time::timeout(std::time::Duration::from_secs(5), s.read(&mut buf)).await.expect("must not hang").unwrap_or(0);
    assert_eq!(n, 0);
}

#[tokio::test]
async fn a_database_connection_works_through_the_tunnel() {
    use myq_driver::{mysql_async::prelude::Queryable, pool, ConnConfig, SslMode};
    let direct = pool(&ConnConfig { host: "127.0.0.1".into(), port: 3306, user: "root".into(), password: None, database: None, ssl: SslMode::Disabled, ..Default::default() });
    if direct.get_conn().await.is_err() {
        return eprintln!("SKIP: no MySQL on 127.0.0.1:3306");
    }
    let Some(sshd) = start_sshd() else { return };
    let trust = HostTrust { known: Some(sshd.host_fp.clone()), accept_new: None };
    let tunnel = open(&config(&sshd, "client"), &SshSecrets::default(), trust, "127.0.0.1", 3306).await.unwrap();
    let via = pool(&ConnConfig { host: "127.0.0.1".into(), port: tunnel.local_port, user: "root".into(), password: None, database: None, ssl: SslMode::Disabled, ..Default::default() });
    let mut c = via.get_conn().await.unwrap();
    let v: Option<String> = c.query_first("SELECT VERSION()").await.unwrap();
    assert!(v.is_some());
}
