//! `ipc_serve`: a throw-away IPC server for the other-account check (ZK-112, `.github/workflows/ipc-security.yml`).
//!
//! `ipc_serve [--seconds N]` starts a server under the suffix `probe`, prints one JSON line
//! `{"endpoint": …, "token_path": …}` and keeps running for N seconds (default 120).

use std::time::Duration;
use znimok_ipc::{Config, RpcError, Server};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seconds = args
        .iter()
        .position(|a| a == "--seconds")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(120u64);
    let cfg = Config {
        suffix: Some("probe".into()),
        ..Config::default()
    };
    let server = Server::start(cfg.clone(), |_: &str, p: serde_json::Value| {
        Ok::<_, RpcError>(p)
    })
    .expect("server");
    println!(
        "{}",
        serde_json::json!({
            "endpoint": server.endpoint(),
            "token_path": cfg.token_path().display().to_string(),
        })
    );
    std::thread::sleep(Duration::from_secs(seconds));
    eprintln!("wrong tokens seen: {}", server.auth_failures());
}
