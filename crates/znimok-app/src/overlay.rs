//! The capture overlay (ZK-39/40, both OSes): the display under the pointer is frozen, shown
//! full-screen in a borderless top-most window, and the user picks what to keep:
//! drag = region, click = the window under the pointer (or the whole screen over the desktop),
//! Space = whole screen, Enter = what is highlighted, Shift on release = straight to the
//! clipboard and the library, Alt (⌥) on release = edit right over the screen (ZK-58),
//! Esc / right click / the capture key again = cancel.
//! Guides through the pointer across the whole screen are the cursor (Little Helpers).
//! The magnifier is off until the wheel turns it on (×4 → ×8 → ×16 → off, as in LH CAPS-86);
//! it is drawn here pixel by pixel (nearest neighbour, grid from ×8, the centre pixel boxed).
//! Regions and the whole screen are cut from the frozen frame (instant, identical on both OSes);
//! a clicked window is captured alone (without what overlaps it), falling back to the cut.
//! A second click (a double click, or a click followed at once by a drag) takes the shot after a
//! 3-2-1 countdown (ZK-128, LH CAPS-85), from a fresh frame of the same display — time to open a
//! menu that closes on a hotkey. Losing the focus (switching programs) cancels.
//! Not yet: regions spanning several displays.

use std::cell::RefCell;

use slint::ComponentHandle;

use crate::capture::{Frozen, PxRect};
use crate::{Overlay, io};

/// One overlay window: a display, and where it is in the frozen frame (ZK-139).
struct Part {
    ui: Overlay,
    /// The display in the frame, frame pixels.
    rect: PxRect,
    /// Frame pixels per logical pixel of this window, horizontally and vertically.
    kx: f32,
    ky: f32,
}

struct Session {
    /// A window per display; the selection, the lens and the gestures work in frame pixels
    /// across all of them, so a region may cross from one screen to the next.
    parts: Vec<Part>,
    /// The window the pointer is in (or that holds the drag).
    active: usize,
    frozen: Frozen,
    drag_start: Option<(f32, f32)>,
    dragging: bool,
    sel: Option<PxRect>,
    /// Id of the highlighted window when `sel` is a window.
    window: Option<u64>,
    /// 0 = magnifier off, else 4 / 8 / 16 screen pixels per frame pixel.
    zoom: u32,
    wheel_acc: f32,
    wheel_pause: Option<std::time::Instant>,
    last_pointer: (f32, f32),
    /// The editor window was visible before the capture (show it again on cancel).
    editor_was_visible: bool,
    /// A click waiting to see whether a second one follows (then: with the countdown).
    pending: Option<(f32, f32, Gesture)>,
    /// The button went down again while a click was pending.
    second: bool,
}

/// How long a click waits for a second one (LH: the system's double-click time, at most 400 ms).
const DOUBLE_CLICK: std::time::Duration = std::time::Duration::from_millis(400);

pub const COUNTDOWN_TITLE: &str = "Znimok countdown";

struct Count {
    ui: crate::Countdown,
    timer: slint::Timer,
    editor_was_visible: bool,
    then: Option<Box<dyn FnOnce()>>,
}

thread_local! {
    static COUNT: RefCell<Option<Count>> = const { RefCell::new(None) };
}

/// Esc from anywhere (a global hotkey while the overlay or the countdown is up).
pub fn escape() {
    if is_open() {
        cancel();
    } else if crate::scroll::active() {
        crate::scroll::cancel();
    } else {
        cancel_countdown();
    }
}

/// The countdown window, for the self-test.
pub fn countdown_open() -> bool {
    COUNT.with(|c| c.borrow().is_some())
}

pub fn countdown_window() -> Option<crate::Countdown> {
    COUNT.with(|c| c.borrow().as_ref().map(|c| c.ui.clone_strong()))
}

fn countdown_ms() -> u64 {
    std::env::var("ZNIMOK_COUNTDOWN_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(3000)
}

/// 3-2-1 in the bottom-right corner of `display`, then `then` (not called if Esc cancels).
fn countdown(
    display: znimok_platform::Rect,
    editor_was_visible: bool,
    then: impl FnOnce() + 'static,
) {
    let Ok(ui) = crate::Countdown::new() else {
        then();
        return;
    };
    let total = countdown_ms();
    ui.set_left(total.div_ceil(1000) as i32);
    let _ = ui.show();
    {
        use slint::winit_030::WinitWindowAccessor;
        // The mouse goes through to what is under it.
        ui.window().with_winit_window(|w| {
            let _ = w.set_cursor_hittest(false);
        });
    }
    crate::frame::round_window(ui.window());
    // Bottom-right of the usable area (above the taskbar / the Dock, ZK-147), 32 px from the
    // edges; desktop units are pixels on Windows, points on macOS.
    let display = crate::system::work_area(display);
    if cfg!(target_os = "macos") {
        ui.window().set_position(slint::LogicalPosition::new(
            (display.x + display.width as i32 - 32 - 120) as f32,
            (display.y + display.height as i32 - 32 - 120) as f32,
        ));
    } else {
        let k = ui.window().scale_factor();
        let side = (152.0 * k).round() as i32;
        ui.window().set_position(slint::PhysicalPosition::new(
            display.x + display.width as i32 - side,
            display.y + display.height as i32 - side,
        ));
    }
    crate::hotkeys::grab_escape(true);
    let start = std::time::Instant::now();
    let timer = slint::Timer::default();
    let weak = ui.as_weak();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(50),
        move || {
            let elapsed = start.elapsed().as_millis() as u64;
            if elapsed < total {
                if let Some(ui) = weak.upgrade() {
                    ui.set_left((total - elapsed).div_ceil(1000) as i32);
                }
                return;
            }
            let Some(mut c) = COUNT.with(|c| c.borrow_mut().take()) else {
                return;
            };
            c.timer.stop();
            let _ = c.ui.hide();
            crate::hotkeys::grab_escape(false);
            if let Some(then) = c.then.take() {
                // The compositor takes the countdown off the screen before the frame.
                slint::Timer::single_shot(std::time::Duration::from_millis(80), then);
            }
        },
    );
    COUNT.with(|c| {
        *c.borrow_mut() = Some(Count {
            ui,
            timer,
            editor_was_visible,
            then: Some(Box::new(then)),
        })
    });
}

fn cancel_countdown() {
    let Some(c) = COUNT.with(|c| c.borrow_mut().take()) else {
        return;
    };
    c.timer.stop();
    let _ = c.ui.hide();
    crate::hotkeys::grab_escape(false);
    if c.editor_was_visible {
        crate::with_ctx(|_, ui| crate::show_window(ui));
    }
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

/// The open overlay window, for the self-test.
pub fn handle() -> Option<Overlay> {
    SESSION.with(|s| {
        s.borrow()
            .as_ref()
            .and_then(|s| s.parts.first().map(|p| p.ui.clone_strong()))
    })
}

/// How many overlay windows are open (one per display), for the self-test.
pub fn window_count() -> usize {
    SESSION.with(|s| s.borrow().as_ref().map_or(0, |s| s.parts.len()))
}

pub fn is_open() -> bool {
    SESSION.with(|s| s.borrow().is_some())
}

/// Closes the overlay without capturing (the capture key pressed again).
pub fn cancel() {
    with_session(|_| Some(Outcome::Cancel));
}

/// macOS keeps ordinary windows below the menu bar, so a window the size of the display ends
/// up shifted down and squeezed (the frozen menu bar showed twice — owner's live tests 28.09;
/// "simple fullscreen" did not help: it hides the menu bar only while Znimok is the active app,
/// and a global hotkey leaves another app active). Like the system screenshot tool, the overlay
/// goes above the menu bar and the Dock (screen-saver window level), on every Space and over
/// full-screen apps, with its frame set to the whole screen.
#[cfg(target_os = "macos")]
fn ns_window(ui: &Overlay) -> Option<objc2::rc::Retained<objc2_app_kit::NSWindow>> {
    use slint::winit_030::WinitWindowAccessor;
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    ui.window()
        .with_winit_window(|w| {
            let handle = w.window_handle().ok()?;
            let RawWindowHandle::AppKit(a) = handle.as_raw() else {
                return None;
            };
            // SAFETY: winit hands out the NSView of a live window; we are on the main thread.
            let view: &objc2_app_kit::NSView = unsafe { a.ns_view.cast().as_ref() };
            view.window()
        })
        .flatten()
}

/// AppKit's `-[NSWindow constrainFrameRect:toScreen:]` keeps a window's top edge below the menu
/// bar on every `setFrame:` / `setFrameOrigin:` — whatever the level or style mask (Mac
/// self-tests 28.09: level 1000, borderless, still 1800×1098 on a 1800×1169 screen). winit's
/// `WinitWindow` does not override it, so the override is added to that class at run time:
/// windows at the screen-saver level (our overlay) get the requested rect unchanged, every
/// other window (the editor) goes through `NSWindow`'s implementation as before.
///
/// The class of the *object* is left alone: winit observes the window through KVO, so its
/// isa is `NSKVONotifying_WinitWindow`, and swapping that for a subclass broke KVO's
/// bookkeeping — the next `setStyleMask` delivered a change without the old value and winit's
/// observer panicked inside an `extern "C"` frame (the crash of 28.09).
#[cfg(target_os = "macos")]
fn unconstrain() {
    use objc2::encode::Encode;
    use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
    use objc2::{ClassType, msg_send, sel};
    use objc2_app_kit::{NSScreenSaverWindowLevel, NSWindow};
    use objc2_foundation::NSRect;
    use std::sync::Once;

    unsafe extern "C-unwind" fn constrain(
        this: *const AnyObject,
        _sel: Sel,
        rect: NSRect,
        screen: *const AnyObject,
    ) -> NSRect {
        // SAFETY: AppKit calls this on a live WinitWindow (an NSWindow).
        let win: &NSWindow = unsafe { &*this.cast() };
        if win.level() >= NSScreenSaverWindowLevel {
            return rect;
        }
        // SAFETY: NSWindow is WinitWindow's superclass; the arguments are AppKit's own.
        unsafe {
            msg_send![super(win, NSWindow::class()), constrainFrameRect: rect, toScreen: screen]
        }
    }

    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let Some(cls) = AnyClass::get(c"WinitWindow") else {
            trace("WinitWindow class not found");
            return;
        };
        let types = format!("{}@:{}@", NSRect::ENCODING, NSRect::ENCODING);
        let types = std::ffi::CString::new(types).expect("encoding");
        // SAFETY: the implementation matches the selector's signature and encoding.
        let ok = unsafe {
            objc2::ffi::class_addMethod(
                (cls as *const AnyClass).cast_mut(),
                sel!(constrainFrameRect:toScreen:),
                std::mem::transmute::<
                    unsafe extern "C-unwind" fn(
                        *const AnyObject,
                        Sel,
                        NSRect,
                        *const AnyObject,
                    ) -> NSRect,
                    Imp,
                >(constrain),
                types.as_ptr(),
            )
        };
        trace(&format!(
            "constrainFrameRect override added: {}",
            ok.as_bool()
        ));
    });
}

/// The same frame override for the editor window covering a display (ZK-58).
#[cfg(target_os = "macos")]
pub(crate) fn unconstrain_windows() {
    guarded("unconstrain", unconstrain);
}

#[cfg(target_os = "macos")]
thread_local! {
    /// What the last `cover_display` asked for and got right away — for the self-test report.
    static COVER_LOG: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// Step trace of the macOS window set-up: stderr (the self-test's console.txt) and the report.
#[cfg(target_os = "macos")]
fn trace(step: &str) {
    eprintln!("[overlay] {step}");
    COVER_LOG.with(|l| {
        let mut l = l.borrow_mut();
        if !l.is_empty() {
            l.push_str(" · ");
        }
        l.push_str(step);
    });
}

/// Runs an AppKit step, turning an Objective-C exception into a trace line instead of an
/// abort ("panic in a function that cannot unwind", owner's Mac 28.09).
#[cfg(target_os = "macos")]
fn guarded(step: &str, f: impl FnOnce()) {
    match objc2::exception::catch(std::panic::AssertUnwindSafe(f)) {
        Ok(()) => trace(step),
        Err(e) => trace(&format!("{step} threw {e:?}")),
    }
}

fn cover_display(ui: &Overlay) {
    #[cfg(target_os = "macos")]
    if let Some(win) = ns_window(ui) {
        use objc2_app_kit::{
            NSScreenSaverWindowLevel, NSWindowCollectionBehavior, NSWindowStyleMask,
        };
        COVER_LOG.with(|l| l.borrow_mut().clear());
        guarded("unconstrain", unconstrain);
        guarded("style", || {
            if win.styleMask() != NSWindowStyleMask::Borderless {
                win.setStyleMask(NSWindowStyleMask::Borderless);
            }
        });
        guarded("level", || win.setLevel(NSScreenSaverWindowLevel));
        guarded("behavior", || {
            win.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::FullScreenAuxiliary
                    | NSWindowCollectionBehavior::Stationary,
            )
        });
        if let Some(screen) = win.screen() {
            let want = screen.frame();
            guarded("setFrame", || win.setFrame_display(want, true));
            let got = win.frame();
            let vis = screen.visibleFrame();
            let safe = screen.safeAreaInsets();
            let obj: &objc2::runtime::AnyObject = &win;
            trace(&format!(
                "set {:.0},{:.0} {:.0}×{:.0} → got {:.0},{:.0} {:.0}×{:.0} · visible {:.0},{:.0} {:.0}×{:.0} · safe top {:.0} · class {}",
                want.origin.x,
                want.origin.y,
                want.size.width,
                want.size.height,
                got.origin.x,
                got.origin.y,
                got.size.width,
                got.size.height,
                vis.origin.x,
                vis.origin.y,
                vis.size.width,
                vis.size.height,
                safe.top,
                obj.class().name().to_string_lossy()
            ));
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = ui;
}

/// For the self-test: does the overlay window cover its whole screen, menu bar included?
/// `None` where the question does not apply (Windows places the window by pixels).
pub fn covers_screen() -> Option<(bool, String)> {
    #[cfg(target_os = "macos")]
    {
        let ui = handle()?;
        let win = ns_window(&ui)?;
        let screen = win.screen()?;
        let (f, s) = (win.frame(), screen.frame());
        let same = (f.origin.x - s.origin.x).abs() < 1.0
            && (f.origin.y - s.origin.y).abs() < 1.0
            && (f.size.width - s.size.width).abs() < 1.0
            && (f.size.height - s.size.height).abs() < 1.0;
        Some((
            same,
            format!(
                "window {:.0},{:.0} {:.0}×{:.0} · screen {:.0},{:.0} {:.0}×{:.0} · level {} · style {:?}",
                f.origin.x,
                f.origin.y,
                f.size.width,
                f.size.height,
                s.origin.x,
                s.origin.y,
                s.size.width,
                s.size.height,
                win.level(),
                win.styleMask()
            ) + " · "
                + &COVER_LOG.with(|l| l.borrow().clone()),
        ))
    }
    #[cfg(not(target_os = "macos"))]
    None
}

/// Shows the overlay over the frozen screen: a window per display. Runs on the UI thread.
pub fn open(frozen: Frozen, editor_was_visible: bool) -> Result<(), slint::PlatformError> {
    let mut parts = Vec::new();
    for (i, d) in frozen.parts().into_iter().enumerate() {
        let ui = Overlay::new()?;
        let piece = frozen
            .crop(d.rect)
            .unwrap_or_else(|| znimok_core::Raster::new(1, 1, vec![0, 0, 0, 255]));
        let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
            &piece.rgba,
            piece.width,
            piece.height,
        );
        ui.set_shot(slint::Image::from_rgba8(buf));
        let b = d.bounds;
        // Desktop units are physical pixels on Windows (per-monitor DPI aware) and points on
        // macOS.
        if cfg!(target_os = "macos") {
            ui.window()
                .set_position(slint::LogicalPosition::new(b.x as f32, b.y as f32));
            ui.window()
                .set_size(slint::LogicalSize::new(b.width as f32, b.height as f32));
        } else {
            ui.window()
                .set_position(slint::PhysicalPosition::new(b.x, b.y));
            ui.window()
                .set_size(slint::PhysicalSize::new(b.width, b.height));
        }
        ui.set_mac(cfg!(target_os = "macos"));
        // Switching to another program (Cmd+Tab, Alt+Tab) cancels — once the overlay has had
        // the focus (it may never get it when a global hotkey leaves another program in front).
        // Focus moving between our own windows (another display) is not a switch.
        {
            use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
            let had = std::rc::Rc::new(std::cell::Cell::new(false));
            let selftest = std::env::var_os("ZNIMOK_SELFTEST").is_some();
            ui.window().on_winit_window_event(move |_, ev| {
                if let winit::event::WindowEvent::Focused(f) = ev {
                    if *f {
                        had.set(true);
                    } else if had.get() && !selftest {
                        slint::Timer::single_shot(std::time::Duration::from_millis(60), || {
                            if !overlay_has_focus() {
                                cancel();
                            }
                        });
                    }
                }
                EventResult::Propagate
            });
        }
        ui.on_pointer(move |kind, x, y, shift, alt| {
            with_session(|s| {
                s.active = i;
                s.pointer(
                    kind,
                    x,
                    y,
                    Gesture {
                        shift,
                        alt,
                        delayed: false,
                    },
                )
            })
        });
        ui.on_key(|text, shift, alt| {
            with_session(|s| {
                s.key(
                    &text,
                    Gesture {
                        shift,
                        alt,
                        delayed: false,
                    },
                )
            })
        });
        ui.on_wheel(move |dy| {
            with_session(|s| {
                s.active = i;
                // A trackpad sends a stream of small deltas (and keeps going with momentum): add
                // them up, step once past a threshold, then ignore the rest of the gesture for a
                // moment — one flick = one zoom level (owner, MacBook, 28.09).
                let now = std::time::Instant::now();
                if s.wheel_pause.is_some_and(|t| now < t) {
                    return None;
                }
                if s.wheel_acc.signum() != dy.signum() {
                    s.wheel_acc = 0.0;
                }
                s.wheel_acc += dy;
                if s.wheel_acc.abs() < 24.0 {
                    return None;
                }
                let dy = s.wheel_acc;
                s.wheel_acc = 0.0;
                s.wheel_pause = Some(now + std::time::Duration::from_millis(260));
                let before = s.zoom;
                s.zoom = if dy > 0.0 {
                    match s.zoom {
                        0 => 4,
                        z => (z * 2).min(16),
                    }
                } else if s.zoom <= 4 {
                    0
                } else {
                    s.zoom / 2
                };
                let (x, y) = s.last_pointer;
                s.update_lens(x, y);
                if s.zoom != 0 && s.zoom != before {
                    // A short "breath" of the lens on each level change.
                    s.ui().set_lens_pulse(true);
                    let weak = s.ui().as_weak();
                    slint::Timer::single_shot(std::time::Duration::from_millis(90), move || {
                        if let Some(ui) = weak.upgrade() {
                            ui.set_lens_pulse(false);
                        }
                    });
                }
                None
            })
        });
        if cfg!(target_os = "macos") {
            ui.set_on_top(false);
        }
        parts.push(Part {
            ui,
            rect: d.rect,
            kx: 1.0,
            ky: 1.0,
        });
    }
    for p in &parts {
        p.ui.show()?;
        cover_display(&p.ui);
        // Slint and winit apply window properties (size, level) after `show` returns, which
        // undid the level and frame set above (Mac self-test 28.09: level 3, frame below the
        // menu bar). Set them again once the window has settled.
        for ms in [30u64, 150, 400] {
            let weak = p.ui.as_weak();
            slint::Timer::single_shot(std::time::Duration::from_millis(ms), move || {
                if let Some(ui) = weak.upgrade() {
                    cover_display(&ui);
                }
            });
        }
    }
    if let Some(p) = parts.first() {
        use slint::winit_030::WinitWindowAccessor;
        p.ui.window().with_winit_window(|w| w.focus_window());
        p.ui.invoke_grab_focus();
    }
    let session = Session {
        parts,
        active: 0,
        frozen,
        drag_start: None,
        dragging: false,
        sel: None,
        window: None,
        zoom: 0,
        wheel_acc: 0.0,
        wheel_pause: None,
        last_pointer: (-100.0, -100.0),
        editor_was_visible,
        pending: None,
        second: false,
    };
    SESSION.with(|s| *s.borrow_mut() = Some(session));
    crate::hotkeys::grab_escape(true);
    Ok(())
}

/// One of the overlay's windows has the keyboard focus.
fn overlay_has_focus() -> bool {
    SESSION.with(|s| {
        s.borrow().as_ref().is_some_and(|s| {
            use slint::winit_030::WinitWindowAccessor;
            s.parts.iter().any(|p| {
                p.ui.window()
                    .with_winit_window(|w| w.has_focus())
                    .unwrap_or(false)
            })
        })
    })
}

/// Modifiers at the moment the choice is made: Shift = to the clipboard and the library,
/// Alt (⌥) = edit over the screen.
#[derive(Clone, Copy)]
struct Gesture {
    shift: bool,
    alt: bool,
    /// A second click: take the shot after the countdown.
    delayed: bool,
}

enum Outcome {
    /// Rectangle in frame pixels, source label, window id (for an unoccluded capture), gesture.
    Keep(PxRect, &'static str, Option<u64>, Gesture),
    /// Q: read QR codes and barcodes in this part of the frame (ZK-119).
    Codes(PxRect),
    /// S: a scrolling capture of this part of the display (ZK-141).
    Scroll(PxRect),
    Cancel,
}

/// Runs `f` on the open session; finishes the capture when it returns an outcome.
fn with_session(f: impl FnOnce(&mut Session) -> Option<Outcome>) {
    let outcome = SESSION.with(|s| s.borrow_mut().as_mut().and_then(f));
    let Some(outcome) = outcome else { return };
    let Some(session) = SESSION.with(|s| s.borrow_mut().take()) else {
        return;
    };
    crate::hotkeys::grab_escape(false);
    for p in &session.parts {
        let _ = p.ui.hide();
    }
    let Session {
        frozen,
        editor_was_visible,
        ..
    } = session;
    let display = frozen.bounds;
    match outcome {
        // ZK-128: the countdown (in the corner of the display with the choice), then a fresh
        // frame of the same displays, cut the same way.
        Outcome::Keep(rect, source, id, g) if g.delayed => {
            let corner = frozen.part_at(rect).bounds;
            drop(frozen);
            countdown(corner, editor_was_visible, move || {
                std::thread::spawn(move || {
                    let fresh = crate::capture::freeze_display(Some(display));
                    let _ = slint::invoke_from_event_loop(move || match fresh {
                        Ok(f) => finish(
                            f,
                            Outcome::Keep(
                                rect,
                                source,
                                id,
                                Gesture {
                                    delayed: false,
                                    ..g
                                },
                            ),
                            editor_was_visible,
                        ),
                        Err(_) => {
                            if editor_was_visible {
                                crate::with_ctx(|_, ui| crate::show_window(ui));
                            }
                        }
                    });
                });
            });
        }
        other => finish(frozen, other, editor_was_visible),
    }
}

/// What the choice becomes: the editor, the clipboard, over the screen — or nothing.
fn finish(frozen: Frozen, outcome: Outcome, editor_was_visible: bool) {
    let display = frozen.bounds;
    // The card after the capture goes to the display with the choice (ZK-139).
    let card = |r: PxRect| frozen.part_at(r).bounds;
    match outcome {
        Outcome::Codes(rect) => {
            if let Some(r) = frozen.crop(rect) {
                crate::codes::read_and_show(r);
            }
        }
        Outcome::Scroll(rect) => {
            let size = (frozen.raster.width, frozen.raster.height);
            drop(frozen);
            // The overlay is gone from the screen before the first frame.
            slint::Timer::single_shot(std::time::Duration::from_millis(120), move || {
                crate::scroll::start(display, size, rect, editor_was_visible);
            });
        }
        Outcome::Cancel => {
            // (`invoke_from_event_loop` wakes the loop; a zero timer waits for the next event.)
            let _ = slint::invoke_from_event_loop(move || {
                crate::with_ctx(|_, ui| {
                    if editor_was_visible {
                        crate::show_window(ui);
                    }
                })
            });
        }
        // Over the screen (ZK-58): the whole frozen display is the document, the choice its
        // frame — a window too (what overlaps it stays visible around the frame anyway).
        // With several displays: the one with the middle of the choice (the editor window covers
        // one display; the frame is cut to it).
        Outcome::Keep(rect, source, _, g) if g.alt => {
            let d = frozen.part_at(rect);
            let (raster, frame, display) = if frozen.displays.len() > 1 {
                let x0 = rect.x.max(d.rect.x);
                let y0 = rect.y.max(d.rect.y);
                let x1 = (rect.x + rect.w).min(d.rect.x + d.rect.w);
                let y1 = (rect.y + rect.h).min(d.rect.y + d.rect.h);
                (
                    frozen.crop(d.rect).unwrap_or_else(|| frozen.raster.clone()),
                    znimok_core::IRect::new(
                        x0 - d.rect.x,
                        y0 - d.rect.y,
                        (x1 - x0).max(1),
                        (y1 - y0).max(1),
                    ),
                    d.bounds,
                )
            } else {
                (
                    frozen.raster,
                    znimok_core::IRect::new(rect.x, rect.y, rect.w, rect.h),
                    display,
                )
            };
            let _ = slint::invoke_from_event_loop(move || {
                crate::with_ctx(|a, ui| {
                    a.over_open(ui, raster, frame, source, display, editor_was_visible);
                    crate::show_window(ui);
                })
            });
        }
        Outcome::Keep(rect, source, Some(id), Gesture { shift, .. }) => {
            // The window alone: capture it now that the overlay is gone; on failure keep the
            // cut from the frozen frame (it may include what overlapped the window).
            let fallback = frozen.crop(rect);
            let display = card(rect);
            std::thread::spawn(move || {
                // Give the compositor a moment to take the overlay off the screen.
                std::thread::sleep(std::time::Duration::from_millis(80));
                let raster = crate::capture::capture_window(id).ok().or(fallback);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(r) = raster {
                        deliver(r, source, shift, editor_was_visible, display);
                    }
                });
            });
        }
        Outcome::Keep(rect, source, None, Gesture { shift, .. }) => {
            let raster = frozen.crop(rect);
            let display = card(rect);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(r) = raster {
                    deliver(r, source, shift, editor_was_visible, display);
                }
            });
        }
    }
}

/// The picked pixels go to the editor, or with Shift to the clipboard and the library.
fn deliver(
    raster: znimok_core::Raster,
    source: &str,
    to_clipboard: bool,
    editor_was_visible: bool,
    display: znimok_platform::Rect,
) {
    crate::with_ctx(|a, ui| {
        if to_clipboard {
            let (w, h) = (raster.width, raster.height);
            let copied = io::copy_image(w, h, raster.rgba.clone());
            let saved = a.store_quietly(ui, raster.clone(), source);
            if editor_was_visible {
                crate::show_window(ui);
            }
            // ZK-41: the card in the corner of this display says where the shot went.
            match (copied, saved) {
                (copied, Ok((path, name))) => {
                    let mut args = znimok_i18n::FluentArgs::new();
                    args.set("width", w);
                    args.set("height", h);
                    let heading = if copied.is_err() {
                        a.tr.tr("pill-saved")
                    } else {
                        a.tr.tr(match source {
                            "window" => "pill-window-copied",
                            "screen" => "pill-screen-copied",
                            _ => "pill-region-copied",
                        })
                    };
                    let sub = a.tr.tr_args("pill-where", &args);
                    crate::pill::show(raster, path, name, heading, sub, display);
                }
                (_, Err(e)) => a.toast(ui, e),
            }
        } else {
            crate::show_window(ui);
            a.new_document(ui, raster, source, None);
        }
    });
}

/// Frame pixels across the magnifier: odd, so the pointer's pixel sits in the middle
/// (Little Helpers `RgnLensSide`: 144 points of lens, at least 5 pixels).
fn lens_pixels(zoom: u32, scale: f32) -> i32 {
    let mut n = (144.0 * scale) as i32 / zoom.max(1) as i32;
    if n % 2 == 0 {
        n -= 1;
    }
    n.max(5)
}

/// The magnifier picture: `n × n` frame pixels around (cx, cy), each `z × z` screen pixels,
/// a faint grid from ×8, the centre pixel boxed white-on-black (visible on any background).
fn render_lens(f: &znimok_core::Raster, cx: i32, cy: i32, n: i32, z: i32) -> (Vec<u8>, u32) {
    let side = (n * z) as usize;
    let mut out = vec![0u8; side * side * 4];
    let (fw, fh) = (f.width as i32, f.height as i32);
    for j in 0..n {
        for i in 0..n {
            let (sx, sy) = (cx - n / 2 + i, cy - n / 2 + j);
            let px = if sx >= 0 && sy >= 0 && sx < fw && sy < fh {
                let o = ((sy * fw + sx) * 4) as usize;
                [f.rgba[o], f.rgba[o + 1], f.rgba[o + 2], 255]
            } else {
                [0, 0, 0, 255]
            };
            for y in 0..z {
                let row = ((j * z + y) as usize * side + (i * z) as usize) * 4;
                for x in 0..z as usize {
                    out[row + x * 4..row + x * 4 + 4].copy_from_slice(&px);
                }
            }
        }
    }
    let mut put = |x: i32, y: i32, c: [u8; 3], a: u8| {
        if x < 0 || y < 0 || x >= side as i32 || y >= side as i32 {
            return;
        }
        let o = (y as usize * side + x as usize) * 4;
        for (k, ck) in c.iter().enumerate() {
            let v = out[o + k] as u32 * (255 - a as u32) + *ck as u32 * a as u32;
            out[o + k] = (v / 255) as u8;
        }
    };
    if z >= 8 {
        for i in 1..n {
            for t in 0..side as i32 {
                put(i * z, t, [0, 0, 0], 55);
                put(t, i * z, [0, 0, 0], 55);
            }
        }
    }
    let c = (n / 2) * z;
    for t in -1..=z {
        for (x, y) in [
            (c + t, c - 1),
            (c + t, c + z),
            (c - 1, c + t),
            (c + z, c + t),
        ] {
            put(x, y, [0, 0, 0], 255);
        }
    }
    for t in 0..z {
        for (x, y) in [
            (c + t, c),
            (c + t, c + z - 1),
            (c, c + t),
            (c + z - 1, c + t),
        ] {
            put(x, y, [255, 255, 255], 255);
        }
    }
    let s = side as i32;
    for t in 0..s {
        for (x, y) in [(t, 0), (t, s - 1), (0, t), (s - 1, t)] {
            put(x, y, [255, 255, 255], 235);
        }
    }
    (out, side as u32)
}

impl Session {
    fn ui(&self) -> &Overlay {
        &self.parts[self.active].ui
    }

    fn part(&self) -> &Part {
        &self.parts[self.active]
    }

    fn update_k(&mut self) {
        for p in &mut self.parts {
            let size = p.ui.window().size();
            let sf = p.ui.window().scale_factor().max(0.1);
            let (lw, lh) = (size.width as f32 / sf, size.height as f32 / sf);
            if lw > 1.0 && lh > 1.0 {
                p.kx = p.rect.w as f32 / lw;
                p.ky = p.rect.h as f32 / lh;
            }
        }
    }

    /// A logical point of the active window → frame pixels (outside the window too, while a
    /// drag started in it goes on over another display).
    fn px(&self, x: f32, y: f32) -> (i32, i32) {
        let p = self.part();
        (
            p.rect.x + (x * p.kx).floor() as i32,
            p.rect.y + (y * p.ky).floor() as i32,
        )
    }

    fn window_at(&self, x: i32, y: i32) -> Option<(PxRect, u64)> {
        self.frozen
            .windows
            .iter()
            .find(|w| w.rect.contains(x, y))
            .map(|w| (w.rect, w.id))
    }

    /// Colour shown on screen at a frame pixel: the frozen frame, darkened by the veil outside
    /// the selection (veil = black at 0x73 / 255, see `app.slint`).
    fn shown(&self, x: i32, y: i32) -> [u8; 3] {
        let f = &self.frozen.raster;
        let o = ((y * f.width as i32 + x) * 4) as usize;
        let c = [f.rgba[o], f.rgba[o + 1], f.rgba[o + 2]];
        let lit = self.sel.is_some_and(|r| r.contains(x, y));
        if lit {
            c
        } else {
            c.map(|v| ((v as u32 * (255 - 0x73)) / 255) as u8)
        }
    }

    /// A guide line across the active window: black over light pixels, white over dark ones
    /// (owner, 28.09).
    fn guide(&self, horizontal: bool, at: i32) -> slint::Image {
        let r = self.part().rect;
        let (fw, fh) = (
            self.frozen.raster.width as i32,
            self.frozen.raster.height as i32,
        );
        let len = if horizontal { r.w } else { r.h };
        let mut rgba = Vec::with_capacity(len.max(0) as usize * 4);
        for t in 0..len {
            let (x, y) = if horizontal {
                ((r.x + t).clamp(0, fw - 1), at.clamp(0, fh - 1))
            } else {
                (at.clamp(0, fw - 1), (r.y + t).clamp(0, fh - 1))
            };
            let [r, g, b] = self.shown(x, y);
            // Relative luminance (sRGB weights) against the middle grey.
            let lum = 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32;
            let v = if lum > 128.0 { 0 } else { 255 };
            rgba.extend_from_slice(&[v, v, v, 230]);
        }
        let (w, h) = if horizontal {
            (len as u32, 1)
        } else {
            (1, len as u32)
        };
        let buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&rgba, w, h);
        slint::Image::from_rgba8(buf)
    }

    fn update_lens(&mut self, x: f32, y: f32) {
        // The guides and the lens belong to the window the pointer is in.
        for (i, p) in self.parts.iter().enumerate() {
            if i != self.active {
                p.ui.set_pointer_x(-100.0);
                p.ui.set_pointer_y(-100.0);
                p.ui.set_lens_visible(false);
            }
        }
        let kx = self.part().kx;
        let ui = self.ui();
        ui.set_pointer_x(x);
        ui.set_pointer_y(y);
        let (gx, gy) = self.px(x, y);
        ui.set_guide_h(self.guide(true, gy));
        ui.set_guide_v(self.guide(false, gx));
        if self.zoom == 0 {
            ui.set_lens_visible(false);
            return;
        }
        let (fw, fh) = (
            self.frozen.raster.width as i32,
            self.frozen.raster.height as i32,
        );
        let (px, py) = self.px(x, y);
        let (px, py) = (px.clamp(0, fw - 1), py.clamp(0, fh - 1));
        // One frame pixel = `zoom` screen (= frame) pixels; the lens is sized in frame pixels.
        let n = lens_pixels(self.zoom, kx);
        let (rgba, side) = render_lens(&self.frozen.raster, px, py, n, self.zoom as i32);
        let buf =
            slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&rgba, side, side);
        ui.set_lens(slint::Image::from_rgba8(buf));
        ui.set_lens_size(side as f32 / kx.max(0.01));
        let coords = match self.sel.filter(|_| self.dragging) {
            Some(r) => format!("{px}, {py}   {} × {}", r.w, r.h),
            None => format!("{px}, {py}   ×{}", self.zoom),
        };
        ui.set_lens_coords(coords.into());
        let o = ((py * fw + px) * 4) as usize;
        let p = &self.frozen.raster.rgba[o..o + 3];
        ui.set_lens_hex(format!("#{:02X}{:02X}{:02X}", p[0], p[1], p[2]).into());
        ui.set_lens_color(slint::Color::from_rgb_u8(p[0], p[1], p[2]));
        ui.set_lens_visible(true);
    }

    fn show(&self) {
        for p in &self.parts {
            self.show_in(p);
        }
    }

    /// The selection as this window sees it: its part, in its own logical pixels (edges past
    /// the window are simply not drawn).
    fn show_in(&self, p: &Part) {
        let ui = &p.ui;
        let meets = |r: &PxRect| {
            r.x < p.rect.x + p.rect.w
                && r.x + r.w > p.rect.x
                && r.y < p.rect.y + p.rect.h
                && r.y + r.h > p.rect.y
        };
        match self.sel.filter(meets) {
            Some(r) => {
                ui.set_has_sel(true);
                ui.set_sel_x((r.x - p.rect.x) as f32 / p.kx.max(0.01));
                ui.set_sel_y((r.y - p.rect.y) as f32 / p.ky.max(0.01));
                ui.set_sel_w(r.w as f32 / p.kx.max(0.01));
                ui.set_sel_h(r.h as f32 / p.ky.max(0.01));
                // A window shows its title too (shortened), a region only its size.
                let title = self
                    .window
                    .and_then(|id| self.frozen.windows.iter().find(|w| w.id == id))
                    .map(|w| w.title.trim())
                    .filter(|t| !t.is_empty())
                    .map(|t| {
                        let short: String = t.chars().take(48).collect();
                        if short.len() < t.len() {
                            format!("{short}… · ")
                        } else {
                            format!("{short} · ")
                        }
                    })
                    .unwrap_or_default();
                ui.set_sel_label(format!("{title}{} × {}", r.w, r.h).into());
                ui.set_is_window(self.window.is_some());
            }
            None => ui.set_has_sel(false),
        }
    }

    fn pointer(&mut self, kind: i32, x: f32, y: f32, g: Gesture) -> Option<Outcome> {
        self.update_k();
        self.last_pointer = (x, y);
        let (px, py) = self.px(x, y);
        match kind {
            // left down
            0 => {
                if self.pending.is_some() {
                    self.second = true;
                }
                self.drag_start = Some((x, y));
                self.dragging = false;
                None
            }
            // move
            1 => {
                match self.drag_start {
                    Some((sx, sy)) => {
                        if !self.dragging && ((x - sx).abs() > 4.0 || (y - sy).abs() > 4.0) {
                            self.dragging = true;
                        }
                        if self.dragging {
                            let (ax, ay) = self.px(sx, sy);
                            self.sel = Some(PxRect {
                                x: ax.min(px),
                                y: ay.min(py),
                                w: (px - ax).abs().max(1),
                                h: (py - ay).abs().max(1),
                            });
                            self.window = None;
                        }
                    }
                    None => {
                        let hit = self.window_at(px, py);
                        self.sel = hit.map(|(r, _)| r);
                        self.window = hit.map(|(_, id)| id);
                    }
                }
                self.show();
                self.update_lens(x, y);
                None
            }
            // left up
            2 => {
                let was_drag = self.dragging && self.drag_start.is_some();
                self.drag_start = None;
                self.dragging = false;
                let second = std::mem::take(&mut self.second);
                if was_drag {
                    // A click and then at once a drag: the region, after the countdown.
                    self.pending = None;
                    let r = self.sel?;
                    let g = Gesture {
                        delayed: second,
                        ..g
                    };
                    return (r.w >= 3 && r.h >= 3).then_some(Outcome::Keep(r, "region", None, g));
                }
                if second && let Some((x0, y0, g0)) = self.pending.take() {
                    // A double click: what the first click chose, after the countdown.
                    return Some(self.click_outcome(
                        x0,
                        y0,
                        Gesture {
                            delayed: true,
                            ..g0
                        },
                    ));
                }
                // A single click is kept for a moment: a second one may follow.
                self.pending = Some((x, y, g));
                slint::Timer::single_shot(DOUBLE_CLICK, || {
                    with_session(|s| {
                        if s.second {
                            return None;
                        }
                        let (x, y, g) = s.pending.take()?;
                        Some(s.click_outcome(x, y, g))
                    })
                });
                let _ = (px, py);
                None
            }
            // right button: cancel
            3 | 4 => Some(Outcome::Cancel),
            _ => None,
        }
    }

    /// A click at (x, y): the window under it, or the whole screen over the desktop.
    fn click_outcome(&mut self, x: f32, y: f32, g: Gesture) -> Outcome {
        let (px, py) = self.px(x, y);
        match self.window_at(px, py) {
            Some((r, id)) => Outcome::Keep(r, "window", Some(id), g),
            None => Outcome::Keep(self.frozen.whole(), "screen", None, g),
        }
    }

    fn key(&mut self, text: &str, g: Gesture) -> Option<Outcome> {
        match text {
            "\u{1b}" => Some(Outcome::Cancel),
            // Q (Й in the Ukrainian layout): codes in the highlighted part, or on the whole screen.
            "q" | "Q" | "й" | "Й" => Some(Outcome::Codes(
                self.sel.unwrap_or_else(|| self.frozen.whole()),
            )),
            // S (І in the Ukrainian layout): the highlighted window or region, with scrolling.
            "s" | "S" | "і" | "І" => Some(Outcome::Scroll(
                self.sel.unwrap_or_else(|| self.frozen.whole()),
            )),
            " " => Some(Outcome::Keep(self.frozen.whole(), "screen", None, g)),
            "\n" | "\r" => match self.sel {
                Some(r) => Some(match self.window {
                    Some(id) => Outcome::Keep(r, "window", Some(id), g),
                    None => Outcome::Keep(r, "region", None, g),
                }),
                None => Some(Outcome::Keep(self.frozen.whole(), "screen", None, g)),
            },
            _ => None,
        }
    }
}
