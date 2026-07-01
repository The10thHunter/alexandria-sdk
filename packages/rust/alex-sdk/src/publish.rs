//! Publish a packed `.atool` / `.aagent` archive to an Alexandria registry.
//!
//! Mirrors the TypeScript SDK `publish()`: the archive is re-verified (hashes +
//! schema) then a multipart body is POSTed to `{registry}/v1/submit` — the
//! missing consumer-side half of the registry loop (`alexandria install` pulls;
//! nothing pushed until now).
//!
//! The HTTP send is abstracted behind the [`Transport`] trait so callers (and
//! tests) can inject a mock. [`publish`] uses the built-in [`UreqTransport`].

use std::path::Path;

use crate::pack::verify;
use crate::{Error, Result};

/// Outcome of a publish attempt.
#[derive(Debug, Clone)]
pub struct PublishResult {
    pub status: u16,
    pub ok: bool,
    pub body: String,
    pub name: String,
    pub version: String,
    pub artifact_type: String,
}

/// Options controlling a publish call.
#[derive(Debug, Clone, Default)]
pub struct PublishOptions {
    /// Optional bearer token (sent as `Authorization: Bearer <token>`).
    pub token: Option<String>,
    /// Override the artifact_type; defaults to the manifest's kind.
    pub artifact_type: Option<String>,
}

/// Pluggable HTTP transport. `send` receives the fully-formed URL, the raw
/// multipart body, its content-type, and an optional bearer token, and returns
/// `(status, response_body)`.
pub trait Transport {
    fn send(
        &self,
        url: &str,
        content_type: &str,
        body: &[u8],
        token: Option<&str>,
    ) -> Result<(u16, String)>;
}

/// Default transport backed by the `ureq` blocking HTTP client.
pub struct UreqTransport;

impl Transport for UreqTransport {
    fn send(
        &self,
        url: &str,
        content_type: &str,
        body: &[u8],
        token: Option<&str>,
    ) -> Result<(u16, String)> {
        let mut req = ureq::post(url).header("content-type", content_type);
        if let Some(t) = token {
            req = req.header("authorization", &format!("Bearer {t}"));
        }
        match req.send(body) {
            Ok(mut resp) => {
                let status = resp.status().as_u16();
                let text = resp.body_mut().read_to_string().unwrap_or_default();
                Ok((status, text))
            }
            // ureq returns Err for non-2xx status codes; surface them as a
            // normal (status, body) pair rather than a hard error.
            Err(ureq::Error::StatusCode(code)) => Ok((code, String::new())),
            Err(e) => Err(Error::Other(format!("http transport error: {e}"))),
        }
    }
}

/// Assemble a `multipart/form-data` body carrying an `artifact_type` text field
/// and a `tarball` file part. Returns `(body, content_type)`.
pub fn build_multipart(artifact_type: &str, filename: &str, tarball: &[u8]) -> (Vec<u8>, String) {
    let boundary = format!("----alexsdk{:x}", fastrand_boundary());
    let mut body: Vec<u8> = Vec::with_capacity(tarball.len() + 256);

    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"artifact_type\"\r\n\r\n");
    body.extend_from_slice(artifact_type.as_bytes());
    body.extend_from_slice(b"\r\n");

    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"tarball\"; filename=\"{filename}\"\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(b"Content-Type: application/gzip\r\n\r\n");
    body.extend_from_slice(tarball);
    body.extend_from_slice(b"\r\n");

    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    let content_type = format!("multipart/form-data; boundary={boundary}");
    (body, content_type)
}

/// Cheap boundary nonce — no crypto needed, just uniqueness within a process.
fn fastrand_boundary() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    nanos ^ (std::process::id() as u128).rotate_left(64)
}

/// Publish `pkg_path` to `{registry}/v1/submit` using the given transport.
///
/// The archive is re-verified before it ships — never publish an archive the
/// local runtime would itself reject.
pub fn publish_with<T: Transport>(
    pkg_path: &Path,
    registry: &str,
    opts: &PublishOptions,
    transport: &T,
) -> Result<PublishResult> {
    let manifest = verify(pkg_path)?;
    let kind = serde_json::to_value(&manifest.kind)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_default();
    let artifact_type = opts.artifact_type.clone().unwrap_or(kind);

    let tarball = std::fs::read(pkg_path)?;
    let filename = pkg_path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "package".to_string());
    let (body, content_type) = build_multipart(&artifact_type, &filename, &tarball);

    let base = registry.trim_end_matches('/');
    let url = format!("{base}/v1/submit");

    let (status, text) = transport.send(&url, &content_type, &body, opts.token.as_deref())?;

    Ok(PublishResult {
        status,
        ok: (200..300).contains(&status),
        body: text,
        name: manifest.name,
        version: manifest.version,
        artifact_type,
    })
}

/// Publish `pkg_path` using the default [`UreqTransport`].
pub fn publish(pkg_path: &Path, registry: &str, opts: &PublishOptions) -> Result<PublishResult> {
    publish_with(pkg_path, registry, opts, &UreqTransport)
}
