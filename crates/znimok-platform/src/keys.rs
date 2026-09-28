//! Key combinations for global hotkeys.
//!
//! Keys are **physical positions** named after the US layout (like W3C `KeyboardEvent.code`):
//! `Alt+Shift+3` is the same key with a Ukrainian or a German layout active — as `RegisterHotKey`
//! with virtual digit keys and Carbon key codes behave. Text form: `Ctrl+Alt+Shift+Meta+Key`,
//! parsed case-insensitively with the usual aliases (`Cmd`, `Win`, `Super` = Meta; `Option` = Alt;
//! `Control` = Ctrl). `display(Os::MacOs)` gives the menu glyphs `⌃⌥⇧⌘`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers(u8);

impl Modifiers {
    pub const NONE: Self = Self(0);
    pub const CTRL: Self = Self(1);
    pub const ALT: Self = Self(2);
    pub const SHIFT: Self = Self(4);
    /// Windows key on Windows, Command on macOS.
    pub const META: Self = Self(8);

    pub const fn contains(self, o: Self) -> bool {
        self.0 & o.0 == o.0
    }
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
    pub const fn bits(self) -> u8 {
        self.0
    }
}

impl std::ops::BitOr for Modifiers {
    type Output = Self;
    fn bitor(self, o: Self) -> Self {
        Self(self.0 | o.0)
    }
}

macro_rules! keys {
    ($($v:ident = $name:literal),* $(,)?) => {
        /// A physical key (US-layout name).
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Key { $($v),* }

        impl Key {
            pub const ALL: &'static [Key] = &[$(Key::$v),*];

            /// Canonical text name (`"3"`, `"E"`, `"F12"`, `"PrintScreen"`).
            pub const fn name(self) -> &'static str {
                match self { $(Key::$v => $name),* }
            }
        }
    };
}

keys! {
    A = "A", B = "B", C = "C", D = "D", E = "E", F = "F", G = "G", H = "H", I = "I", J = "J",
    K = "K", L = "L", M = "M", N = "N", O = "O", P = "P", Q = "Q", R = "R", S = "S", T = "T",
    U = "U", V = "V", W = "W", X = "X", Y = "Y", Z = "Z",
    Digit0 = "0", Digit1 = "1", Digit2 = "2", Digit3 = "3", Digit4 = "4",
    Digit5 = "5", Digit6 = "6", Digit7 = "7", Digit8 = "8", Digit9 = "9",
    F1 = "F1", F2 = "F2", F3 = "F3", F4 = "F4", F5 = "F5", F6 = "F6", F7 = "F7", F8 = "F8",
    F9 = "F9", F10 = "F10", F11 = "F11", F12 = "F12", F13 = "F13", F14 = "F14", F15 = "F15",
    F16 = "F16", F17 = "F17", F18 = "F18", F19 = "F19", F20 = "F20", F21 = "F21", F22 = "F22",
    F23 = "F23", F24 = "F24",
    Space = "Space", Enter = "Enter", Escape = "Escape", Tab = "Tab", Backspace = "Backspace",
    Delete = "Delete", Insert = "Insert", Home = "Home", End = "End", PageUp = "PageUp",
    PageDown = "PageDown", Left = "Left", Right = "Right", Up = "Up", Down = "Down",
    PrintScreen = "PrintScreen", Pause = "Pause",
    Minus = "-", Equal = "=", BracketLeft = "[", BracketRight = "]", Backslash = "\\",
    Semicolon = ";", Quote = "'", Backquote = "`", Comma = ",", Period = ".", Slash = "/",
}

impl Key {
    fn parse(s: &str) -> Option<Key> {
        let alias = match s.to_ascii_lowercase().as_str() {
            "esc" => Some(Key::Escape),
            "return" => Some(Key::Enter),
            "del" => Some(Key::Delete),
            "ins" => Some(Key::Insert),
            "prtsc" | "prtscr" | "printscr" | "print" => Some(Key::PrintScreen),
            "pgup" => Some(Key::PageUp),
            "pgdn" => Some(Key::PageDown),
            "plus" => Some(Key::Equal),
            _ => None,
        };
        alias.or_else(|| {
            Key::ALL
                .iter()
                .copied()
                .find(|k| k.name().eq_ignore_ascii_case(s))
        })
    }
}

/// Which OS a combination is shown for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    MacOs,
}

impl Os {
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Windows
        }
    }
}

/// Modifiers + one key. At least one modifier is required unless the key is F13–F24,
/// PrintScreen or Pause (a bare letter as a global hotkey would steal typing).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyCombo {
    pub mods: Modifiers,
    pub key: Key,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComboError {
    Empty,
    UnknownPart(String),
    NoKey,
    TwoKeys,
    NeedsModifier(Key),
}

impl fmt::Display for ComboError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "порожня комбінація"),
            Self::UnknownPart(p) => write!(f, "невідома частина «{p}»"),
            Self::NoKey => write!(f, "немає основної клавіші"),
            Self::TwoKeys => write!(f, "дві основні клавіші"),
            Self::NeedsModifier(k) => write!(
                f,
                "{} без модифікатора перехоплюватиме набір тексту",
                k.name()
            ),
        }
    }
}

impl std::error::Error for ComboError {}

impl KeyCombo {
    pub const fn new(mods: Modifiers, key: Key) -> Self {
        Self { mods, key }
    }

    fn standalone_ok(key: Key) -> bool {
        matches!(
            key,
            Key::F13
                | Key::F14
                | Key::F15
                | Key::F16
                | Key::F17
                | Key::F18
                | Key::F19
                | Key::F20
                | Key::F21
                | Key::F22
                | Key::F23
                | Key::F24
                | Key::PrintScreen
                | Key::Pause
        )
    }

    pub fn parse(s: &str) -> Result<Self, ComboError> {
        let s = s.trim();
        if s.is_empty() {
            return Err(ComboError::Empty);
        }
        // "+" as the key itself: "Ctrl++" → treat the trailing empty part as "=" (plus).
        let parts: Vec<&str> = if let Some(head) = s.strip_suffix("++") {
            head.split('+').chain(["plus"]).collect()
        } else {
            s.split('+').collect()
        };
        let (mut mods, mut key) = (Modifiers::NONE, None);
        for p in parts.iter().map(|p| p.trim()) {
            let m = match p.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "⌃" => Some(Modifiers::CTRL),
                "alt" | "option" | "opt" | "⌥" => Some(Modifiers::ALT),
                "shift" | "⇧" => Some(Modifiers::SHIFT),
                "meta" | "cmd" | "command" | "win" | "super" | "⌘" => Some(Modifiers::META),
                _ => None,
            };
            match m {
                Some(m) => mods = mods | m,
                None => {
                    let k = Key::parse(p).ok_or_else(|| ComboError::UnknownPart(p.to_string()))?;
                    if key.replace(k).is_some() {
                        return Err(ComboError::TwoKeys);
                    }
                }
            }
        }
        let key = key.ok_or(ComboError::NoKey)?;
        if mods.is_empty() && !Self::standalone_ok(key) {
            return Err(ComboError::NeedsModifier(key));
        }
        Ok(Self { mods, key })
    }

    /// How the combination is written in this OS's menus and settings.
    pub fn display(&self, os: Os) -> String {
        let m = self.mods;
        match os {
            Os::Windows => {
                let mut s = String::new();
                for (flag, name) in [
                    (Modifiers::CTRL, "Ctrl+"),
                    (Modifiers::ALT, "Alt+"),
                    (Modifiers::SHIFT, "Shift+"),
                    (Modifiers::META, "Win+"),
                ] {
                    if m.contains(flag) {
                        s.push_str(name);
                    }
                }
                s + self.key.name()
            }
            Os::MacOs => {
                // Apple order: Control, Option, Shift, Command.
                let mut s = String::new();
                for (flag, glyph) in [
                    (Modifiers::CTRL, '⌃'),
                    (Modifiers::ALT, '⌥'),
                    (Modifiers::SHIFT, '⇧'),
                    (Modifiers::META, '⌘'),
                ] {
                    if m.contains(flag) {
                        s.push(glyph);
                    }
                }
                s + self.key.name()
            }
        }
    }
}

/// Canonical text form (stored in settings): `Ctrl+Alt+Shift+Meta+Key`, independent of the OS.
impl fmt::Display for KeyCombo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (flag, name) in [
            (Modifiers::CTRL, "Ctrl+"),
            (Modifiers::ALT, "Alt+"),
            (Modifiers::SHIFT, "Shift+"),
            (Modifiers::META, "Meta+"),
        ] {
            if self.mods.contains(flag) {
                f.write_str(name)?;
            }
        }
        f.write_str(self.key.name())
    }
}

impl std::str::FromStr for KeyCombo {
    type Err = ComboError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl Serialize for KeyCombo {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for KeyCombo {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for KeyCombo {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "KeyCombo".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "Modifiers and one physical key (US-layout name), e.g. \"Alt+Shift+3\", \"Ctrl+Alt+E\", \"Meta+Shift+4\"",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_print_round_trip() {
        for s in [
            "Alt+Shift+3",
            "Ctrl+Alt+E",
            "Ctrl+Alt+4",
            "Shift+Meta+5",
            "Ctrl+Shift+F12",
            "PrintScreen",
            "Ctrl+=",
        ] {
            let c = KeyCombo::parse(s).unwrap();
            assert_eq!(KeyCombo::parse(&c.to_string()).unwrap(), c, "{s}");
        }
        assert_eq!(
            KeyCombo::parse("Shift+Alt+3").unwrap().to_string(),
            "Alt+Shift+3"
        );
    }

    #[test]
    fn aliases() {
        let a = KeyCombo::parse("cmd+option+shift+4").unwrap();
        assert_eq!(
            a,
            KeyCombo::new(
                Modifiers::META | Modifiers::ALT | Modifiers::SHIFT,
                Key::Digit4
            )
        );
        assert_eq!(KeyCombo::parse("Win+PrtSc").unwrap().key, Key::PrintScreen);
        assert_eq!(KeyCombo::parse("Ctrl++").unwrap().key, Key::Equal);
        assert_eq!(
            KeyCombo::parse("⌘+⇧+3").unwrap().to_string(),
            "Shift+Meta+3"
        );
    }

    #[test]
    fn display_per_os() {
        let c = KeyCombo::parse("Ctrl+Alt+Shift+Meta+E").unwrap();
        assert_eq!(c.display(Os::Windows), "Ctrl+Alt+Shift+Win+E");
        assert_eq!(c.display(Os::MacOs), "⌃⌥⇧⌘E");
    }

    #[test]
    fn rejects_bad_combos() {
        assert_eq!(KeyCombo::parse(""), Err(ComboError::Empty));
        assert_eq!(KeyCombo::parse("Ctrl+Alt"), Err(ComboError::NoKey));
        assert_eq!(KeyCombo::parse("Ctrl+A+B"), Err(ComboError::TwoKeys));
        assert_eq!(KeyCombo::parse("E"), Err(ComboError::NeedsModifier(Key::E)));
        assert!(matches!(
            KeyCombo::parse("Ctrl+Hyper"),
            Err(ComboError::UnknownPart(_))
        ));
        assert!(KeyCombo::parse("F13").is_ok());
    }

    #[test]
    fn serde_as_string() {
        let c = KeyCombo::parse("Alt+Shift+3").unwrap();
        let j = serde_json::to_string(&c).unwrap();
        assert_eq!(j, "\"Alt+Shift+3\"");
        assert_eq!(serde_json::from_str::<KeyCombo>(&j).unwrap(), c);
        assert!(serde_json::from_str::<KeyCombo>("\"Q\"").is_err());
    }
}
