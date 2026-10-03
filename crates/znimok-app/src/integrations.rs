//! Sending to Telegram, Jira, Slack, Redmine and webhooks from the app (ZK-101): the queue's
//! worker (one, started with the app), the settings it reads at the moment of sending, and what
//! it says back. The targets themselves are `znimok-share`.

use std::sync::{Mutex, OnceLock};

use znimok_settings::Integrations;
use znimok_share::queue::{self, Event, Worker};
use znimok_share::{Item, TargetId};

static WORKER: OnceLock<Worker> = OnceLock::new();
/// The integrations as the settings have them now (the worker reads them for each sending).
static SETTINGS: Mutex<Option<Integrations>> = Mutex::new(None);

pub fn queue_dir() -> std::path::PathBuf {
    crate::library::cache_dir().join("share-queue")
}

/// The settings changed: the next sending uses them.
pub fn set_settings(i: &Integrations) {
    if let Ok(mut s) = SETTINGS.lock() {
        *s = Some(i.clone());
    }
}

/// Starts the worker once (the library window, the first one): what was left from the last run
/// is sent too.
pub fn start() {
    WORKER.get_or_init(|| {
        queue::start(
            queue_dir(),
            || {
                SETTINGS
                    .lock()
                    .ok()
                    .and_then(|s| s.clone())
                    .unwrap_or_default()
            },
            |ev| {
                let _ = slint::invoke_from_event_loop(move || {
                    crate::with_ctx(|a, ui| a.share_event(ui, ev));
                });
            },
        )
    });
}

/// Puts a file into the queue for `target` and wakes the worker.
pub fn send(target: TargetId, item: Item, bytes: &[u8]) -> Result<(), String> {
    start();
    queue::enqueue(&queue_dir(), target, item, bytes).map_err(|e| e.to_string())?;
    if let Some(w) = WORKER.get() {
        w.wake();
    }
    Ok(())
}

/// The target's name in messages: the menu's name for it.
pub fn target_name(i: &Integrations, t: &TargetId) -> String {
    znimok_share::ready(i)
        .into_iter()
        .find(|(id, _)| id == t)
        .map(|(_, n)| n)
        .unwrap_or_else(|| match t {
            TargetId::Webhook(_) => "Webhook".into(),
            other => {
                let k = other.key();
                let mut c = k.chars();
                c.next()
                    .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                    .unwrap_or(k)
            }
        })
}

/// What the worker said, as a toast's words: `(key, target)`; the app translates.
pub fn event_words(ev: &Event) -> (&'static str, &TargetId, Option<String>) {
    match ev {
        Event::Sent { job, sent } => ("share-sent", &job.target, sent.url.clone()),
        Event::Retrying { job, error } => ("share-retrying", &job.target, Some(error.clone())),
        Event::Failed { job, error } => ("share-failed", &job.target, Some(error.clone())),
    }
}

/// The media type of a file by its extension.
pub fn mime_of(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "mp4" => "video/mp4",
        "html" => "text/html",
        "zreport" => "application/zip",
        "znimok" => "application/octet-stream",
        _ => "application/octet-stream",
    }
}

/// The target «Send» in one click goes to: the default one if it is ready, else the first ready.
pub fn quick_target(i: &Integrations) -> Option<(String, String)> {
    let ready = znimok_share::ready(i);
    ready
        .iter()
        .find(|(id, _)| id.key() == i.default_target)
        .or_else(|| ready.first())
        .map(|(id, n)| (id.key(), n.clone()))
}

/// The integrations as the settings had them last (no borrow of the App).
pub fn current() -> Integrations {
    SETTINGS
        .lock()
        .ok()
        .and_then(|s| s.clone())
        .unwrap_or_default()
}

/// A webhook's id: short, made once.
pub fn new_webhook_id() -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{:x}", t & 0xff_ffff_ffff)
}
