//! Prototype global hotkey on macOS: ⌃⇧4 takes a screenshot (global-hotkey over Carbon, as in
//! P3). ⌘⇧4 stays with the system unless the user frees it — offering that is ZK-44.

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

/// Keep the returned manager alive for as long as the key should work.
pub fn register() -> Option<GlobalHotKeyManager> {
    let manager = GlobalHotKeyManager::new().ok()?;
    let key = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Digit4);
    if let Err(e) = manager.register(key) {
        eprintln!("⌃⇧4: {e}");
        return None;
    }
    let id = key.id();
    // Events arrive from Carbon on the main thread; hop through the event loop anyway.
    GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
        if e.id() == id && e.state() == HotKeyState::Pressed {
            let _ = slint::invoke_from_event_loop(crate::shot_from_hotkey);
        }
    }));
    Some(manager)
}
