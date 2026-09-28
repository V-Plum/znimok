//! P3 — macOS prototype (ZK-16): ScreenCaptureKit still frames, the system content picker,
//! Screen Recording permission guide, global shortcuts, menu bar item and login item.
//!
//! Runs as a menu bar app bundle (`LSUIElement`). Logs and screenshots go to
//! `/Users/Shared/znimok-builds/inbox`, so the agent that builds it (another macOS account)
//! can read what happened in the owner's session.

#[cfg(target_os = "macos")]
mod mac;

fn main() {
    #[cfg(target_os = "macos")]
    mac::run();
    #[cfg(not(target_os = "macos"))]
    eprintln!("znimok-p3 is a macOS prototype; nothing to do on this OS.");
}
