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
//! reaches one of our windows.
//!
//! The state is suspect only when the hotkey came from a Remote Desktop client in front (ZK-358).
//! Elsewhere the quarantine did harm: with Znimok only in the tray the overlay often never gets
//! the keyboard focus (a global hotkey leaves the other program in front), so no event could lift
//! it — Shift held from Alt+Shift+4 to the release opened the editor instead of copying. Outside
//! an RDP client, a modifier held from the hotkey counts, as it always did.
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
    /// The last capture started from a Remote Desktop client: our windows gaining the focus
    /// re-arm the quarantine until the next capture.
    static REMOTE: Cell<bool> = const { Cell::new(false) };
}

/// Clients that swallow key releases with a low-level hook while they have the focus.
#[cfg(windows)]
const REMOTE_EXES: [&str; 6] = [
    "mstsc.exe",
    "msrdc.exe",
    "msrdcw.exe",
    "vmconnect.exe",
    "rdcman.exe",
    "remotedesktopmanager.exe",
];

/// The window in front belongs to a Remote Desktop client.
#[cfg(windows)]
fn remote_client_in_front() -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId,
    };
    use windows::core::PWSTR;
    // SAFETY: plain queries on the foreground window; the process handle is closed below and the
    // buffers' lengths are passed along.
    unsafe {
        let h = GetForegroundWindow();
        if h.0.is_null() {
            return false;
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(h, &mut class).max(0) as usize;
        if String::from_utf16_lossy(&class[..n]) == "TscShellContainerClass" {
            return true;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        let Ok(p) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut buf = [0u16; 520];
        let mut len = buf.len() as u32;
        let ok =
            QueryFullProcessImageNameW(p, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
                .is_ok();
        let _ = CloseHandle(p);
        if !ok {
            return false;
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
        let exe = full.rsplit(['\\', '/']).next().unwrap_or(&full);
        REMOTE_EXES.contains(&exe)
    }
}

#[cfg(not(windows))]
fn remote_client_in_front() -> bool {
    false
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

/// A capture starts: from a Remote Desktop client in front the state is suspect and whatever is
/// down now is quarantined until seen moving; from anything else nothing is (ZK-358).
pub fn arm() {
    if !cfg!(windows) {
        return;
    }
    let remote = remote_client_in_front();
    REMOTE.with(|c| c.set(remote));
    if remote {
        quarantine_down();
    } else {
        QUARANTINE.with(|c| c.set(0));
    }
}

/// Whatever is down now goes into the quarantine.
fn quarantine_down() {
    let mut q = 0;
    for k in [Key::Shift, Key::Alt, Key::Ctrl] {
        if os_down(k) {
            q |= k.bit();
        }
    }
    QUARANTINE.with(|c| c.set(q));
}

/// The self-test (and unit tests) press modifiers only as window events, never on the keyboard.
fn events_only() -> bool {
    static SELFTEST: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    cfg!(test) || *SELFTEST.get_or_init(|| std::env::var_os("ZNIMOK_SELFTEST").is_some())
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
        // Up by the OS — a real release, so the key is fresh and not held, whatever a window's
        // table still says (ZK-358: an Alt it saw go down but not up made the capture open
        // over the screen). Only the self-test's synthetic modifiers live in the events alone.
        fresh(k);
        return event && events_only();
    }
    QUARANTINE.with(|c| c.get() & k.bit() == 0)
}

/// Feeds the window events of any of our windows: the focus arms the quarantine, a key event
/// for a modifier lifts it.
pub fn observe(ev: &slint::winit_030::winit::event::WindowEvent) {
    use slint::winit_030::winit::event::WindowEvent;
    use slint::winit_030::winit::keyboard::{KeyCode, PhysicalKey};
    match ev {
        // Only after a capture from a Remote Desktop client (the editor it opened gains the
        // focus with the client's stuck modifiers); otherwise a held key is a held key.
        WindowEvent::Focused(true) => {
            if cfg!(windows) && REMOTE.with(|c| c.get()) {
                quarantine_down();
            }
        }
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
