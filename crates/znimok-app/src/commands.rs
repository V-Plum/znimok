//! The command layer over IPC (ZK-213): what the hotkeys, the tray and the editor's buttons do,
//! for other programs — the Logi Options+ plugin (ZK-118) first, the CLI and MCP later — and the
//! events they want to hear (haptics, button states).
//!
//! - Commands come in on the IPC thread and are queued; the UI thread drains them a few times a
//!   second ([`poll`], from the devtools timer) and runs them through the same functions the
//!   hotkeys use. The answer is immediate (`{queued: true}` with the state at that moment): a
//!   command that cannot run in the current state does nothing, and failures come as events.
//! - `app.state` answers from a snapshot the UI thread refreshes ([`set_state`]) — the IPC
//!   thread never borrows the app.
//! - `app.wait {since, timeout_ms}` is a long poll like `devtools.wait`: the events after
//!   `since`, waiting up to the timeout for one; the plugin keeps one call waiting.
//!
//! Event names (the plugin's event source maps them to waveforms): `shotTaken`, `copied`,
//! `recordStart`, `recordStop`, `recordPause`, `recordResume`, `exportDone`, `textRead`,
//! `codesRead`, `failed`.

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

use serde_json::{Value, json};

/// Events kept for late pollers.
const KEEP: usize = 200;
/// Commands waiting for the UI thread (a runaway client must not eat memory).
const MAX_QUEUE: usize = 64;

/// Methods the layer answers, for a client that wants to know.
pub const METHODS: &[&str] = &[
    "app.state",
    "app.wait",
    "capture.start",
    "record.toggle",
    "record.pause",
    "record.resume",
    "record.stop",
    "editor.undo",
    "editor.redo",
    "editor.tool",
    "editor.zoom",
    "video.scrub",
];

struct State {
    seq: u64,
    events: VecDeque<(u64, Value)>,
    queue: VecDeque<(String, Value)>,
    snapshot: Value,
}

pub struct Hub {
    st: Mutex<State>,
    cv: Condvar,
}

static HUB: OnceLock<Hub> = OnceLock::new();

pub fn hub() -> &'static Hub {
    HUB.get_or_init(Hub::new)
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

impl Hub {
    pub fn new() -> Self {
        Self {
            st: Mutex::new(State {
                seq: 0,
                events: VecDeque::new(),
                queue: VecDeque::new(),
                snapshot: json!({"page": "library"}),
            }),
            cv: Condvar::new(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Something happened (UI thread): waiters wake, late pollers get it from the ring.
    pub fn emit(&self, name: &str) {
        let mut st = self.lock();
        st.seq += 1;
        let seq = st.seq;
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64);
        st.events
            .push_back((seq, json!({"seq": seq, "name": name, "t": t})));
        while st.events.len() > KEEP {
            st.events.pop_front();
        }
        self.cv.notify_all();
    }

    /// The state other programs see (UI thread, whenever it may have changed).
    pub fn set_state(&self, snapshot: Value) {
        let mut st = self.lock();
        if st.snapshot != snapshot {
            st.snapshot = snapshot;
            // A state change is worth waking for: button pictures follow it.
            self.cv.notify_all();
        }
    }

    pub fn state(&self) -> Value {
        self.lock().snapshot.clone()
    }

    /// Events after `since`, waiting up to `timeout` for one (or for the state to change).
    pub fn wait(&self, since: u64, timeout: Duration) -> (u64, Vec<Value>, Value) {
        let st = self.lock();
        let before = st.snapshot.clone();
        let (st, _) = self
            .cv
            .wait_timeout_while(st, timeout, |s| {
                !s.events.iter().any(|(n, _)| *n > since) && s.snapshot == before
            })
            .unwrap_or_else(|p| p.into_inner());
        let events = st
            .events
            .iter()
            .filter(|(n, _)| *n > since)
            .map(|(_, e)| e.clone())
            .collect();
        (st.seq, events, st.snapshot.clone())
    }

    /// Commands queued for the UI thread, oldest first.
    pub fn take_commands(&self) -> Vec<(String, Value)> {
        self.lock().queue.drain(..).collect()
    }

    /// The IPC methods of the layer; `None` for anything that is not ours.
    pub fn handle(
        &self,
        method: &str,
        params: &Value,
    ) -> Option<Result<Value, znimok_ipc::RpcError>> {
        Some(match method {
            "app.state" => Ok(self.state()),
            "app.wait" => {
                let since = params.get("since").and_then(Value::as_u64).unwrap_or(0);
                let ms = params
                    .get("timeout_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or(15_000)
                    .min(30_000);
                let (seq, events, state) = self.wait(since, Duration::from_millis(ms));
                Ok(json!({"seq": seq, "events": events, "state": state}))
            }
            m if METHODS.contains(&m) => {
                let mut st = self.lock();
                if st.queue.len() >= MAX_QUEUE {
                    return Some(Err(znimok_ipc::RpcError::invalid_params(
                        "too many commands waiting",
                    )));
                }
                st.queue.push_back((m.to_string(), params.clone()));
                Ok(json!({"queued": true, "state": st.snapshot}))
            }
            _ => return None,
        })
    }
}

/// UI thread, a few times a second: runs what came in, then refreshes the state.
pub fn poll() {
    for (method, params) in hub().take_commands() {
        crate::run_command(&method, &params);
    }
    crate::refresh_state();
}

/// A shorthand for the places where something happened.
pub fn emit(name: &str) {
    hub().emit(name);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Events are numbered, a late poller gets what it missed, a waiter wakes on an event and
    /// on a state change, commands queue up to a cap.
    #[test]
    fn events_wait_and_queue() {
        let h = Hub::new();
        assert_eq!(h.wait(0, Duration::from_millis(10)).1.len(), 0);
        h.emit("shotTaken");
        h.emit("copied");
        let (seq, ev, _) = h.wait(0, Duration::ZERO);
        assert_eq!(seq, 2);
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[1]["name"], "copied");
        let (_, ev, _) = h.wait(1, Duration::ZERO);
        assert_eq!(ev.len(), 1);
        // A state change wakes a waiter with no new event.
        let h2 = std::sync::Arc::new(Hub::new());
        let h3 = h2.clone();
        let t = std::thread::spawn(move || h3.wait(0, Duration::from_secs(5)));
        std::thread::sleep(Duration::from_millis(50));
        h2.set_state(json!({"page": "editor"}));
        let (_, ev, state) = t.join().unwrap();
        assert!(ev.is_empty());
        assert_eq!(state["page"], "editor");
        // Commands queue; unknown methods are not ours.
        let r = h
            .handle("capture.start", &json!({"mode": "region"}))
            .unwrap()
            .unwrap();
        assert_eq!(r["queued"], true);
        assert!(h.handle("nope.nothing", &json!({})).is_none());
        assert_eq!(h.take_commands().len(), 1);
        for _ in 0..MAX_QUEUE {
            h.handle("editor.undo", &json!({}));
        }
        assert!(h.handle("editor.undo", &json!({})).unwrap().is_err());
        assert_eq!(h.take_commands().len(), MAX_QUEUE);
    }
}
