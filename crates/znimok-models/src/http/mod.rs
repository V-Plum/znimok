//! HTTPS through the OS: Windows.Web.Http on Windows, NSURLSession on macOS. The OS brings the
//! trusted certificates, the user's proxy (including corporate PAC files) and TLS updates; our
//! dependency tree gets no TLS stack. [`Transport`] is a trait so tests run without a network.

#[cfg(target_os = "macos")]
mod mac;
#[cfg(windows)]
mod win;

use std::fmt;
use std::time::Duration;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
    /// `retry-after`, seconds, when the server sent it (429 / 529).
    pub retry_after: Option<u64>,
    /// `location` (only [`Transport::request`] fills it): where a resumable upload goes on (Google
    /// Drive, ZK-260).
    pub location: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HttpError {
    /// No connection, DNS, TLS, proxy… — the OS message.
    Network(String),
    Timeout,
    Unsupported,
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(m) => write!(f, "network: {m}"),
            Self::Timeout => write!(f, "network: timed out"),
            Self::Unsupported => write!(f, "network: not available on this system"),
        }
    }
}

impl std::error::Error for HttpError {}

pub trait Transport: Send + Sync {
    /// POST `body` (JSON) to `url` with extra `headers`.
    fn post_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        timeout: Duration,
    ) -> Result<Response, HttpError>;

    /// GET `url` with extra `headers`; the body as bytes (a release's files, ZK-122). Refuses a
    /// body larger than `max_bytes`.
    fn get(
        &self,
        _url: &str,
        _headers: &[(&str, &str)],
        _timeout: Duration,
        _max_bytes: usize,
    ) -> Result<Response, HttpError> {
        Err(HttpError::Unsupported)
    }

    /// Any request (ZK-101: uploads to Telegram, Jira, Slack, Redmine, a webhook): `method`,
    /// the body with its `content_type` (`None`: no body), the answer's body as bytes, at most
    /// `max_bytes`.
    fn request(
        &self,
        _method: &str,
        _url: &str,
        _headers: &[(&str, &str)],
        _body: Option<(&[u8], &str)>,
        _timeout: Duration,
        _max_bytes: usize,
    ) -> Result<Response, HttpError> {
        Err(HttpError::Unsupported)
    }
}

/// The OS transport.
pub fn system() -> Box<dyn Transport> {
    #[cfg(windows)]
    return Box::new(win::WinHttp);
    #[cfg(target_os = "macos")]
    return Box::new(mac::MacHttp);
    #[cfg(not(any(windows, target_os = "macos")))]
    return Box::new(NoHttp);
}

#[cfg(not(any(windows, target_os = "macos")))]
struct NoHttp;

#[cfg(not(any(windows, target_os = "macos")))]
impl Transport for NoHttp {
    fn post_json(
        &self,
        _: &str,
        _: &[(&str, &str)],
        _: &[u8],
        _: Duration,
    ) -> Result<Response, HttpError> {
        Err(HttpError::Unsupported)
    }
}

/// Parses `retry-after` (seconds; an HTTP date is not used by the Claude API).
pub(crate) fn retry_after(v: Option<String>) -> Option<u64> {
    v.and_then(|s| s.trim().parse().ok())
}

/// Reaches the real Claude API without a key: the answer must be 401 with a JSON error —
/// proves TLS, proxy and HTTP work end to end and costs nothing.
#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn real_https_to_the_claude_api_without_a_key() {
        if std::env::var_os("ZNIMOK_OFFLINE").is_some() {
            return;
        }
        let r = system()
            .post_json(
                "https://api.anthropic.com/v1/messages",
                &[
                    ("anthropic-version", "2023-06-01"),
                    ("x-api-key", "invalid"),
                ],
                br#"{"model":"claude-haiku-4-5","max_tokens":1,"messages":[]}"#,
                Duration::from_secs(30),
            )
            .unwrap();
        assert_eq!(r.status, 401, "{}", String::from_utf8_lossy(&r.body));
        let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(v["type"], "error", "{v}");
    }
}
