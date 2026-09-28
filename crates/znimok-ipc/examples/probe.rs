//! Tries to reach an IPC server the way another program would (ZK-112): open the endpoint and
//! read the token file. Run as the same user it must reach both; run as another (ordinary) user
//! the OS must refuse both.
//!
//! `probe <endpoint> <token-path> --expect ok|denied` prints one JSON line and exits with 0 when
//! both outcomes match the expectation, 1 otherwise.

use std::io::ErrorKind;

fn outcome(r: std::io::Result<()>) -> String {
    match r {
        Ok(()) => "ok".into(),
        Err(e) if e.kind() == ErrorKind::PermissionDenied => "denied".into(),
        Err(e) => format!("error: {e}"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(endpoint), Some(token_path)) = (args.get(1), args.get(2)) else {
        eprintln!("usage: probe <endpoint> <token-path> --expect ok|denied");
        std::process::exit(2);
    };
    let expect = args
        .iter()
        .position(|a| a == "--expect")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "denied".into());
    let connect = outcome(znimok_ipc::open_endpoint(endpoint));
    let token = outcome(std::fs::read(token_path).map(|_| ()));
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default();
    println!(
        "{}",
        serde_json::json!({ "user": user, "connect": connect, "token_file": token, "expect": expect })
    );
    std::process::exit(if connect == expect && token == expect {
        0
    } else {
        1
    });
}
