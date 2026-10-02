//! The system's file dialogs, off the UI thread on Windows (ZK-215). A modal dialog run on the
//! UI thread stops the event loop while it is up, and the owner saw the export of a report end
//! there: the Explorer window came up and nothing answered any more — not the dialog, not the
//! app. The dialog now runs on a thread of its own (rfd initialises COM there), the UI thread
//! keeps its loop, and the answer comes back through it to a continuation kept here. macOS:
//! the panel runs on the main thread as before (rfd dispatches to it anyway).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;

type Then = Box<dyn FnOnce(Option<PathBuf>)>;

thread_local! {
    /// The continuations of the dialogs up, by ticket (UI thread only).
    static PENDING: RefCell<HashMap<u64, Then>> = RefCell::new(HashMap::new());
    static NEXT: Cell<u64> = const { Cell::new(1) };
}

/// A dialog is up: the buttons that would open another do nothing meanwhile.
pub fn busy() -> bool {
    PENDING.with(|p| !p.borrow().is_empty())
}

/// «Save as»: `then` gets the path, or `None` when cancelled.
pub fn save_file(dlg: rfd::FileDialog, then: impl FnOnce(Option<PathBuf>) + 'static) {
    run(move || dlg.save_file(), then);
}

#[cfg(windows)]
fn run(
    show: impl FnOnce() -> Option<PathBuf> + Send + 'static,
    then: impl FnOnce(Option<PathBuf>) + 'static,
) {
    let id = NEXT.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    PENDING.with(|p| p.borrow_mut().insert(id, Box::new(then)));
    std::thread::Builder::new()
        .name("file-dialog".into())
        .spawn(move || {
            let path = show();
            // The event loop is gone: nothing to tell.
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(then) = PENDING.with(|p| p.borrow_mut().remove(&id)) {
                    then(path);
                }
            });
        })
        .expect("a thread for the file dialog");
}

#[cfg(not(windows))]
fn run(
    show: impl FnOnce() -> Option<PathBuf> + Send + 'static,
    then: impl FnOnce(Option<PathBuf>) + 'static,
) {
    then(show());
}
