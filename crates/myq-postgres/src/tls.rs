//! TLS for PostgreSQL connections, with the same four choices as the MySQL ones (`SslMode`).

use myq_driver::{ConnConfig, SslMode};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::crypto::{aws_lc_rs, verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{CertificateError, ClientConfig, DigitallySignedStruct, Error, RootCertStore, SignatureScheme};
use std::sync::Arc;

fn read_pem(path: &str) -> Result<Vec<u8>, String> {
    let full = match (path.strip_prefix("~/"), std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"))) {
        (Some(rest), Ok(home)) => std::path::PathBuf::from(home).join(rest),
        _ => path.into(),
    };
    std::fs::read(&full).map_err(|e| format!("cannot read {path}: {e}"))
}

fn certificates(path: &str) -> Result<Vec<CertificateDer<'static>>, String> {
    let found: Vec<_> = rustls_pemfile::certs(&mut read_pem(path)?.as_slice()).collect::<Result<_, _>>().map_err(|e| format!("{path}: {e}"))?;
    if found.is_empty() {
        return Err(format!("{path} holds no certificate"));
    }
    Ok(found)
}

/// `VerifyCa` checks the chain but not the name; `SkipVerify` checks neither. Signatures are always checked.
#[derive(Debug)]
struct Relaxed {
    chain: Option<Arc<WebPkiServerVerifier>>,
    provider: CryptoProvider,
}

impl ServerCertVerifier for Relaxed {
    fn verify_server_cert(&self, cert: &CertificateDer<'_>, intermediates: &[CertificateDer<'_>], name: &ServerName<'_>, ocsp: &[u8], now: UnixTime) -> Result<ServerCertVerified, Error> {
        let Some(chain) = &self.chain else { return Ok(ServerCertVerified::assertion()) };
        match chain.verify_server_cert(cert, intermediates, name, ocsp, now) {
            Err(Error::InvalidCertificate(CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. })) => Ok(ServerCertVerified::assertion()),
            other => other,
        }
    }
    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }
    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

pub(crate) fn client_config(cfg: &ConnConfig) -> Result<ClientConfig, String> {
    let provider = aws_lc_rs::default_provider();
    let mut roots = RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    if let Some(ca) = cfg.ssl_ca.as_deref().filter(|c| !c.is_empty()) {
        for cert in certificates(ca)? {
            roots.add(cert).map_err(|e| format!("{ca}: {e}"))?;
        }
    }
    let text = |e: Error| e.to_string();
    let builder = ClientConfig::builder_with_provider(Arc::new(provider.clone())).with_safe_default_protocol_versions().map_err(text)?;
    let builder = match cfg.ssl {
        SslMode::VerifyCa => {
            let chain = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), Arc::new(provider.clone())).build().map_err(|e| e.to_string())?;
            builder.dangerous().with_custom_certificate_verifier(Arc::new(Relaxed { chain: Some(chain), provider }))
        }
        SslMode::SkipVerify => builder.dangerous().with_custom_certificate_verifier(Arc::new(Relaxed { chain: None, provider })),
        _ => builder.with_root_certificates(roots),
    };
    match (cfg.ssl_cert.as_deref().filter(|c| !c.is_empty()), cfg.ssl_key.as_deref().filter(|k| !k.is_empty())) {
        (Some(cert), Some(key)) => {
            let chain = certificates(cert)?;
            let key: PrivateKeyDer<'static> = rustls_pemfile::private_key(&mut read_pem(key)?.as_slice()).map_err(|e| format!("{key}: {e}"))?.ok_or_else(|| format!("{key} holds no private key"))?;
            builder.with_client_auth_cert(chain, key).map_err(text)
        }
        _ => Ok(builder.with_no_client_auth()),
    }
}

// ---- the glue between rustls and tokio-postgres ----

pub(crate) mod connector {
    use rustls::pki_types::ServerName;
    use std::future::Future;
    use std::io;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
    use tokio_postgres::tls::{ChannelBinding, MakeTlsConnect, TlsConnect, TlsStream};
    use tokio_rustls::{client, TlsConnector};

    #[derive(Clone)]
    pub struct Make(pub Arc<rustls::ClientConfig>);

    pub struct Connect {
        config: Arc<rustls::ClientConfig>,
        name: ServerName<'static>,
    }

    pub struct Stream<S>(client::TlsStream<S>);

    impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> MakeTlsConnect<S> for Make {
        type Stream = Stream<S>;
        type TlsConnect = Connect;
        type Error = rustls::pki_types::InvalidDnsNameError;
        fn make_tls_connect(&mut self, domain: &str) -> Result<Connect, Self::Error> {
            Ok(Connect { config: self.0.clone(), name: ServerName::try_from(domain.to_string())? })
        }
    }

    impl<S: AsyncRead + AsyncWrite + Unpin + Send + 'static> TlsConnect<S> for Connect {
        type Stream = Stream<S>;
        type Error = io::Error;
        type Future = Pin<Box<dyn Future<Output = io::Result<Stream<S>>> + Send>>;
        fn connect(self, stream: S) -> Self::Future {
            Box::pin(async move { TlsConnector::from(self.config).connect(self.name, stream).await.map(Stream) })
        }
    }

    impl<S: AsyncRead + AsyncWrite + Unpin> TlsStream for Stream<S> {
        fn channel_binding(&self) -> ChannelBinding {
            ChannelBinding::none()
        }
    }

    impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for Stream<S> {
        fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.0).poll_read(cx, buf)
        }
    }

    impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for Stream<S> {
        fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
            Pin::new(&mut self.0).poll_write(cx, buf)
        }
        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.0).poll_flush(cx)
        }
        fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.0).poll_shutdown(cx)
        }
    }
}
