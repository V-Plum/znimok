//! The browser log of a recording (ZK-97).
//!
//! ```text
//! the extension (Chrome / Edge, chrome.debugger: console, errors, network with headers and
//! bodies, navigations; times by the system clock)
//!   ⇄ Native Messaging (stdin/stdout, u32 length + JSON) ⇄ `znimok` started by the browser
//!   ⇄ znimok-ipc (named pipe / unix socket, the session token) ⇄ the app's [`Hub`]
//!   → the events of a recording mapped onto the video's time without its pauses → `DEVT`
//! ```
//!
//! The browser starts the host itself (`znimok chrome-extension://…/`); the app registers the
//! host for Chrome and Edge in the user's profile ([`register`]), no administrator. Commands to
//! the extension (start, stop, the recording's state, answers to «record this window») wait in
//! the hub until a host takes them with a long poll ([`Hub::wait`]).
//!
//! What is written follows the owner's decision of 30.09.2026: everything the DevTools Network
//! panel shows — headers, payloads, responses — for debugging; hiding secrets is a choice at
//! export, not here.

pub mod host;
pub mod register;

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use znimok_format::video::{DevEvent, DevLog};

/// The Native Messaging host's name (the browser's registry key and the manifest's file).
pub const HOST_NAME: &str = "com.znimok.devtools";

/// The extensions allowed to talk to the host: the one loaded unpacked (its id from the `key` in
/// its manifest) and the Chrome Web Store's (ZK-182 — the store gives its own id).
pub const EXTENSION_IDS: &[&str] = &[DEV_EXTENSION_ID, STORE_EXTENSION_ID];
pub const DEV_EXTENSION_ID: &str = "mmkhmcoabdpolbfkghgpaihcpjlliakn";
/// The extension from the Chrome Web Store.
pub const STORE_EXTENSION_ID: &str = "jhnaichejniloonjcimjpfeggkmcmpek";

/// At most this many events in a log (the format's limit).
pub const MAX_EVENTS: usize = 200_000;

/// The recording's state as the extension sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecState {
    Idle,
    Rec,
    Paused,
}

impl RecState {
    fn name(self) -> &'static str {
        match self {
            RecState::Idle => "idle",
            RecState::Rec => "rec",
            RecState::Paused => "paused",
        }
    }
}

#[derive(Default)]
struct State {
    rec: Option<Recording>,
    /// Messages for the extensions: (sequence number, message), the last ones only.
    outbox: VecDeque<(u64, Value)>,
    seq: u64,
    /// Requests from the extensions for the app (record this window, stop, pause, resume).
    requests: VecDeque<Value>,
    /// The log is written (Settings → Recording); the extension may start recordings.
    log: bool,
    control: bool,
    /// Hosts inside their long poll now, and when the last one left it: a browser is connected
    /// while its host polls (ZK-296 — a count of hellos and byes went wrong with every host that
    /// ended without a bye).
    polling: usize,
    polled: Option<std::time::Instant>,
}

struct Recording {
    /// Wall clock of the first frame, Unix ms.
    wall0: i64,
    /// Pauses: (from, to) wall ms; an open one has `to` = None.
    pauses: Vec<(i64, Option<i64>)>,
    events: Vec<(i64, String)>,
    dropped: usize,
}

/// The app's side: owned by the app, shared with the IPC server's threads.
pub struct Hub {
    st: Mutex<State>,
    cv: Condvar,
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

/// Unix ms now.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

impl Hub {
    pub fn new() -> Self {
        Self {
            st: Mutex::new(State {
                log: true,
                control: true,
                ..Default::default()
            }),
            cv: Condvar::new(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn post(st: &mut State, msg: Value) {
        st.seq += 1;
        let n = st.seq;
        st.outbox.push_back((n, msg));
        while st.outbox.len() > 64 {
            st.outbox.pop_front();
        }
    }

    fn state_of(st: &State) -> Value {
        let (s, ms) = match &st.rec {
            None => (RecState::Idle, 0),
            Some(r) => {
                let now = now_ms();
                let s = if r.pauses.last().is_some_and(|p| p.1.is_none()) {
                    RecState::Paused
                } else {
                    RecState::Rec
                };
                (s, video_ms(r, now).max(0))
            }
        };
        json!({"state": s.name(), "ms": ms, "log": u8::from(st.log), "ctl": u8::from(st.control), "app": true})
    }

    /// Settings: whether the log is written and whether the extension may start recordings.
    pub fn set_options(&self, log: bool, control: bool) {
        let mut st = self.lock();
        st.log = log;
        st.control = control;
        let s = Self::state_of(&st);
        Self::post(&mut st, s);
        self.cv.notify_all();
    }

    /// A recording started; its first frame was at `wall0` (Unix ms).
    pub fn start(&self, wall0: i64) {
        let mut st = self.lock();
        st.rec = Some(Recording {
            wall0,
            pauses: Vec::new(),
            events: Vec::new(),
            dropped: 0,
        });
        if st.log {
            Self::post(&mut st, json!({"cmd": "start"}));
        }
        let s = Self::state_of(&st);
        Self::post(&mut st, s);
        self.cv.notify_all();
    }

    pub fn pause(&self, on: bool) {
        let mut st = self.lock();
        let now = now_ms();
        if let Some(r) = st.rec.as_mut() {
            match (on, r.pauses.last_mut()) {
                (true, Some((_, None))) => {}
                (true, _) => r.pauses.push((now, None)),
                (false, Some(p @ (_, None))) => p.1 = Some(now),
                (false, _) => {}
            }
        }
        let s = Self::state_of(&st);
        Self::post(&mut st, s);
        self.cv.notify_all();
    }

    /// The recording ended: its log (None without events or with the log off).
    pub fn stop(&self) -> Option<DevLog> {
        let mut st = self.lock();
        let r = st.rec.take();
        Self::post(&mut st, json!({"cmd": "stop"}));
        let s = Self::state_of(&st);
        Self::post(&mut st, s);
        self.cv.notify_all();
        let r = r?;
        if r.dropped > 0 {
            eprintln!("devtools: {} events over the limit left out", r.dropped);
        }
        map_log(r.wall0, &r.pauses, r.events)
    }

    pub fn recording(&self) -> bool {
        self.lock().rec.is_some()
    }

    /// Requests of the extensions for the app, oldest first.
    pub fn take_requests(&self) -> Vec<Value> {
        self.lock().requests.drain(..).collect()
    }

    /// An answer to a «record this window» request (`{rec: found | ok | fail, rid, …}`).
    pub fn reply(&self, msg: Value) {
        let mut st = self.lock();
        Self::post(&mut st, msg);
        self.cv.notify_all();
    }

    /// Browsers connected now: hosts inside their long poll (one that just left it is between
    /// two polls).
    pub fn hosts(&self) -> usize {
        let st = self.lock();
        if st.polling > 0 {
            st.polling
        } else {
            usize::from(
                st.polled
                    .is_some_and(|t| t.elapsed() < Duration::from_secs(3)),
            )
        }
    }

    /// Messages after `since`, waiting up to `timeout` for one.
    pub fn wait(&self, since: u64, timeout: Duration) -> (u64, Vec<Value>) {
        let st = self.lock();
        let (st, _) = self
            .cv
            .wait_timeout_while(st, timeout, |s| !s.outbox.iter().any(|(n, _)| *n > since))
            .unwrap_or_else(|p| p.into_inner());
        let msgs: Vec<Value> = st
            .outbox
            .iter()
            .filter(|(n, _)| *n > since)
            .map(|(_, m)| m.clone())
            .collect();
        (st.seq, msgs)
    }

    /// The IPC methods of the hub (`devtools.*`); anything else is not ours.
    pub fn handle(
        &self,
        method: &str,
        params: &Value,
    ) -> Option<Result<Value, znimok_ipc::RpcError>> {
        Some(match method {
            "devtools.hello" => {
                let st = self.lock();
                let s = Self::state_of(&st);
                let rec = st.rec.is_some() && st.log;
                Ok(json!({"state": s, "seq": st.seq, "recording": rec}))
            }
            // Hosts before ZK-296 said it; the count is by the polls now.
            "devtools.bye" => Ok(json!({})),
            "devtools.events" => {
                let events = params
                    .get("events")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let mut st = self.lock();
                let log = st.log;
                if let Some(r) = st.rec.as_mut().filter(|_| log) {
                    for e in events {
                        let t = e.get("t").and_then(Value::as_f64).unwrap_or(0.0) as i64;
                        if r.events.len() >= MAX_EVENTS {
                            r.dropped += 1;
                            continue;
                        }
                        r.events.push((t, e.to_string()));
                    }
                }
                Ok(json!({}))
            }
            "devtools.wait" => {
                let since = params.get("since").and_then(Value::as_u64).unwrap_or(0);
                let ms = params
                    .get("timeout_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or(15_000)
                    .min(30_000);
                self.lock().polling += 1;
                let (seq, msgs) = self.wait(since, Duration::from_millis(ms));
                {
                    let mut st = self.lock();
                    st.polling = st.polling.saturating_sub(1);
                    st.polled = Some(std::time::Instant::now());
                }
                Ok(json!({"seq": seq, "msgs": msgs}))
            }
            "devtools.cmd" => {
                let mut st = self.lock();
                st.requests.push_back(params.clone());
                Ok(json!({}))
            }
            _ => return None,
        })
    }
}

/// Video time of a wall time: from the first frame, without the pauses before it.
fn video_ms(r: &Recording, t: i64) -> i64 {
    let mut ms = t - r.wall0;
    for (a, b) in &r.pauses {
        let b = b.unwrap_or(i64::MAX);
        if t >= b {
            ms -= b - a;
        } else if t > *a {
            ms -= t - a;
        }
    }
    ms
}

/// The events of a recording as its log: on the video's time, without what fell into a pause
/// or before the first frame, in time order.
pub fn map_log(
    wall0: i64,
    pauses: &[(i64, Option<i64>)],
    mut events: Vec<(i64, String)>,
) -> Option<DevLog> {
    if events.is_empty() {
        return None;
    }
    let r = Recording {
        wall0,
        pauses: pauses.to_vec(),
        events: Vec::new(),
        dropped: 0,
    };
    events.sort_by_key(|(t, _)| *t);
    let paused = |t: i64| {
        pauses
            .iter()
            .any(|(a, b)| t >= *a && t < b.unwrap_or(i64::MAX))
    };
    let out: Vec<DevEvent> = events
        .into_iter()
        .filter(|(t, _)| *t >= wall0 && !paused(*t))
        .map(|(t, json)| DevEvent {
            ms: video_ms(&r, t).clamp(0, i64::from(i32::MAX)) as i32,
            json,
        })
        .collect();
    (!out.is_empty()).then_some(DevLog {
        wall0_ms: wall0,
        events: out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_follow_the_video_time() {
        let log = map_log(
            1000,
            &[(3000, Some(5000))],
            vec![
                (4000, "paused".into()),
                (900, "before".into()),
                (6000, "after".into()),
                (2000, "a".into()),
            ],
        )
        .unwrap();
        let got: Vec<(i32, &str)> = log.events.iter().map(|e| (e.ms, e.json.as_str())).collect();
        assert_eq!(got, vec![(1000, "a"), (3000, "after")]);
        assert_eq!(log.wall0_ms, 1000);
    }

    #[test]
    fn the_hub_hands_out_commands_and_keeps_events() {
        let hub = Hub::new();
        let r = hub.handle("devtools.hello", &json!({})).unwrap().unwrap();
        let seq = r["seq"].as_u64().unwrap();
        hub.start(now_ms() - 10);
        let (seq2, msgs) = hub.wait(seq, Duration::from_millis(10));
        assert!(msgs.iter().any(|m| m["cmd"] == "start"), "{msgs:?}");
        assert!(msgs.iter().any(|m| m["state"] == "rec"));
        let t = now_ms();
        hub.handle(
            "devtools.events",
            &json!({"events": [{"t": t, "k": "console", "text": "hi"}]}),
        )
        .unwrap()
        .unwrap();
        hub.handle("devtools.cmd", &json!({"cmd": "pause"}))
            .unwrap()
            .unwrap();
        assert_eq!(hub.take_requests()[0]["cmd"], "pause");
        let log = hub.stop().unwrap();
        assert_eq!(log.events.len(), 1);
        assert!(log.events[0].json.contains("\"hi\""));
        let (_, msgs) = hub.wait(seq2, Duration::from_millis(10));
        assert!(msgs.iter().any(|m| m["cmd"] == "stop"));
        assert!(hub.handle("other", &json!({})).is_none());
    }

    /// ZK-296: a browser is connected while its host polls — however the host ends.
    #[test]
    fn hosts_are_the_ones_that_poll() {
        let hub = std::sync::Arc::new(Hub::new());
        assert_eq!(hub.hosts(), 0);
        hub.handle("devtools.hello", &json!({})).unwrap().unwrap();
        assert_eq!(hub.hosts(), 0, "a hello alone is not a connected browser");
        let h2 = hub.clone();
        let poll = std::thread::spawn(move || {
            h2.handle("devtools.wait", &json!({"since": 0, "timeout_ms": 300}))
                .unwrap()
                .unwrap()
        });
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(hub.hosts(), 1);
        poll.join().unwrap();
        // Between two polls it is still there; no bye is needed for it to go later.
        assert_eq!(hub.hosts(), 1);
    }
}
