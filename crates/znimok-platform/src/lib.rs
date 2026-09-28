//! Znimok platform interfaces (PLAN §5.2, ZK-35): what the app needs from the OS, as traits
//! with no Win32/AppKit types in their signatures — capture, window list, cursor, hotkeys, tray,
//! clipboard, share, shell, notifications, permissions, autostart, file associations.
//!
//! `znimok-win` and `znimok-mac` implement them; [`synthetic`] implements all of them in memory
//! with knobs and inspection for tests; [`conformance`] holds checks every implementation must
//! pass (the OS crates run them against the real thing).

pub mod conformance;
pub mod frame;
pub mod geom;
pub mod keys;
pub mod synthetic;
mod traits;

pub use frame::{ColorInfo, Frame, PixelFormat, Transfer};
pub use geom::{Point, Rect};
pub use keys::{ComboError, Key, KeyCombo, Modifiers, Os};
pub use traits::*;

use std::fmt;
use std::sync::Arc;

pub type Result<T> = std::result::Result<T, PlatformError>;

#[derive(Clone, Debug, PartialEq)]
pub enum PlatformError {
    /// This OS or implementation cannot do it.
    Unsupported(&'static str),
    PermissionDenied(Permission),
    /// The display / window / file is gone.
    NotFound(String),
    /// Taken by someone else (hotkey, clipboard lock).
    Busy(String),
    /// The user cancelled (picker, prompt).
    Cancelled,
    /// An OS call failed: HRESULT / Win32 error / OSStatus.
    Os {
        code: i64,
        message: String,
    },
    Other(String),
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(what) => write!(f, "не підтримується: {what}"),
            Self::PermissionDenied(p) => write!(f, "немає дозволу: {p:?}"),
            Self::NotFound(what) => write!(f, "не знайдено: {what}"),
            Self::Busy(what) => write!(f, "зайнято: {what}"),
            Self::Cancelled => write!(f, "скасовано"),
            Self::Os { code, message } => write!(f, "помилка ОС 0x{code:08x}: {message}"),
            Self::Other(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for PlatformError {}

/// Everything the app gets from the OS, as one value it can pass around (or build from
/// [`synthetic`] in tests).
#[derive(Clone)]
pub struct Platform {
    pub capture: Arc<dyn Capture>,
    pub windows: Arc<dyn WindowList>,
    pub cursor: Arc<dyn Cursor>,
    pub hotkeys: Arc<dyn Hotkeys>,
    pub tray: Arc<dyn Tray>,
    pub clipboard: Arc<dyn Clipboard>,
    pub share: Arc<dyn Share>,
    pub shell: Arc<dyn Shell>,
    pub notifications: Arc<dyn Notifications>,
    pub permissions: Arc<dyn Permissions>,
    pub autostart: Arc<dyn Autostart>,
    pub file_assoc: Arc<dyn FileAssoc>,
}
