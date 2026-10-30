//! Checking for a newer BadTerm: one HTTPS request for the newest release
//! on the project's Forgejo, made only when the user has turned the check
//! on. It sends nothing but the request itself (with a "BadTerm/x.y.z"
//! user agent).

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

const HOST: &str = "git.badcluster.dev";
const PATH: &str = "/api/v1/repos/arelas/badterm/releases?limit=1";
/// Where people download BadTerm.
pub const DOWNLOAD_PAGE: &str = "https://badterm.com/";
const TIMEOUT: Duration = Duration::from_secs(20);
/// More than any release listing needs.
const MAX_RESPONSE: usize = 1 << 20;

/// The newest release: its version, without the "v".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
}

/// Ask for the newest release. Blocks; run it on its own thread.
pub fn latest(current: &str) -> Result<Release> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let body = rt
        .block_on(async { tokio::time::timeout(TIMEOUT, get(current)).await })
        .map_err(|_| anyhow!("timed out"))??;
    parse(&body)
}

/// Whether `candidate` ("0.1.8") is a later version than `current`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (version(candidate), version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

fn version(v: &str) -> Option<Vec<u64>> {
    v.trim().trim_start_matches('v').split('.').map(|n| n.parse().ok()).collect()
}

fn parse(body: &[u8]) -> Result<Release> {
    let releases: serde_json::Value = serde_json::from_slice(body).context("reading the release list")?;
    let tag = releases
        .get(0)
        .and_then(|r| r.get("tag_name"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow!("no releases listed"))?;
    Ok(Release { version: tag.trim_start_matches('v').to_string() })
}

async fn get(current: &str) -> Result<Vec<u8>> {
    let roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let tcp = TcpStream::connect((HOST, 443)).await.with_context(|| format!("connecting to {HOST}"))?;
    let name = ServerName::try_from(HOST)?;
    let mut tls = TlsConnector::from(Arc::new(config)).connect(name, tcp).await?;
    // HTTP/1.0: the reply ends when the connection does.
    let request = format!(
        "GET {PATH} HTTP/1.0\r\nHost: {HOST}\r\nUser-Agent: BadTerm/{current}\r\nAccept: application/json\r\n\r\n"
    );
    tls.write_all(request.as_bytes()).await?;
    let mut response = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match tls.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => response.extend_from_slice(&buf[..n]),
            // Servers often close without TLS's close_notify.
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
        if response.len() > MAX_RESPONSE {
            bail!("reply too long");
        }
    }
    body(&response)
}

/// The body of an HTTP response, checking for a 200 and undoing chunked
/// transfer encoding if the server used it anyway.
fn body(response: &[u8]) -> Result<Vec<u8>> {
    let split = response.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| anyhow!("no HTTP headers"))?;
    let head = String::from_utf8_lossy(&response[..split]).to_ascii_lowercase();
    let status = head.lines().next().unwrap_or("");
    if status.split_whitespace().nth(1) != Some("200") {
        bail!("the server answered {status}");
    }
    let mut body = &response[split + 4..];
    if !head.lines().any(|l| l.starts_with("transfer-encoding:") && l.contains("chunked")) {
        return Ok(body.to_vec());
    }
    let mut out = Vec::new();
    loop {
        let end = body.windows(2).position(|w| w == b"\r\n").ok_or_else(|| anyhow!("bad chunk"))?;
        let size_text = String::from_utf8_lossy(&body[..end]);
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)?;
        body = &body[end + 2..];
        if size == 0 {
            return Ok(out);
        }
        let chunk = body.get(..size).ok_or_else(|| anyhow!("short chunk"))?;
        out.extend_from_slice(chunk);
        body = body.get(size + 2..).unwrap_or(&[]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(is_newer("0.1.8", "0.1.7"));
        assert!(is_newer("v0.2.0", "0.1.10"));
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(!is_newer("0.1.7", "0.1.7"));
        assert!(!is_newer("0.1.6", "0.1.7"));
        assert!(!is_newer("nightly", "0.1.7"));
    }

    #[test]
    fn responses() {
        let json = br#"[{"tag_name":"v0.1.8","name":"v0.1.8"}]"#;
        let plain = [b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n".as_slice(), json].concat();
        assert_eq!(parse(&body(&plain).unwrap()).unwrap().version, "0.1.8");
        let chunked = [
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".as_slice(),
            format!("{:x}\r\n", 10).as_bytes(),
            &json[..10],
            b"\r\n",
            format!("{:x}\r\n", json.len() - 10).as_bytes(),
            &json[10..],
            b"\r\n0\r\n\r\n",
        ]
        .concat();
        assert_eq!(body(&chunked).unwrap(), json);
        assert!(body(b"HTTP/1.1 404 Not Found\r\n\r\n").is_err());
        assert!(parse(b"[]").is_err());
    }
}
