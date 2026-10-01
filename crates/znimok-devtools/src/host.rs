//! The Native Messaging host: the browser starts `znimok chrome-extension://<id>/` (on Windows
//! with `--parent-window=…` after it) and talks to it over stdin / stdout — each message a native
//! `u32` length and that many bytes of JSON. The host relays to the app's hub over znimok-ipc:
//! events in batches (`devtools.events`), the extension's requests (`devtools.cmd`), and back the
//! hub's messages with a long poll (`devtools.wait`). Without a running app it says so
//! (`{state: idle, app: false}`) and keeps trying. It ends when the browser closes its stdin.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use znimok_ipc::{Client, Config};

/// The process was started by a browser as the host (its first argument is the extension's
/// origin).
pub fn is_host_invocation(args: &[OsString]) -> bool {
    args.get(1)
        .and_then(|a| a.to_str())
        .is_some_and(|a| a.starts_with("chrome-extension://"))
}

/// One message from the browser; None at the end of its stdin.
pub fn read_frame(r: &mut impl Read) -> Option<Value> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len).ok()?;
    let n = u32::from_ne_bytes(len) as usize;
    // The browser sends at most 64 MB; anything bigger is not a message.
    if n > 64 << 20 {
        return None;
    }
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf).ok()?;
    serde_json::from_slice(&buf).ok()
}

/// One message to the browser (at most 1 MB — the browser's limit for this direction).
pub fn write_frame(w: &mut impl Write, v: &Value) -> std::io::Result<()> {
    let s = v.to_string();
    if s.len() > 1 << 20 {
        return Ok(());
    }
    w.write_all(&(s.len() as u32).to_ne_bytes())?;
    w.write_all(s.as_bytes())?;
    w.flush()
}

/// ZNIMOK_HOST_LOG=<file>: the host notes what it does there (the browser hides its stderr).
fn note(what: &str) {
    if let Some(p) = std::env::var_os("ZNIMOK_HOST_LOG")
        && let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
    {
        let _ = writeln!(f, "{} {what}", crate::now_ms());
    }
}

fn connect(cfg: &Config) -> Option<Client> {
    match Client::connect(cfg, "native-host") {
        Ok(c) => Some(c),
        Err(e) => {
            note(&format!("connect: {e}"));
            None
        }
    }
}

/// Runs the host until the browser closes stdin. Returns the process's exit code.
pub fn run(cfg: &Config) -> i32 {
    note(&format!("started, suffix {:?}", cfg.suffix));
    let out = Arc::new(Mutex::new(std::io::stdout()));
    let say = {
        let out = out.clone();
        move |v: &Value| {
            if let Ok(mut o) = out.lock() {
                let _ = write_frame(&mut *o, v);
            }
        }
    };
    // The hub's messages to the browser, by a long poll.
    {
        let cfg = cfg.clone();
        let say = say.clone();
        std::thread::spawn(move || {
            let mut told_idle = false;
            loop {
                let Some(mut c) = connect(&cfg) else {
                    if !told_idle {
                        say(&json!({"state": "idle", "app": false, "ms": 0}));
                        told_idle = true;
                    }
                    std::thread::sleep(Duration::from_secs(2));
                    continue;
                };
                told_idle = false;
                note("connected to the app");
                let mut since = match c.call("devtools.hello", json!({})) {
                    Ok(r) => {
                        say(&r["state"]);
                        if r["recording"] == json!(true) {
                            say(&json!({"cmd": "start"}));
                        }
                        r["seq"].as_u64().unwrap_or(0)
                    }
                    Err(_) => continue,
                };
                loop {
                    match c.call(
                        "devtools.wait",
                        json!({"since": since, "timeout_ms": 15000}),
                    ) {
                        Ok(r) => {
                            since = r["seq"].as_u64().unwrap_or(since);
                            for m in r["msgs"].as_array().into_iter().flatten() {
                                say(m);
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        });
    }
    // Events go in batches: at most every 100 ms or 200 at once.
    let batch: Arc<Mutex<Vec<Value>>> = Arc::default();
    {
        let cfg = cfg.clone();
        let batch = batch.clone();
        std::thread::spawn(move || {
            let mut c: Option<Client> = None;
            loop {
                std::thread::sleep(Duration::from_millis(100));
                let events: Vec<Value> = match batch.lock() {
                    Ok(mut b) => b.drain(..).collect(),
                    Err(_) => continue,
                };
                if events.is_empty() {
                    continue;
                }
                if c.is_none() {
                    c = connect(&cfg);
                }
                if let Some(cl) = c.as_mut() {
                    for chunk in events.chunks(200) {
                        if cl
                            .call("devtools.events", json!({"events": chunk}))
                            .is_err()
                        {
                            c = None;
                            break;
                        }
                    }
                }
            }
        });
    }
    let mut cmd: Option<Client> = None;
    let mut stdin = std::io::stdin().lock();
    let mut last_warn = Instant::now() - Duration::from_secs(60);
    while let Some(m) = read_frame(&mut stdin) {
        if m.get("k").is_some() {
            if let Ok(mut b) = batch.lock() {
                // A stalled app: the oldest events give way rather than memory growing.
                if b.len() > 50_000 {
                    b.drain(..10_000);
                }
                b.push(m);
            }
        } else if m.get("cmd").is_some() {
            if cmd.is_none() {
                cmd = connect(cfg);
            }
            let ok = cmd
                .as_mut()
                .is_some_and(|c| c.call("devtools.cmd", m.clone()).is_ok());
            if !ok {
                cmd = None;
                if let Some(rid) = m.get("rid") {
                    say(&json!({"rec": "fail", "rid": rid, "why": "no-app"}));
                }
                if last_warn.elapsed() > Duration::from_secs(5) {
                    last_warn = Instant::now();
                    say(&json!({"state": "idle", "app": false, "ms": 0}));
                }
            }
        }
        // `hello` and `ping` need nothing: the poll thread answers with the state.
    }
    if let Some(mut c) = cmd {
        let _ = c.call("devtools.bye", json!({}));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &json!({"k": "console", "text": "привіт"})).unwrap();
        assert_eq!(
            u32::from_ne_bytes(buf[..4].try_into().unwrap()) as usize,
            buf.len() - 4
        );
        let v = read_frame(&mut &buf[..]).unwrap();
        assert_eq!(v["text"], "привіт");
        assert!(read_frame(&mut &buf[..2]).is_none());
        let args = ["znimok".into(), "chrome-extension://abc/".into()];
        assert!(is_host_invocation(&args));
        assert!(!is_host_invocation(&["znimok".into(), "info".into()]));
    }
}
