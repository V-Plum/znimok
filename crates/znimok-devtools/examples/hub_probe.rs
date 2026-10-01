//! A stand-in for the app in the end-to-end test (tools/devtools_e2e.py): the hub behind an IPC
//! server named by ZNIMOK_IPC_SUFFIX; once a browser's host says hello it «records» for the given
//! seconds and prints the log as JSON (one line).
//!
//! `cargo run -p znimok-devtools --example hub_probe -- <seconds>`

use std::time::{Duration, Instant};

use serde_json::json;
use znimok_devtools::{Hub, now_ms};

fn main() {
    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(6);
    let cfg = znimok_ipc::Config {
        suffix: std::env::var("ZNIMOK_IPC_SUFFIX").ok(),
        ..Default::default()
    };
    let hub: &'static Hub = Box::leak(Box::new(Hub::new()));
    let _server = znimok_ipc::Server::start(cfg, move |m: &str, p: serde_json::Value| {
        hub.handle(m, &p)
            .unwrap_or_else(|| Err(znimok_ipc::RpcError::method_not_found(m)))
    })
    .expect("IPC server");
    eprintln!("probe: waiting for a browser");
    let t0 = Instant::now();
    while hub.hosts() == 0 {
        if t0.elapsed() > Duration::from_secs(60) {
            println!("{}", json!({"error": "no browser connected"}));
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    eprintln!("probe: recording {secs} s");
    hub.start(now_ms());
    std::thread::sleep(Duration::from_secs(secs));
    let log = hub.stop();
    let events: Vec<serde_json::Value> = log
        .map(|l| {
            l.events
                .into_iter()
                .map(|e| json!({"ms": e.ms, "json": serde_json::from_str::<serde_json::Value>(&e.json).unwrap_or_default()}))
                .collect()
        })
        .unwrap_or_default();
    println!("{}", json!({"events": events}));
    // Let the stop reach the browser before the server goes.
    std::thread::sleep(Duration::from_millis(500));
}
