//! The developer report of a recording (ZK-98): what a bug report needs in one place — the video
//! with its marks, the browser's DevTools log beside it (the list follows the video; a row takes
//! the video to its moment; a request opens in tabs as in DevTools), and where it was recorded.
//!
//! Two forms, one viewer:
//! - **HTML** — a single page, the MP4 inside it ([`HTML_LIMIT`] at most), for a chat or a ticket;
//! - **`.zreport`** — a ZIP: `report.html` (the same viewer, the video beside it), `video.mp4`,
//!   `log.json`, `datalayer.json`, `meta.json`, `poster.png`. Znimok opens it back as a recording
//!   ([`read_zreport`]).
//!
//! The log's times are the exported video's (the cuts taken out); sensitive values are hidden when
//! the person chose so ([`mask`]). The page is self-contained: no requests, no external scripts.

pub mod mask;
pub mod zip;

use std::collections::BTreeMap;

use serde_json::{Value, json};
use znimok_format::video::{DevEvent, DevLog};

/// The largest single HTML report: beyond it the `.zreport` (the video beside the page) is the way.
pub const HTML_LIMIT: u64 = 100 << 20;

/// The viewer: its markup, style and script; the placeholders are filled by [`page`].
const VIEWER: &str = include_str!("viewer.html");

/// What the header of a report says.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Meta {
    pub title: String,
    /// The labelled facts, in order: «Recorded: 01.10.2026 09:41», «Length: 0:38», …
    pub rows: Vec<(String, String)>,
    /// «12 values hidden», or empty.
    pub masked: String,
    /// The footer: «Made with Znimok 0.0.3».
    pub foot: String,
    /// The exported video's size.
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub seconds: f64,
    /// The video has a sound track.
    pub audio: bool,
}

/// Where the page's video comes from.
pub enum VideoSrc<'a> {
    /// Inside the page, base64.
    Inline(&'a [u8]),
    /// A file beside the page (the `.zreport`).
    File(&'a str),
}

/// The log's events for a report: each the extension's JSON with `at`, its time in the exported
/// video, seconds; events in parts that were cut out are left out (`at_of` gives `None`).
pub fn log_events(log: &DevLog, at_of: impl Fn(i32) -> Option<f64>) -> Vec<Value> {
    log.events
        .iter()
        .filter_map(|e| {
            let mut v: Value = serde_json::from_str(&e.json).ok()?;
            let at = at_of(e.ms)?;
            v.as_object_mut()?
                .insert("at".into(), json!((at * 1000.0).round() / 1000.0));
            Some(v)
        })
        .collect()
}

/// The JSON for a `<script type="application/json">`: no `</` or `<!--` that could close it
/// — spelled with JSON's own escapes (`<\!--` was not one, ZK-228: `JSON.parse` threw and the
/// page showed the video alone whenever the log held an HTML comment).
fn script_json(v: &Value) -> String {
    v.to_string()
        .replace("</", "<\\/")
        .replace("<!--", "<\\u0021--")
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// The report page. `strings`: the viewer's words by their keys (the app's `devp-*` and
/// `report-*`); `layers`: the marks' `<img class="m" data-t=…>` from the HTML export.
pub fn page(
    meta: &Meta,
    strings: &BTreeMap<String, String>,
    events: &[Value],
    video: VideoSrc,
    layers: &str,
) -> String {
    let data = json!({
        "meta": {
            "rows": meta.rows.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>(),
            "masked": meta.masked,
            "foot": meta.foot,
        },
        "strings": strings,
        "events": events,
    });
    let video_src = match video {
        VideoSrc::Inline(b) => format!("data:video/mp4;base64,{}", base64(b)),
        VideoSrc::File(name) => esc(name),
    };
    let title = esc(&meta.title);
    let mut out = String::with_capacity(VIEWER.len() + video_src.len() + layers.len() + 4096);
    let mut rest = VIEWER;
    while let Some(i) = rest.find("{{") {
        out.push_str(&rest[..i]);
        let Some(j) = rest[i..].find("}}") else { break };
        match &rest[i + 2..i + j] {
            "TITLE" => out.push_str(&title),
            "VIDEO" => out.push_str(&video_src),
            "LAYERS" => out.push_str(layers),
            "VW" => out.push_str(&meta.width.max(320).to_string()),
            "DATA" => out.push_str(&script_json(&data)),
            other => {
                out.push_str("{{");
                out.push_str(other);
                out.push_str("}}");
            }
        }
        rest = &rest[i + j + 2..];
    }
    out.push_str(rest);
    out
}

/// The `.zreport`: the page with the video beside it, the video, the log and the dataLayer as
/// JSON, the facts, the poster.
pub fn write_zreport(
    out: &mut impl std::io::Write,
    meta: &Meta,
    strings: &BTreeMap<String, String>,
    events: &[Value],
    mp4: &[u8],
    poster_png: Option<&[u8]>,
    layers: &str,
) -> std::io::Result<()> {
    let html = page(meta, strings, events, VideoSrc::File("video.mp4"), layers);
    let log = serde_json::to_vec_pretty(events).unwrap_or_default();
    let dl: Vec<Value> = events
        .iter()
        .filter(|e| e["k"] == "dl")
        .map(|e| json!({"at": e["at"], "event": e["ev"], "data": e["data"]}))
        .collect();
    let dl = serde_json::to_vec_pretty(&dl).unwrap_or_default();
    let facts = serde_json::to_vec_pretty(&json!({
        "format": "zreport",
        "version": 1,
        "title": meta.title,
        "width": meta.width,
        "height": meta.height,
        "fps": meta.fps,
        "seconds": meta.seconds,
        "audio": meta.audio,
        "rows": meta.rows.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>(),
        "masked": meta.masked,
    }))
    .unwrap_or_default();
    let mut files: Vec<(&str, &[u8])> = vec![
        ("report.html", html.as_bytes()),
        ("video.mp4", mp4),
        ("log.json", &log),
        ("datalayer.json", &dl),
        ("meta.json", &facts),
    ];
    if let Some(p) = poster_png {
        files.push(("poster.png", p));
    }
    zip::write(out, &files)
}

/// A `.zreport` read back: what a recording is made of again.
pub struct Imported {
    pub title: String,
    pub mp4: Vec<u8>,
    pub poster_png: Option<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub seconds: f64,
    pub audio: bool,
    /// The log on the video's time (`at` → ms), the events as they were exported.
    pub log: Option<DevLog>,
}

pub fn read_zreport(bytes: &[u8]) -> Result<Imported, String> {
    let files = zip::read(bytes)?;
    let get = |name: &str| {
        files
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, b)| b.as_slice())
    };
    let meta: Value = get("meta.json")
        .and_then(|b| serde_json::from_slice(b).ok())
        .ok_or("not a .zreport (no meta.json)")?;
    if meta["format"] != "zreport" {
        return Err("not a .zreport".into());
    }
    let mp4 = get("video.mp4")
        .ok_or("the .zreport has no video")?
        .to_vec();
    let events: Vec<Value> = get("log.json")
        .and_then(|b| serde_json::from_slice(b).ok())
        .unwrap_or_default();
    let mut log: Vec<DevEvent> = events
        .into_iter()
        .filter_map(|mut v| {
            let at = v.as_object_mut()?.remove("at")?.as_f64()?;
            Some(DevEvent {
                ms: (at * 1000.0).round().clamp(0.0, f64::from(i32::MAX)) as i32,
                json: v.to_string(),
            })
        })
        .collect();
    log.sort_by_key(|e| e.ms);
    let num = |k: &str| meta[k].as_f64().unwrap_or(0.0);
    Ok(Imported {
        title: meta["title"].as_str().unwrap_or("").to_string(),
        mp4,
        poster_png: get("poster.png").map(<[u8]>::to_vec),
        width: num("width") as u32,
        height: num("height") as u32,
        fps: num("fps"),
        seconds: num("seconds"),
        audio: meta["audio"] == Value::Bool(true),
        log: (!log.is_empty()).then_some(DevLog {
            wall0_ms: 0,
            events: log,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log() -> DevLog {
        let e = |ms, j: &str| DevEvent { ms, json: j.into() };
        DevLog {
            wall0_ms: 0,
            events: vec![
                e(500, r#"{"k":"nav","s":0,"url":"https://example.org/"}"#),
                e(1500, r#"{"k":"console","s":0,"text":"cut out"}"#),
                e(
                    2500,
                    r#"{"k":"dl","s":0,"ev":"purchase","data":{"event":"purchase","value":42}}"#,
                ),
                e(2600, r#"{"k":"console","s":0,"text":"</script><b>x</b>"}"#),
            ],
        }
    }

    #[test]
    fn events_on_the_exported_time() {
        // 1–2 s cut out: 2.5 s becomes 1.5 s; the event inside the cut goes.
        let ev = log_events(&log(), |ms| match ms {
            ..1000 => Some(f64::from(ms) / 1000.0),
            1000..2000 => None,
            _ => Some(f64::from(ms - 1000) / 1000.0),
        });
        let at: Vec<f64> = ev.iter().map(|e| e["at"].as_f64().unwrap()).collect();
        assert_eq!(at, [0.5, 1.5, 1.6]);
    }

    #[test]
    fn the_page_is_whole_and_safe() {
        let ev = log_events(&log(), |ms| Some(f64::from(ms) / 1000.0));
        let meta = Meta {
            title: "Звіт <1>".into(),
            rows: vec![("Записано".into(), "01.10.2026".into())],
            width: 640,
            ..Default::default()
        };
        let mut strings = BTreeMap::new();
        strings.insert("devp-search".to_string(), "Пошук".to_string());
        let html = page(&meta, &strings, &ev, VideoSrc::Inline(b"mp4"), "");
        assert!(html.contains("<title>Звіт &lt;1&gt;</title>"));
        assert!(html.contains("data:video/mp4;base64,bXA0"));
        assert!(!html.contains("{{"));
        // The console text cannot close the data script.
        let data = &html[html.find("id=\"zn-data\">").unwrap()..];
        let data = &data[..data.find("</script>").unwrap()];
        assert!(data.contains(r"<\/script><b>x<\/b>"));
        let parsed: Value = serde_json::from_str(&data["id=\"zn-data\">".len()..]).unwrap();
        assert_eq!(parsed["events"].as_array().unwrap().len(), 4);
    }

    /// ZK-228: a log with an HTML comment in it (Google Tag Manager's page source in a console
    /// line) must still parse — `<\!--` was not a JSON escape and killed the viewer.
    #[test]
    fn html_comments_in_the_log_do_not_break_the_page() {
        let ev = vec![serde_json::json!({
            "at": 0.5, "k": "console", "lvl": "log",
            "text": "<script>x</script>\n<!-- Google Tag Manager -->\n<!-- End -->"
        })];
        let meta = Meta {
            title: "Звіт".into(),
            width: 640,
            ..Default::default()
        };
        let html = page(
            &meta,
            &BTreeMap::new(),
            &ev,
            VideoSrc::File("v.mp4".into()),
            "",
        );
        let data = &html[html.find("id=\"zn-data\">").unwrap() + "id=\"zn-data\">".len()..];
        let data = &data[..data.find("</script>").unwrap()];
        assert!(!data.contains("<!--") && !data.contains("</s"));
        let parsed: Value = serde_json::from_str(data).expect("the embedded JSON parses");
        assert_eq!(
            parsed["events"][0]["text"].as_str().unwrap(),
            "<script>x</script>\n<!-- Google Tag Manager -->\n<!-- End -->"
        );
    }

    #[test]
    fn zreport_round_trip() {
        let ev = log_events(&log(), |ms| Some(f64::from(ms) / 1000.0));
        let meta = Meta {
            title: "Звіт".into(),
            width: 640,
            height: 360,
            fps: 30.0,
            seconds: 3.0,
            ..Default::default()
        };
        let mut buf = Vec::new();
        write_zreport(
            &mut buf,
            &meta,
            &BTreeMap::new(),
            &ev,
            b"MP4DATA",
            Some(b"PNG"),
            "",
        )
        .unwrap();
        let back = read_zreport(&buf).unwrap();
        assert_eq!(back.mp4, b"MP4DATA");
        assert_eq!(back.poster_png.as_deref(), Some(&b"PNG"[..]));
        assert_eq!((back.width, back.height, back.fps), (640, 360, 30.0));
        let log = back.log.unwrap();
        assert_eq!(log.events.len(), 4);
        assert_eq!(log.events[2].ms, 2500);
        assert!(!log.events[2].json.contains("\"at\""));
        // The page inside points at the video beside it.
        let files = zip::read(&buf).unwrap();
        let html = String::from_utf8(files[0].1.clone()).unwrap();
        assert!(html.contains("src=\"video.mp4\""));
        assert!(read_zreport(b"not a zip").is_err());
    }
}
