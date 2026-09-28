//! The interfaces. Implementations: `znimok-win`, `znimok-mac`, and [`crate::synthetic`] for tests.
//! Every trait is `Send + Sync`; callbacks run on the implementation's event thread, so the app
//! forwards them into its command queue (PLAN §5.1: no shared mutable state between parts).

use crate::frame::{ColorInfo, Frame};
use crate::geom::{Point, Rect};
use crate::keys::KeyCombo;
use crate::{PlatformError, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------------------------
// Displays, windows, capture

/// Stable within a session: Windows device name (`\\.\DISPLAY1`), macOS `CGDirectDisplayID`.
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct DisplayId(pub String);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DisplayInfo {
    pub id: DisplayId,
    /// Human name (monitor model), may be empty.
    pub name: String,
    /// Whole display, desktop units.
    pub bounds: Rect,
    /// Without the taskbar / menu bar and Dock.
    pub work_area: Rect,
    /// UI scale the user chose (1.5 at 150 % / 144 DPI; 2.0 on a Retina display).
    pub scale_factor: f32,
    /// Physical pixels per desktop unit: 1.0 on Windows, the backing scale on macOS.
    pub pixels_per_unit: f32,
    pub primary: bool,
    pub refresh_hz: Option<f32>,
    /// What a capture of this display delivers (HDR state, SDR white).
    pub color: ColorInfo,
}

impl DisplayInfo {
    /// Size of a full capture of this display in pixels.
    pub fn pixel_size(&self) -> (u32, u32) {
        let r = self.bounds.scaled(self.pixels_per_unit);
        (r.width, r.height)
    }
}

/// Stable while the window lives: `HWND` value, macOS `CGWindowID`.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub struct WindowId(pub u64);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WindowInfo {
    pub id: WindowId,
    pub title: String,
    /// Executable name (Windows) or bundle name (macOS).
    pub app: String,
    pub pid: u32,
    /// Visible frame in desktop units — without the invisible resize border and shadow
    /// (`DWMWA_EXTENDED_FRAME_BOUNDS` on Windows).
    pub bounds: Rect,
    /// The display holding most of the window.
    pub display: Option<DisplayId>,
    pub scale_factor: f32,
    pub minimized: bool,
    /// Belongs to Znimok itself (overlay, preview) — excluded from pickers.
    pub own: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CaptureTarget {
    Display {
        id: DisplayId,
    },
    Window {
        id: WindowId,
    },
    /// Desktop units; must lie within one display (implementations may refuse a region that spans two).
    Region {
        rect: Rect,
    },
    /// What the system content picker returned (macOS); opaque token valid for this session.
    Picked {
        token: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CaptureOptions {
    /// Draw the mouse pointer into the frame. Znimok draws its own pointer, so usually false.
    pub cursor: bool,
    /// Deliver HDR frames as they are (`Rgba16Float` / `Rgb10A2`); false = SDR 8-bit if the
    /// implementation can do it cheaper.
    pub keep_hdr: bool,
}

impl Default for CaptureOptions {
    fn default() -> Self {
        Self {
            cursor: false,
            keep_hdr: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CaptureCaps {
    /// No yellow border / privacy indicator in the captured area.
    pub borderless: bool,
    pub hdr: bool,
    pub window_capture: bool,
    /// Permission that capture needs on this OS (Screen Recording on macOS).
    pub needs_permission: Option<Permission>,
    /// [`Capture::pick`] shows the system content picker.
    pub system_picker: bool,
}

pub type PickCallback = Box<dyn FnOnce(Result<CaptureTarget>) + Send>;

pub trait Capture: Send + Sync {
    fn caps(&self) -> CaptureCaps;

    /// All displays, primary first.
    fn displays(&self) -> Result<Vec<DisplayInfo>>;

    /// One still frame of the target.
    fn capture(&self, target: &CaptureTarget, opts: &CaptureOptions) -> Result<Frame>;

    /// Show the system content picker; `done` gets a target for [`Capture::capture`] or
    /// [`PlatformError::Cancelled`]. Default: not available.
    fn pick(&self, done: PickCallback) {
        done(Err(PlatformError::Unsupported("системний пікер")));
    }

    /// The display under a desktop point.
    fn display_at(&self, p: Point) -> Result<Option<DisplayInfo>> {
        Ok(self.displays()?.into_iter().find(|d| d.bounds.contains(p)))
    }
}

pub trait WindowList: Send + Sync {
    /// Visible top-level windows, **front to back**; cloaked/zero-size windows left out.
    fn windows(&self) -> Result<Vec<WindowInfo>>;

    /// Topmost window under a desktop point (skipping minimized and own windows).
    fn window_at(&self, p: Point) -> Result<Option<WindowInfo>> {
        Ok(self
            .windows()?
            .into_iter()
            .find(|w| !w.minimized && !w.own && w.bounds.contains(p)))
    }
}

/// Mouse pointer position (display under the cursor, click highlighting later).
pub trait Cursor: Send + Sync {
    fn position(&self) -> Result<Point>;
}

// ---------------------------------------------------------------------------------------------
// Hotkeys

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HotkeyId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    Free,
    /// Another program (or another of our ids) holds it.
    Taken,
    /// An OS shortcut uses it (e.g. ⌘⇧3 while macOS screenshots are on).
    TakenBySystem,
    /// The OS cannot tell (macOS may swallow conflicts silently).
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotkeyEvent {
    pub id: HotkeyId,
    pub pressed: bool,
}

pub type HotkeyHandler = Box<dyn Fn(HotkeyEvent) + Send + Sync>;

pub trait Hotkeys: Send + Sync {
    /// Register a global hotkey; [`PlatformError::Busy`] if the combination is taken.
    fn register(&self, id: HotkeyId, combo: KeyCombo) -> Result<()>;
    fn unregister(&self, id: HotkeyId) -> Result<()>;
    /// Trial registration without keeping it.
    fn probe(&self, combo: KeyCombo) -> Availability;
    /// Release all our hotkeys temporarily (LH «пауза клавіш»); `false` registers them again.
    fn set_paused(&self, paused: bool) -> Result<()>;
    fn set_handler(&self, handler: HotkeyHandler);
}

// ---------------------------------------------------------------------------------------------
// Tray / menu bar

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayState {
    Idle,
    /// Capturing or exporting — icon shows activity.
    Busy,
    Recording {
        seconds: u32,
    },
    Paused {
        seconds: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuEntry {
    Item {
        id: String,
        label: String,
        enabled: bool,
        checked: Option<bool>,
    },
    Separator,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    Click,
    DoubleClick,
    Menu(String),
}

pub type TrayHandler = Box<dyn Fn(TrayEvent) + Send + Sync>;

pub trait Tray: Send + Sync {
    fn set_state(&self, state: TrayState);
    fn set_tooltip(&self, text: &str);
    fn set_menu(&self, menu: Vec<MenuEntry>);
    fn set_handler(&self, handler: TrayHandler);
}

// ---------------------------------------------------------------------------------------------
// Clipboard, share, shell, notifications

#[derive(Clone, Debug, PartialEq)]
pub struct ClipImage {
    pub width: u32,
    pub height: u32,
    /// Straight (not premultiplied) RGBA8.
    pub rgba: Vec<u8>,
    /// Encoded PNG when the writer has it (put on the clipboard as is, keeps metadata).
    pub png: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClipItem {
    Image(ClipImage),
    Files(Vec<PathBuf>),
    Text(String),
}

pub trait Clipboard: Send + Sync {
    /// Replace the clipboard with all `items` at once (image + file = both formats offered).
    fn write(&self, items: &[ClipItem]) -> Result<()>;
    /// Everything readable now, richest first.
    fn read(&self) -> Result<Vec<ClipItem>>;
}

pub trait Share: Send + Sync {
    fn available(&self) -> bool;
    /// System share sheet for `files`, anchored near `anchor` (desktop units) if given.
    fn share(&self, files: &[PathBuf], anchor: Option<Rect>) -> Result<()>;
}

pub trait Shell: Send + Sync {
    /// Open with the default program.
    fn open(&self, path: &Path) -> Result<()>;
    /// Show in Explorer / Finder with the file selected.
    fn reveal(&self, path: &Path) -> Result<()>;
}

pub trait Notifications: Send + Sync {
    fn notify(&self, title: &str, body: &str) -> Result<()>;
}

// ---------------------------------------------------------------------------------------------
// Permissions, autostart, file associations

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    ScreenRecording,
    Microphone,
    InputMonitoring,
    Accessibility,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PermissionState {
    Granted,
    Denied,
    NotDetermined,
    /// This OS has no such gate (Windows screen capture).
    NotNeeded,
}

pub trait Permissions: Send + Sync {
    fn status(&self, p: Permission) -> PermissionState;
    /// May show the system prompt. On macOS a fresh Screen Recording grant needs a restart,
    /// so the answer can stay `NotDetermined` until then.
    fn request(&self, p: Permission) -> PermissionState;
    /// Open the settings page where the user grants it.
    fn open_settings(&self, p: Permission) -> Result<()>;
}

/// Start at login, as the OS sees it (the settings page reads it from here, not from a file).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AutostartState {
    On,
    Off,
    /// Registered, but the user still has to allow it (macOS: System Settings → General → Login
    /// Items).
    NeedsApproval,
    /// Registered, but switched off in the OS (Windows: Task Manager → Startup apps). Turning it
    /// on in Znimok is the user's explicit choice and clears that.
    DisabledInSystem,
    /// This copy cannot start at login (macOS: not inside an .app bundle).
    Unavailable,
}

pub trait Autostart: Send + Sync {
    fn is_enabled(&self) -> Result<bool>;
    fn set_enabled(&self, on: bool) -> Result<()>;
    /// The finer state for the settings page.
    fn state(&self) -> Result<AutostartState> {
        Ok(if self.is_enabled()? {
            AutostartState::On
        } else {
            AutostartState::Off
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssocState {
    Ours,
    /// Another program opens it (its ProgID / bundle id).
    Other(String),
    None,
}

pub trait FileAssoc: Send + Sync {
    /// `ext` without the dot, e.g. `"znimok"`.
    fn state(&self, ext: &str) -> Result<AssocState>;
    fn register(&self, ext: &str) -> Result<()>;
    fn unregister(&self, ext: &str) -> Result<()>;
}
