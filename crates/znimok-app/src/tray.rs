//! The tray / menu bar icon's picture and the single-instance rule.
//!
//! Closing the window leaves Znimok running in the tray (the hotkey keeps working), so starting
//! the app again must not create a second hidden process: the new process leaves a "wake" file
//! next to the lock and exits; the running one sees it on its timer and shows its window.
//! The lock is an OS file lock — it goes away with the process, even after a crash.

use std::fs::File;
use std::path::{Path, PathBuf};

/// The designer's app icon, one hand-made PNG per size (`icons/`, see `build_icons.py`).
const APP: &[(u32, &[u8])] = &[
    (16, include_bytes!("../icons/app-16.png")),
    (20, include_bytes!("../icons/app-20.png")),
    (24, include_bytes!("../icons/app-24.png")),
    (30, include_bytes!("../icons/app-30.png")),
    (32, include_bytes!("../icons/app-32.png")),
    (36, include_bytes!("../icons/app-36.png")),
    (40, include_bytes!("../icons/app-40.png")),
    (48, include_bytes!("../icons/app-48.png")),
    (64, include_bytes!("../icons/app-64.png")),
    (96, include_bytes!("../icons/app-96.png")),
    (128, include_bytes!("../icons/app-128.png")),
    (256, include_bytes!("../icons/app-256.png")),
];

/// The macOS menu bar glyph: black on transparent, a template image (macOS tints it).
#[cfg(target_os = "macos")]
const MENU_BAR: &[u8] = include_bytes!("../icons/menubar-32.png");

fn decode(png: &[u8]) -> slint::Image {
    match image::load_from_memory_with_format(png, image::ImageFormat::Png) {
        Ok(img) => {
            let img = img.to_rgba8();
            let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                img.as_raw(),
                img.width(),
                img.height(),
            );
            slint::Image::from_rgba8(buf)
        }
        Err(_) => slint::Image::default(),
    }
}

/// The app icon closest to `size` pixels: the smallest one at least that big (sharp, not
/// enlarged), or the biggest.
pub fn app_icon(size: u32) -> slint::Image {
    let png = APP
        .iter()
        .find(|(s, _)| *s >= size)
        .or(APP.last())
        .map(|(_, p)| *p)
        .unwrap_or_default();
    decode(png)
}

/// The system's UI scale (Windows: 1.0 at 100 %, 1.5 at 150 %).
fn system_scale() -> f32 {
    #[cfg(windows)]
    {
        // SAFETY: a plain query, no arguments.
        let dpi = unsafe { windows::Win32::UI::HiDpi::GetDpiForSystem() };
        if dpi > 0 {
            return dpi as f32 / 96.0;
        }
    }
    1.0
}

/// The window's own icon (title bar, taskbar, Alt+Tab): the size the taskbar asks for.
pub fn window_icon() -> slint::Image {
    app_icon((32.0 * system_scale()).round() as u32)
}

/// The tray / menu bar picture. Windows: the colour icon at the notification area's size
/// (16 px at 100 %). macOS: the black-and-white glyph, made a template by [`template_menu_icon`].
pub fn tray_icon() -> slint::Image {
    #[cfg(target_os = "macos")]
    {
        decode(MENU_BAR)
    }
    #[cfg(not(target_os = "macos"))]
    {
        app_icon((16.0 * system_scale()).round() as u32)
    }
}

/// Slint gives the status item a plain image; a template image is what lets macOS draw it
/// white on a dark menu bar and dimmed when inactive. The status item lives in a window of our
/// own process (NSStatusBarWindow); its button (NSStatusBarButton) may be the content view or
/// sit deeper in it, so the whole view tree is searched. Returns how many images were marked.
#[cfg(target_os = "macos")]
pub fn template_menu_icon() -> usize {
    use objc2_app_kit::NSApplication;
    let Some(mtm) = objc2::MainThreadMarker::new() else {
        return 0;
    };
    let debug = std::env::var_os("ZNIMOK_TRAY_DEBUG").is_some();
    let mut marked = 0;
    for w in NSApplication::sharedApplication(mtm).windows().iter() {
        let class = w.class().name().to_string_lossy().into_owned();
        if debug {
            eprintln!("tray: window {class}");
        }
        if !class.contains("StatusBar") {
            continue;
        }
        if let Some(view) = w.contentView() {
            marked += template_buttons(&view, debug);
        }
    }
    if debug {
        eprintln!("tray: {marked} status item image(s) made templates");
    }
    marked
}

#[cfg(target_os = "macos")]
fn template_buttons(view: &objc2_app_kit::NSView, debug: bool) -> usize {
    use objc2::ClassType;
    use objc2_app_kit::NSButton;
    use objc2_foundation::NSObjectProtocol;
    let mut marked = 0;
    if debug {
        eprintln!("tray:   view {}", view.class().name().to_string_lossy());
    }
    if view.isKindOfClass(NSButton::class()) {
        // SAFETY: checked just above that the view is an NSButton.
        let button: &NSButton = unsafe { &*(view as *const _ as *const NSButton) };
        if let Some(img) = button.image() {
            if !img.isTemplate() {
                img.setTemplate(true);
                button.setImage(Some(&img));
            }
            marked += 1;
        }
    }
    for sub in view.subviews().iter() {
        marked += template_buttons(&sub, debug);
    }
    marked
}

/// macOS: Znimok lives in the menu bar; the Dock icon (and the app menu) only while the editor
/// window is open. Cheap to call on a timer: it acts only on a change.
#[cfg(target_os = "macos")]
pub fn dock(shown: bool) {
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    thread_local! {
        static SHOWN: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
    }
    if SHOWN.with(|c| c.replace(Some(shown))) == Some(shown) {
        return;
    }
    let Some(mtm) = objc2::MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let policy = if shown {
        NSApplicationActivationPolicy::Regular
    } else {
        NSApplicationActivationPolicy::Accessory
    };
    if app.activationPolicy() == policy {
        return;
    }
    app.setActivationPolicy(policy);
    if shown {
        // Back from the menu bar only: bring the window to the front with the app.
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
    }
    // A policy change may give the status item a new button image.
    template_menu_icon();
}

pub struct Instance {
    _lock: Option<File>,
    wake: PathBuf,
}

pub enum Start {
    /// This is the only instance; keep the value alive for the whole run.
    First(Instance),
    /// Another instance runs and was asked to show its window.
    Woke,
}

/// The lock lives next to the library, so a test run with its own `ZNIMOK_LIBRARY` is separate.
pub fn start(dir: &Path) -> Start {
    let _ = std::fs::create_dir_all(dir);
    let wake = dir.join(".znimok-wake");
    let lock = match File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(".znimok-instance.lock"))
    {
        Ok(f) => f,
        // No lock possible (read-only folder): run anyway rather than refuse to start.
        Err(_) => return Start::First(Instance { _lock: None, wake }),
    };
    if lock.try_lock().is_ok() {
        let _ = std::fs::remove_file(&wake);
        Start::First(Instance {
            _lock: Some(lock),
            wake,
        })
    } else {
        let _ = std::fs::write(&wake, std::process::id().to_string());
        Start::Woke
    }
}

impl Instance {
    /// True once per wake request from another start.
    pub fn take_wake(&self) -> bool {
        std::fs::remove_file(&self.wake).is_ok()
    }
}
