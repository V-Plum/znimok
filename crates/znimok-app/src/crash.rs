//! ZK-32: after a crash, offer its report once at the next start — open a prefilled GitHub issue
//! or show the folder. Nothing is sent by Znimok itself.

use znimok_i18n::args;

use crate::{AppWindow, Shared, dialog};

/// Asked in the window (owner, 28.09: no system message boxes), which comes to the front for it.
pub fn offer_last_crash(app: &Shared, ui: &AppWindow) {
    let reports = znimok_log::pending_reports();
    let Some(last) = reports.first().cloned() else {
        return;
    };
    // Offer once: several crashes in a row are one conversation, the newest is the one to show.
    for r in &reports {
        znimok_log::mark_seen(r);
    }
    let (title, body, buttons) = {
        let a = app.borrow();
        let tr = &a.tr;
        (
            tr.tr("crash-title"),
            tr.tr_args("crash-body", &args!(summary = last.summary())),
            vec![
                tr.tr("common-close"),
                tr.tr("crash-show-folder"),
                tr.tr("crash-open-issue"),
            ],
        )
    };
    crate::show_window(ui);
    dialog::ask(
        ui,
        title,
        body,
        buttons,
        2,
        Some(0),
        move |_, answer| match answer {
            Some(2) => znimok_log::open_in_os(&znimok_log::issue_url(&last)),
            Some(1) => znimok_log::open_in_os(&znimok_log::crashes_dir().to_string_lossy()),
            _ => {}
        },
    );
}
