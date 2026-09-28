//! ZK-32: after a crash, offer its report once at the next start — open a prefilled GitHub issue
//! or show the folder. Nothing is sent by Znimok itself.

use znimok_i18n::{Localizer, args};

pub fn offer_last_crash(tr: &Localizer) {
    let reports = znimok_log::pending_reports();
    let Some(last) = reports.first() else { return };
    // Offer once: several crashes in a row are one conversation, the newest is the one to show.
    for r in &reports {
        znimok_log::mark_seen(r);
    }
    let issue = tr.tr("crash-open-issue");
    let folder = tr.tr("crash-show-folder");
    let close = tr.tr("common-close");
    let answer = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title(tr.tr("crash-title"))
        .set_description(tr.tr_args("crash-body", &args!(summary = last.summary())))
        .set_buttons(rfd::MessageButtons::YesNoCancelCustom(
            issue.clone(),
            folder.clone(),
            close,
        ))
        .show();
    match answer {
        rfd::MessageDialogResult::Custom(c) if c == issue => {
            znimok_log::open_in_os(&znimok_log::issue_url(last))
        }
        rfd::MessageDialogResult::Custom(c) if c == folder => {
            znimok_log::open_in_os(&znimok_log::crashes_dir().to_string_lossy())
        }
        _ => {}
    }
}
