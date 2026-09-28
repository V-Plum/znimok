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
#[cfg(windows)]
mod hotkey_win;
mod io;
mod library;
mod selftest;

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
    #[cfg(windows)]
    znimok_win::init_process();

    let lang = znimok_i18n::choose_language(None, znimok_i18n::system_language().as_deref());
    let tr = znimok_i18n::Localizer::new(lang);

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
        .select()?;
    if lang != znimok_i18n::FALLBACK {
        let _ = slint::select_bundled_translation(lang);
    }

    let ui = AppWindow::new()?;
    ui.set_capture_available(capture::available());
    ui.set_capture_key(
        if cfg!(target_os = "macos") {
            "⌃⇧4"
        } else {
            "Ctrl+Shift+4"
        }
        .into(),
    );

    let app: Shared = Rc::new(RefCell::new(App::new(tr, library::default_dir())));
    CTX.with(|c| *c.borrow_mut() = Some((app.clone(), ui.as_weak())));
    app.borrow_mut().refresh_library(&ui);

    wire(&ui, &app);

    // Files from the command line (and "Open with…" on Windows).
    let files: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    #[cfg(windows)]
    if std::env::var_os("ZNIMOK_SELFTEST").is_none() {
        let ok = hotkey_win::register(|| {
            let _ = slint::invoke_from_event_loop(|| {
                CTX.with(|c| {
                    let ctx = c.borrow().clone();
                    if let Some((app, weak)) = ctx
                        && let Some(ui) = weak.upgrade()
                    {
                        new_shot(&app, &ui);
                    }
                });
            });
        });
        if !ok {
            eprintln!("Ctrl+Shift+4 is taken by another program");
        }
    }

    if let Some(dir) = std::env::var_os("ZNIMOK_SELFTEST") {
        selftest::start(app.clone(), &ui, PathBuf::from(dir), files.first().cloned());
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

    ui.show()?;
    slint::run_event_loop_until_quit()?;
    drop(timer);
    Ok(())
}

/// Asks what to do with unsaved changes when autosave is off. `false` = stay.
fn confirm_leave(app: &Shared, ui: &AppWindow) -> bool {
    let (unsaved, autosave, saving, name) = {
        let a = app.borrow();
        (a.is_unsaved(), a.autosave, a.saving, a.doc_name())
    };
    if !unsaved && !saving {
        return true;
    }
    // A background save may still be running (and may fail): save once more, synchronously,
    // after it — the save lock orders the two writers.
    if autosave || !unsaved {
        return app.borrow_mut().save_now(ui);
    }
    let (title, body, save, dont, cancel) = {
        let a = app.borrow();
        let mut args = znimok_i18n::FluentArgs::new();
        args.set("name", name);
        (
            a.tr.tr_args("confirm-save-title", &args),
            a.tr.tr("confirm-save-body"),
            a.tr.tr("common-save"),
            a.tr.tr("common-dont-save"),
            a.tr.tr("common-cancel"),
        )
    };
    // No borrow is held while the modal dialog runs its own message loop.
    let answer = rfd::MessageDialog::new()
        .set_title(&title)
        .set_description(&body)
        .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
            save.clone(),
            dont.clone(),
            cancel,
        ))
        .show();
    match answer {
        rfd::MessageDialogResult::Custom(c) if c == save => app.borrow_mut().save_now(ui),
        rfd::MessageDialogResult::Custom(c) if c == dont => true,
        rfd::MessageDialogResult::Yes => app.borrow_mut().save_now(ui),
        rfd::MessageDialogResult::No => true,
        _ => false,
    }
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
    if !confirm_leave(app, ui) {
        return;
    }
    if let Some(p) = pick_open(app) {
        app.borrow_mut().open_path(ui, &p);
    }
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
        app.borrow_mut().export_to(ui, &p);
    }
}

/// Screenshot of the display under the pointer: the window steps aside, the capture runs on a
/// worker thread (WinRT wants the multithreaded apartment, the UI thread is OLE's STA), then
/// the shot opens in the editor.
fn new_shot(app: &Shared, ui: &AppWindow) {
    if !capture::available() || !confirm_leave(app, ui) {
        return;
    }
    let _ = ui.hide();
    slint::Timer::single_shot(Duration::from_millis(250), move || {
        std::thread::spawn(move || {
            let r = capture::display_under_cursor();
            let _ = slint::invoke_from_event_loop(move || {
                with_ctx(|a, ui| {
                    let _ = ui.show();
                    match r {
                        Ok(raster) => a.new_document(ui, raster, "screen", None),
                        Err(e) => {
                            let mut args = znimok_i18n::FluentArgs::new();
                            args.set("reason", e);
                            let msg = a.tr.tr_args("err-capture-generic", &args);
                            a.toast(ui, msg);
                        }
                    }
                });
            });
        });
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
        let app = app.clone();
        let weak = ui.as_weak();
        ui.window().on_winit_window_event(move |_, ev| {
            if let winit::event::WindowEvent::DroppedFile(path) = ev {
                let path = path.clone();
                let app = app.clone();
                let weak = weak.clone();
                // Leave winit's handler first: the confirmation dialog runs a nested loop.
                slint::Timer::single_shot(Duration::ZERO, move || {
                    let Some(ui) = weak.upgrade() else { return };
                    if confirm_leave(&app, &ui) {
                        app.borrow_mut().open_path(&ui, &path);
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
            if !confirm_leave(&app, &ui) {
                return slint::CloseRequestResponse::KeepWindowShown;
            }
            let _ = slint::quit_event_loop();
            slint::CloseRequestResponse::HideWindow
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
            if confirm_leave(&app, &ui) {
                app.borrow_mut().open_clipboard(&ui);
            }
        });
    }

    // --- editor
    {
        let app = app.clone();
        let weak = ui.as_weak();
        ui.on_back(move || {
            let Some(ui) = weak.upgrade() else { return };
            if confirm_leave(&app, &ui) {
                app.borrow_mut().close_document(&ui);
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
                export_with_dialog(&app, &ui);
            }
        });
    }
    on!(ui, app, on_autosave_toggled, |a, w, on| {
        a.autosave = on;
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
                              _alt| {
        a.pointer(&w, kind, x, y, button, shift);
    });
    on!(ui, app, on_wheel, |a, w, x, y, dy, ctrl, alt| {
        a.wheel(&w, x, y, dy, ctrl || alt);
    });
    {
        let app = app.clone();
        let weak = ui.as_weak();
        ui.on_key(move |text, ctrl, shift, _alt| {
            let Some(ui) = weak.upgrade() else { return };
            let action = app.borrow_mut().key(&ui, &text, ctrl, shift);
            match action {
                KeyAction::Copy => app.borrow_mut().copy(&ui),
                KeyAction::Export => export_with_dialog(&app, &ui),
                KeyAction::Open => open_with_dialog(&app, &ui),
                KeyAction::None => {}
            }
        });
    }
    on!(ui, app, on_commit_text, |a, w, text| {
        a.commit_text(&w, &text);
    });
    on!(ui, app, on_cancel_text, |a, w| {
        a.cancel_text(&w);
    });
    on!(ui, app, on_set_color, |a, w, i| {
        a.set_color(&w, i.max(0) as usize);
        w.invoke_focus_canvas();
    });
    on!(ui, app, on_set_thick, |a, w, i| {
        a.set_thick(&w, i.max(0) as usize);
        w.invoke_focus_canvas();
    });
    on!(ui, app, on_zoom_fit, |a, w| {
        a.zoom_fit(&w);
    });
    on!(ui, app, on_zoom_100, |a, w| {
        a.zoom_100(&w);
    });
}
