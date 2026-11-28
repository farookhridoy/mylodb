//! SSL modes against a local MySQL that has TLS enabled (MySQL generates a self-signed CA on first start).

use myq_driver::mysql_async::prelude::Queryable;
use myq_driver::{pool, ConnConfig, SslMode};

fn cfg(ssl: SslMode, ca: Option<String>) -> ConnConfig {
    ConnConfig {
        host: std::env::var("MYLODB_TEST_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port: std::env::var("MYLODB_TEST_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3306),
        user: std::env::var("MYLODB_TEST_USER").unwrap_or_else(|_| "root".into()),
        password: std::env::var("MYLODB_TEST_PASSWORD").ok(),
        ssl,
        ssl_ca: ca,
        ..Default::default()
    }
}

async fn cipher(c: ConnConfig) -> Result<Option<String>, String> {
    let p = pool(&c);
    let mut conn = p.get_conn().await.map_err(|e| e.to_string())?;
    let row: Option<(String, String)> = conn.query_first("SHOW SESSION STATUS LIKE 'Ssl_cipher'").await.map_err(|e| e.to_string())?;
    Ok(row.map(|r| r.1).filter(|c| !c.is_empty()))
}

#[tokio::test]
async fn tls_modes_behave_as_documented() {
    // plain connection works, and is not encrypted
    let plain = match cipher(cfg(SslMode::Disabled, None)).await {
        Ok(c) => c,
        Err(e) => return eprintln!("SKIP: no MySQL reachable: {e}"),
    };
    assert_eq!(plain, None, "SslMode::Disabled must not use TLS");

    // find out whether the server speaks TLS at all
    let p = pool(&cfg(SslMode::Disabled, None));
    let mut c = p.get_conn().await.unwrap();
    let have_ssl: Option<(String, String)> = c.query_first("SHOW VARIABLES LIKE 'have_ssl'").await.unwrap();
    // MySQL 9 removed the variable; it speaks TLS unless told otherwise
    if have_ssl.is_some_and(|v| v.1 != "YES") {
        return eprintln!("SKIP: the server has no TLS");
    }
    let datadir: Option<String> = c.query_first("SELECT @@datadir").await.unwrap();
    drop(c);

    // skipping verification accepts the self-signed certificate and encrypts
    let c = cipher(cfg(SslMode::SkipVerify, None)).await.expect("skip-verify connects");
    assert!(c.is_some(), "the session must actually be encrypted");

    // full verification must refuse a certificate nobody vouches for
    assert!(cipher(cfg(SslMode::Required, None)).await.is_err(), "an untrusted certificate must be rejected");

    // trusting the server's own CA makes verification pass (the host name check is skipped by VerifyCa)
    let ca = datadir.map(|d| format!("{}ca.pem", d)).filter(|p| std::fs::metadata(p).is_ok());
    match ca {
        Some(ca) => {
            // MySQL's own certificate does not name the host: VerifyCa accepts it all the same
            let c = cipher(cfg(SslMode::VerifyCa, Some(ca.clone()))).await.expect("verify-ca with the server's CA connects");
            assert!(c.is_some());
            // while full verification, trusting the same CA, still insists on the name
            assert!(cipher(cfg(SslMode::Required, Some(ca))).await.is_err_and(|e| e.contains("not valid for")), "the host name is checked");
            // a CA that did not sign the server certificate is refused
            let other = std::env::temp_dir().join(format!("mylodb_wrong_ca_{}.pem", std::process::id()));
            std::fs::write(&other, include_str!("wrong_ca.pem")).unwrap();
            let r = cipher(cfg(SslMode::VerifyCa, Some(other.to_string_lossy().into()))).await;
            let _ = std::fs::remove_file(&other);
            assert!(r.is_err(), "a different CA must not validate the server");
        }
        None => eprintln!("NOTE: the server's ca.pem is not readable here; verify-ca was not exercised"),
    }
}

/// The application links two TLS crypto libraries; an encrypted connection must still not panic. (The test needs no server.)
#[tokio::test]
async fn an_encrypted_connection_never_panics_for_want_of_a_crypto_provider() {
    let _ = pool(&cfg(SslMode::Required, None));
    assert!(rustls::crypto::CryptoProvider::get_default().is_some());
}
