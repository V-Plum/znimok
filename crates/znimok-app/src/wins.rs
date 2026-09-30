//! One window per document (ZK-107, owner's decision of 30.09): the library window is the hub —
//! settings, tray, hotkeys, updates, the library index; every document opens in an editor
//! window of its own (the same document twice raises the window that has it). "Over the
//! screen" is such a window too, covering the display without chrome.
//!
//! The registry here keeps the editor windows alive and answers the questions the rest of the
//! app has about them. Each window has its own `App` (the editor state was per window already:
//! session, view, renderer, canvas); what is shared lives in the library window's `App`
//! (`Role::Library`) and is reached through `crate::with_ctx`.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use slint::ComponentHandle;

use crate::app::App;
use crate::{AppWindow, Shared};

/// What a window's `App` is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// The hub: the library, the settings, everything shared. Never holds a document.
    Library,
    /// One document (or none while it is being closed).
    Editor,
}

/// A handle a window's own timers and worker threads use to get back to their window: a
/// number, so it can cross threads; the window is looked up on the UI thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeakCtx {
    id: u64,
}

impl WeakCtx {
    /// Before `App::bind`: points at nothing.
    pub const UNBOUND: WeakCtx = WeakCtx { id: u64::MAX };
    /// The library window.
    pub const LIBRARY: WeakCtx = WeakCtx { id: 0 };

    /// Runs `f` on the window, if it still exists and is not busy in another callback.
    pub fn with(&self, f: impl FnOnce(&mut App, &AppWindow)) {
        if let Some((app, ui)) = find(self.id)
            && let Ok(mut a) = app.try_borrow_mut()
        {
            f(&mut a, &ui);
        }
    }
}

fn find(id: u64) -> Option<(Shared, AppWindow)> {
    if id == 0 {
        return library();
    }
    EDITORS.with(|e| {
        e.borrow()
            .iter()
            .find(|w| w.id == id)
            .map(|w| (w.app.clone(), w.ui.clone_strong()))
    })
}

struct Editor {
    id: u64,
    app: Shared,
    ui: AppWindow,
}

thread_local! {
    static EDITORS: RefCell<Vec<Editor>> = const { RefCell::new(Vec::new()) };
    static NEXT_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
    /// Windows hidden for a capture (`step_aside`), to be shown again by `come_back`.
    static ASIDE: RefCell<Vec<slint::Weak<AppWindow>>> = const { RefCell::new(Vec::new()) };
}

/// The library window (the hub).
pub fn library() -> Option<(Shared, AppWindow)> {
    let ctx = crate::CTX.with(|c| c.borrow().clone());
    let (app, weak) = ctx?;
    Some((app, weak.upgrade()?))
}

/// The editor windows, oldest first.
pub fn editors() -> Vec<(Shared, AppWindow)> {
    EDITORS.with(|e| {
        e.borrow()
            .iter()
            .map(|w| (w.app.clone(), w.ui.clone_strong()))
            .collect()
    })
}

/// Every window: the library first, then the editors.
pub fn all() -> Vec<(Shared, AppWindow)> {
    let mut v: Vec<_> = library().into_iter().collect();
    v.extend(editors());
    v
}

/// A new editor window, shown, with the library's language, folder and settings.
pub fn spawn_editor(parent: &App) -> Result<(Shared, AppWindow), slint::PlatformError> {
    let ui = AppWindow::new()?;
    ui.set_app_icon(crate::tray::window_icon());
    crate::frame::before_show(&ui);
    ui.set_mac(cfg!(target_os = "macos"));
    ui.global::<crate::Keys>()
        .set_mac(cfg!(target_os = "macos"));
    ui.set_capture_available(crate::capture::available());
    let id = NEXT_ID.with(|n| n.replace(n.get() + 1));
    let app: Shared = Rc::new(RefCell::new(App::new_editor(parent)));
    app.borrow_mut().bind(WeakCtx { id });
    app.borrow_mut().use_settings(&ui, parent.store_handle());
    app.borrow().show_capture_key(&ui);
    crate::wire(&ui, &app);
    // Next to the library window, a little down and to the right of it (and of the last
    // editor), so windows do not stack exactly on each other.
    if let Some((_, lib)) = library() {
        let n = EDITORS.with(|e| e.borrow().len()) as i32 + 1;
        let p = lib.window().position();
        let k = lib.window().scale_factor();
        let step = (36.0 * k) as i32;
        ui.window()
            .set_position(slint::PhysicalPosition::new(p.x + step * n, p.y + step * n));
    }
    EDITORS.with(|e| {
        e.borrow_mut().push(Editor {
            id,
            app: app.clone(),
            ui: ui.clone_strong(),
        })
    });
    #[cfg(target_os = "macos")]
    crate::tray::dock(true);
    ui.show()?;
    crate::frame::after_show(&ui);
    Ok((app, ui))
}

/// The editor window that has `path` open.
pub fn editor_of(path: &Path) -> Option<(Shared, AppWindow)> {
    editors().into_iter().find(|(app, _)| {
        app.try_borrow()
            .is_ok_and(|a| a.s.as_ref().is_some_and(|s| s.path == path))
    })
}

/// The paths open in editor windows (the library's retention never trashes them).
pub fn open_paths() -> Vec<PathBuf> {
    editors()
        .into_iter()
        .filter_map(|(app, _)| app.try_borrow().ok()?.s.as_ref().map(|s| s.path.clone()))
        .collect()
}

/// The newest editor window with a document (the self-test drives that one).
pub fn newest_editor() -> Option<(Shared, AppWindow)> {
    editors()
        .into_iter()
        .rev()
        .find(|(app, _)| app.try_borrow().is_ok_and(|a| a.s.is_some()))
}

/// Brings a window to the front.
pub fn focus(ui: &AppWindow) {
    crate::show_window(ui);
}

/// Shows the library window and brings it to the front.
pub fn show_library() {
    if let Some((_, ui)) = library() {
        crate::show_window(&ui);
    }
}

/// An editor window is done: hidden now, dropped once the current callback has returned (a
/// window cannot be destroyed from inside its own callback). Quits when it was the last window
/// and there is no tray icon.
pub fn destroy(me: WeakCtx) {
    // Out of the registry at once (so the windows are counted right), hidden now, dropped once
    // the current callback has returned — a window cannot be destroyed from inside its own.
    let taken = EDITORS.with(|e| {
        let mut e = e.borrow_mut();
        let i = e.iter().position(|w| w.id == me.id)?;
        Some(e.remove(i))
    });
    let Some(w) = taken else { return };
    let _ = w.ui.hide();
    slint::Timer::single_shot(Duration::ZERO, move || drop(w));
    maybe_quit();
}

/// Closes the editor window that has `path` (the document was saved by the caller's rules).
pub fn close_editor_of(path: &Path) {
    if let Some((app, ui)) = editor_of(path)
        && let Ok(mut a) = app.try_borrow_mut()
    {
        a.close_document(&ui);
    }
}

/// Any window on screen (the macOS Dock icon follows this).
pub fn any_visible() -> bool {
    all().iter().any(|(_, ui)| ui.window().is_visible())
}

/// An editor is "over the screen".
pub fn over_active() -> bool {
    editors()
        .iter()
        .any(|(app, _)| app.try_borrow().is_ok_and(|a| a.over.is_some()))
}

/// Without a tray icon (the self-test, a run with `--no-tray`) the app ends with its last window.
pub fn maybe_quit() {
    if !crate::TRAY.with(|c| c.get()) && !any_visible() {
        let _ = slint::quit_event_loop();
    }
}

/// Hides every visible window before a capture, so the frozen screen does not contain them;
/// returns whether any was visible. `come_back` shows them again.
pub fn step_aside() -> bool {
    let mut hidden = Vec::new();
    for (_, ui) in all() {
        if ui.window().is_visible() {
            let _ = ui.hide();
            hidden.push(ui.as_weak());
        }
    }
    let any = !hidden.is_empty();
    ASIDE.with(|a| *a.borrow_mut() = hidden);
    any
}

/// The windows hidden by `step_aside`, back on screen.
pub fn come_back() {
    let hidden = ASIDE.with(|a| std::mem::take(&mut *a.borrow_mut()));
    for w in hidden {
        if let Some(ui) = w.upgrade() {
            crate::show_window(&ui);
        }
    }
}

/// Runs `f` on every window that is not busy in a callback right now.
pub fn for_each(mut f: impl FnMut(&mut App, &AppWindow)) {
    for (app, ui) in all() {
        if let Ok(mut a) = app.try_borrow_mut() {
            f(&mut a, &ui);
        }
    }
}

/// Runs `f` on the library window once the current callback has returned (from inside an
/// editor's callback the library may be borrowed — a card click opened the editor).
pub fn library_later(f: impl FnOnce(&mut App, &AppWindow) + 'static) {
    slint::Timer::single_shot(Duration::ZERO, move || crate::with_ctx(f));
}

/// The settings changed in the window of `app`: every other window re-reads them once the
/// current callback has returned.
pub fn prefs_changed(from: WeakCtx) {
    slint::Timer::single_shot(Duration::ZERO, move || {
        for (a, ui) in all() {
            if let Ok(mut a) = a.try_borrow_mut()
                && a.me() != from
            {
                a.reload_prefs(&ui);
            }
        }
    });
}

/// «Quit» from the tray: every editor saves (or asks), then the loop ends.
pub fn quit_all() {
    quit_from(editors(), 0);
}

fn quit_from(eds: Vec<(Shared, AppWindow)>, i: usize) {
    let Some((app, ui)) = eds.get(i).map(|(a, u)| (a.clone(), u.clone_strong())) else {
        let _ = slint::quit_event_loop();
        return;
    };
    crate::confirm_leave(&app, &ui, move |_, _| quit_from(eds, i + 1));
}
