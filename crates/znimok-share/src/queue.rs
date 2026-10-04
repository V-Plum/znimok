//! What is being sent, on disk (ZK-101): a job is a JSON file and a copy of the bytes in the
//! queue's folder, so a sending survives no network, a busy service and a restart of the app.
//! One worker thread sends them in turn; what fails for a reason that passes is tried again after
//! 10 s, 30 s, 2 min, 10 min and 30 min, what fails for good is dropped with its reason. The
//! settings (and the tokens in the OS store) are read at the moment of sending, so a token fixed
//! in the settings is used by the next attempt.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use znimok_settings::{Integrations, Vault};

use crate::{Item, Sent, ShareError, TargetId};

/// Waits before each next attempt.
const BACKOFF_S: [u64; 5] = [10, 30, 120, 600, 1800];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub target: TargetId,
    pub item: Item,
    /// Attempts made.
    pub attempts: u32,
    /// Not before this, ms since the epoch.
    pub next_at_ms: i64,
}

/// What happened to a job (the app shows it).
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Sent {
        job: Job,
        sent: Sent,
    },
    /// It will be tried again (the first time only, so the app says it once).
    Retrying {
        job: Job,
        error: String,
    },
    Failed {
        job: Job,
        error: String,
    },
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn paths(dir: &Path, id: &str) -> (PathBuf, PathBuf) {
    (
        dir.join(format!("{id}.json")),
        dir.join(format!("{id}.bin")),
    )
}

/// Puts a job into the queue folder (the bytes copied there).
pub fn enqueue(dir: &Path, target: TargetId, item: Item, bytes: &[u8]) -> std::io::Result<Job> {
    std::fs::create_dir_all(dir)?;
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let id = format!(
        "{:x}-{:x}",
        now_ms(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let job = Job {
        id: id.clone(),
        target,
        item,
        attempts: 0,
        next_at_ms: 0,
    };
    let (json, bin) = paths(dir, &id);
    std::fs::write(&bin, bytes)?;
    // The JSON last: a job without its bytes is never seen.
    let tmp = json.with_extension("json.part");
    std::fs::write(&tmp, serde_json::to_vec(&job).unwrap_or_default())?;
    std::fs::rename(&tmp, &json)?;
    Ok(job)
}

/// The jobs waiting in the folder, oldest first.
pub fn pending(dir: &Path) -> Vec<Job> {
    let mut out: Vec<Job> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok())
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn drop_job(dir: &Path, id: &str) {
    let (json, bin) = paths(dir, id);
    let _ = std::fs::remove_file(json);
    let _ = std::fs::remove_file(bin);
}

fn save(dir: &Path, job: &Job) {
    let (json, _) = paths(dir, &job.id);
    let tmp = json.with_extension("json.part");
    if std::fs::write(&tmp, serde_json::to_vec(job).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, &json);
    }
}

/// What sends one job: the target, the item, the bytes.
pub type SendFn<'a> = dyn Fn(&TargetId, &Item, &[u8]) -> Result<Sent, ShareError> + 'a;

/// One attempt of a job: sends it and updates the folder; the event to report, if any.
pub fn attempt(dir: &Path, job: &Job, send: &SendFn<'_>) -> Option<Event> {
    let (_, bin) = paths(dir, &job.id);
    let Ok(bytes) = std::fs::read(&bin) else {
        drop_job(dir, &job.id);
        return Some(Event::Failed {
            job: job.clone(),
            error: "the file to send is gone".into(),
        });
    };
    match send(&job.target, &job.item, &bytes) {
        Ok(sent) => {
            drop_job(dir, &job.id);
            Some(Event::Sent {
                job: job.clone(),
                sent,
            })
        }
        Err(e) if e.retry() && (job.attempts as usize) < BACKOFF_S.len() => {
            let mut next = job.clone();
            next.next_at_ms = now_ms() + BACKOFF_S[job.attempts as usize] as i64 * 1000;
            next.attempts += 1;
            save(dir, &next);
            (job.attempts == 0).then(|| Event::Retrying {
                job: next,
                error: e.to_string(),
            })
        }
        Err(e) => {
            drop_job(dir, &job.id);
            Some(Event::Failed {
                job: job.clone(),
                error: e.to_string(),
            })
        }
    }
}

/// The running worker: [`Worker::wake`] after [`enqueue`] sends at once.
pub struct Worker {
    tx: Sender<()>,
}

impl Worker {
    pub fn wake(&self) {
        let _ = self.tx.send(());
    }
}

/// Starts the worker over `dir`: it sends what is due — the jobs left from the last run too —
/// with the settings `settings()` gives at that moment, and reports through `notify`.
pub fn start(
    dir: PathBuf,
    settings: impl Fn() -> Integrations + Send + 'static,
    notify: impl Fn(Event) + Send + 'static,
) -> Worker {
    let (tx, rx): (Sender<()>, Receiver<()>) = mpsc::channel();
    std::thread::Builder::new()
        .name("znimok-share".into())
        .spawn(move || {
            let transport = znimok_models::http::system();
            let vault = Vault::default();
            loop {
                let jobs = pending(&dir);
                let now = now_ms();
                let mut next_due: Option<i64> = None;
                for job in jobs {
                    if job.next_at_ms > now {
                        next_due =
                            Some(next_due.map_or(job.next_at_ms, |d: i64| d.min(job.next_at_ms)));
                        continue;
                    }
                    let cfg = settings();
                    let send = |t: &TargetId, i: &Item, b: &[u8]| {
                        crate::send(transport.as_ref(), &vault, &cfg, t, i, b)
                    };
                    if let Some(ev) = attempt(&dir, &job, &send) {
                        notify(ev);
                    }
                }
                let wait = next_due
                    .map(|d| Duration::from_millis((d - now_ms()).max(500) as u64))
                    .unwrap_or(Duration::from_secs(3600));
                match rx.recv_timeout(wait) {
                    Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        })
        .expect("a thread for sending");
    Worker { tx }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("znimok-share-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn sent_jobs_go_retried_ones_wait_failed_ones_go_with_their_reason() {
        let d = dir("q");
        let item = Item {
            file_name: "a.png".into(),
            ..Default::default()
        };
        let j1 = enqueue(&d, TargetId::Telegram(String::new()), item.clone(), b"one").unwrap();
        let j2 = enqueue(&d, TargetId::Jira(String::new()), item.clone(), b"two").unwrap();
        assert_eq!(pending(&d).len(), 2);
        // Sent: gone.
        let ok = |_: &TargetId, _: &Item, b: &[u8]| {
            assert_eq!(b, b"one");
            Ok(Sent {
                url: Some("u".into()),
            })
        };
        assert!(matches!(attempt(&d, &j1, &ok), Some(Event::Sent { .. })));
        assert_eq!(pending(&d).len(), 1);
        // Busy: waits, said once.
        let busy = |_: &TargetId, _: &Item, _: &[u8]| Err(ShareError::Again("503".into()));
        assert!(matches!(
            attempt(&d, &j2, &busy),
            Some(Event::Retrying { .. })
        ));
        let j2b = pending(&d).pop().unwrap();
        assert_eq!(j2b.attempts, 1);
        assert!(j2b.next_at_ms > now_ms());
        assert_eq!(attempt(&d, &j2b, &busy), None);
        // For good: gone, with the reason.
        let calls = Cell::new(0);
        let bad = |_: &TargetId, _: &Item, _: &[u8]| {
            calls.set(calls.get() + 1);
            Err(ShareError::Fail("token".into()))
        };
        let j2c = pending(&d).pop().unwrap();
        match attempt(&d, &j2c, &bad) {
            Some(Event::Failed { error, .. }) => assert_eq!(error, "token"),
            e => panic!("{e:?}"),
        }
        assert!(pending(&d).is_empty());
        assert_eq!(calls.get(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn retries_end() {
        let d = dir("end");
        let mut job = enqueue(&d, TargetId::Slack(String::new()), Item::default(), b"x").unwrap();
        let busy = |_: &TargetId, _: &Item, _: &[u8]| Err(ShareError::Again("timeout".into()));
        for _ in 0..BACKOFF_S.len() {
            attempt(&d, &job, &busy);
            job = pending(&d).pop().unwrap();
        }
        assert!(matches!(
            attempt(&d, &job, &busy),
            Some(Event::Failed { .. })
        ));
        assert!(pending(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
