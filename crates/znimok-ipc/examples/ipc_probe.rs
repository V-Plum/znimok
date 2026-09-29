//! Tries to reach an IPC server the way another program would (ZK-112): open the endpoint and
//! read the token file. Run as the same user it must reach both; run as another (ordinary) user
//! the OS must refuse both.
//!
//! `ipc_probe <endpoint> <token-path> --expect ok|denied` prints one JSON line and exits with 0 when
//! both outcomes match the expectation, 1 otherwise.

use std::io::ErrorKind;

fn outcome(r: std::io::Result<()>) -> String {
    match r {
        Ok(()) => "ok".into(),
        Err(e) if e.kind() == ErrorKind::PermissionDenied => "denied".into(),
        Err(e) => format!("error: {e}"),
    }
}

/// The account of this process's token — not `%USERNAME%`, which `Start-Process -Credential`
/// inherits from the caller.
#[cfg(windows)]
fn account() -> String {
    use windows::Win32::System::WindowsProgramming::GetUserNameW;
    let mut buf = [0u16; 257];
    let mut len = buf.len() as u32;
    // SAFETY: buffer and its length in characters.
    match unsafe { GetUserNameW(Some(windows::core::PWSTR(buf.as_mut_ptr())), &mut len) } {
        Ok(()) => String::from_utf16_lossy(&buf[..len.saturating_sub(1) as usize]),
        Err(e) => format!("? {e}"),
    }
}

/// `sudo -u` sets `USER` to the target account.
#[cfg(not(windows))]
fn account() -> String {
    std::env::var("USER").unwrap_or_default()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(endpoint), Some(token_path)) = (args.get(1), args.get(2)) else {
        eprintln!("usage: ipc_probe <endpoint> <token-path> --expect ok|denied");
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
    let user = account();
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
