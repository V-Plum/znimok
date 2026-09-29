//! Global hotkeys (ZK-44): the combinations from the settings, registered through
//! `global-hotkey` on both systems (RegisterHotKey on Windows, Carbon on macOS). Keys are
//! physical positions (`znimok_platform::KeyCombo`), so a combination works whatever keyboard
//! layout is on. A combination another program holds is refused — the previous one stays
//! ("rollback"); on Windows the region shot falls back to Ctrl+Shift+4, which the prototype used,
//! when the Little Helpers default Alt+Shift+4 is taken (LH itself still runs next to Znimok).
//! "Pause hotkeys" releases them all until resumed; switching capture off in the settings does too.

use std::cell::RefCell;
use std::collections::HashMap;
use std::str::FromStr;

use global_hotkey::hotkey::{Code, HotKey, Modifiers as HkMods};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use znimok_platform::{Key, KeyCombo, Modifiers};

/// What a hotkey does (the settings' `Hotkeys` fields, video comes with v2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Region,
    Screen,
    Clipboard,
    Editor,
    /// QR codes and barcodes on the screen (ZK-146).
    ReadCodes,
}

impl Action {
    /// In the order of the rows on the Hotkeys page.
    pub const ALL: [Action; 5] = [
        Action::Region,
        Action::Screen,
        Action::Clipboard,
        Action::Editor,
        Action::ReadCodes,
    ];

    pub fn of(h: &znimok_settings::Hotkeys, a: Action) -> Option<KeyCombo> {
        match a {
            Action::Region => h.region,
            Action::Screen => h.screen,
            Action::Clipboard => h.clipboard,
            Action::Editor => h.editor,
            Action::ReadCodes => h.read_codes,
        }
    }

    pub fn set(h: &mut znimok_settings::Hotkeys, a: Action, c: Option<KeyCombo>) {
        match a {
            Action::Region => h.region = c,
            Action::Screen => h.screen = c,
            Action::Clipboard => h.clipboard = c,
            Action::Editor => h.editor = c,
            Action::ReadCodes => h.read_codes = c,
        }
    }
}

/// Where a key registration stands, for the settings page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Off,
    On(KeyCombo),
    /// The wanted combination is held by another program (and what works instead, if anything).
    Taken {
        wanted: KeyCombo,
        active: Option<KeyCombo>,
    },
}

struct Service {
    manager: GlobalHotKeyManager,
    /// Registered: action → (combination, global-hotkey id).
    active: HashMap<Action, (KeyCombo, HotKey)>,
    state: HashMap<Action, State>,
    paused: bool,
    /// Esc, held globally while the capture overlay is open (see [`grab_escape`]).
    esc: Option<HotKey>,
}

thread_local! {
    static SVC: RefCell<Option<Service>> = const { RefCell::new(None) };
}

/// Starts the service on the UI thread (Windows: the hidden window's messages come through
/// winit's loop) and registers the settings' keys.
pub fn start(keys: &znimok_settings::Hotkeys, enabled: bool) {
    let Ok(manager) = GlobalHotKeyManager::new() else {
        return;
    };
    GlobalHotKeyEvent::set_event_handler(Some(|e: GlobalHotKeyEvent| {
        if e.state() != HotKeyState::Pressed {
            return;
        }
        let id = e.id();
        let _ = slint::invoke_from_event_loop(move || {
            let esc = SVC.with(|s| {
                s.borrow()
                    .as_ref()
                    .and_then(|s| s.esc)
                    .is_some_and(|hk| hk.id() == id)
            });
            if esc {
                crate::overlay::escape();
                return;
            }
            let action = SVC.with(|s| {
                s.borrow().as_ref().and_then(|s| {
                    s.active
                        .iter()
                        .find(|(_, (_, hk))| hk.id() == id)
                        .map(|(a, _)| *a)
                })
            });
            if let Some(a) = action {
                crate::hotkey_pressed(a);
            }
        });
    }));
    SVC.with(|s| {
        *s.borrow_mut() = Some(Service {
            manager,
            active: HashMap::new(),
            state: HashMap::new(),
            paused: !enabled,
            esc: None,
        })
    });
    apply(keys, enabled);
}

/// Registers the combinations of the settings (after a change, a reset, capture on/off).
pub fn apply(keys: &znimok_settings::Hotkeys, enabled: bool) {
    SVC.with(|s| {
        let mut s = s.borrow_mut();
        let Some(svc) = s.as_mut() else { return };
        for (_, (_, hk)) in svc.active.drain() {
            let _ = svc.manager.unregister(hk);
        }
        svc.paused = !enabled;
        for a in Action::ALL {
            let want = Action::of(keys, a);
            let st = match want {
                None => State::Off,
                Some(_) if !enabled => State::Off,
                Some(c) => match register(svc, a, c) {
                    true => State::On(c),
                    false => {
                        // The prototype's region key as the way out of a clash with LH.
                        let fallback = (a == Action::Region && cfg!(windows))
                            .then(|| KeyCombo::parse("Ctrl+Shift+4").ok())
                            .flatten()
                            .filter(|f| *f != c && register(svc, a, *f));
                        State::Taken {
                            wanted: c,
                            active: fallback,
                        }
                    }
                },
            };
            svc.state.insert(a, st);
        }
    });
}

/// Esc cancels the capture overlay even when the keyboard is still with another program: the
/// overlay comes up from a global hotkey, and the system may leave the focus where it was
/// (owner 29.09: Esc did nothing). Held only while the overlay is open.
pub fn grab_escape(on: bool) {
    SVC.with(|s| {
        let mut s = s.borrow_mut();
        let Some(svc) = s.as_mut() else { return };
        if let Some(hk) = svc.esc.take() {
            let _ = svc.manager.unregister(hk);
        }
        if on {
            let hk = HotKey::new(None, Code::Escape);
            if svc.manager.register(hk).is_ok() {
                svc.esc = Some(hk);
            }
        }
    });
}

fn register(svc: &mut Service, a: Action, c: KeyCombo) -> bool {
    let Some(hk) = to_hotkey(c) else { return false };
    if svc.manager.register(hk).is_err() {
        return false;
    }
    svc.active.insert(a, (c, hk));
    true
}

/// A new combination for one action: kept only if the system gives it; the old one returns
/// otherwise. `None` switches the action off. Returns whether the new combination took.
pub fn try_set(a: Action, c: Option<KeyCombo>) -> bool {
    SVC.with(|s| {
        let mut s = s.borrow_mut();
        let Some(svc) = s.as_mut() else { return false };
        let old = svc.active.remove(&a);
        if let Some((_, hk)) = old {
            let _ = svc.manager.unregister(hk);
        }
        let Some(c) = c else {
            svc.state.insert(a, State::Off);
            return true;
        };
        // Not twice: a combination another action of ours holds is taken too.
        let ours = svc.active.values().any(|(k, _)| *k == c);
        if !ours && register(svc, a, c) {
            svc.state.insert(a, State::On(c));
            return true;
        }
        if let Some((k, _)) = old {
            register(svc, a, k);
        }
        false
    })
}

/// Everything released while a new combination is being typed (else our own key would fire
/// instead of reaching the field), or while paused from the tray.
pub fn set_paused(paused: bool, keys: &znimok_settings::Hotkeys, enabled: bool) {
    if paused {
        SVC.with(|s| {
            if let Some(svc) = s.borrow_mut().as_mut() {
                for (_, (_, hk)) in svc.active.drain() {
                    let _ = svc.manager.unregister(hk);
                }
                svc.paused = true;
            }
        });
    } else {
        apply(keys, enabled);
    }
}

pub fn is_paused() -> bool {
    SVC.with(|s| s.borrow().as_ref().is_some_and(|s| s.paused))
}

pub fn state(a: Action) -> State {
    SVC.with(|s| {
        s.borrow()
            .as_ref()
            .and_then(|s| s.state.get(&a).cloned())
            .unwrap_or(State::Off)
    })
}

/// The combination that works for an action now (for the hints: "Press … to capture").
pub fn active(a: Action) -> Option<KeyCombo> {
    SVC.with(|s| {
        s.borrow()
            .as_ref()
            .and_then(|s| s.active.get(&a).map(|(k, _)| *k))
    })
}

fn to_hotkey(c: KeyCombo) -> Option<HotKey> {
    let code = Code::from_str(&w3c(c.key)).ok()?;
    let mut m = HkMods::empty();
    if c.mods.contains(Modifiers::CTRL) {
        m |= HkMods::CONTROL;
    }
    if c.mods.contains(Modifiers::ALT) {
        m |= HkMods::ALT;
    }
    if c.mods.contains(Modifiers::SHIFT) {
        m |= HkMods::SHIFT;
    }
    if c.mods.contains(Modifiers::META) {
        m |= HkMods::SUPER;
    }
    Some(HotKey::new((!m.is_empty()).then_some(m), code))
}

/// The W3C `KeyboardEvent.code` of a key — the names both `global-hotkey` and winit use.
pub fn w3c(k: Key) -> String {
    let n = k.name();
    match k {
        Key::Left => "ArrowLeft".into(),
        Key::Right => "ArrowRight".into(),
        Key::Up => "ArrowUp".into(),
        Key::Down => "ArrowDown".into(),
        Key::Minus => "Minus".into(),
        Key::Equal => "Equal".into(),
        Key::BracketLeft => "BracketLeft".into(),
        Key::BracketRight => "BracketRight".into(),
        Key::Backslash => "Backslash".into(),
        Key::Semicolon => "Semicolon".into(),
        Key::Quote => "Quote".into(),
        Key::Backquote => "Backquote".into(),
        Key::Comma => "Comma".into(),
        Key::Period => "Period".into(),
        Key::Slash => "Slash".into(),
        _ if n.len() == 1 && n.as_bytes()[0].is_ascii_digit() => format!("Digit{n}"),
        _ if n.len() == 1 => format!("Key{n}"),
        _ => n.into(),
    }
}

/// Back from a W3C code name (winit's `KeyCode` debug name) to our key.
pub fn from_w3c(code: &str) -> Option<Key> {
    Key::ALL.iter().copied().find(|k| w3c(*k) == code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_has_a_code_both_ways() {
        for k in Key::ALL {
            let c = w3c(*k);
            assert!(Code::from_str(&c).is_ok(), "{c} is not a W3C code");
            assert_eq!(from_w3c(&c), Some(*k));
        }
        let hk = to_hotkey(KeyCombo::parse("Alt+Shift+4").unwrap()).unwrap();
        assert_eq!(hk.key, Code::Digit4);
    }
}
