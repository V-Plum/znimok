//! QR codes and barcodes (ZK-119): from the capture overlay (Q), the tray («Зчитати QR-код з
//! екрана») and the editor (Image tab). The picture is read on a worker thread, on the device;
//! the answer is a question in the app's window with what each code is and what can be done.
//! A link opens only after a second question that shows the whole address (QR phishing).

use znimok_codes::{Code, Kind};
use znimok_core::Raster;

use crate::{AppWindow, dialog};

/// Reads `raster` in the background, then shows what was found.
pub fn read_and_show(raster: Raster, me: crate::wins::WeakCtx) {
    std::thread::spawn(move || {
        let t = std::time::Instant::now();
        let codes = znimok_codes::read(raster.width, raster.height, &raster.rgba);
        let ms = t.elapsed().as_millis();
        let _ = slint::invoke_from_event_loop(move || {
            // The answer in the window that asked (ZK-107).
            me.with(|a, ui| {
                crate::show_window(ui);
                show(a, ui, codes);
                eprintln!("[codes] {ms} ms");
            })
        });
    });
}

/// The question with the codes (or that there are none).
pub fn show(a: &crate::app::App, ui: &AppWindow, codes: Vec<Code>) {
    LAST.with(|l| *l.borrow_mut() = codes.clone());
    if codes.is_empty() {
        dialog::ask(
            ui,
            a.tr.tr("codes-none-title"),
            a.tr.tr("codes-none-body"),
            vec![a.tr.tr("common-close")],
            0,
            Some(0),
            |_, _| {},
        );
        return;
    }
    let lines: Vec<String> = codes
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{}. {}", i + 1, describe(a, c)))
        .collect();
    let link = codes.iter().find_map(|c| match &c.kind {
        Kind::Link(u) => Some(u.clone()),
        _ => None,
    });
    let mut buttons = vec![a.tr.tr("codes-copy")];
    if link.is_some() {
        buttons.push(a.tr.tr("codes-open-link"));
    }
    buttons.push(a.tr.tr("common-close"));
    let close = buttons.len() - 1;
    let title = a.tr.tr_args(
        "codes-title",
        &crate::app::fargs(&[("count", codes.len().to_string())]),
    );
    let texts: Vec<String> = codes.iter().map(|c| c.text.clone()).collect();
    dialog::ask(
        ui,
        title,
        lines.join("\n"),
        buttons,
        0,
        Some(close),
        move |ui, answer| match answer {
            Some(0) => {
                let copied = arboard::Clipboard::new()
                    .and_then(|mut cb| cb.set_text(texts.join("\n")))
                    .is_ok();
                crate::with_ctx(|a, _| {
                    let msg = if copied {
                        a.tr.tr("clipboard-copied")
                    } else {
                        a.tr.tr("clipboard-error")
                    };
                    a.toast(ui, msg);
                });
            }
            Some(1) if close == 2 => {
                if let Some(url) = link {
                    confirm_open(ui, url);
                }
            }
            _ => {}
        },
    );
}

thread_local! {
    /// The last codes shown, for the self-test.
    static LAST: std::cell::RefCell<Vec<Code>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn last() -> Vec<Code> {
    LAST.with(|l| l.borrow().clone())
}

pub fn clear_last() {
    LAST.with(|l| l.borrow_mut().clear());
}

/// The whole address, and only then the browser.
fn confirm_open(ui: &AppWindow, url: String) {
    let (title, body, open, cancel) = {
        let mut t = None;
        crate::with_ctx(|a, _| {
            t = Some((
                a.tr.tr("codes-open-title"),
                a.tr.tr_args(
                    "codes-open-body",
                    &crate::app::fargs(&[("url", url.clone())]),
                ),
                a.tr.tr("codes-open"),
                a.tr.tr("common-cancel"),
            ));
        });
        match t {
            Some(t) => t,
            None => return,
        }
    };
    dialog::ask(
        ui,
        title,
        body,
        vec![open, cancel],
        1,
        Some(1),
        move |_, answer| {
            if answer == Some(0) {
                open_url(&url);
            }
        },
    );
}

/// Only http(s) ever gets here (`Kind::Link`); no shell is involved.
pub(crate) fn open_url(url: &str) {
    #[cfg(windows)]
    let _ = std::process::Command::new("rundll32.exe")
        .arg("url.dll,FileProtocolHandler")
        .arg(url)
        .spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = url;
}

fn describe(a: &crate::app::App, c: &Code) -> String {
    let short = |s: &str| {
        let s = s.replace(['\r', '\n'], " ");
        if s.chars().count() > 160 {
            format!("{}…", s.chars().take(160).collect::<String>())
        } else {
            s
        }
    };
    let f = |k: &str, pairs: &[(&'static str, String)]| a.tr.tr_args(k, &crate::app::fargs(pairs));
    match &c.kind {
        Kind::Link(u) => f("codes-link", &[("url", short(u))]),
        Kind::Wifi {
            ssid,
            password,
            security,
            ..
        } => match password {
            Some(p) => f(
                "codes-wifi",
                &[
                    ("ssid", ssid.clone()),
                    ("password", p.clone()),
                    ("security", security.clone()),
                ],
            ),
            None => f("codes-wifi-open", &[("ssid", ssid.clone())]),
        },
        Kind::Contact => format!("{} — {}", a.tr.tr("codes-contact"), short(&c.text)),
        Kind::Event => format!("{} — {}", a.tr.tr("codes-event"), short(&c.text)),
        Kind::Email(e) => f("codes-email", &[("address", e.clone())]),
        Kind::Phone(p) => f("codes-phone", &[("number", p.clone())]),
        Kind::Text => f("codes-text", &[("text", short(&c.text))]),
    }
}

/// The tray item: the display under the pointer, read whole.
pub fn from_screen() {
    std::thread::spawn(|| {
        if let Ok(f) = crate::capture::freeze() {
            let raster = f.raster;
            let codes = znimok_codes::read(raster.width, raster.height, &raster.rgba);
            let _ = slint::invoke_from_event_loop(move || {
                crate::with_ctx(|a, ui| {
                    crate::show_window(ui);
                    show(a, ui, codes);
                })
            });
        }
    });
}
