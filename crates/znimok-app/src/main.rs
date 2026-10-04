//! Znimok — first working prototype of the application (ZK-46 editor shell, ZK-47 canvas,
//! ZK-59 open / paste / drop). Library as the home screen, the editor on the shared core, the
//! renderer and the `.znimok` format. Runs on Windows and macOS; screen capture is Windows-only
//! until the macOS platform layer lands (ZK-37).
//!
//! `znimok-app [FILE…]` opens files (`.znimok` in place, pictures as new library documents).
//! `ZNIMOK_LIBRARY` overrides the library folder.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod agentipc;
mod app;
mod capture;
mod codes;
mod commands;
mod crash;
mod devlog;
mod devpanel;
mod dialog;
#[cfg(target_os = "macos")]
mod dnd_mac;
#[cfg(windows)]
mod dnd_win;
mod filedlg;
mod filemeta;
mod frame;
mod hotkeys;
mod integrations;
mod io;
mod library;
mod ocrindex;
mod over;
mod overlay;
mod pill;
mod rec;
mod scroll;
mod selftest;
mod system;
mod text;
mod tray;
#[cfg(test)]
mod ui_tests;
mod update;
mod vexport;
mod video;
mod wins;

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
    /// The library window — the hub (ZK-107): for results that come back from worker threads
    /// through `invoke_from_event_loop` and for everything shared. The editor windows are in
    /// `windows`.
    static CTX: RefCell<Option<(Shared, slint::Weak<AppWindow>)>> = const { RefCell::new(None) };
}

/// The settings, from the library window (worker results and other modules).
// Used by the recording (Windows until ZK-88).
#[cfg_attr(not(windows), allow(dead_code))]
fn with_prefs<T>(f: impl FnOnce(&znimok_settings::Settings) -> T) -> Option<T> {
    let mut out = None;
    CTX.with(|c| {
        if let Some((app, _)) = c.borrow().as_ref()
            && let Ok(a) = app.try_borrow()
        {
            out = Some(f(&a.prefs()));
        }
    });
    out
}

/// The library folder, from the library window.
// Used by the recording (Windows until ZK-88).
#[cfg_attr(not(windows), allow(dead_code))]
fn with_lib_dir() -> Option<PathBuf> {
    let mut out = None;
    CTX.with(|c| {
        if let Some((app, _)) = c.borrow().as_ref()
            && let Ok(a) = app.try_borrow()
        {
            out = Some(a.lib_dir.clone());
        }
    });
    out
}

thread_local! {
    /// The timer that keeps looking for the updater's note after an update (ZK-225).
    static UPDATE_POLL: RefCell<Option<slint::Timer>> = const { RefCell::new(None) };
}

/// Runs `f` on the library window.
fn with_ctx(f: impl FnOnce(&mut App, &AppWindow)) {
    CTX.with(|c| {
        if let Some((app, ui)) = c.borrow().as_ref()
            && let Some(ui) = ui.upgrade()
        {
            f(&mut app.borrow_mut(), &ui);
        }
    });
}

/// As `with_ctx`, but does nothing when the app is already borrowed further up the stack (a call
/// that may come from inside `with_ctx`, ZK-212). Returns whether `f` ran.
fn try_with_ctx(f: impl FnOnce(&mut App, &AppWindow)) -> bool {
    CTX.with(|c| {
        if let Some((app, ui)) = c.borrow().as_ref()
            && let Some(ui) = ui.upgrade()
            && let Ok(mut a) = app.try_borrow_mut()
        {
            f(&mut a, &ui);
            return true;
        }
        false
    })
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

/// The wgpu device for Slint, made the way Slint would (the adapter from WGPU_ADAPTER_NAME or the
/// first one of the chosen backends) but with the adapter's own limits and, where the adapter has
/// them, NV12 textures — the video player's compute pass and zero-copy path (ZK-92).
fn manual_wgpu(s: &slint::wgpu_30::WGPUSettings) -> Option<slint::wgpu_30::WGPUConfiguration> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: s.backends,
        flags: s.instance_flags,
        backend_options: s.backend_options.clone(),
        memory_budget_thresholds: s.instance_memory_budget_thresholds,
        display: None,
    });
    let adapter = pollster::block_on(wgpu::util::initialize_adapter_from_env(&instance, None))
        .ok()
        .or_else(|| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: s.power_preference,
                force_fallback_adapter: false,
                compatible_surface: None,
                apply_limit_buckets: false,
            }))
            .ok()
        })?;
    let nv12 = adapter.features() & wgpu::Features::TEXTURE_FORMAT_NV12;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("znimok"),
        required_features: s.device_required_features | nv12,
        required_limits: adapter.limits(),
        experimental_features: s.device_experimental_features,
        memory_hints: s.device_memory_hints.clone(),
        trace: wgpu::Trace::default(),
    }))
    .map_err(|e| eprintln!("wgpu: own device failed ({e}); Slint chooses"))
    .ok()?;
    // wgpu's answer to an error nobody caught is a panic — and `Surface::configure` fails on
    // DX12 when the GPU will not go idle for a resize (ZK-249: a crash after an hour of work).
    // Logged instead: the next frame finds the surface lost and configures it anew.
    device.on_uncaptured_error(std::sync::Arc::new(|e: wgpu::Error| {
        static SEEN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = SEEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if n < 20 || n.is_multiple_of(500) {
            tracing::error!(n, "wgpu: {e}");
        }
    }));
    device.set_device_lost_callback(|reason, message| {
        tracing::error!(?reason, "wgpu: device lost: {message}");
    });
    Some(slint::wgpu_30::WGPUConfiguration::Manual {
        instance,
        adapter,
        device,
        queue,
    })
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
    }
    .map(Rc::new);
    let prefs = store.as_ref().map(|s| s.get()).unwrap_or_default();
    let lang = znimok_i18n::choose_language(
        prefs.general.language.as_deref(),
        znimok_i18n::system_language().as_deref(),
    );
    let tr = Rc::new(znimok_i18n::Localizer::new(lang));
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
    let selector = slint::BackendSelector::new();
    // macOS: a start at login stays in the menu bar only — no Dock icon until the editor window
    // is shown (tray::dock follows the window from then on).
    #[cfg(target_os = "macos")]
    let selector = {
        use slint::winit_030::winit::platform::macos::{
            ActivationPolicy, EventLoopBuilderExtMacOS,
        };
        let mut b = slint::winit_030::winit::event_loop::EventLoop::with_user_event();
        if std::env::args_os().any(|a| a == "--background") {
            b.with_activation_policy(ActivationPolicy::Accessory);
        }
        selector.with_winit_event_loop_builder(b)
    };
    // The device is made here (ZK-92): with the adapter's own limits (Slint's defaults allow no
    // storage buffers or textures — the video player converts its frames in a compute pass)
    // and, on Windows, NV12 textures when the adapter has them (frames without a copy).
    // Anything that fails leaves Slint to choose as before.
    let config =
        manual_wgpu(&settings).unwrap_or(slint::wgpu_30::WGPUConfiguration::Automatic(settings));
    selector
        .require_wgpu_30(config)
        // The card after a capture (ZK-41) must not take the focus or show in the taskbar.
        .with_winit_window_attributes_hook(|attrs| {
            if attrs.title != pill::TITLE
                && attrs.title != "Znimok rec"
                && attrs.title != overlay::COUNTDOWN_TITLE
                && attrs.title != scroll::PANEL_TITLE
            {
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
    ui.set_app_icon(tray::window_icon());
    frame::before_show(&ui);
    ui.set_mac(cfg!(target_os = "macos"));
    ui.global::<Keys>().set_mac(cfg!(target_os = "macos"));
    ui.set_capture_available(capture::available());
    ui.set_capture_key("".into());

    let app: Shared = Rc::new(RefCell::new(App::new(tr, lib_dir)));
    app.borrow_mut().bind(wins::WeakCtx::LIBRARY);
    CTX.with(|c| *c.borrow_mut() = Some((app.clone(), ui.as_weak())));
    app.borrow_mut().use_settings(&ui, store);
    app.borrow_mut().refresh_library(&ui);

    wire(&ui, &app);
    // The browser's log (ZK-97): the IPC server the Native Messaging host talks to, the host
    // registered for Chrome and Edge, and the extension's requests a few times a second. Not in
    // the self-test (a running Znimok holds the endpoint).
    let _ipc = if selftest_dir.is_none() {
        devlog::register_host();
        // Sending to Telegram, Jira… (ZK-101): what was left in the queue goes now.
        integrations::start();
        devlog::start_server()
    } else {
        None
    };
    let devtools_timer = slint::Timer::default();
    devtools_timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(200),
        || {
            devlog::poll();
            commands::poll();
        },
    );
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
    capture::warm_up_tone();

    // Global hotkeys from the settings (ZK-44); the self-test uses them too (recording a key).
    {
        let p = app.borrow().prefs();
        hotkeys::start(&p.capture.hotkeys, p.capture.enabled);
    }
    app.borrow().show_capture_key(&ui);

    // Tray / menu bar icon; while it exists, closing the window keeps the app running.
    let tray_ui = if selftest_dir.is_none() {
        let t = AppTray::new()?;
        t.set_tray_icon(tray::tray_icon());
        // macOS: the 400 ms timer makes the menu bar glyph a template once the item exists.
        t.set_capture_key(ui.get_capture_key());
        {
            // The recording hotkey, shown next to «Record video» (ZK-180).
            let os = znimok_platform::Os::current();
            let k = hotkeys::active(hotkeys::Action::Video)
                .or(app.borrow().prefs().capture.hotkeys.video)
                .map(|k| k.display(os))
                .unwrap_or_default();
            t.set_record_key(k.into());
        }
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
        // ZK-119: the display under the pointer, read for QR codes and barcodes.
        t.on_read_codes(|| {
            if capture::available() {
                codes::from_screen();
            }
        });
        // ZK-180: record video / stop recording.
        {
            let app = app.clone();
            let weak = ui.as_weak();
            t.on_record(move || {
                if let Some(ui) = weak.upgrade() {
                    rec::toggle(|| new_shot(&app, &ui));
                }
            });
        }
        rec::TRAY.with(|r| *r.borrow_mut() = Some(t.as_weak()));
        // ZK-185: the overlay in its text mode — the chosen part's text to the clipboard.
        {
            let app = app.clone();
            let weak = ui.as_weak();
            t.on_read_text(move || {
                if let Some(ui) = weak.upgrade()
                    && capture::available()
                    && !overlay::is_open()
                {
                    overlay::set_text_mode(true);
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
            // Every editor window saves (or asks) first (ZK-107).
            t.on_quit(wins::quit_all);
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

    // ZK-142: this version came up (a running update waits for that, or rolls back), and what
    // the last update did, once.
    if selftest_dir.is_none() {
        slint::Timer::single_shot(Duration::from_millis(1500), || {
            update::confirm_start();
            // macOS: Sparkle from the bundle, when it is there (ZK-143).
            #[cfg(target_os = "macos")]
            update::mac::init();
            // ZK-225: the updater writes its note only after this version has come up (it
            // waits for that before it decides between «installed» and a rollback), so the
            // first start after an update keeps looking for the note for a while.
            if !update::show_outcome() {
                let mut left = 90;
                let timer = slint::Timer::default();
                timer.start(
                    slint::TimerMode::Repeated,
                    Duration::from_secs(2),
                    move || {
                        left -= 1;
                        if update::show_outcome() || left == 0 {
                            UPDATE_POLL.with(|t| {
                                if let Some(t) = t.borrow().as_ref() {
                                    t.stop();
                                }
                            });
                        }
                    },
                );
                UPDATE_POLL.with(|t| *t.borrow_mut() = Some(timer));
            }
        });
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
                // macOS: the Dock icon only while a window is open; the menu bar glyph stays a
                // template (a no-op once it is one; heals an image replaced by Slint).
                #[cfg(target_os = "macos")]
                if TRAY.with(|c| c.get()) {
                    tray::dock(wins::any_visible());
                    tray::template_menu_icon();
                }
                app.borrow_mut().lib_poll(&ui, false);
                // Every window (ZK-107): its message, its autosave; the update check in the
                // library and in a window that started one (editors first: they take Sparkle's
                // events for the check they began).
                for (app, ui) in wins::all().into_iter().rev() {
                    let Ok(mut a) = app.try_borrow_mut() else {
                        continue;
                    };
                    a.tick_toast(&ui);
                    a.update_tick(&ui);
                    if let Some((path, doc, opts, video)) = a.autosave_job(&ui) {
                        let me = a.me();
                        std::thread::spawn(move || {
                            if let Some(dir) = path.parent() {
                                let _ = std::fs::create_dir_all(dir);
                            }
                            let r = {
                                let _guard =
                                    app::SAVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                                znimok_format::save_same_kind(&path, &doc, video.as_ref(), &opts)
                                    .map_err(|e| e.to_string())
                            };
                            let _ = slint::invoke_from_event_loop(move || {
                                me.with(|a, ui| a.save_finished(ui, &path, r));
                            });
                        });
                    }
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
    // An installer updating Znimok (an MSI started by hand) or a log-off asks the process to end:
    // leave the loop and go out the usual way below; Windows starts Znimok again, to the tray,
    // once the installer is done (ZK-170).
    #[cfg(windows)]
    if selftest_dir.is_none() {
        let _ = znimok_win::on_session_end(
            || {
                let _ = slint::quit_event_loop();
            },
            znimok_win::BACKGROUND_ARG,
        );
    }
    slint::run_event_loop_until_quit()?;
    drop(timer);
    drop(tray_ui);
    // ZK-146: leaving main hands the windows kept in thread-locals (the card after a capture,
    // the overlay…) to the runtime's thread-local cleanup at process exit. On Windows their
    // wgpu/DXGI swap chains are then released after the windows are gone: DXGI raises
    // 0x087A0001 (DXGI_ERROR_INVALID_CALL) and the exit is reported as a crash. So: close what
    // must be closed cleanly, wait for background saves, flush the log, and end the process
    // without that cleanup — the OS frees the rest.
    app.borrow_mut().before_exit();
    drop(app::SAVE_LOCK.lock().unwrap_or_else(|e| e.into_inner()));
    drop(_log);
    #[cfg(windows)]
    {
        use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
        let code = selftest::EXIT_CODE.load(std::sync::atomic::Ordering::SeqCst) as u32;
        // SAFETY: ends this process; everything that must persist is written above.
        let _ = unsafe { TerminateProcess(GetCurrentProcess(), code) };
    }
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
    #[cfg(target_os = "macos")]
    tray::dock(true);
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
    perform(&app, &ui, a);
}

/// What a capture action does — for the hotkeys and for the command layer (ZK-213).
fn perform(app: &Shared, ui: &AppWindow, a: hotkeys::Action) {
    match a {
        hotkeys::Action::Region => new_shot(app, ui),
        hotkeys::Action::Screen => {
            if overlay::is_open() || !capture::available() {
                return;
            }
            start_capture(app, ui, Pick::Whole(overlay::Next::default()));
        }
        // A window of its own (ZK-107); nothing in the clipboard: the library says so.
        hotkeys::Action::Clipboard => {
            if !app.borrow_mut().open_clipboard(ui) {
                show_window(ui);
            }
        }
        hotkeys::Action::Editor => app.borrow_mut().open_blank(ui),
        // As the tray's «Зчитати коди»: the screen under the pointer, the answer in the window.
        hotkeys::Action::ReadCodes => {
            if !overlay::is_open() && capture::available() {
                codes::from_screen();
            }
        }
        // The overlay, to choose the part whose text goes to the clipboard (ZK-185).
        hotkeys::Action::ReadText => {
            if !overlay::is_open() && capture::available() {
                overlay::set_text_mode(true);
                new_shot(app, ui);
            }
        }
        // ZK-180: the overlay chooses what to record; the same key stops it.
        hotkeys::Action::Video => {
            rec::toggle(|| new_shot(app, ui));
        }
    }
}

/// The window the editor's commands go to: the newest editor, else the library window when it
/// holds a document.
fn front_document() -> Option<(Shared, AppWindow)> {
    wins::newest_editor().or_else(|| wins::library().filter(|(a, _)| a.borrow().s.is_some()))
}

/// A command of the layer (ZK-213) on the UI thread; nothing when the state does not allow it.
pub(crate) fn run_command(method: &str, params: &serde_json::Value) {
    let ctx = CTX.with(|c| c.borrow().clone());
    let Some((app, weak)) = ctx else { return };
    let Some(ui) = weak.upgrade() else { return };
    let text = |k: &str| {
        params
            .get(k)
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
    };
    // `to: "clipboard"` sends the shot to the clipboard, `delay: 3` takes it after a countdown
    // of that many seconds (ZK-214).
    let delay = params
        .get("delay")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0)
        .min(30);
    let next = |intent| overlay::Next {
        intent,
        clip: text("to") == "clipboard",
        delayed: delay > 0,
    };
    if delay > 0 {
        overlay::set_countdown_ms(delay * 1000);
    }
    let ready = || !overlay::is_open() && !rec::is_recording() && capture::available();
    match method {
        "capture.start" => match text("mode") {
            "screen" | "window" if ready() => {
                let n = next(overlay::Intent::Keep);
                let pick = if text("mode") == "window" {
                    Pick::Window(n)
                } else {
                    Pick::Whole(n)
                };
                start_capture(&app, &ui, pick);
            }
            "screen" | "window" => {}
            "clipboard" => perform(&app, &ui, hotkeys::Action::Clipboard),
            "editor" => perform(&app, &ui, hotkeys::Action::Editor),
            "codes" => perform(&app, &ui, hotkeys::Action::ReadCodes),
            mode => {
                if !ready() {
                    return;
                }
                overlay::set_next(next(match mode {
                    "text" => overlay::Intent::Text,
                    "codes-region" => overlay::Intent::Codes,
                    "scroll" => overlay::Intent::Scroll,
                    _ => overlay::Intent::Keep,
                }));
                new_shot(&app, &ui);
            }
        },
        // A recording of the region chosen in the overlay, of the whole display or of the
        // active window — after the countdown with `delay` (ZK-214).
        "record.start" if ready() => {
            let n = next(overlay::Intent::Keep);
            match text("what") {
                "screen" => {
                    rec::set_video_mode(true);
                    start_capture(&app, &ui, Pick::Whole(n));
                }
                "window" => {
                    rec::set_video_mode(true);
                    start_capture(&app, &ui, Pick::Window(n));
                }
                _ => {
                    overlay::set_next(n);
                    rec::toggle(|| new_shot(&app, &ui));
                }
            }
        }
        "record.start" => {}
        // The sound of the next recording: none | system | mic | both, or the next one.
        "record.sound" => {
            let mode = match text("mode") {
                "none" => 0,
                "system" => 1,
                "mic" | "microphone" => 2,
                "both" => 3,
                _ => (rec::sound_mode() + 1) % 4,
            };
            app.borrow_mut().setting(&ui, "rec-sound", mode);
            overlay::sound_changed(mode);
        }
        "record.toggle" => rec::toggle(|| new_shot(&app, &ui)),
        "record.pause" if rec::is_recording() && !rec::is_paused() => rec::toggle_pause(),
        "record.resume" if rec::is_paused() => rec::toggle_pause(),
        "record.stop" if rec::is_recording() => rec::stop(),
        "editor.undo" | "editor.redo" | "editor.tool" | "editor.zoom" | "video.scrub" => {
            let Some((app, ui)) = front_document() else {
                return;
            };
            let mut a = app.borrow_mut();
            match method {
                "editor.undo" => a.undo(&ui),
                "editor.redo" => a.redo(&ui),
                "editor.tool" => {
                    let name = text("name");
                    let i = params
                        .get("index")
                        .and_then(serde_json::Value::as_u64)
                        .map(|i| i as usize)
                        .or_else(|| {
                            crate::app::tool::NAMES
                                .iter()
                                .position(|n| n.strip_prefix("tool-") == Some(name))
                        });
                    if let Some(i) = i {
                        a.set_tool(&ui, i);
                    }
                }
                "editor.zoom" => match text("step") {
                    "fit" => a.zoom_fit(&ui),
                    "100" => a.zoom_100(&ui),
                    "-1" | "out" => a.zoom_step(&ui, -1),
                    _ => a.zoom_step(&ui, 1),
                },
                _ => {
                    let n = params
                        .get("frames")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(1)
                        .clamp(-120, 120);
                    if a.is_video() {
                        let what = if n < 0 { "back" } else { "fwd" };
                        for _ in 0..n.unsigned_abs() {
                            a.vid_transport(&ui, what);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// The state other programs see (ZK-213): refreshed after each round of commands.
pub(crate) fn refresh_state() {
    use serde_json::json;
    let mut state = json!({"page": "library"});
    if let Some((app, ui)) = front_document().or_else(wins::library)
        && let Ok(a) = app.try_borrow()
    {
        let page = match ui.get_page() {
            1 => "editor",
            0 => "library",
            _ => "settings",
        };
        let (tool, zoom) = a.tool_and_zoom();
        state = json!({
            "page": page,
            "document": a.s.is_some().then(|| a.doc_name()),
            "video": a.is_video(),
            "tool": crate::app::tool::NAMES.get(tool).and_then(|n| n.strip_prefix("tool-")),
            "zoom": (zoom * 100.0).round() as i64,
            "can_undo": ui.get_can_undo(),
            "can_redo": ui.get_can_redo(),
        });
    }
    if overlay::is_open() {
        state["page"] = json!("overlay");
    }
    if rec::is_recording() {
        state["page"] = json!("recording");
        state["recording"] = json!({"paused": rec::is_paused(), "time": rec::bar_time()});
    }
    // An agent at work through MCP (ZK-241).
    if let Some(agent) = agentipc::active() {
        state["agent"] = agent;
    }
    state["sound"] =
        json!(["none", "system", "mic", "both"][rec::sound_mode().clamp(0, 3) as usize]);
    commands::hub().set_state(state);
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

/// The window's close button (ZK-107): an editor saves (or asks) and goes; the library hides.
fn close_window(app: &Shared, ui: &AppWindow) {
    if app.borrow().role == wins::Role::Editor {
        confirm_leave(app, ui, |app, ui| app.borrow_mut().close_document(ui));
        return;
    }
    let _ = ui.hide();
    wins::maybe_quit();
}

/// «Бібліотека» / Esc in an editor (ZK-107): the document is saved (or asks), its window goes,
/// the library comes to the front. The library window holds a document too since ZK-192 (a
/// card opens in the same window): there the document closes and the grid comes back (ZK-196).
fn back_to_library(app: &Shared, ui: &AppWindow) {
    if app.borrow().s.is_none() {
        return;
    }
    confirm_leave(app, ui, |app, ui| {
        app.borrow_mut().close_document(ui);
        wins::show_library();
    });
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

/// The Image button / I (ZK-163): a picture file as a mark on the open document. The dialog
/// off the UI thread (ZK-223).
fn insert_image_with_dialog(app: &Shared, ui: &AppWindow) {
    if filedlg::busy() {
        return;
    }
    let dlg = rfd::FileDialog::new().add_filter("PNG, JPEG, WebP, GIF, BMP", io::IMAGE_EXTENSIONS);
    let (app, weak) = (app.clone(), ui.as_weak());
    filedlg::pick_file(dlg, move |p| {
        let Some(ui) = weak.upgrade() else { return };
        if let Some(p) = p {
            app.borrow_mut().insert_image_file(&ui, &p);
        }
        ui.invoke_focus_canvas();
    });
}

/// «Open…»: a document, a report or a picture (ZK-223: the dialog off the UI thread).
fn open_with_dialog(app: &Shared, ui: &AppWindow) {
    if filedlg::busy() {
        return;
    }
    let dir = app.borrow().lib_dir.clone();
    let mut exts: Vec<&str> = io::IMAGE_EXTENSIONS.to_vec();
    exts.push("znimok");
    exts.push("zreport");
    let dlg = rfd::FileDialog::new()
        .add_filter("Znimok, .zreport, PNG, JPEG, WebP, GIF, BMP", &exts)
        .set_directory(dir);
    let (app, weak) = (app.clone(), ui.as_weak());
    filedlg::pick_file(dlg, move |p| {
        if let (Some(p), Some(ui)) = (p, weak.upgrade()) {
            app.borrow_mut().open_path(&ui, &p);
        }
    });
}

/// «Export» in the sheet (ZK-187): a file asks where, with the chosen format's extension.
fn export_go(app: &Shared, ui: &AppWindow) {
    if filedlg::busy() {
        return;
    }
    let Some((dir, name, format)) = app.borrow_mut().export_go(ui) else {
        return;
    };
    let (label, ext) = match format {
        znimok_settings::ExportFormat::Png => ("PNG", "png"),
        znimok_settings::ExportFormat::Jpeg => ("JPEG", "jpg"),
        znimok_settings::ExportFormat::Webp => ("WebP", "webp"),
    };
    let mut dlg = rfd::FileDialog::new()
        .add_filter(label, &[ext])
        .set_file_name(name);
    if let Some(d) = dir {
        dlg = dlg.set_directory(d);
    }
    let (app, weak) = (app.clone(), ui.as_weak());
    filedlg::save_file(dlg, move |p| {
        if let (Some(p), Some(ui)) = (p, weak.upgrade()) {
            app.borrow_mut().export_write(&ui, &p);
        }
    });
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
    start_capture(app, ui, Pick::Overlay);
}

/// What the frozen display becomes: the overlay to choose, or — without it — the whole display
/// or its active window (the whole-screen hotkey, the command layer's actions, ZK-214).
#[derive(Clone, Copy, Debug)]
pub(crate) enum Pick {
    Overlay,
    Whole(overlay::Next),
    Window(overlay::Next),
}

/// Freezes the display under the pointer, then `pick`.
fn start_capture(_app: &Shared, _ui: &AppWindow, pick: Pick) {
    // Already editing over the screen (ZK-58): finish that first.
    if wins::over_active() {
        return;
    }
    // Every window steps aside so the frozen screen does not contain them (on macOS the capture
    // filter would drop them anyway, but the overlay should not sit on top of them either).
    let was_visible = wins::step_aside();
    // Windows: give DWM time to take the editor off the screen before the frame is grabbed.
    let delay = Duration::from_millis(if was_visible && cfg!(windows) { 250 } else { 0 });
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        {
            let r = capture::freeze();
            let _ = slint::invoke_from_event_loop(move || {
                with_ctx(|a, ui| match r {
                    // Out of `with_ctx` first: the shot borrows the app (ZK-212).
                    Ok(frozen) if !matches!(pick, Pick::Overlay) => {
                        let (window, next) = match pick {
                            Pick::Window(n) => (true, n),
                            Pick::Whole(n) => (false, n),
                            Pick::Overlay => (false, overlay::Next::default()),
                        };
                        slint::Timer::single_shot(Duration::ZERO, move || {
                            overlay::keep(frozen, window, next, was_visible);
                        });
                    }
                    Ok(frozen) => {
                        if let Err(e) = overlay::open(frozen, was_visible) {
                            commands::emit("failed");
                            wins::come_back();
                            show_window(ui);
                            a.toast(ui, e.to_string());
                        }
                    }
                    // macOS without the Screen Recording permission (ZK-129): the permission, or
                    // the system picker now — without it, with macOS's sharing badge on the shot.
                    Err(capture::Fail::Permission) => {
                        commands::emit("failed");
                        wins::come_back();
                        show_window(ui);
                        let (title, body, pick, close) = (
                            a.tr.tr("perm-missing-title"),
                            a.tr.tr("err-capture-mac-perm") + " " + &a.tr.tr("perm-picker-hint"),
                            a.tr.tr("perm-use-picker"),
                            a.tr.tr("common-close"),
                        );
                        dialog::ask(
                            ui,
                            title,
                            body,
                            vec![pick, close],
                            0,
                            Some(1),
                            |_, answer| {
                                if answer == Some(0) {
                                    pick_without_permission();
                                }
                            },
                        );
                    }
                    Err(e) => {
                        wins::come_back();
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

/// The system content picker (macOS): a window or a display, captured without the permission.
fn pick_without_permission() {
    #[cfg(target_os = "macos")]
    znimok_mac::picker::pick_and_capture(|r| {
        let _ = slint::invoke_from_event_loop(move || {
            with_ctx(|a, ui| match r {
                Ok(Some(p)) => {
                    wins::come_back();
                    let raster = znimok_core::Raster::new(p.width, p.height, p.rgba);
                    a.new_document(ui, raster, "picker", None);
                }
                Ok(None) => {}
                Err(e) => {
                    let mut args = znimok_i18n::FluentArgs::new();
                    args.set("reason", e);
                    let msg = a.tr.tr_args("err-capture-generic", &args);
                    a.toast(ui, msg);
                }
            })
        });
    });
}

/// Ties a window's callbacks to its state; the library window and every editor window get
/// the same wiring (ZK-107) — what differs is decided by the `App`'s role.
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
        let app = app.clone();
        let weak = ui.as_weak();
        let me = app.borrow().me();
        ui.window().on_winit_window_event(move |_, ev| {
            let Some(ui) = weak.upgrade() else {
                return EventResult::Propagate;
            };
            if let winit::event::WindowEvent::Resized(_) = ev {
                frame::on_resized(&ui);
            }
            // The system switched light / dark (ZK-46): "as the system" follows at once, in
            // every window. (The event's value is the window's own appearance on macOS — ours —
            // so the system is asked directly.)
            if let winit::event::WindowEvent::ThemeChanged(_) = ev {
                let dark = system::system_dark();
                let _ = slint::invoke_from_event_loop(move || {
                    wins::for_each(|a, ui| a.system_theme(ui, dark));
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
                            me.with(|a, ui| a.hotkey_key(ui, &name, mods));
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
                    if let Ok(mut a) = app.try_borrow_mut() {
                        a.pinch(&ui, delta);
                    }
                    return EventResult::PreventDefault;
                }
                winit::event::WindowEvent::DoubleTapGesture { .. } => {
                    if let Ok(mut a) = app.try_borrow_mut() {
                        a.smart_zoom(&ui);
                    }
                    return EventResult::PreventDefault;
                }
                _ => {}
            }
            if let winit::event::WindowEvent::DroppedFile(path) = ev {
                let path = path.clone();
                // Leave winit's handler first (a message box or a new window from inside it
                // is asking for trouble).
                let _ = slint::invoke_from_event_loop(move || {
                    // On an open document a picture becomes a mark; otherwise it opens (in a
                    // window of its own, ZK-107).
                    me.with(|a, ui| {
                        if !a.drop_image_mark(ui, &path) {
                            a.open_path(ui, &path);
                        }
                    });
                });
                return EventResult::PreventDefault;
            }
            EventResult::Propagate
        });
    }

    // --- closing: an editor window saves (or asks when autosave is off) and goes; the library
    // window hides — with a tray icon the app stays (hotkey, "Quit" in the tray menu), without
    // one (self-test) the last window closed ends it.
    {
        let app = app.clone();
        let weak = ui.as_weak();
        ui.window().on_close_requested(move || {
            let Some(ui) = weak.upgrade() else {
                return slint::CloseRequestResponse::HideWindow;
            };
            close_window(&app, &ui);
            slint::CloseRequestResponse::KeepWindowShown
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
            // Same as the system close button.
            if let Some(ui) = weak.upgrade() {
                close_window(&app, &ui);
            }
        });
    }

    // --- library
    on!(ui, app, on_search, |a, w, text| {
        a.set_filter(&w, &text);
    });
    {
        let app = app.clone();
        let weak = ui.as_weak();
        // A plain click: the library opens the document; nothing opens from the trash (ZK-175).
        ui.on_open_card(move |path| {
            let Some(ui) = weak.upgrade() else { return };
            app.borrow_mut()
                .card_click(&ui, std::path::Path::new(path.as_str()), 0);
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
    on!(ui, app, on_open_clipboard, |a, w| {
        a.open_clipboard(&w);
    });

    // --- editor
    {
        let app = app.clone();
        let weak = ui.as_weak();
        ui.on_back(move || {
            if let Some(ui) = weak.upgrade() {
                back_to_library(&app, &ui);
            }
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
                app.borrow_mut().export_open(&ui);
            }
        });
    }
    on!(ui, app, on_autosave_toggled, |a, w, on| {
        a.setting(&w, "autosave", on as i32);
        a.sync(&w);
    });
    {
        let (app, weak) = (app.clone(), ui.as_weak());
        ui.on_insert_image(move || {
            if let Some(ui) = weak.upgrade() {
                insert_image_with_dialog(&app, &ui);
            }
        });
    }
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
                KeyAction::Export => app.borrow_mut().export_open(&ui),
                KeyAction::ExportRepeat => {
                    let done = app.borrow_mut().export_repeat(&ui);
                    if !done {
                        app.borrow_mut().export_open(&ui);
                    }
                }
                KeyAction::Open => open_with_dialog(&app, &ui),
                KeyAction::InsertImage => insert_image_with_dialog(&app, &ui),
                KeyAction::Back => back_to_library(&app, &ui),
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
    // The export sheet (ZK-187).
    on!(ui, app, on_exp_set, |a, w, key, v| {
        a.export_set(&w, &key, v);
    });
    on!(ui, app, on_exp_width_set, |a, w, text| {
        a.export_width(&w, &text);
    });
    on!(ui, app, on_exp_close, |a, w| {
        a.export_close(&w);
        w.invoke_focus_canvas();
    });
    {
        let (app, weak) = (app.clone(), ui.as_weak());
        ui.on_exp_go_clicked(move || {
            if let Some(ui) = weak.upgrade() {
                export_go(&app, &ui);
            }
        });
    }
    on!(ui, app, on_text_start, |a, w| {
        a.text_open(&w);
    });
    on!(ui, app, on_text_close, |a, w| {
        a.text_close(&w);
        w.invoke_focus_canvas();
    });
    on!(ui, app, on_text_copy_all, |a, w| {
        a.text_copy_all(&w);
    });
    on!(ui, app, on_read_codes, |a, _w| {
        if let Some((w, h, rgba)) = a.flatten() {
            codes::read_and_show(znimok_core::Raster::new(w, h, rgba), a.me());
        }
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
    // The colour picker (ZK-160).
    {
        let pick = ui.global::<ColourPick>();
        pick.on_hex(|c| app::hex_of(znimok_core::Rgb::new(c.red(), c.green(), c.blue())).into());
        let (a2, weak) = (app.clone(), ui.as_weak());
        pick.on_pick(move |key, c| {
            let Some(w) = weak.upgrade() else { return };
            let v = ((c.red() as i32) << 16) | ((c.green() as i32) << 8) | c.blue() as i32;
            a2.borrow_mut().set_prop(&w, &format!("{key}-rgb"), v);
        });
        let (a2, weak) = (app.clone(), ui.as_weak());
        pick.on_hex_entered(move |key, text| {
            let Some(w) = weak.upgrade() else { return };
            a2.borrow_mut().colour_hex(&w, &key, &text);
        });
        let (a2, weak) = (app.clone(), ui.as_weak());
        pick.on_eyedrop(move |key| {
            let Some(w) = weak.upgrade() else { return };
            a2.borrow_mut().eyedrop(&w, &key);
            w.invoke_focus_canvas();
        });
    }
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
    {
        let app = app.clone();
        let weak = ui.as_weak();
        ui.on_card_trash(move |path, forever| {
            let Some(w) = weak.upgrade() else { return };
            let path = std::path::PathBuf::from(path.as_str());
            if !forever {
                app.borrow_mut().lib_trash(&w, &path);
                return;
            }
            // Shift: for good, after one question (it cannot be undone).
            let (title, body, delete, cancel) = {
                let a = app.borrow();
                let name = library::read_entry(&path)
                    .map(|e| e.name)
                    .unwrap_or_else(|| {
                        path.file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into()
                    });
                let mut args = znimok_i18n::FluentArgs::new();
                args.set("name", name);
                (
                    a.tr.tr("lib-delete-forever-title"),
                    a.tr.tr_args("lib-delete-forever-body", &args),
                    a.tr.tr("common-delete"),
                    a.tr.tr("common-cancel"),
                )
            };
            let app = app.clone();
            dialog::ask(
                &w,
                title,
                body,
                vec![delete, cancel],
                1,
                Some(1),
                move |ui, answer| {
                    if answer == Some(0) {
                        app.borrow_mut().lib_delete_forever(ui, &path);
                    }
                },
            );
        });
    }
    // ZK-175/176: picking cards, the trash and its actions.
    on!(ui, app, on_card_click, |a, w, path, mode| {
        a.card_click(&w, std::path::Path::new(path.as_str()), mode);
    });
    on!(ui, app, on_lib_action, |a, w, what, path| {
        a.lib_action(&w, &what, std::path::Path::new(path.as_str()));
    });
    // ZK-177: the grid's columns; ZK-179: its keys.
    {
        // Slint reports new columns while laying out, which a window shown from inside the
        // app's own code can trigger: then the app is busy — take it right after.
        let (app, weak) = (app.clone(), ui.as_weak());
        ui.on_lib_layout(move |cols| {
            let Some(w) = weak.upgrade() else { return };
            if let Ok(mut a) = app.try_borrow_mut() {
                a.lib_layout(&w, cols);
                return;
            }
            let (app, weak) = (app.clone(), weak.clone());
            slint::Timer::single_shot(Duration::ZERO, move || {
                if let (Some(w), Ok(mut a)) = (weak.upgrade(), app.try_borrow_mut()) {
                    a.lib_layout(&w, cols);
                }
            });
        });
    }
    on!(ui, app, on_lib_key, |a, w, key, shift| {
        a.lib_key(&w, &key, shift);
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
    // ZK-181: the video's transport and timeline.
    on!(ui, app, on_vid_transport, |a, w, what| {
        a.vid_transport(&w, &what);
    });
    on!(ui, app, on_vid_action, |a, w, what, arg| {
        a.vid_action(&w, &what, arg);
    });
    on!(ui, app, on_vid_share, |a, w, what| {
        a.vid_share(&w, &what);
    });
    on!(ui, app, on_vid_out_edited, |a, w, field, text| {
        a.vid_out_edited(&w, &field, &text);
    });
    on!(ui, app, on_tl_pointer, |a, w, kind, x, y, shift| {
        a.tl_pointer(&w, kind, x as i32, y as i32, shift);
    });
    on!(ui, app, on_tl_wheel, |a, w, x, dy, ctrl| {
        a.tl_wheel(&w, x as i32, dy, ctrl);
    });
    on!(ui, app, on_devp_action, |a, w, what, i| {
        a.devp_action(&w, &what, i);
    });
    on!(ui, app, on_devp_search, |a, w, q| {
        a.devp_search(&w, &q);
    });
    on!(ui, app, on_setting_text, |a, w, key, text| {
        a.setting_text(&w, &key, &text);
    });
    on!(ui, app, on_int_action, |a, w, what, target| {
        a.int_action(&w, &what, &target);
    });
    on!(ui, app, on_sh_set, |a, w, what, value| {
        a.share_set(&w, &what, &value);
    });
    {
        // Reported while laying out, possibly from inside the app's own code (as lib-layout).
        let (app, weak) = (app.clone(), ui.as_weak());
        ui.on_tl_layout(move |w| {
            let Some(ui) = weak.upgrade() else { return };
            if let Ok(mut a) = app.try_borrow_mut() {
                a.tl_layout(&ui, w as i32);
                return;
            }
            let (app, weak) = (app.clone(), weak.clone());
            slint::Timer::single_shot(Duration::ZERO, move || {
                if let (Some(ui), Ok(mut a)) = (weak.upgrade(), app.try_borrow_mut()) {
                    a.tl_layout(&ui, w as i32);
                }
            });
        });
    }
}
