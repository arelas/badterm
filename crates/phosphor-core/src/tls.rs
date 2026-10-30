//! Telnet over TLS ("TelnetS", usually port 992), on rustls with the same
//! ring crypto as SSH.
//!
//! A certificate is first checked against the web's certificate authorities
//! (the Mozilla roots in `webpki-roots`). Many BBSes use self-signed
//! certificates, so one that fails that check is trusted on first use and
//! pinned by its SHA-256 fingerprint in `tls_known_hosts`, like SSH host
//! keys; a later, different certificate is refused. A system once verified
//! by an authority is remembered as such, so it cannot later be passed off
//! with a self-signed certificate.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::{Context, Result, anyhow, bail};
use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::TlsConnector;

use crate::transport::{CONNECT_TIMEOUT, Cmd, Emitter, NetEvent, Writer, WriterKind};

/// The pin for a system verified by a certificate authority.
const CA_VERIFIED: &str = "ca";

pub(crate) fn spawn_tls<F>(host: String, port: u16, pins: PathBuf, sink: F)
where
    F: Fn(NetEvent) + Send + 'static,
{
    thread::spawn(move || {
        let emitter = Emitter::new(sink);
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(e) => return emitter.emit(NetEvent::Closed(Some(format!("starting TLS runtime: {e}")))),
        };
        let result = rt.block_on(run_tls(&host, port, pins, emitter.clone()));
        emitter.emit(NetEvent::Closed(result.err().map(|e| format!("{e:#}"))));
    });
}

async fn run_tls(host: &str, port: u16, pins: PathBuf, emitter: Emitter) -> Result<()> {
    let tcp = match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host, port))).await {
        Err(_) => bail!("connecting to {host}:{port}: timed out"),
        Ok(r) => r.with_context(|| format!("connecting to {host}:{port}"))?,
    };
    let _ = tcp.set_nodelay(true);

    let verifier = Arc::new(Verifier::new(host, port, pins, emitter.clone())?);
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .dangerous()
        .with_custom_certificate_verifier(verifier.clone())
        .with_no_client_auth();
    let name = ServerName::try_from(host.to_string()).map_err(|_| anyhow!("{host} is not a valid host name"))?;
    let handshake = TlsConnector::from(Arc::new(config)).connect(name, tcp);
    let tls = match tokio::time::timeout(CONNECT_TIMEOUT, handshake).await {
        Err(_) => bail!("TLS handshake with {host}:{port}: timed out"),
        Ok(Err(e)) => {
            if let Some(msg) = verifier.problem.lock().unwrap_or_else(|e| e.into_inner()).take() {
                bail!(msg);
            }
            return Err(e).with_context(|| format!("TLS handshake with {host}:{port}"));
        }
        Ok(Ok(s)) => s,
    };

    let (tx, mut rx) = mpsc::unbounded_channel();
    emitter.emit(NetEvent::Connected(Writer(WriterKind::Channel(tx))));
    let (mut read, mut write) = tokio::io::split(tls);
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        tokio::select! {
            n = read.read(&mut buf) => match n {
                Ok(0) => break,
                Ok(n) => emitter.emit(NetEvent::Data(buf[..n].to_vec())),
                // Many servers close without TLS's close_notify.
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e).context("reading"),
            },
            cmd = rx.recv() => match cmd {
                Some(Cmd::Data(d)) => {
                    write.write_all(&d).await.context("sending data")?;
                    write.flush().await.context("sending data")?;
                }
                Some(Cmd::Close) | None => {
                    let _ = write.shutdown().await;
                    break;
                }
            },
        }
    }
    Ok(())
}

/// Certificate authorities first, then trust on first use.
struct Verifier {
    webpki: Arc<WebPkiServerVerifier>,
    host: String,
    port: u16,
    pins: PathBuf,
    emitter: Emitter,
    /// Why the certificate was refused, reported instead of rustls's error.
    problem: Mutex<Option<String>>,
}

impl fmt::Debug for Verifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Verifier").field("host", &self.host).field("port", &self.port).finish()
    }
}

impl Verifier {
    fn new(host: &str, port: u16, pins: PathBuf, emitter: Emitter) -> Result<Verifier> {
        let roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let provider: Arc<CryptoProvider> = Arc::new(rustls::crypto::ring::default_provider());
        let webpki = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider).build()?;
        Ok(Verifier { webpki, host: host.to_string(), port, pins, emitter, problem: Mutex::new(None) })
    }

    fn refuse(&self, msg: String) -> rustls::Error {
        *self.problem.lock().unwrap_or_else(|e| e.into_inner()) = Some(msg.clone());
        rustls::Error::General(msg)
    }
}

impl ServerCertVerifier for Verifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let ca = self.webpki.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now);
        let fingerprint = fingerprint(end_entity);
        match check_pin(&self.pins, &self.host, self.port, &fingerprint, ca.is_ok()) {
            Ok(notice) => {
                if let Some(n) = notice {
                    self.emitter.emit(NetEvent::Notice(n));
                }
                Ok(ServerCertVerified::assertion())
            }
            Err(msg) => Err(self.refuse(msg)),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.webpki.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.webpki.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.webpki.supported_verify_schemes()
    }
}

/// "sha256:" and the certificate's SHA-256 in hex.
fn fingerprint(cert: &CertificateDer<'_>) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, cert.as_ref());
    let hex: String = digest.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

/// Decide on a certificate with this `fingerprint`, which a certificate
/// authority vouched for (`ca_ok`) or not, and update the pins. Returns a
/// notice for the user when something new was trusted, or why the
/// certificate is refused.
pub fn check_pin(pins: &Path, host: &str, port: u16, fingerprint: &str, ca_ok: bool) -> Result<Option<String>, String> {
    let key = pin_key(host, port);
    let text = match fs::read_to_string(pins) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("reading {}: {e}", pins.display())),
    };
    let pinned = text.lines().find_map(|l| l.split_once(' ').filter(|(k, _)| *k == key).map(|(_, v)| v.trim()));
    let save = |value: &str| -> Result<(), String> {
        let mut lines: Vec<String> =
            text.lines().filter(|l| l.split_once(' ').is_none_or(|(k, _)| k != key)).map(String::from).collect();
        lines.push(format!("{key} {value}"));
        if let Some(dir) = pins.parent() {
            let _ = fs::create_dir_all(dir);
        }
        fs::write(pins, lines.join("\n") + "\n").map_err(|e| format!("saving {}: {e}", pins.display()))
    };
    match (pinned, ca_ok) {
        (Some(CA_VERIFIED), true) => Ok(None),
        // Verified now: remember that, replacing any self-signed pin.
        (_, true) => save(CA_VERIFIED).map(|()| None),
        (Some(CA_VERIFIED), false) => Err(format!(
            "The TLS certificate of {host} is no longer signed by a certificate authority, as it was before. \
             Someone may be intercepting the connection. If the BBS really changed it, remove the line for \
             {key} from {}.",
            pins.display()
        )),
        (Some(p), false) if p == fingerprint => Ok(None),
        (Some(_), false) => Err(format!(
            "The TLS certificate of {host} has CHANGED (now {fingerprint}). Someone may be intercepting the \
             connection. If the BBS really changed it, remove the line for {key} from {}.",
            pins.display()
        )),
        (None, false) => save(fingerprint).map(|()| {
            Some(format!("Trusted the self-signed TLS certificate of {host} (first connection): {fingerprint}"))
        }),
    }
}

fn pin_key(host: &str, port: u16) -> String {
    let host = host.to_ascii_lowercase();
    if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins() {
        let dir = std::env::temp_dir().join(format!("phosphor-tls-pins-{}", std::process::id()));
        let pins = dir.join("tls_known_hosts");
        let _ = fs::remove_dir_all(&dir);

        // Self-signed: trusted on first use, then held to its fingerprint.
        assert!(check_pin(&pins, "bbs.example", 992, "sha256:aa", false).unwrap().is_some());
        assert_eq!(check_pin(&pins, "BBS.example", 992, "sha256:aa", false), Ok(None));
        assert!(check_pin(&pins, "bbs.example", 992, "sha256:bb", false).unwrap_err().contains("CHANGED"));
        // Another port is another system.
        assert!(check_pin(&pins, "bbs.example", 993, "sha256:bb", false).unwrap().is_some());

        // Verified by an authority: remembered, so self-signed is refused.
        assert_eq!(check_pin(&pins, "bbs.example", 992, "sha256:cc", true), Ok(None));
        assert!(check_pin(&pins, "bbs.example", 992, "sha256:aa", false).unwrap_err().contains("no longer"));
        assert_eq!(check_pin(&pins, "other.example", 992, "sha256:dd", true), Ok(None));

        let text = fs::read_to_string(&pins).unwrap();
        assert_eq!(text, "bbs.example:993 sha256:bb\nbbs.example:992 ca\nother.example:992 ca\n");
        assert_eq!(pin_key("::1", 992), "[::1]:992");
        fs::remove_dir_all(&dir).unwrap();
    }
}
