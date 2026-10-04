//! Sharing targets (ZK-101): a screenshot, a recording or a report sent where the work is
//! discussed — Google Drive (ZK-260), a Telegram chat, a Jira issue, a Slack channel, a Redmine issue, or any service
//! through a webhook.
//!
//! Each target can check its connection ([`check`]) and send one file ([`send`]). The HTTP goes
//! through the OS stack ([`znimok_models::http`]): the system's certificates and proxy, no TLS of
//! our own. Tokens and keys come from the OS store ([`znimok_settings::Vault`]) at the moment of
//! sending, never from `settings.json`. [`queue`] keeps what is being sent on disk and retries
//! what failed for a reason that passes (no network, the service busy).

pub mod google;
pub mod jira;
mod multipart;
pub mod queue;
pub mod redmine;
pub mod slack;
pub mod telegram;
pub mod webhook;

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use znimok_models::http::{HttpError, Response, Transport};
use znimok_settings::{Integrations, Vault};

/// What is sent.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// The file's name as the receiver shows it (`Знімок 2026-10-04.png`).
    pub file_name: String,
    /// `image/png`, `video/mp4`, `text/html`, `application/zip`…
    pub mime: String,
    /// The document's title (a new issue's summary, the message's first line).
    pub title: String,
    /// Words that go with it (a caption, a comment); may be empty.
    pub text: String,
    /// `screenshot`, `video`, `report`, `document` (a `.znimok`), `log` (JSON) — for a webhook's
    /// receiver.
    pub kind: String,
}

/// Which target: the keys of [`Integrations::default_target`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TargetId {
    Google,
    Telegram,
    Jira,
    Slack,
    Redmine,
    Webhook(String),
}

impl TargetId {
    pub fn key(&self) -> String {
        match self {
            Self::Google => "google".into(),
            Self::Telegram => "telegram".into(),
            Self::Jira => "jira".into(),
            Self::Slack => "slack".into(),
            Self::Redmine => "redmine".into(),
            Self::Webhook(id) => format!("webhook:{id}"),
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "google" => Self::Google,
            "telegram" => Self::Telegram,
            "jira" => Self::Jira,
            "slack" => Self::Slack,
            "redmine" => Self::Redmine,
            _ => Self::Webhook(s.strip_prefix("webhook:")?.to_string()),
        })
    }
}

/// The targets that are switched on and filled in enough to send, in the order of the menu.
pub fn ready(cfg: &Integrations) -> Vec<(TargetId, String)> {
    let mut out = Vec::new();
    if cfg.google.enabled
        && let Some(a) = cfg.google.current()
    {
        out.push((TargetId::Google, format!("Google Drive · {}", a.email)));
    }
    if cfg.telegram.enabled && !cfg.telegram.chat_id.trim().is_empty() {
        let t = &cfg.telegram;
        let name = if t.chat_title.is_empty() {
            "Telegram".into()
        } else {
            format!("Telegram · {}", t.chat_title)
        };
        out.push((TargetId::Telegram, name));
    }
    if cfg.jira.enabled && !cfg.jira.site.trim().is_empty() && !cfg.jira.project.trim().is_empty() {
        let j = &cfg.jira;
        let what = if j.issue.trim().is_empty() {
            j.project.trim().to_string()
        } else {
            j.issue.trim().to_string()
        };
        out.push((TargetId::Jira, format!("Jira · {what}")));
    }
    if cfg.slack.enabled && !cfg.slack.channel.trim().is_empty() {
        out.push((
            TargetId::Slack,
            format!("Slack · {}", cfg.slack.channel.trim()),
        ));
    }
    if cfg.redmine.enabled && !cfg.redmine.url.trim().is_empty() {
        let r = &cfg.redmine;
        let what = if r.issue.trim().is_empty() {
            r.project.trim().to_string()
        } else {
            format!("#{}", r.issue.trim())
        };
        out.push((TargetId::Redmine, format!("Redmine · {what}")));
    }
    for w in &cfg.webhooks {
        if w.enabled && !w.url.trim().is_empty() {
            let name = if w.name.trim().is_empty() {
                "Webhook".to_string()
            } else {
                w.name.trim().to_string()
            };
            out.push((TargetId::Webhook(w.id.clone()), name));
        }
    }
    out
}

/// What a sending gave: where to look at it, when the service says.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sent {
    pub url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShareError {
    /// Worth trying again later: no network, a timeout, the service busy (429, 5xx).
    Again(String),
    /// Will not pass by waiting: a wrong token, a missing chat or project, a file too large.
    Fail(String),
}

impl ShareError {
    pub fn retry(&self) -> bool {
        matches!(self, Self::Again(_))
    }
}

impl fmt::Display for ShareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Again(m) | Self::Fail(m) => f.write_str(m),
        }
    }
}

impl From<HttpError> for ShareError {
    fn from(e: HttpError) -> Self {
        match e {
            HttpError::Unsupported => Self::Fail(e.to_string()),
            _ => Self::Again(e.to_string()),
        }
    }
}

/// An HTTP answer that is not a success, as an error: 401/403/404/400 stop, 429/5xx pass.
pub(crate) fn status_error(service: &str, r: &Response) -> ShareError {
    let detail = String::from_utf8_lossy(&r.body);
    let detail: String = detail.chars().take(300).collect();
    let msg = format!("{service}: HTTP {} {}", r.status, detail.trim());
    if r.status == 429 || r.status >= 500 || r.status == 408 {
        ShareError::Again(msg)
    } else {
        ShareError::Fail(msg)
    }
}

/// How long one request may take; an upload of a large video gets more.
pub(crate) fn timeout_for(bytes: usize) -> Duration {
    Duration::from_secs(30 + (bytes / (256 * 1024)) as u64)
}

/// Answers up to this size are read (JSON of the services).
pub(crate) const MAX_ANSWER: usize = 4 << 20;

fn secret(v: &Vault, s: znimok_settings::Secret, what: &str) -> Result<String, ShareError> {
    match v.get(s) {
        Ok(Some(t)) if !t.trim().is_empty() => Ok(t.trim().to_string()),
        Ok(_) => Err(ShareError::Fail(format!(
            "{what}: no token in the settings"
        ))),
        Err(e) => Err(ShareError::Fail(format!("{what}: {e}"))),
    }
}

/// An access token for the Google account sending goes to.
fn google_access(
    t: &dyn Transport,
    vault: &Vault,
    cfg: &Integrations,
) -> Result<String, ShareError> {
    let client = google::client()
        .ok_or_else(|| ShareError::Fail("Google: not available in this build".into()))?;
    let account = cfg
        .google
        .current()
        .ok_or_else(|| ShareError::Fail("Google: no account signed in".into()))?;
    let refresh = match vault.get_named(&google::secret_name(&account.id)) {
        Ok(Some(r)) if !r.trim().is_empty() => r,
        Ok(_) => {
            return Err(ShareError::Fail(format!(
                "Google: sign in again ({})",
                account.email
            )));
        }
        Err(e) => return Err(ShareError::Fail(format!("Google: {e}"))),
    };
    google::access(t, &client, refresh.trim())
}

/// The name of a webhook's header value in the OS store.
pub fn webhook_secret_name(id: &str) -> String {
    format!("share-webhook-{id}")
}

/// Checks a target's connection with what is in the settings now; the answer says who or where
/// (the bot, the user, the project…). `probe` is the text of the test message where the target
/// sends one (Telegram, a webhook).
pub fn check(
    t: &dyn Transport,
    vault: &Vault,
    cfg: &Integrations,
    id: &TargetId,
    probe: &str,
) -> Result<String, ShareError> {
    use znimok_settings::Secret;
    match id {
        TargetId::Google => google::check(t, &google_access(t, vault, cfg)?),
        TargetId::Telegram => {
            let token = secret(vault, Secret::TelegramBotToken, "Telegram")?;
            telegram::check(t, &token, &cfg.telegram.chat_id, probe)
        }
        TargetId::Jira => {
            let token = secret(vault, Secret::JiraApiToken, "Jira")?;
            jira::check(t, &cfg.jira, &token)
        }
        TargetId::Slack => {
            let token = secret(vault, Secret::SlackBotToken, "Slack")?;
            slack::check(t, &token, &cfg.slack.channel)
        }
        TargetId::Redmine => {
            let key = secret(vault, Secret::RedmineApiKey, "Redmine")?;
            redmine::check(t, &cfg.redmine, &key)
        }
        TargetId::Webhook(wid) => {
            let w = cfg
                .webhooks
                .iter()
                .find(|w| &w.id == wid)
                .ok_or_else(|| ShareError::Fail("no such webhook".into()))?;
            let value = vault.get_named(&webhook_secret_name(wid)).ok().flatten();
            webhook::check(t, w, value.as_deref(), probe)
        }
    }
}

/// Sends `item` with `bytes` to a target.
pub fn send(
    t: &dyn Transport,
    vault: &Vault,
    cfg: &Integrations,
    id: &TargetId,
    item: &Item,
    bytes: &[u8],
) -> Result<Sent, ShareError> {
    use znimok_settings::Secret;
    match id {
        TargetId::Google => {
            let a = google_access(t, vault, cfg)?;
            google::send(t, &a, cfg.google.link_anyone, item, bytes)
        }
        TargetId::Telegram => {
            let token = secret(vault, Secret::TelegramBotToken, "Telegram")?;
            telegram::send(t, &token, &cfg.telegram.chat_id, item, bytes)
        }
        TargetId::Jira => {
            let token = secret(vault, Secret::JiraApiToken, "Jira")?;
            jira::send(t, &cfg.jira, &token, item, bytes)
        }
        TargetId::Slack => {
            let token = secret(vault, Secret::SlackBotToken, "Slack")?;
            slack::send(t, &token, &cfg.slack.channel, item, bytes)
        }
        TargetId::Redmine => {
            let key = secret(vault, Secret::RedmineApiKey, "Redmine")?;
            redmine::send(t, &cfg.redmine, &key, item, bytes)
        }
        TargetId::Webhook(wid) => {
            let w = cfg
                .webhooks
                .iter()
                .find(|w| &w.id == wid)
                .ok_or_else(|| ShareError::Fail("no such webhook".into()))?;
            let value = vault.get_named(&webhook_secret_name(wid)).ok().flatten();
            webhook::send(t, w, value.as_deref(), item, bytes)
        }
    }
}

/// A transport for tests: answers from a list, keeps every request.
#[cfg(test)]
pub(crate) mod fake {
    use std::sync::Mutex;
    use std::time::Duration;

    use znimok_models::http::{HttpError, Response, Transport};

    #[derive(Clone, Debug)]
    pub struct Req {
        pub method: String,
        pub url: String,
        pub headers: Vec<(String, String)>,
        pub body: Vec<u8>,
        pub content_type: String,
    }

    impl Req {
        pub fn body_text(&self) -> String {
            String::from_utf8_lossy(&self.body).into_owned()
        }
        pub fn header(&self, k: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(k))
                .map(|(_, v)| v.as_str())
        }
    }

    pub struct Fake {
        pub answers: Mutex<Vec<(u16, String)>>,
        pub seen: Mutex<Vec<Req>>,
    }

    impl Fake {
        pub fn new(answers: &[(u16, &str)]) -> Self {
            Self {
                answers: Mutex::new(
                    answers
                        .iter()
                        .rev()
                        .map(|(s, b)| (*s, b.to_string()))
                        .collect(),
                ),
                seen: Mutex::new(Vec::new()),
            }
        }
        pub fn seen(&self) -> Vec<Req> {
            self.seen.lock().unwrap().clone()
        }
        fn answer(&self) -> Result<Response, HttpError> {
            let (status, body) = self
                .answers
                .lock()
                .unwrap()
                .pop()
                .unwrap_or((500, "no more answers".into()));
            // «Location: …» on the first line is the answer's header (a resumable upload).
            let (location, body) = match body.strip_prefix("Location: ") {
                Some(rest) => {
                    let (l, b) = rest.split_once('\n').unwrap_or((rest, ""));
                    (Some(l.to_string()), b.to_string())
                }
                None => (None, body),
            };
            Ok(Response {
                status,
                body: body.into_bytes(),
                retry_after: None,
                location,
            })
        }
    }

    impl Transport for Fake {
        fn post_json(
            &self,
            url: &str,
            headers: &[(&str, &str)],
            body: &[u8],
            _: Duration,
        ) -> Result<Response, HttpError> {
            self.request(
                "POST",
                url,
                headers,
                Some((body, "application/json")),
                Duration::ZERO,
                0,
            )
        }
        fn get(
            &self,
            url: &str,
            headers: &[(&str, &str)],
            _: Duration,
            _: usize,
        ) -> Result<Response, HttpError> {
            self.request("GET", url, headers, None, Duration::ZERO, 0)
        }
        fn request(
            &self,
            method: &str,
            url: &str,
            headers: &[(&str, &str)],
            body: Option<(&[u8], &str)>,
            _: Duration,
            _: usize,
        ) -> Result<Response, HttpError> {
            self.seen.lock().unwrap().push(Req {
                method: method.into(),
                url: url.into(),
                headers: headers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                body: body.map(|b| b.0.to_vec()).unwrap_or_default(),
                content_type: body.map(|b| b.1.to_string()).unwrap_or_default(),
            });
            self.answer()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_ids_and_the_menu() {
        for id in [
            TargetId::Google,
            TargetId::Telegram,
            TargetId::Jira,
            TargetId::Slack,
            TargetId::Redmine,
            TargetId::Webhook("a1".into()),
        ] {
            assert_eq!(TargetId::parse(&id.key()), Some(id));
        }
        assert_eq!(TargetId::parse("nothing"), None);
        let mut cfg = Integrations::default();
        assert!(ready(&cfg).is_empty());
        cfg.telegram.enabled = true;
        cfg.telegram.chat_id = "42".into();
        cfg.telegram.chat_title = "Plum".into();
        cfg.jira.enabled = true;
        cfg.jira.site = "x.atlassian.net".into();
        cfg.jira.project = "ZT".into();
        cfg.webhooks.push(znimok_settings::WebhookTarget {
            id: "w1".into(),
            enabled: true,
            name: "n8n".into(),
            url: "https://example.org/hook".into(),
            header: String::new(),
            content: String::new(),
        });
        let names: Vec<String> = ready(&cfg).into_iter().map(|(_, n)| n).collect();
        assert_eq!(names, ["Telegram · Plum", "Jira · ZT", "n8n"]);
        // Google: switched on and an account signed in; the active one, or the first.
        cfg.google.enabled = true;
        assert_eq!(ready(&cfg).len(), 3);
        for (id, email) in [("1", "a@x.org"), ("2", "b@x.org")] {
            cfg.google.accounts.push(znimok_settings::GoogleAccount {
                id: id.into(),
                email: email.into(),
            });
        }
        assert_eq!(ready(&cfg)[0].1, "Google Drive · a@x.org");
        cfg.google.active = "2".into();
        assert_eq!(ready(&cfg)[0].1, "Google Drive · b@x.org");
        cfg.google.active = "gone".into();
        assert_eq!(ready(&cfg)[0].1, "Google Drive · a@x.org");
    }

    #[test]
    fn statuses_that_pass_and_that_do_not() {
        let r = |s| Response {
            status: s,
            body: b"x".to_vec(),
            retry_after: None,
            location: None,
        };
        assert!(status_error("T", &r(503)).retry());
        assert!(status_error("T", &r(429)).retry());
        assert!(!status_error("T", &r(401)).retry());
        assert!(!status_error("T", &r(404)).retry());
    }
}
