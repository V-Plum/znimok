//! Questions asked inside the window, in the app's own look (owner, 28.09: no system message
//! boxes). The answer comes back to a continuation — the dialog does not block the event loop.

use std::cell::RefCell;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::AppWindow;

type Then = Box<dyn FnOnce(&AppWindow, Option<usize>)>;

thread_local! {
    static PENDING: RefCell<Option<Then>> = const { RefCell::new(None) };
}

/// Shows the question; `then` gets the index of the pressed button, or `None` for Esc when
/// there is no cancel button. A new question replaces one still open (its answer is `None`).
pub fn ask(
    ui: &AppWindow,
    title: String,
    body: String,
    buttons: Vec<String>,
    primary: usize,
    cancel: Option<usize>,
    then: impl FnOnce(&AppWindow, Option<usize>) + 'static,
) {
    if let Some(old) = PENDING.with(|p| p.borrow_mut().take()) {
        old(ui, None);
    }
    PENDING.with(|p| *p.borrow_mut() = Some(Box::new(then)));
    let buttons: Vec<SharedString> = buttons.into_iter().map(Into::into).collect();
    ui.set_dialog_title(title.into());
    ui.set_dialog_body(body.into());
    ui.set_dialog_buttons(ModelRc::new(VecModel::from(buttons)));
    ui.set_dialog_primary(primary as i32);
    ui.set_dialog_cancel(cancel.map_or(-1, |c| c as i32));
    ui.set_dialog_open(true);
    ui.window().request_redraw();
}

pub fn is_open(ui: &AppWindow) -> bool {
    ui.get_dialog_open()
}

/// Wired to `dialog-answer`.
pub fn answered(ui: &AppWindow, index: i32) {
    ui.set_dialog_open(false);
    ui.invoke_focus_canvas();
    let then = PENDING.with(|p| p.borrow_mut().take());
    if let Some(then) = then {
        then(ui, usize::try_from(index).ok());
    }
}
