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
    /// Where in the target, chosen when sharing (ZK-278): a Slack channel's ID, a Jira project
    /// (`ZK`) or issue (`ZK-101`), a Redmine project or issue (`#42`), a Telegram chat. Empty =
    /// the one in the settings.
    #[serde(default)]
    pub place: String,
}

/// A place in a target to send to (ZK-278): what goes into [`Item::place`], and its name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    pub id: String,
    pub name: String,
}

/// The places of a target the person can pick from when sharing: the Slack channels the token
/// sees, the Jira or Redmine projects, the Telegram chats that wrote to the bot (and the one in
/// the settings). Google Drive and webhooks have none.
pub fn places(
    t: &dyn Transport,
    vault: &Vault,
    cfg: &Integrations,
    id: &TargetId,
) -> Result<Vec<Place>, ShareError> {
    let pairs = match id {
        TargetId::Telegram(a) => {
            let acc = account(cfg.telegram_account(a))?;
            let mut out = telegram::find_chats(t, &token(vault, id)?)?;
            let known = acc.chat_id.trim();
            if !known.is_empty() && !out.iter().any(|(i, _)| i == known) {
                let name = if acc.chat_title.is_empty() {
                    known.to_string()
                } else {
                    acc.chat_title.clone()
                };
                out.push((known.to_string(), name));
            }
            out
        }
        TargetId::Slack(_) => slack::channels(t, &slack_token(t, vault, id)?)?,
        TargetId::Jira(a) => {
            let acc = account(cfg.jira_account(a))?;
            jira::projects(t, acc, &jira_token(t, vault, id, acc)?)?
        }
        TargetId::Redmine(a) => {
            redmine::projects(t, account(cfg.redmine_account(a))?, &token(vault, id)?)?
        }
        TargetId::Google(_) | TargetId::Gmail(_) | TargetId::Webhook(_) => Vec::new(),
    };
    Ok(pairs
        .into_iter()
        .map(|(id, name)| Place { id, name })
        .collect())
}

fn account<T>(a: Option<&T>) -> Result<&T, ShareError> {
    a.ok_or_else(|| ShareError::Fail("this account is no longer in the settings".into()))
}

/// `ZK-101` is an issue, `ZK` a project.
fn jira_issue_key(s: &str) -> bool {
    s.split_once('-').is_some_and(|(p, n)| {
        !p.is_empty() && !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())
    })
}

/// Which target: the keys of [`Integrations::default_target`]. A service's target carries the
/// account (ZK-280): empty = the first one (`slack`), else `slack:<id>`; Google's is the
/// account's `sub` (empty = the one marked in the settings).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TargetId {
    Google(String),
    /// Gmail (ZK-262): the file into the Google account's Drive, open to anyone with the link,
    /// then Gmail's new letter with the link — no permission to the mail.
    Gmail(String),
    Telegram(String),
    Jira(String),
    Slack(String),
    Redmine(String),
    Webhook(String),
}

impl TargetId {
    /// The service's word: `google`, `telegram`, `jira`, `slack`, `redmine`, `webhook`.
    pub fn service(&self) -> &'static str {
        match self {
            Self::Google(_) => "google",
            Self::Gmail(_) => "gmail",
            Self::Telegram(_) => "telegram",
            Self::Jira(_) => "jira",
            Self::Slack(_) => "slack",
            Self::Redmine(_) => "redmine",
            Self::Webhook(_) => "webhook",
        }
    }

    /// The account (or the webhook) inside the service.
    pub fn account(&self) -> &str {
        match self {
            Self::Google(a)
            | Self::Gmail(a)
            | Self::Telegram(a)
            | Self::Jira(a)
            | Self::Slack(a)
            | Self::Redmine(a)
            | Self::Webhook(a) => a,
        }
    }

    pub fn key(&self) -> String {
        match (self, self.account()) {
            (Self::Webhook(id), _) => format!("webhook:{id}"),
            (_, "") => self.service().to_string(),
            (_, a) => format!("{}:{a}", self.service()),
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let (service, account) = s.split_once(':').unwrap_or((s, ""));
        let a = account.to_string();
        Some(match service {
            "google" => Self::Google(a),
            "gmail" => Self::Gmail(a),
            "telegram" => Self::Telegram(a),
            "jira" => Self::Jira(a),
            "slack" => Self::Slack(a),
            "redmine" => Self::Redmine(a),
            "webhook" if !a.is_empty() => Self::Webhook(a),
            _ => return None,
        })
    }
}

// On disk (the queue's jobs) a target is its key. A job left by 0.0.16 has the old form — a
// variant's name (`"Telegram"`) or `{"Webhook": "<id>"}` — and is read too.
impl Serialize for TargetId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.key())
    }
}

impl<'de> Deserialize<'de> for TargetId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Key(String),
            Webhook {
                #[serde(rename = "Webhook")]
                webhook: String,
            },
        }
        let key = match Raw::deserialize(d)? {
            Raw::Key(k) => k.to_lowercase_first(),
            Raw::Webhook { webhook } => format!("webhook:{webhook}"),
        };
        TargetId::parse(&key).ok_or_else(|| serde::de::Error::custom(format!("no target {key}")))
    }
}

trait LowerFirst {
    fn to_lowercase_first(self) -> String;
}

impl LowerFirst for String {
    /// `Telegram` → `telegram`; `slack:a1` stays.
    fn to_lowercase_first(self) -> String {
        let mut c = self.chars();
        match c.next() {
            Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
            None => self,
        }
    }
}

/// «Slack · Client A» when the account has a name, else the service with its place.
fn label(service: &str, name: &str, place: &str, n: usize) -> String {
    if !name.trim().is_empty() {
        format!("{service} · {}", name.trim())
    } else if !place.is_empty() || n == 0 {
        named(service, place)
    } else {
        format!("{service} {}", n + 1)
    }
}

/// The targets that are switched on and filled in enough to send, in the order of the menu:
/// each account of a service is a target of its own (ZK-280).
pub fn ready(cfg: &Integrations) -> Vec<(TargetId, String)> {
    let mut out = Vec::new();
    if cfg.google.enabled {
        for a in &cfg.google.accounts {
            out.push((
                TargetId::Google(a.id.clone()),
                format!("Google Drive · {}", a.email),
            ));
        }
        for a in &cfg.google.accounts {
            out.push((
                TargetId::Gmail(a.id.clone()),
                format!("Gmail · {}", a.email),
            ));
        }
    }
    if cfg.telegram.enabled {
        for (n, t) in cfg.telegram_accounts().enumerate() {
            let place = if t.chat_title.is_empty() {
                t.chat_id.trim()
            } else {
                t.chat_title.as_str()
            };
            out.push((
                TargetId::Telegram(t.id.clone()),
                label("Telegram", &t.name, place, n),
            ));
        }
    }
    if cfg.jira.enabled {
        for (n, j) in cfg.jira_accounts().enumerate() {
            if j.site.trim().is_empty() {
                continue;
            }
            let what = if j.issue.trim().is_empty() {
                j.project.trim()
            } else {
                j.issue.trim()
            };
            out.push((
                TargetId::Jira(j.id.clone()),
                label("Jira", &j.name, what, n),
            ));
        }
    }
    if cfg.slack.enabled {
        for (n, a) in cfg.slack_accounts().enumerate() {
            out.push((
                TargetId::Slack(a.id.clone()),
                label("Slack", &a.name, a.channel.trim(), n),
            ));
        }
    }
    if cfg.redmine.enabled {
        for (n, r) in cfg.redmine_accounts().enumerate() {
            if r.url.trim().is_empty() {
                continue;
            }
            let what = if r.issue.trim().is_empty() {
                r.project.trim().to_string()
            } else {
                format!("#{}", r.issue.trim())
            };
            out.push((
                TargetId::Redmine(r.id.clone()),
                label("Redmine", &r.name, &what, n),
            ));
        }
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

/// The targets that can send without asking where exactly (ZK-279): the one-click «Send to» of
/// the card after a shot and of the export sheets. A target that needs a place has one in the
/// settings or one sent to last.
pub fn ready_now(cfg: &Integrations) -> Vec<(TargetId, String)> {
    ready(cfg)
        .into_iter()
        .filter(|(id, _)| !needs_place(id) || !default_place(cfg, id).is_empty())
        .collect()
}

/// Whether a target sends to a place inside it (a channel, a project, a chat).
pub fn needs_place(id: &TargetId) -> bool {
    matches!(
        id,
        TargetId::Telegram(_) | TargetId::Slack(_) | TargetId::Jira(_) | TargetId::Redmine(_)
    )
}

/// Where a target sends when nothing is chosen: the place sent to last, else the settings' one.
pub fn default_place(cfg: &Integrations, id: &TargetId) -> String {
    if let Some(p) = cfg
        .share_memory
        .get(&id.key())
        .and_then(|m| m.recent.first())
        .filter(|p| !p.id.trim().is_empty())
    {
        return p.id.clone();
    }
    match id {
        TargetId::Telegram(a) => cfg
            .telegram_account(a)
            .map(|t| t.chat_id.trim().to_string())
            .unwrap_or_default(),
        TargetId::Slack(a) => cfg
            .slack_account(a)
            .map(|t| t.channel.trim().to_string())
            .unwrap_or_default(),
        TargetId::Jira(a) => cfg
            .jira_account(a)
            .map(|j| {
                if j.issue.trim().is_empty() {
                    j.project.trim().to_string()
                } else {
                    j.issue.trim().to_string()
                }
            })
            .unwrap_or_default(),
        TargetId::Redmine(a) => cfg
            .redmine_account(a)
            .map(|r| {
                if r.issue.trim().is_empty() {
                    r.project.trim().to_string()
                } else {
                    format!("#{}", r.issue.trim())
                }
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// «Slack · C0123», or «Slack» when the place is chosen when sharing.
fn named(service: &str, place: &str) -> String {
    if place.is_empty() {
        service.to_string()
    } else {
        format!("{service} · {place}")
    }
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

/// The name in the OS store of a service account's token (ZK-280) — the first account keeps its
/// own [`znimok_settings::Secret`].
pub fn account_secret_name(service: &str, id: &str) -> String {
    format!("share-{service}-{id}")
}

/// A target account's token into the OS store (empty = deleted): the first account keeps its
/// own secret, the others one named after them (ZK-280).
pub fn set_token(vault: &Vault, id: &TargetId, value: &str) -> Result<(), String> {
    use znimok_settings::Secret;
    let legacy = match id {
        TargetId::Telegram(_) => Secret::TelegramBotToken,
        TargetId::Jira(_) => Secret::JiraApiToken,
        TargetId::Slack(_) => Secret::SlackBotToken,
        TargetId::Redmine(_) => Secret::RedmineApiKey,
        _ => return Ok(()),
    };
    let r = match (id.account(), value.is_empty()) {
        ("", true) => vault.delete(legacy).map(|_| ()),
        ("", false) => vault.set(legacy, value),
        (a, true) => vault
            .delete_named(&account_secret_name(id.service(), a))
            .map(|_| ()),
        (a, false) => vault.set_named(&account_secret_name(id.service(), a), value),
    };
    r.map_err(|e| e.to_string())
}

/// Slack's token for a call: a pasted one as it is; a signed-in account's refreshed (ZK-273),
/// the turned-over refresh token kept.
fn slack_token(t: &dyn Transport, vault: &Vault, id: &TargetId) -> Result<String, ShareError> {
    let stored = token(vault, id)?;
    let Some(rt) = stored.strip_prefix(slack::SIGNED_IN) else {
        return Ok(stored);
    };
    let cid = slack::client_id().ok_or_else(|| {
        ShareError::Fail("Slack: signing in is not available in this build".into())
    })?;
    let (access, next) = slack::refresh(t, cid, slack::client_secret(), rt)?;
    if next != rt {
        set_token(vault, id, &format!("{}{next}", slack::SIGNED_IN)).map_err(ShareError::Fail)?;
    }
    Ok(access)
}

/// Jira's token for a call: a pasted API token as it is; a signed-in account's refreshed into
/// `bearer:<cloud id>:<access>` (ZK-273), the turned-over refresh token kept.
fn jira_token(
    t: &dyn Transport,
    vault: &Vault,
    id: &TargetId,
    acc: &znimok_settings::JiraTarget,
) -> Result<String, ShareError> {
    let stored = token(vault, id)?;
    let Some(rt) = stored.strip_prefix(slack::SIGNED_IN) else {
        return Ok(stored);
    };
    let client = jira::client().ok_or_else(|| {
        ShareError::Fail("Jira: signing in is not available in this build".into())
    })?;
    if acc.cloud_id.is_empty() {
        return Err(ShareError::Fail("Jira: sign in to Atlassian again".into()));
    }
    let (access, next) = jira::refresh(t, &client, rt)?;
    if next != rt {
        set_token(vault, id, &format!("{}{next}", slack::SIGNED_IN)).map_err(ShareError::Fail)?;
    }
    Ok(format!("{}{}:{access}", jira::SIGNED_IN_CALL, acc.cloud_id))
}

/// The token (the key) of a target's account, from the OS store.
pub fn token(vault: &Vault, id: &TargetId) -> Result<String, ShareError> {
    use znimok_settings::Secret;
    let what = match id {
        TargetId::Telegram(_) => "Telegram",
        TargetId::Jira(_) => "Jira",
        TargetId::Slack(_) => "Slack",
        TargetId::Redmine(_) => "Redmine",
        _ => return Err(ShareError::Fail("no token for this target".into())),
    };
    let got = match (id, id.account()) {
        (TargetId::Telegram(_), "") => vault.get(Secret::TelegramBotToken),
        (TargetId::Jira(_), "") => vault.get(Secret::JiraApiToken),
        (TargetId::Slack(_), "") => vault.get(Secret::SlackBotToken),
        (TargetId::Redmine(_), "") => vault.get(Secret::RedmineApiKey),
        (_, a) => vault.get_named(&account_secret_name(id.service(), a)),
    };
    match got {
        Ok(Some(t)) if !t.trim().is_empty() => Ok(t.trim().to_string()),
        Ok(_) => Err(ShareError::Fail(format!(
            "{what}: no token in the settings"
        ))),
        Err(e) => Err(ShareError::Fail(format!("{what}: {e}"))),
    }
}

/// The place chosen when sharing, else the one in the settings.
fn or<'a>(chosen: &'a str, settings: &'a str) -> &'a str {
    if chosen.trim().is_empty() {
        settings
    } else {
        chosen.trim()
    }
}

/// An access token for a Google account (`sub`; empty = the one marked in the settings).
fn google_access(
    t: &dyn Transport,
    vault: &Vault,
    cfg: &Integrations,
    sub: &str,
) -> Result<String, ShareError> {
    let client = google::client()
        .ok_or_else(|| ShareError::Fail("Google: not available in this build".into()))?;
    let account = if sub.is_empty() {
        cfg.google.current()
    } else {
        cfg.google.accounts.iter().find(|a| a.id == sub)
    }
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

/// Gmail's new letter (ZK-262) from `email`'s account: the subject, the words and the link; the
/// person adds who it goes to.
pub fn gmail_compose_url(email: &str, subject: &str, text: &str, link: &str) -> String {
    use multipart::percent;
    let body = if text.trim().is_empty() {
        link.to_string()
    } else {
        format!("{}\n\n{link}", text.trim())
    };
    let mut u = format!(
        "https://mail.google.com/mail/?view=cm&fs=1&su={}&body={}",
        percent(subject),
        percent(&body)
    );
    if !email.is_empty() {
        u.push_str(&format!("&authuser={}", percent(email)));
    }
    u
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
    match id {
        TargetId::Google(sub) | TargetId::Gmail(sub) => {
            google::check(t, &google_access(t, vault, cfg, sub)?)
        }
        TargetId::Telegram(a) => {
            let chat = account(cfg.telegram_account(a))?.chat_id.clone();
            telegram::check(t, &token(vault, id)?, &chat, probe)
        }
        TargetId::Jira(a) => {
            let acc = account(cfg.jira_account(a))?;
            jira::check(t, acc, &jira_token(t, vault, id, acc)?)
        }
        TargetId::Slack(a) => {
            let channel = account(cfg.slack_account(a))?.channel.clone();
            slack::check(t, &slack_token(t, vault, id)?, &channel)
        }
        TargetId::Redmine(a) => {
            redmine::check(t, account(cfg.redmine_account(a))?, &token(vault, id)?)
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
    let chosen;
    let item = if item.place.trim().is_empty() && needs_place(id) {
        chosen = Item {
            place: default_place(cfg, id),
            ..item.clone()
        };
        &chosen
    } else {
        item
    };
    match id {
        TargetId::Google(sub) => {
            let a = google_access(t, vault, cfg, sub)?;
            google::send(t, &a, cfg.google.link_anyone, item, bytes)
        }
        // The people the letter goes to must open the link.
        TargetId::Gmail(sub) => {
            let a = google_access(t, vault, cfg, sub)?;
            google::send(t, &a, true, item, bytes)
        }
        TargetId::Telegram(a) => {
            let acc = account(cfg.telegram_account(a))?;
            telegram::send(
                t,
                &token(vault, id)?,
                or(&item.place, &acc.chat_id),
                item,
                bytes,
            )
        }
        TargetId::Jira(a) => {
            let mut j = account(cfg.jira_account(a))?.clone();
            let place = item.place.trim();
            if jira_issue_key(place) {
                j.issue = place.to_uppercase();
            } else if !place.is_empty() {
                j.project = place.to_uppercase();
                j.issue.clear();
            }
            let tok = jira_token(t, vault, id, &j)?;
            jira::send(t, &j, &tok, item, bytes)
        }
        TargetId::Slack(a) => {
            let acc = account(cfg.slack_account(a))?;
            slack::send(
                t,
                &slack_token(t, vault, id)?,
                or(&item.place, &acc.channel),
                item,
                bytes,
            )
        }
        TargetId::Redmine(a) => {
            let mut r = account(cfg.redmine_account(a))?.clone();
            let place = item.place.trim();
            if let Some(n) = place.strip_prefix('#') {
                r.issue = n.to_string();
            } else if !place.is_empty() {
                r.project = place.to_string();
                r.issue.clear();
            }
            redmine::send(t, &r, &token(vault, id)?, item, bytes)
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
            TargetId::Google(String::new()),
            TargetId::Telegram(String::new()),
            TargetId::Jira(String::new()),
            TargetId::Slack(String::new()),
            TargetId::Redmine(String::new()),
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
        // ZK-280: each account is a target of its own.
        let all = ready(&cfg);
        assert_eq!(
            all[0],
            (
                TargetId::Google("1".into()),
                "Google Drive · a@x.org".into()
            )
        );
        assert_eq!(
            all[1],
            (
                TargetId::Google("2".into()),
                "Google Drive · b@x.org".into()
            )
        );
        cfg.slack.enabled = true;
        cfg.slack.name = "Plum".into();
        cfg.slack_more.push(znimok_settings::SlackTarget {
            id: "c2".into(),
            name: "Client B".into(),
            ..Default::default()
        });
        cfg.slack_more.push(znimok_settings::SlackTarget {
            id: "c3".into(),
            ..Default::default()
        });
        let slack: Vec<(String, String)> = ready(&cfg)
            .into_iter()
            .filter(|(id, _)| id.service() == "slack")
            .map(|(id, n)| (id.key(), n))
            .collect();
        assert_eq!(
            slack,
            [
                ("slack".to_string(), "Slack · Plum".to_string()),
                ("slack:c2".into(), "Slack · Client B".into()),
                ("slack:c3".into(), "Slack 3".into()),
            ]
        );
        assert_eq!(
            TargetId::parse("slack:c2"),
            Some(TargetId::Slack("c2".into()))
        );
    }

    #[test]
    fn gmail_next_to_drive_and_its_letter() {
        let mut cfg = Integrations::default();
        cfg.google.enabled = true;
        cfg.google.accounts.push(znimok_settings::GoogleAccount {
            id: "1".into(),
            email: "a@x.org".into(),
        });
        let keys: Vec<String> = ready(&cfg).into_iter().map(|(id, _)| id.key()).collect();
        assert_eq!(keys, ["google:1", "gmail:1"]);
        assert!(!needs_place(&TargetId::Gmail("1".into())));
        let u = gmail_compose_url(
            "a@x.org",
            "Знімок",
            "див. кнопку",
            "https://drive.google.com/file/d/F1/view",
        );
        assert!(u.starts_with("https://mail.google.com/mail/?view=cm&fs=1&su=%D0%97"));
        assert!(u.contains("&body=%D0%B4") && u.contains("%0A%0Ahttps%3A%2F%2Fdrive.google.com"));
        assert!(u.ends_with("&authuser=a%40x.org"));
    }

    #[test]
    fn a_job_left_by_0_0_16_is_read() {
        // The queue keeps the target as its key now; the old form is read too.
        for (json, key) in [
            (r#""Telegram""#, "telegram"),
            (r#"{"Webhook":"w1"}"#, "webhook:w1"),
            (r#""slack:c2""#, "slack:c2"),
        ] {
            let id: TargetId = serde_json::from_str(json).unwrap();
            assert_eq!(id.key(), key);
        }
        let back = serde_json::to_string(&TargetId::Jira("a".into())).unwrap();
        assert_eq!(back, r#""jira:a""#);
    }

    #[test]
    fn each_account_its_token() {
        let v = Vault::new("znimok-test-zk280");
        let _ = v.set_named(&account_secret_name("slack", "c2"), "xoxb-two");
        assert_eq!(
            token(&v, &TargetId::Slack("c2".into())).unwrap(),
            "xoxb-two"
        );
        assert!(token(&v, &TargetId::Slack("c9".into())).is_err());
        let _ = v.delete_named(&account_secret_name("slack", "c2"));
    }

    #[test]
    fn the_place_chosen_when_sharing_wins() {
        assert!(jira_issue_key("ZK-101") && !jira_issue_key("ZK") && !jira_issue_key("A-B"));
        // Slack: the chosen channel instead of the settings' one.
        let f = fake::Fake::new(&[
            (
                200,
                r#"{"ok":true,"upload_url":"https://u/1","file_id":"F1"}"#,
            ),
            (200, "OK"),
            (200, r#"{"ok":true,"files":[{"permalink":"p"}]}"#),
        ]);
        let mut cfg = Integrations::default();
        cfg.slack.channel = "CSETTINGS".into();
        let item = Item {
            file_name: "a.png".into(),
            mime: "image/png".into(),
            place: "CCHOSEN".into(),
            ..Default::default()
        };
        slack::send(&f, "xoxb", or(&item.place, &cfg.slack.channel), &item, b"x").unwrap();
        assert!(f.seen()[2].body_text().contains("channel_id=CCHOSEN"));
        assert_eq!(or("", "CSETTINGS"), "CSETTINGS");
    }

    #[test]
    fn ready_now_wants_a_place_where_one_is_needed() {
        let mut cfg = Integrations::default();
        cfg.slack.enabled = true;
        cfg.telegram.enabled = true;
        // Ready in the window (the place is chosen there), not in one click.
        assert_eq!(ready(&cfg).len(), 2);
        assert!(ready_now(&cfg).is_empty());
        cfg.slack.channel = "C1".into();
        assert_eq!(ready_now(&cfg)[0].0, TargetId::Slack(String::new()));
        // The place sent to last counts as well, and comes first.
        cfg.share_memory.insert(
            "telegram".into(),
            znimok_settings::ShareMemory {
                recent: vec![znimok_settings::SharePlace {
                    id: "42".into(),
                    name: "Plum".into(),
                }],
                ..Default::default()
            },
        );
        assert_eq!(ready_now(&cfg).len(), 2);
        assert_eq!(
            default_place(&cfg, &TargetId::Telegram(String::new())),
            "42"
        );
        cfg.redmine.issue = "7".into();
        assert_eq!(default_place(&cfg, &TargetId::Redmine(String::new())), "#7");
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
