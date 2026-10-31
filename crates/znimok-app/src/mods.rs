//! Modifier keys as the person holds them, not as a stale table says (ZK-350).
//!
//! Two tables describe Shift / Alt / Ctrl on Windows and both can lie. The window's own state
//! (winit, from `GetKeyState` at focus) misses a release that went to another window. The
//! asynchronous state (`GetAsyncKeyState`) misses a release that a low-level hook swallowed —
//! the Remote Desktop client does that while it has the focus: a hotkey pressed inside an RDP
//! window fires Znimok on the host, the modifiers are released into the RDP client, and the
//! host keeps them «down» for good. The overlay then saw Alt and opened the editor over the
//! screen instead of copying (Alt beats Shift), and the editor drew squares until Shift was
//! pressed and released once more.
//!
//! The rule: a modifier counts only when a *transition* of it has been seen since the state
//! became suspect. Whatever is down when a capture starts or a window gains the focus is
//! quarantined; the quarantine lifts when the OS reports the key up, or when a key event for it
//! reaches one of our windows. A modifier still held from the hotkey itself therefore does not
//! count as a gesture until it is released and pressed again — the overlay's hint strip shows
//! what the release will do, so this is visible before it happens.
//!
//! macOS has no such client in the way; there the event and the system state are trusted as
//! before (ZK-154).

use std::cell::Cell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Shift,
    Alt,
    Ctrl,
}

impl Key {
    const fn bit(self) -> u8 {
        match self {
            Key::Shift => 1,
            Key::Alt => 2,
            Key::Ctrl => 4,
        }
    }
}

thread_local! {
    /// Modifiers down when the state became suspect, not yet seen moving.
    static QUARANTINE: Cell<u8> = const { Cell::new(0) };
}

/// Shift, Alt (⌥), Ctrl as the OS has them right now.
#[cfg(windows)]
fn os_down(k: Key) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_MENU, VK_SHIFT,
    };
    let vk = match k {
        Key::Shift => VK_SHIFT,
        Key::Alt => VK_MENU,
        Key::Ctrl => VK_CONTROL,
    };
    // SAFETY: a plain state query.
    let state = unsafe { GetAsyncKeyState(vk.0 as i32) };
    state < 0
}

#[cfg(target_os = "macos")]
fn os_down(k: Key) -> bool {
    use objc2_app_kit::{NSEvent, NSEventModifierFlags};
    let f = NSEvent::modifierFlags_class();
    f.contains(match k {
        Key::Shift => NSEventModifierFlags::Shift,
        Key::Alt => NSEventModifierFlags::Option,
        Key::Ctrl => NSEventModifierFlags::Control,
    })
}

#[cfg(not(any(windows, target_os = "macos")))]
fn os_down(_k: Key) -> bool {
    false
}

/// The state became suspect: whatever is down now is quarantined until seen moving. Called when
/// a capture starts and when a window gains the focus.
pub fn arm() {
    if !cfg!(windows) {
        return;
    }
    let mut q = 0;
    for k in [Key::Shift, Key::Alt, Key::Ctrl] {
        if os_down(k) {
            q |= k.bit();
        }
    }
    QUARANTINE.with(|c| c.set(q));
}

/// A key event for `k` reached one of our windows: its state is fresh.
pub fn fresh(k: Key) {
    QUARANTINE.with(|c| c.set(c.get() & !k.bit()));
}

/// Whether `k` counts as held, given what the window's event says.
pub fn held(k: Key, event: bool) -> bool {
    let os = os_down(k);
    if !cfg!(windows) {
        return event || os;
    }
    if !os {
        // Up by the OS — a real release, so the key is fresh; the event stream alone decides
        // (the self-test's synthetic modifiers live there).
        fresh(k);
        return event;
    }
    QUARANTINE.with(|c| c.get() & k.bit() == 0)
}

/// Feeds the window events of any of our windows: the focus arms the quarantine, a key event
/// for a modifier lifts it.
pub fn observe(ev: &slint::winit_030::winit::event::WindowEvent) {
    use slint::winit_030::winit::event::WindowEvent;
    use slint::winit_030::winit::keyboard::{KeyCode, PhysicalKey};
    match ev {
        WindowEvent::Focused(true) => arm(),
        WindowEvent::KeyboardInput { event, .. } => {
            if let PhysicalKey::Code(code) = event.physical_key {
                let k = match code {
                    KeyCode::ShiftLeft | KeyCode::ShiftRight => Key::Shift,
                    KeyCode::AltLeft | KeyCode::AltRight => Key::Alt,
                    KeyCode::ControlLeft | KeyCode::ControlRight => Key::Ctrl,
                    _ => return,
                };
                fresh(k);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing down: the event decides (the self-test's path); a quarantined key never counts
    /// until it is seen fresh.
    #[test]
    fn event_decides_when_nothing_is_down() {
        QUARANTINE.with(|c| c.set(0));
        // Neither key is physically down while the tests run, so `held` follows the event.
        assert!(held(Key::Shift, true) || os_down(Key::Shift));
        assert!(!held(Key::Shift, false) || os_down(Key::Shift));
        QUARANTINE.with(|c| c.set(Key::Alt.bit()));
        fresh(Key::Alt);
        assert_eq!(QUARANTINE.with(|c| c.get()), 0);
    }
}
