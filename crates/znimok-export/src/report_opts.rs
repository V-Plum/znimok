//! A report's options from a recording and the settings (ZK-243: the app's export sheet and an
//! agent's export make the same page): the header's facts, the signature, the viewer's words in
//! the page's language, what is hidden.

use znimok_core::Document;
use znimok_format::video::Video;
use znimok_i18n::{FluentArgs, Localizer};
use znimok_settings::Settings;

use crate::ReportOptions;

/// Znimok's site in a language: the Ukrainian page, else the English one (ZK-256).
pub fn site_url(lang: &str) -> &'static str {
    if lang == "uk" {
        "https://v-plum.github.io/znimok/"
    } else {
        "https://v-plum.github.io/znimok/en/"
    }
}

/// `m:ss.cc`, as the editor shows a time.
fn fmt_time(secs: f64) -> String {
    let secs = secs.max(0.0);
    let m = (secs / 60.0).floor() as i64;
    let s = secs - m as f64 * 60.0;
    format!("{m}:{s:05.2}")
}

fn one(k: &str, v: String) -> FluentArgs<'static> {
    let mut a = FluentArgs::new();
    a.set(k.to_string(), v);
    a
}

/// The report of `doc` (its recording `video`, `seconds` long after the cuts — counted here when
/// not given), its words by `tr`, hiding the settings' keys when `hide`.
pub fn report_options(
    doc: &Document,
    video: &Video,
    prefs: &Settings,
    tr: &Localizer,
    seconds: Option<f64>,
    zip: bool,
    hide: bool,
) -> ReportOptions {
    let mut rows = Vec::new();
    // Who recorded it and whose it is — when the person signs their reports.
    let v = &prefs.video;
    let by = [v.sign_name.as_str(), v.sign_contact.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    if v.report_sign && !by.is_empty() {
        rows.push((tr.tr("report-author"), by));
    }
    let rights = if v.report_sign {
        v.sign_rights.clone()
    } else {
        String::new()
    };
    if doc.meta.created_ms > 0
        && let Some(t) = chrono::DateTime::from_timestamp_millis(doc.meta.created_ms)
    {
        rows.push((
            tr.tr("report-recorded"),
            t.with_timezone(&chrono::Local)
                .format("%d.%m.%Y %H:%M")
                .to_string(),
        ));
    }
    let seconds = seconds.unwrap_or_else(|| {
        let keep = crate::keep_of(doc, i64::from(video.info.frames));
        let fps = f64::from(video.info.fps_milli) / 1000.0;
        znimok_video::export::kept_frames(&keep) as f64 / fps.max(1e-6)
    });
    rows.push((tr.tr("report-length"), fmt_time(seconds)));
    let (_, out) = crate::out_geometry(doc, video);
    rows.push((tr.tr("report-size"), format!("{} × {}", out.0, out.1)));
    // Where it was recorded: the browser and the page the log starts on.
    let events: Vec<serde_json::Value> = video
        .devlog
        .as_ref()
        .map(|l| {
            l.events
                .iter()
                .take(200)
                .filter_map(|e| serde_json::from_str(&e.json).ok())
                .collect()
        })
        .unwrap_or_default();
    if let Some(b) = events.iter().find_map(|e| e["b"].as_str()) {
        rows.push((tr.tr("report-browser"), b.to_string()));
    }
    if let Some(u) = events
        .iter()
        .filter(|e| e["k"] == "tab" || e["k"] == "nav")
        .filter_map(|e| e["url"].as_str())
        // The site, not the browser's own page the recording started on (ZK-246).
        .find(|u| u.starts_with("http://") || u.starts_with("https://"))
    {
        rows.push((tr.tr("report-page"), u.to_string()));
    }
    let mut strings = std::collections::BTreeMap::new();
    for k in REPORT_STRINGS {
        strings.insert(k.to_string(), tr.tr(k));
    }
    strings.insert(
        "devp-binary".into(),
        tr.tr_args("devp-binary", &one("size", "{size}".into())),
    );
    ReportOptions {
        zip,
        mask: hide.then(|| prefs.video.hide_keys.clone()),
        meta: znimok_report::Meta {
            title: doc.name.clone(),
            rows,
            masked: tr.tr_args("report-masked", &one("n", "{n}".into())),
            foot: tr.tr_args(
                "report-foot",
                &one("version", env!("CARGO_PKG_VERSION").into()),
            ),
            rights,
            link: site_url(tr.lang()).into(),
            ..Default::default()
        },
        strings,
    }
}

/// The viewer's words, by their keys.
pub const REPORT_STRINGS: &[&str] = &[
    "report-tab-details",
    "report-drag",
    "report-fold-log",
    "report-show-log",
    "report-fold-det",
    "report-show-det",
    "report-files",
    "report-download",
    "report-more",
    "report-cmp-hint",
    "report-copy-hint",
    "report-tl-hint",
    "report-page",
    "report-chip-posthog",
    "report-tab-posthog",
    "report-curl",
    "report-link",
    "report-search-hint",
    "report-ph-flags",
    "report-ph-replay",
    "report-cmp",
    "report-cmp-save",
    "report-cmp-name",
    "report-cmp-import",
    "report-cmp-export",
    "report-cmp-stop",
    "report-cmp-none",
    "report-cmp-sum",
    "report-cmp-only",
    "report-cmp-gone",
    "report-cmp-new",
    "report-cmp-full",
    "common-delete",
    "report-pick",
    "report-copy",
    "report-h-type",
    "report-h-size",
    "report-h-source",
    "report-h-title",
    "report-chip-clicks",
    "report-click",
    "report-keys",
    "devp-search",
    "devp-empty",
    "devp-chip-all",
    "devp-chip-errors",
    "devp-chip-warnings",
    "devp-chip-network",
    "devp-chip-console",
    "devp-chip-nav",
    "devp-chip-datalayer",
    "devp-tab-headers",
    "devp-tab-payload",
    "devp-tab-preview",
    "devp-tab-response",
    "devp-tab-timing",
    "devp-general",
    "devp-res-headers",
    "devp-req-headers",
    "devp-h-url",
    "devp-h-method",
    "devp-h-status",
    "devp-h-remote",
    "devp-h-protocol",
    "devp-h-initiator",
    "devp-h-cache",
    "devp-h-error",
    "devp-stack",
    "devp-nothing",
    "devp-dl-pre",
    "devp-dl-frame",
    "devp-t-queue",
    "devp-t-dns",
    "devp-t-connect",
    "devp-t-tls",
    "devp-t-send",
    "devp-t-wait",
    "devp-t-download",
    "devp-t-total",
    "report-no-log",
    "common-close",
];
