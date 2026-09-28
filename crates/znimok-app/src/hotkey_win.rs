//! Prototype global hotkey on Windows: Ctrl+Shift+4 takes a screenshot. `RegisterHotKey` on a
//! thread of its own with its own message loop, so winit's loop is not involved. The platform
//! `Hotkeys` implementation with physical keys and conflict probing (ZK-35/36) replaces this.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};

/// Starts the listener; `on_press` runs on the hotkey thread. Returns false when another
/// program already holds the combination.
pub fn register(on_press: impl Fn() + Send + 'static) -> bool {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("znimok-hotkey".into())
        .spawn(move || {
            // VK '4' = 0x34; with MOD_NOREPEAT holding the keys does not fire repeatedly.
            // SAFETY: plain Win32 call; the hotkey belongs to this thread's message queue.
            let ok =
                unsafe { RegisterHotKey(None, 1, MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT, 0x34) }
                    .is_ok();
            let _ = tx.send(ok);
            if !ok {
                return;
            }
            let mut msg = MSG::default();
            // SAFETY: standard message loop on this thread; ends when the process exits.
            while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
                if msg.message == WM_HOTKEY {
                    on_press();
                }
            }
        })
        .ok();
    rx.recv().unwrap_or(false)
}
