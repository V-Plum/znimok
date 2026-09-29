//! Znimok — first working prototype of the application (ZK-46 editor shell, ZK-47 canvas,
//! ZK-59 open / paste / drop). Library as the home screen, the editor on the shared core, the
//! renderer and the `.znimok` format. Runs on Windows and macOS; screen capture is Windows-only
//! until the macOS platform layer lands (ZK-37).
//!
//! `znimok-app [FILE…]` opens files (`.znimok` in place, pictures as new library documents).
//! `ZNIMOK_LIBRARY` overrides the library folder.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod capture;
mod crash;
mod dialog;
#[cfg(target_os = "macos")]
mod dnd_mac;
#[cfg(windows)]
mod dnd_win;
mod filemeta;
mod frame;
mod hotkeys;
mod io;
mod library;
mod over;
mod overlay;
mod pill;
mod selftest;
mod system;
mod tray;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use slint::ComponentHandle;
use slint::wgpu_30::wgpu;

use app::{App, Gpu, KeyAction};

slint::include_modules!();

type Shared = Rc<RefCell<App>>;

thread_local! {
    /// For results that come back from worker threads through `invoke_from_event_loop`.
    static CTX: RefCell<Option<(Shared, slint::Weak<AppWindow>)>> = const { RefCell::new(None) };
}

fn with_ctx(f: impl FnOnce(&mut App, &AppWindow)) {
    CTX.with(|c| {
        if let Some((app, ui)) = c.borrow().as_ref()
            && let Some(ui) = ui.upgrade()
        {
            f(&mut app.borrow_mut(), &ui);
        }
    });
}

/// Wires a callback that needs the app and the window; the closure gets both borrowed.
macro_rules! on {
    ($ui:ident, $app:ident, $setter:ident, |$a:ident, $w:ident $(, $arg:ident)*| $body:block) => {{
        let app = $app.clone();
        let weak = $ui.as_weak();
        $ui.$setter(move |$($arg),*| {
            let Some($w) = weak.upgrade() else { return Default::default() };
            let mut guard = app.borrow_mut();
            let $a: &mut App = &mut guard;
            $body
        });
    }};
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Log file and crash reports (ZK-32); keep the guard until the end of main to flush the log.
    let _log = znimok_log::init(znimok_log::Config::for_app(
        "znimok-app",
        env!("CARGO_PKG_VERSION"),
    ));
    #[cfg(windows)]
    znimok_win::init_process();

    // Settings first (ZK-56): the language and the library folder come from them. The self-test
    // keeps its own file next to its report.
    let selftest_dir = std::env::var_os("ZNIMOK_SELFTEST").map(PathBuf::from);
    let store = match &selftest_dir {
        Some(d) => Some(znimok_settings::Store::open(d.join("settings.json"))),
        None => znimok_settings::Store::open_default(),
    };
    let prefs = store.as_ref().map(|s| s.get()).unwrap_or_default();
    let lang = znimok_i18n::choose_language(
        prefs.general.language.as_deref(),
        znimok_i18n::system_language().as_deref(),
    );
    let tr = znimok_i18n::Localizer::new(lang);
    let lib_dir = app::library_dir(&prefs);

    // Backend pinned per OS: letting wgpu probe every backend crashed natively on a machine
    // with Intel UHD 630 under RDP (ZK-14). WGPU_BACKEND still overrides.
    let mut settings = slint::wgpu_30::WGPUSettings::default();
    if wgpu::Backends::from_env().is_none() {
        settings.backends = if cfg!(target_os = "macos") {
            wgpu::Backends::METAL
        } else {
            wgpu::Backends::DX12
        };
    }
    slint::BackendSelector::new()
        .require_wgpu_30(slint::wgpu_30::WGPUConfiguration::Automatic(settings))
        // The card after a capture (ZK-41) must not take the focus or show in the taskbar.
        .with_winit_window_attributes_hook(|attrs| {
            if attrs.title != pill::TITLE {
                return attrs;
            }
            let attrs = attrs.with_active(false);
            #[cfg(windows)]
            let attrs = {
                use slint::winit_030::winit::platform::windows::WindowAttributesExtWindows;
                attrs.with_skip_taskbar(true)
            };
            attrs
        })
        .select()?;
    if lang != znimok_i18n::FALLBACK {
        let _ = slint::select_bundled_translation(lang);
    }

    // One Znimok per library: a second start shows the running one's window and exits.
    let instance = if selftest_dir.is_none() {
        match tray::start(&lib_dir) {
            tray::Start::First(i) => Some(i),
            tray::Start::Woke => return Ok(()),
        }
    } else {
        None
    };

    let ui = AppWindow::new()?;
    ui.set_app_icon(tray::icon(64));
    frame::before_show(&ui);
    ui.set_mac(cfg!(target_os = "macos"));
    ui.global::<Keys>().set_mac(cfg!(target_os = "macos"));
    ui.set_capture_available(capture::available());
    ui.set_capture_key("".into());

    let app: Shared = Rc::new(RefCell::new(App::new(tr, lib_dir)));
    app.borrow_mut().use_settings(&ui, store);
    CTX.with(|c| *c.borrow_mut() = Some((app.clone(), ui.as_weak())));
    app.borrow_mut().refresh_library(&ui);

    wire(&ui, &app);
    if selftest_dir.is_none() {
        // Once the loop runs and the window exists: the report question is asked in it.
        let _ = slint::invoke_from_event_loop(|| {
            let ctx = CTX.with(|c| c.borrow().clone());
            if let Some((app, weak)) = ctx
                && let Some(ui) = weak.upgrade()
            {
                crash::offer_last_crash(&app, &ui);
            }
        });
    }

    // Started at login (ZK-77): straight to the tray, no window; "--background" is not a file.
    let background = std::env::args_os().any(|a| a == "--background");
    // Files from the command line (and "Open with…" on Windows).
    let files: Vec<PathBuf> = std::env::args_os()
        .skip(1)
        .filter(|a| a != "--background")
        .map(PathBuf::from)
        .collect();
    // The first-run guide (ZK-57), unless a file was asked for or the start is silent.
    if selftest_dir.is_none()
        && !background
        && files.is_empty()
        && !app.borrow().prefs().general.onboarding_done
    {
        app.borrow_mut().onboarding_open(&ui);
    }
    // Global hotkeys from the settings (ZK-44); the self-test uses them too (recording a key).
    {
        let p = app.borrow().prefs();
        hotkeys::start(&p.capture.hotkeys, p.capture.enabled);
    }
    app.borrow().show_capture_key(&ui);

    // Tray / menu bar icon; while it exists, closing the window keeps the app running.
    let tray_ui = if selftest_dir.is_none() {
        let t = AppTray::new()?;
        t.set_tray_icon(tray::icon(44));
        t.set_capture_key(ui.get_capture_key());
        t.set_capture_available(capture::available());
        t.set_mac(cfg!(target_os = "macos"));
        {
            let app = app.clone();
            let weak = ui.as_weak();
            t.on_shot(move || {
                if let Some(ui) = weak.upgrade() {
                    new_shot(&app, &ui);
                }
            });
        }
        {
            let weak = ui.as_weak();
            t.on_open_window(move || {
                if let Some(ui) = weak.upgrade() {
                    show_window(&ui);
                }
            });
        }
        {
            let app = app.clone();
            let weak = ui.as_weak();
            {
                let weak = ui.as_weak();
                let tw = t.as_weak();
                t.on_toggle_pause(move || {
                    let ctx = CTX.with(|c| c.borrow().clone());
                    if let Some((app, _)) = ctx {
                        let p = app.borrow().prefs();
                        let now = !hotkeys::is_paused();
                        hotkeys::set_paused(now, &p.capture.hotkeys, p.capture.enabled);
                        if let Some(t) = tw.upgrade() {
                            t.set_paused(hotkeys::is_paused());
                        }
                        if let Some(ui) = weak.upgrade() {
                            app.borrow().settings_sync(&ui);
                        }
                    }
                });
            }
            t.on_quit(move || match weak.upgrade() {
                Some(ui) => confirm_leave(&app, &ui, |_, _| {
                    let _ = slint::quit_event_loop();
                }),
                None => {
                    let _ = slint::quit_event_loop();
                }
            });
        }
        t.show()?;
        TRAY.with(|c| c.set(true));
        Some(t)
    } else {
        None
    };

    if let Some(dir) = selftest_dir.clone() {
        selftest::start(app.clone(), &ui, dir, files.first().cloned());
    } else if let Some(f) = files.first() {
        app.borrow_mut().open_path(&ui, f);
    }

    // Autosave, background-save results and toasts.
    let timer = slint::Timer::default();
    {
        let app = app.clone();
        let weak = ui.as_weak();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(400),
            move || {
                let Some(ui) = weak.upgrade() else { return };
                if instance.as_ref().is_some_and(|i| i.take_wake()) {
                    show_window(&ui);
                }
                let mut a = app.borrow_mut();
                a.tick_toast(&ui);
                if let Some((path, doc, opts)) = a.autosave_job(&ui) {
                    std::thread::spawn(move || {
                        if let Some(dir) = path.parent() {
                            let _ = std::fs::create_dir_all(dir);
                        }
                        let r = {
                            let _guard = app::SAVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                            znimok_format::save(&path, &doc, &opts).map_err(|e| e.to_string())
                        };
                        let _ = slint::invoke_from_event_loop(move || {
                            with_ctx(|a, ui| a.save_finished(ui, &path, r));
                        });
                    });
                }
            },
        );
    }

    // Started at login with a tray icon: stay in the tray until called (ZK-77). The window is
    // shown later by the tray, the hotkey or a second start (show_window).
    if !(background && tray_ui.is_some()) {
        ui.show()?;
        frame::after_show(&ui);
    }
    slint::run_event_loop_until_quit()?;
    drop(timer);
    drop(tray_ui);
    Ok(())
}

thread_local! {
    static TRAY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// A hotkey field of the settings is waiting for a combination (ZK-44).
    pub(crate) static REC: std::cell::Cell<Option<hotkeys::Action>> = const { std::cell::Cell::new(None) };
    static MODS: std::cell::Cell<slint::winit_030::winit::keyboard::ModifiersState> =
        const { std::cell::Cell::new(slint::winit_030::winit::keyboard::ModifiersState::empty()) };
}

/// Shows the window and brings it to the front (from the tray, the hotkey, a second start).
fn show_window(ui: &AppWindow) {
    use slint::winit_030::WinitWindowAccessor;
    let first = !ui.window().is_visible();
    let _ = ui.show();
    // The first show after a silent start: our frame (rounded corners, macOS title bar).
    if first {
        frame::after_show(ui);
    }
    ui.window().with_winit_window(|w| {
        w.set_minimized(false);
        w.focus_window();
    });
}

/// A global hotkey was pressed (UI thread).
fn hotkey_pressed(a: hotkeys::Action) {
    let ctx = CTX.with(|c| c.borrow().clone());
    let Some((app, weak)) = ctx else { return };
    let Some(ui) = weak.upgrade() else { return };
    if !app.borrow().prefs().capture.enabled {
        return;
    }
    match a {
        hotkeys::Action::Region => new_shot(&app, &ui),
        hotkeys::Action::Screen => {
            if overlay::is_open() || !capture::available() {
                return;
            }
            confirm_leave(&app, &ui, |app, ui| start_capture(app, ui, true));
        }
        hotkeys::Action::Clipboard => {
            show_window(&ui);
            confirm_leave(&app, &ui, |app, ui| app.borrow_mut().open_clipboard(ui));
        }
        hotkeys::Action::Editor => {
            show_window(&ui);
            confirm_leave(&app, &ui, |app, ui| app.borrow_mut().open_blank(ui));
        }
    }
}

/// Leaving the document (another one, a shot, closing): saves, or — autosave off and changes
/// unsaved — asks in the window first. `then` runs once it is fine to leave.
fn confirm_leave(app: &Shared, ui: &AppWindow, then: impl FnOnce(&Shared, &AppWindow) + 'static) {
    if !must_ask(app) {
        if leave_quietly(app, ui) {
            then(app, ui);
        }
        return;
    }
    if dialog::is_open(ui) {
        return;
    }
    show_window(ui);
    let (title, body, save, dont, cancel) = {
        let a = app.borrow();
        let mut args = znimok_i18n::FluentArgs::new();
        args.set("name", a.doc_name());
        (
            a.tr.tr_args("confirm-save-title", &args),
            a.tr.tr("confirm-save-body"),
            a.tr.tr("common-save"),
            a.tr.tr("common-dont-save"),
            a.tr.tr("common-cancel"),
        )
    };
    let app = app.clone();
    // Order as on both systems: the destructive choice apart on the left, Save on the right.
    dialog::ask(
        ui,
        title,
        body,
        vec![dont, cancel, save],
        2,
        Some(1),
        move |ui, answer| {
            let go = match answer {
                Some(2) => app.borrow_mut().save_now(ui),
                Some(0) => true,
                _ => false,
            };
            if go {
                then(&app, ui);
            }
        },
    );
}

/// Unsaved changes with autosave off: the owner decides.
fn must_ask(app: &Shared) -> bool {
    let a = app.borrow();
    a.is_unsaved() && !a.autosave
}

/// Leaving without a question: a background save may still be running (and may fail), so
/// save once more, synchronously, after it — the save lock orders the two writers.
fn leave_quietly(app: &Shared, ui: &AppWindow) -> bool {
    let (unsaved, saving) = {
        let a = app.borrow();
        (a.is_unsaved(), a.saving)
    };
    if !unsaved && !saving {
        return true;
    }
    app.borrow_mut().save_now(ui)
}

fn pick_open(app: &Shared) -> Option<PathBuf> {
    let dir = app.borrow().lib_dir.clone();
    let mut exts: Vec<&str> = io::IMAGE_EXTENSIONS.to_vec();
    exts.push("znimok");
    rfd::FileDialog::new()
        .add_filter("Znimok, PNG, JPEG, WebP, GIF, BMP", &exts)
        .set_directory(dir)
        .pick_file()
}

fn open_with_dialog(app: &Shared, ui: &AppWindow) {
    confirm_leave(app, ui, |app, ui| {
        if let Some(p) = pick_open(app) {
            app.borrow_mut().open_path(ui, &p);
        }
    });
}

fn export_with_dialog(app: &Shared, ui: &AppWindow) {
    let name = app.borrow().doc_name();
    let file = rfd::FileDialog::new()
        .add_filter("PNG", &["png"])
        .add_filter("JPEG", &["jpg", "jpeg"])
        .add_filter("WebP", &["webp"])
        .set_file_name(format!("{name}.png"))
        .save_file();
    if let Some(p) = file {
        let mut a = app.borrow_mut();
        a.export_to(ui, &p);
        a.set_last_share(ui, true);
    }
}

/// Screenshot of the display under the pointer: the window steps aside, the capture runs on a
/// worker thread (WinRT wants the multithreaded apartment, the UI thread is OLE's STA), then
/// the shot opens in the editor.
fn new_shot(app: &Shared, ui: &AppWindow) {
    // The capture key while the overlay is open cancels it (ZK-40).
    if overlay::is_open() {
        overlay::cancel();
        return;
    }
    if !capture::available() {
        return;
    }
    confirm_leave(app, ui, start_shot);
}

fn start_shot(app: &Shared, ui: &AppWindow) {
    start_capture(app, ui, false);
}

/// Freezes the display under the pointer; `whole` = straight into the editor (the whole-screen
/// hotkey), otherwise the overlay to choose what to keep.
fn start_capture(app: &Shared, ui: &AppWindow, whole: bool) {
    // Already editing over the screen (ZK-58): finish that first.
    if app.try_borrow().map(|a| a.over.is_some()).unwrap_or(true) {
        return;
    }
    // The editor steps aside so the frozen screen does not contain it (on macOS the capture
    // filter would drop it anyway, but the overlay should not sit on top of it either).
    let was_visible = ui.window().is_visible();
    let _ = ui.hide();
    // Windows: give DWM time to take the editor off the screen before the frame is grabbed.
    let delay = Duration::from_millis(if was_visible && cfg!(windows) { 250 } else { 0 });
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        {
            let r = capture::freeze();
            let _ = slint::invoke_from_event_loop(move || {
                with_ctx(|a, ui| match r {
                    Ok(frozen) if whole => {
                        show_window(ui);
                        a.new_document(ui, frozen.raster, "screen", None);
                    }
                    Ok(frozen) => {
                        if let Err(e) = overlay::open(frozen, was_visible) {
                            show_window(ui);
                            a.toast(ui, e.to_string());
                        }
                    }
                    Err(e) => {
                        show_window(ui);
                        let msg = match e {
                            capture::Fail::Permission => a.tr.tr("err-capture-mac-perm"),
                            capture::Fail::Other(e) => {
                                let mut args = znimok_i18n::FluentArgs::new();
                                args.set("reason", e);
                                a.tr.tr_args("err-capture-generic", &args)
                            }
                        };
                        a.toast(ui, msg);
                    }
                });
            });
        }
    });
}

fn wire(ui: &AppWindow, app: &Shared) {
    // --- rendering: the canvas texture is filled right before Slint draws.
    {
        let app = app.clone();
        let weak = ui.as_weak();
        let _ = ui.window().set_rendering_notifier(move |state, api| {
            let Some(ui) = weak.upgrade() else { return };
            let Ok(mut a) = app.try_borrow_mut() else {
                // A modal dialog is running inside a callback that holds the app; draw next time.
                return;
            };
            match state {
                slint::RenderingState::RenderingSetup => {
                    if let slint::GraphicsAPI::WGPU30 { device, queue, .. } = api {
                        a.gpu = Some(Gpu {
                            device: device.clone(),
                            queue: queue.clone(),
                            texture: None,
                        });
                        a.dpr = ui.window().scale_factor() as f64;
                    }
                }
                slint::RenderingState::BeforeRendering => a.before_rendering(&ui),
                slint::RenderingState::RenderingTeardown => a.gpu_lost(),
                _ => {}
            }
        });
    }

    // --- drop files onto the window (any page): open them.
    {
        use slint::winit_030::{EventResult, WinitWindowAccessor, winit};
        ui.window().on_winit_window_event(move |_, ev| {
            if let winit::event::WindowEvent::Resized(_) = ev {
                let ctx = CTX.with(|c| c.borrow().clone());
                if let Some((_, weak)) = ctx
                    && let Some(ui) = weak.upgrade()
                {
                    frame::on_resized(&ui);
                }
            }
            // The system switched light / dark (ZK-46): "as the system" follows at once.
            // (The event's value is the window's own appearance on macOS — ours — so the system
            // is asked directly.)
            if let winit::event::WindowEvent::ThemeChanged(_) = ev {
                let dark = system::system_dark();
                let _ = slint::invoke_from_event_loop(move || {
                    with_ctx(|a, ui| a.system_theme(ui, dark));
                });
            }
            // Settings → hotkeys: the next combination pressed, as physical keys (ZK-44).
            match ev {
                winit::event::WindowEvent::ModifiersChanged(m) => {
                    MODS.with(|c| c.set(m.state()));
                }
                winit::event::WindowEvent::KeyboardInput { event, .. }
                    if REC.with(|r| r.get()).is_some() =>
                {
                    if event.state == winit::event::ElementState::Pressed
                        && let winit::keyboard::PhysicalKey::Code(code) = event.physical_key
                    {
                        let name = format!("{code:?}");
                        let mods = MODS.with(|c| c.get());
                        let _ = slint::invoke_from_event_loop(move || {
                            with_ctx(|a, ui| a.hotkey_key(ui, &name, mods));
                        });
                    }
                    return EventResult::PreventDefault;
                }
                _ => {}
            }
            // macOS trackpad: pinch to zoom, double tap = fit ↔ 100 %.
            match ev {
                winit::event::WindowEvent::PinchGesture { delta, .. } => {
                    let delta = *delta;
                    with_ctx(|a, ui| a.pinch(ui, delta));
                    return EventResult::PreventDefault;
                }
                winit::event::WindowEvent::DoubleTapGesture { .. } => {
                    with_ctx(|a, ui| a.smart_zoom(ui));
                    return EventResult::PreventDefault;
                }
                _ => {}
            }
            if let winit::event::WindowEvent::DroppedFile(path) = ev {
                let path = path.clone();
                // Leave winit's handler first: the confirmation dialog runs a nested loop.
                // (`invoke_from_event_loop` wakes the loop; a zero timer waits for the next event.)
                let _ = slint::invoke_from_event_loop(move || {
                    let ctx = CTX.with(|c| c.borrow().clone());
                    if let Some((app, weak)) = ctx
                        && let Some(ui) = weak.upgrade()
                    {
                        // On an open document a picture becomes a mark; otherwise it opens.
                        if app.borrow_mut().drop_image_mark(&ui, &path) {
                            return;
                        }
                        confirm_leave(&app, &ui, move |app, ui| {
                            app.borrow_mut().open_path(ui, &path);
                        });
                    }
                });
                return EventResult::PreventDefault;
            }
            EventResult::Propagate
        });
    }

    // --- closing the window saves (or asks when autosave is off).
    {
        let app = app.clone();
        let weak = ui.as_weak();
        ui.window().on_close_requested(move || {
            let Some(ui) = weak.upgrade() else {
                return slint::CloseRequestResponse::HideWindow;
            };
            if must_ask(&app) {
                // The question is answered later; the window stays until then.
                confirm_leave(&app, &ui, |_, ui| {
                    let _ = ui.hide();
                    if !TRAY.with(|c| c.get()) {
                        let _ = slint::quit_event_loop();
                    }
                });
                return slint::CloseRequestResponse::KeepWindowShown;
            }
            if !leave_quietly(&app, &ui) {
                return slint::CloseRequestResponse::KeepWindowShown;
            }
            // With a tray icon the app stays (hotkey, "Quit" in the tray menu); without one
            // (self-test) closing the window ends it.
            if !TRAY.with(|c| c.get()) {
                let _ = slint::quit_event_loop();
            }
            slint::CloseRequestResponse::HideWindow
        });
    }

    // --- own title bar
    {
        let weak = ui.as_weak();
        ui.on_window_drag(move || {
            if let Some(ui) = weak.upgrade() {
                frame::drag(&ui);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_window_toggle(move || {
            if let Some(ui) = weak.upgrade() {
                frame::toggle_maximized(&ui);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_window_minimize(move || {
            if let Some(ui) = weak.upgrade() {
                ui.window().set_minimized(true);
            }
        });
    }
    {
        let weak = ui.as_weak();
        ui.on_window_resize(move |dir| {
            if let Some(ui) = weak.upgrade() {
                frame::resize(&ui, dir);
            }
        });
    }
    {
        let app = app.clone();
        let weak = ui.as_weak();
        ui.on_window_close(move || {
            let Some(ui) = weak.upgrade() else { return };
            // Same as the system close button: save or ask, then hide to the tray (or quit).
            confirm_leave(&app, &ui, |_, ui| {
                let _ = ui.hide();
                if !TRAY.with(|c| c.get()) {
                    let _ = slint::quit_event_loop();
                }
            });
        });
    }

    // --- library
    on!(ui, app, on_search, |a, w, text| {
        a.set_filter(&w, &text);
    });
    {
        let app = app.clone();
        let weak = ui.as_weak();
        ui.on_open_card(move |path| {
            let Some(ui) = weak.upgrade() else { return };
            app.borrow_mut()
                .open_path(&ui, std::path::Path::new(path.as_str()));
        });
    }
    {
        let app = app.clone();
        let weak = ui.as_weak();
        ui.on_new_shot(move || {
            if let Some(ui) = weak.upgrade() {
                new_shot(&app, &ui);
            }
        });
    }
    {
        let app = app.clone();
        let weak = ui.as_weak();
        ui.on_open_file(move || {
            if let Some(ui) = weak.upgrade() {
                open_with_dialog(&app, &ui);
            }
        });
    }
    {
        let app = app.clone();
        let weak = ui.as_weak();
        ui.on_open_clipboard(move || {
            let Some(ui) = weak.upgrade() else { return };
            confirm_leave(&app, &ui, |app, ui| app.borrow_mut().open_clipboard(ui));
        });
    }

    // --- editor
    {
        let app = app.clone();
        let weak = ui.as_weak();
        ui.on_back(move || {
            let Some(ui) = weak.upgrade() else { return };
            confirm_leave(&app, &ui, |app, ui| app.borrow_mut().close_document(ui));
        });
    }
    on!(ui, app, on_undo, |a, w| {
        a.undo(&w);
    });
    on!(ui, app, on_redo, |a, w| {
        a.redo(&w);
    });
    on!(ui, app, on_copy, |a, w| {
        a.copy(&w);
    });
    {
        let app = app.clone();
        let weak = ui.as_weak();
        ui.on_export(move || {
            if let Some(ui) = weak.upgrade() {
                export_with_dialog(&app, &ui);
            }
        });
    }
    on!(ui, app, on_autosave_toggled, |a, w, on| {
        a.setting(&w, "autosave", on as i32);
        a.sync(&w);
    });
    on!(ui, app, on_tool_chosen, |a, w, t| {
        a.set_tool(&w, t.max(0) as usize);
        w.invoke_focus_canvas();
    });
    on!(ui, app, on_pointer, |a,
                              w,
                              kind,
                              x,
                              y,
                              button,
                              shift,
                              ctrl| {
        a.pointer(&w, kind, x, y, button, shift, ctrl);
    });
    on!(ui, app, on_wheel, |a, w, x, y, dx, dy, ctrl, alt, shift| {
        a.wheel(&w, x, y, dx, dy, ctrl || alt, shift);
    });
    {
        let app = app.clone();
        let weak = ui.as_weak();
        ui.on_key(move |text, ctrl, shift, _alt| {
            let Some(ui) = weak.upgrade() else { return };
            let action = app.borrow_mut().key(&ui, &text, ctrl, shift);
            let over = app.borrow().over.is_some();
            match action {
                // Over the screen (ZK-58): Enter / Ctrl+C copy and close, Ctrl+S keeps it in
                // the library, the last Esc closes without a trace.
                KeyAction::Copy if over => app.borrow_mut().over_finish(&ui, true, true),
                KeyAction::Save => app.borrow_mut().over_finish(&ui, false, true),
                KeyAction::Back if over => app.borrow_mut().over_finish(&ui, false, false),
                KeyAction::Copy => app.borrow_mut().copy(&ui),
                KeyAction::Export => export_with_dialog(&app, &ui),
                KeyAction::Open => open_with_dialog(&app, &ui),
                KeyAction::Back => {
                    confirm_leave(&app, &ui, |app, ui| app.borrow_mut().close_document(ui))
                }
                KeyAction::None => {}
            }
        });
    }
    // The hidden text input also reports caret moves the app itself made (placing the caret
    // with the mouse, opening a text): those arrive while the app is busy and are skipped.
    {
        let app = app.clone();
        let weak = ui.as_weak();
        ui.on_text_edited(move |cursor, anchor| {
            let Some(w) = weak.upgrade() else { return };
            if let Ok(mut a) = app.try_borrow_mut() {
                a.text_edited(&w, cursor, anchor);
            }
        });
    }
    {
        let app = app.clone();
        let weak = ui.as_weak();
        ui.on_text_cursor(move |cursor, anchor| {
            let Some(w) = weak.upgrade() else { return };
            if let Ok(mut a) = app.try_borrow_mut() {
                a.text_cursor(&w, cursor, anchor);
            }
        });
    }
    on!(ui, app, on_over_to_window, |a, w| {
        a.over_to_window(&w);
    });
    on!(ui, app, on_over_copy, |a, w| {
        a.over_finish(&w, true, true);
    });
    on!(ui, app, on_over_close, |a, w| {
        a.over_finish(&w, false, false);
    });
    on!(ui, app, on_canvas_double, |a, w, x, y| {
        a.canvas_double(&w, x, y);
    });
    on!(ui, app, on_set_text_box, |a, w, text| {
        a.set_text_box(&w, &text);
        w.invoke_focus_canvas();
    });
    on!(ui, app, on_commit_text, |a, w, text| {
        a.commit_text(&w, &text);
    });
    on!(ui, app, on_cancel_text, |a, w| {
        a.cancel_text(&w);
    });
    on!(ui, app, on_arrange, |a, w, to| {
        a.arrange(&w, to);
        w.invoke_focus_canvas();
    });
    on!(ui, app, on_align, |a, w, edge| {
        a.align(&w, edge);
        w.invoke_focus_canvas();
    });
    on!(ui, app, on_distribute, |a, w, axis| {
        a.distribute(&w, axis);
        w.invoke_focus_canvas();
    });
    on!(ui, app, on_group, |a, w, on| {
        a.group(&w, on);
        w.invoke_focus_canvas();
    });
    on!(ui, app, on_duplicate, |a, w| {
        a.duplicate(&w);
        w.invoke_focus_canvas();
    });
    on!(ui, app, on_set_prop, |a, w, name, v| {
        a.set_prop(&w, &name, v);
    });
    on!(ui, app, on_set_text_size, |a, w, text| {
        a.set_text_size(&w, &text);
        w.invoke_focus_canvas();
    });
    on!(ui, app, on_set_alpha, |a, w, v, last| {
        a.set_alpha(&w, v, last);
    });
    on!(ui, app, on_set_geom, |a, w, field, text| {
        a.set_geom(&w, &field, &text);
        w.invoke_focus_canvas();
    });
    on!(ui, app, on_layer_click, |a, w, id, add| {
        a.set_tool(&w, app::tool::SELECT);
        a.layer_click(&w, id, add);
    });
    on!(ui, app, on_layer_eye, |a, w, id| {
        a.layer_eye(&w, id);
    });
    on!(ui, app, on_layer_group_click, |a, w, g, add| {
        a.set_tool(&w, app::tool::SELECT);
        a.layer_group_click(&w, g, add);
    });
    on!(ui, app, on_layer_group_eye, |a, w, g| {
        a.layer_group_eye(&w, g);
    });
    on!(ui, app, on_layer_collapse, |a, w, g| {
        a.layer_collapse(&w, g);
    });
    on!(ui, app, on_layer_drag, |a, w, from, y, phase| {
        a.layer_drag(&w, from, y, phase);
    });
    on!(ui, app, on_meta_edited, |a, w, field, value| {
        a.meta_edited(&w, &field, &value);
    });
    on!(ui, app, on_image_action, |a, w, what| {
        a.image_action(&w, &what);
    });
    on!(ui, app, on_set_tone, |a, w, field, v, last| {
        a.set_tone(&w, &field, v, last);
    });
    on!(ui, app, on_compare, |a, w, on| {
        a.set_compare(&w, on);
    });
    on!(ui, app, on_set_crop_size, |a, w, field, text| {
        a.set_crop_size(&w, &field, &text);
    });
    on!(ui, app, on_size_edited, |a, w, field, text| {
        a.size_edited(&w, &field, &text);
    });
    {
        let weak = ui.as_weak();
        ui.on_dialog_answer(move |i| {
            if let Some(ui) = weak.upgrade() {
                dialog::answered(&ui, i);
            }
        });
    }
    on!(ui, app, on_set_counter_start, |a, w, text| {
        a.set_counter_start(&w, &text);
        w.invoke_focus_canvas();
    });
    on!(ui, app, on_save_as, |a, w| {
        a.save_as(&w);
    });
    on!(ui, app, on_card_trash, |a, w, path| {
        a.lib_trash(&w, std::path::Path::new(path.as_str()));
    });
    on!(ui, app, on_card_rename, |a, w, path, name| {
        a.lib_rename(&w, std::path::Path::new(path.as_str()), &name);
    });
    on!(ui, app, on_card_reveal, |_a, _w, path| {
        library::show_in_folder(std::path::Path::new(path.as_str()));
    });
    on!(ui, app, on_toast_action_clicked, |a, w| {
        a.toast_action(&w);
    });
    on!(ui, app, on_setting, |a, w, key, value| {
        a.setting(&w, &key, value);
    });
    on!(ui, app, on_settings_open, |a, w| {
        a.settings_open(&w);
    });
    on!(ui, app, on_toggle_export_meta, |a, w| {
        a.toggle_export_meta(&w);
    });
    on!(ui, app, on_drag_out, |a, w| {
        a.drag_out(&w);
    });
    on!(ui, app, on_zoom_to, |a, w, pos| {
        a.zoom_to(&w, pos);
    });
    on!(ui, app, on_zoom_fit, |a, w| {
        a.zoom_fit(&w);
    });
    on!(ui, app, on_zoom_100, |a, w| {
        a.zoom_100(&w);
    });
}
