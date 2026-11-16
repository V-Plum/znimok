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
//! Outside a Remote Desktop client a key the OS has down is held (ZK-358): with Znimok only in
//! the tray the overlay often never gets the keyboard focus, so no window event can tell more.
//!
//! When the hotkey comes from a Remote Desktop client in front, Znimok watches Shift, Alt and
//! Ctrl itself while the overlay is open (ZK-359): a low-level keyboard hook of its own, on a
//! thread of its own, installed after the client's and so called before it — it sees the
//! releases the client then swallows. What was down at the start, less what the hook saw go up,
//! is held: Shift kept down from Alt+Shift+4 to the release copies, an Alt let go into the
//! client does not count. A key the hook saw released while the OS still has it down is stale:
//! it is quarantined for later captures and windows until the OS reports it up or a key event
//! for it reaches one of our windows. Should the hook fail, the older rule stands: whatever is
//! down at the start counts only once seen moving.
//!
//! macOS has no such client in the way; there the event and the system state are trusted as
//! before (ZK-154).

use std::cell::Cell;
use std::sync::atomic::{AtomicU8, Ordering};

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
    /// The last capture started from a Remote Desktop client and the hook could not watch it:
    /// our windows gaining the focus re-arm the quarantine until the next capture.
    static REMOTE: Cell<bool> = const { Cell::new(false) };
}

/// Keys the hook saw released while the OS kept them down (`Key::bit`).
static STALE: AtomicU8 = AtomicU8::new(0);

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

/// The keys the OS has down now, as `Key::bit`s.
fn os_down_bits() -> u8 {
    [Key::Shift, Key::Alt, Key::Ctrl]
        .into_iter()
        .filter(|k| os_down(*k))
        .fold(0, |m, k| m | k.bit())
}

/// The modifiers' own hook while a capture from a Remote Desktop client is open (ZK-359).
#[cfg(windows)]
mod watch {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering};

    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_RCONTROL, VK_RMENU, VK_RSHIFT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, HHOOK, KBDLLHOOKSTRUCT, MSG, PM_NOREMOVE,
        PeekMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL,
        WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    /// Left and right of each modifier, one bit each: Shift 0–1, Alt 2–3, Ctrl 4–5.
    const SIDES: [(u16, u8); 6] = [
        (VK_LSHIFT.0, 1),
        (VK_RSHIFT.0, 2),
        (VK_LMENU.0, 4),
        (VK_RMENU.0, 8),
        (VK_LCONTROL.0, 16),
        (VK_RCONTROL.0, 32),
    ];
    /// The hook gives up by itself after this long (an overlay left open, a path that did not
    /// call `stop`).
    const LIFETIME: std::time::Duration = std::time::Duration::from_secs(600);

    /// The sides down as the hook knows them.
    static DOWN: AtomicU8 = AtomicU8::new(0);
    /// The hook thread, while it runs (0 = none).
    static THREAD: AtomicU32 = AtomicU32::new(0);
    /// Which start the running hook belongs to (the lifetime timer of an older one stops nothing).
    static GENERATION: AtomicU64 = AtomicU64::new(0);
    /// One start or stop at a time.
    static LOCK: Mutex<()> = Mutex::new(());

    fn side_bit(vk: u32) -> u8 {
        SIDES
            .iter()
            .find(|(v, _)| u32::from(*v) == vk)
            .map_or(0, |(_, b)| *b)
    }

    /// `Key::bit`s → the sides' bits, and back.
    fn sides_of(keys: u8) -> u8 {
        ((keys & 1) * 3) | (((keys >> 1) & 1) * 12) | (((keys >> 2) & 1) * 48)
    }

    fn keys_of(sides: u8) -> u8 {
        u8::from(sides & 3 != 0) | u8::from(sides & 12 != 0) << 1 | u8::from(sides & 48 != 0) << 2
    }

    unsafe extern "system" fn hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if code >= 0 {
            // SAFETY: for WH_KEYBOARD_LL, lParam points at a KBDLLHOOKSTRUCT for this call.
            let info = unsafe { &*(lp.0 as *const KBDLLHOOKSTRUCT) };
            let bit = side_bit(info.vkCode);
            if bit != 0 {
                match wp.0 as u32 {
                    WM_KEYDOWN | WM_SYSKEYDOWN => {
                        DOWN.fetch_or(bit, Ordering::SeqCst);
                    }
                    WM_KEYUP | WM_SYSKEYUP => {
                        DOWN.fetch_and(!bit, Ordering::SeqCst);
                    }
                    _ => {}
                }
            }
        }
        // SAFETY: passing the key on, as every hook must — nothing is swallowed or kept.
        unsafe { CallNextHookEx(None, code, wp, lp) }
    }

    /// Starts watching, with `keys` (`Key::bit`s) down at the start as far as they count;
    /// whether the hook is in place.
    pub fn start(keys: u8) -> bool {
        stop();
        let _g = LOCK.lock();
        let allowed = sides_of(keys);
        let mut sides = 0;
        for (vk, b) in SIDES {
            // SAFETY: a plain state query.
            if b & allowed != 0 && unsafe { GetAsyncKeyState(i32::from(vk)) } < 0 {
                sides |= b;
            }
        }
        DOWN.store(sides, Ordering::SeqCst);
        let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("znimok-mods".into())
            .spawn(move || {
                // SAFETY: the hook and the message loop belong to this thread; the loop ends on
                // WM_QUIT and the hook is removed before the thread returns.
                unsafe {
                    let mut msg = MSG::default();
                    // Make the thread's message queue before anyone posts to it.
                    let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
                    let module = GetModuleHandleW(None).ok().map(|m| m.into());
                    let h: Option<HHOOK> =
                        SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), module, 0).ok();
                    let _ = tx.send(h.is_some().then(|| GetCurrentThreadId()));
                    let Some(h) = h else { return };
                    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                        DispatchMessageW(&msg);
                    }
                    let _ = UnhookWindowsHookEx(h);
                }
            })
            .is_ok();
        let Some(id) = spawned.then(|| rx.recv().ok().flatten()).flatten() else {
            return false;
        };
        THREAD.store(id, Ordering::SeqCst);
        std::thread::spawn(move || {
            std::thread::sleep(LIFETIME);
            if GENERATION.load(Ordering::SeqCst) == generation {
                stop();
            }
        });
        true
    }

    /// Whether the hook is watching now.
    pub fn active() -> bool {
        THREAD.load(Ordering::SeqCst) != 0
    }

    /// The keys (`Key::bit`s) down as the hook knows them.
    pub fn down() -> u8 {
        keys_of(DOWN.load(Ordering::SeqCst))
    }

    /// Stops watching; the keys down as the hook last knew them (`Key::bit`s), if it ran.
    pub fn stop() -> Option<u8> {
        let _g = LOCK.lock();
        let id = THREAD.swap(0, Ordering::SeqCst);
        if id == 0 {
            return None;
        }
        // SAFETY: posting WM_QUIT to the hook thread's queue ends its loop.
        let _ = unsafe { PostThreadMessageW(id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        Some(down())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn sides_and_keys_round_trip() {
            for keys in 0..8u8 {
                assert_eq!(keys_of(sides_of(keys)), keys);
            }
            assert_eq!(keys_of(2), 1);
            assert_eq!(keys_of(8), 2);
            assert_eq!(keys_of(16), 4);
        }

        /// The hook goes in and comes out; with nothing allowed down at the start, nothing is.
        #[test]
        fn hook_starts_and_stops() {
            assert!(start(0), "the keyboard hook did not install");
            assert!(active());
            assert_eq!(down(), 0);
            assert_eq!(stop(), Some(0));
            assert!(!active());
            assert_eq!(stop(), None);
        }
    }
}

#[cfg(not(windows))]
mod watch {
    pub fn start(_keys: u8) -> bool {
        false
    }
    pub fn active() -> bool {
        false
    }
    pub fn down() -> u8 {
        0
    }
    pub fn stop() -> Option<u8> {
        None
    }
}

/// The stale keys the OS still has down; the ones it has up are no longer stale.
fn stale_down() -> u8 {
    let stale = STALE.load(Ordering::SeqCst) & os_down_bits();
    STALE.store(stale, Ordering::SeqCst);
    stale
}

/// A capture starts. From a Remote Desktop client in front, the hook watches the modifiers until
/// the choice is made (`settle`); without the hook, whatever is down now is quarantined until
/// seen moving. From anything else a held key is held, unless known stale (ZK-358/359).
pub fn arm() {
    if !cfg!(windows) {
        return;
    }
    settle();
    let stale = stale_down();
    QUARANTINE.with(|c| c.set(stale));
    let remote = remote_client_in_front() && !events_only();
    let watched = remote && watch::start(os_down_bits() & !stale);
    REMOTE.with(|c| c.set(remote && !watched));
    if remote && !watched {
        quarantine_down();
    }
}

/// The choice is made (or the capture cancelled): the hook, if one watched, stops, and the keys
/// it saw released while the OS still has them down become stale.
pub fn settle() {
    if let Some(down) = watch::stop() {
        let stuck = os_down_bits() & !down;
        STALE.fetch_or(stuck, Ordering::SeqCst);
    }
}

/// Whatever is down now goes into the quarantine.
fn quarantine_down() {
    QUARANTINE.with(|c| c.set(os_down_bits()));
}

/// The self-test (and unit tests) press modifiers only as window events, never on the keyboard.
fn events_only() -> bool {
    static SELFTEST: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    cfg!(test) || *SELFTEST.get_or_init(|| std::env::var_os("ZNIMOK_SELFTEST").is_some())
}

/// A key event for `k` reached one of our windows (or the OS has it up): its state is fresh.
pub fn fresh(k: Key) {
    QUARANTINE.with(|c| c.set(c.get() & !k.bit()));
    STALE.fetch_and(!k.bit(), Ordering::SeqCst);
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
    if watch::active() {
        // Down by the OS and by the hook — not just stuck after a release the client took.
        return watch::down() & k.bit() != 0;
    }
    QUARANTINE.with(|c| c.get() & k.bit() == 0)
}

/// Feeds the window events of any of our windows: the focus arms the quarantine, a key event
/// for a modifier lifts it.
pub fn observe(ev: &slint::winit_030::winit::event::WindowEvent) {
    use slint::winit_030::winit::event::WindowEvent;
    use slint::winit_030::winit::keyboard::{KeyCode, PhysicalKey};
    match ev {
        // Keys known stuck stay out of a window that gains the focus; after a capture from a
        // Remote Desktop client that the hook could not watch, everything down does (the editor
        // it opened gains the focus with the client's stuck modifiers).
        WindowEvent::Focused(true) => {
            if cfg!(windows) {
                if REMOTE.with(|c| c.get()) {
                    quarantine_down();
                } else {
                    let stale = stale_down();
                    QUARANTINE.with(|c| c.set(c.get() | stale));
                }
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
        STALE.store(Key::Alt.bit(), Ordering::SeqCst);
        fresh(Key::Alt);
        assert_eq!(QUARANTINE.with(|c| c.get()), 0);
        assert_eq!(STALE.load(Ordering::SeqCst), 0);
    }
}
