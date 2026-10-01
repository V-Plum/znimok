//! The browser's DevTools log of a recording in the video editor (ZK-191): the log lane's ticks
//! on the timeline and the panel under it — search, filter chips with counts, the list, and a
//! row's details (a request as the DevTools Network panel shows it: headers, payload, preview,
//! response, timing; the console's full text and stack).
//!
//! The events are the extension's JSON as recorded (`DEVT`, ZK-97): `k` the kind (`console`,
//! `log`, `error`, `net`, `ws`, `nav`, `tab`, `info`, `dl` — a dataLayer push, ZK-195), `s` the
//! severity (0 plain, 1 warning, 2 error).

use serde_json::Value;
use znimok_format::video::DevLog;

/// What an event is, as the list and the ticks tell them apart.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Console,
    Error,
    Net,
    Ws,
    Nav,
    Info,
    /// A value pushed into `dataLayer` (GTM, gtag; ZK-195).
    Dl,
}

pub struct Entry {
    /// Video time, ms.
    pub ms: i32,
    pub kind: Kind,
    pub sev: u8,
    pub v: Value,
}

/// The filter chips: all, errors, warnings, network, console, navigations, dataLayer.
pub const CHIPS: usize = 7;

/// The tick colours: 0 network and the rest (grey), 1 navigation (blue), 2 dataLayer (violet),
/// 3 warning (amber), 4 error (red) — the higher wins where ticks meet.
pub fn class(e: &Entry) -> i32 {
    match (e.sev, e.kind) {
        (2, _) => 4,
        (1, _) => 3,
        (_, Kind::Dl) => 2,
        (_, Kind::Nav) => 1,
        _ => 0,
    }
}

/// The details pane's tabs for a request (ZK-191, owner 30.09: as in DevTools).
pub const TAB_HEADERS: usize = 0;
pub const TAB_PAYLOAD: usize = 1;
pub const TAB_PREVIEW: usize = 2;
pub const TAB_RESPONSE: usize = 3;
pub const TAB_TIMING: usize = 4;

/// What the details pane shows at most; the whole body goes through «Save as…».
const SHOW_MAX: usize = 256 << 10;

pub struct DevPanel {
    pub entries: Vec<Entry>,
    pub open: bool,
    pub chip: usize,
    pub query: String,
    /// Indices into `entries` the list shows, in time order.
    pub shown: Vec<usize>,
    /// The list changed (filter, search): the rows go to the window again.
    pub rows_dirty: bool,
    /// The entry whose details are open (an index into `entries`).
    pub selected: Option<usize>,
    pub tab: usize,
    /// What the ticks were laid out for: the view's offset, zoom and width.
    pub tick_key: Option<(u64, u64, i32)>,
}

impl DevPanel {
    pub fn new(log: &DevLog) -> Self {
        let entries = log
            .events
            .iter()
            .filter_map(|e| {
                let v: Value = serde_json::from_str(&e.json).ok()?;
                let kind = match v["k"].as_str()? {
                    "console" | "log" => Kind::Console,
                    "error" => Kind::Error,
                    "net" => Kind::Net,
                    "ws" => Kind::Ws,
                    "nav" => Kind::Nav,
                    "dl" => Kind::Dl,
                    _ => Kind::Info,
                };
                let sev = v["s"].as_u64().unwrap_or(0).min(2) as u8;
                Some(Entry {
                    ms: e.ms,
                    kind,
                    sev,
                    v,
                })
            })
            .collect();
        let mut p = Self {
            entries,
            open: false,
            chip: 0,
            query: String::new(),
            shown: Vec::new(),
            rows_dirty: true,
            selected: None,
            tab: TAB_HEADERS,
            tick_key: None,
        };
        p.refilter();
        p
    }

    fn in_chip(e: &Entry, chip: usize) -> bool {
        match chip {
            1 => e.sev == 2,
            2 => e.sev == 1,
            3 => matches!(e.kind, Kind::Net | Kind::Ws),
            4 => matches!(e.kind, Kind::Console | Kind::Error),
            5 => e.kind == Kind::Nav,
            6 => e.kind == Kind::Dl,
            _ => true,
        }
    }

    /// How many entries each chip holds (the search narrows them too).
    pub fn counts(&self) -> [usize; CHIPS] {
        let mut c = [0; CHIPS];
        for e in self.entries.iter().filter(|e| self.matches(e)) {
            for (i, n) in c.iter_mut().enumerate() {
                if Self::in_chip(e, i) {
                    *n += 1;
                }
            }
        }
        c
    }

    /// The search: any of the event's texts (message, address, file, payload, a text response),
    /// ignoring case.
    fn matches(&self, e: &Entry) -> bool {
        if self.query.is_empty() {
            return true;
        }
        let q = self.query.to_lowercase();
        // dataLayer: the event's name and every value in it.
        if e.kind == Kind::Dl {
            return e.v["ev"]
                .as_str()
                .is_some_and(|s| s.to_lowercase().contains(&q))
                || e.v["data"].to_string().to_lowercase().contains(&q);
        }
        let has = |k: &str| {
            e.v[k]
                .as_str()
                .is_some_and(|s| s.to_lowercase().contains(&q))
        };
        has("text")
            || has("url")
            || has("src")
            || has("postData")
            || has("data")
            || has("method")
            || (e.v["b64"] != Value::Bool(true) && has("body"))
            || e.v["status"]
                .as_u64()
                .is_some_and(|s| s.to_string().contains(&q))
    }

    pub fn refilter(&mut self) {
        self.shown = (0..self.entries.len())
            .filter(|&i| {
                let e = &self.entries[i];
                Self::in_chip(e, self.chip) && self.matches(e)
            })
            .collect();
        self.rows_dirty = true;
    }

    /// The list's row at the playhead: the last shown entry at or before `ms`.
    pub fn current(&self, ms: i32) -> Option<usize> {
        let n = self.shown.partition_point(|&i| self.entries[i].ms <= ms);
        n.checked_sub(1)
    }

    /// The time of the first error after `ms`, from the start again when there is none after.
    pub fn next_error(&self, ms: i32) -> Option<i32> {
        let errs = || self.entries.iter().filter(|e| e.sev == 2).map(|e| e.ms);
        errs().find(|&t| t > ms).or_else(|| errs().next())
    }

    /// The ticks of the log lane: one per 2 px at most, the most severe event there wins.
    pub fn ticks(&self, x_of: impl Fn(i32) -> i32, width: i32) -> Vec<(i32, i32)> {
        let mut out: Vec<(i32, i32)> = Vec::new();
        for e in &self.entries {
            let x = x_of(e.ms);
            if x < -2 || x > width + 2 {
                continue;
            }
            let c = class(e);
            match out.last_mut() {
                Some(last) if x - last.0 < 2 => last.1 = last.1.max(c),
                _ => out.push((x, c)),
            }
        }
        out
    }
}

// ------------------------------------------------------------------ rows

/// The kind column: `GET 200`, `WS ↑`, `console`, `error`, `nav`.
pub fn label(e: &Entry) -> String {
    let v = &e.v;
    match e.kind {
        Kind::Net => {
            let m = v["method"].as_str().unwrap_or("");
            if v["err"].is_string() {
                format!("{m} ✕")
            } else {
                format!("{m} {}", v["status"].as_u64().unwrap_or(0))
            }
        }
        Kind::Ws => {
            if v["dir"] == "out" {
                "WS ↑".into()
            } else {
                "WS ↓".into()
            }
        }
        Kind::Console => match v["lvl"].as_str() {
            Some("warn") | Some("warning") => "warn".into(),
            Some("error") | Some("assert") => "error".into(),
            Some(l) if !l.is_empty() => l.into(),
            _ => "log".into(),
        },
        Kind::Error => "error".into(),
        Kind::Nav => "nav".into(),
        Kind::Dl => "dataLayer".into(),
        Kind::Info => v["k"].as_str().unwrap_or("info").into(),
    }
}

/// The message column: the console's text, a request's address, a frame's data.
pub fn message(e: &Entry) -> String {
    let v = &e.v;
    let s = match e.kind {
        Kind::Net => {
            let mut s = v["url"].as_str().unwrap_or("").to_string();
            if let Some(err) = v["err"].as_str() {
                s = format!("{s} — {err}");
            }
            s
        }
        Kind::Ws => v["data"].as_str().unwrap_or("").to_string(),
        Kind::Nav => v["url"].as_str().unwrap_or("").to_string(),
        // The event's name, then what else it carries.
        Kind::Dl => {
            let mut rest = v["data"].clone();
            if let Some(o) = rest.as_object_mut() {
                o.remove("event");
            }
            let rest = match &rest {
                Value::Object(o) if o.is_empty() => String::new(),
                Value::Null => String::new(),
                r => r.to_string(),
            };
            match v["ev"].as_str().filter(|s| !s.is_empty()) {
                Some(ev) if rest.is_empty() => ev.to_string(),
                Some(ev) => format!("{ev}  {rest}"),
                None => rest,
            }
        }
        _ => v["text"]
            .as_str()
            .or_else(|| v["url"].as_str())
            .unwrap_or("")
            .to_string(),
    };
    one_line(&s, 300)
}

/// The file column: `file:line`, the last part of the address.
pub fn source(e: &Entry) -> String {
    let s = match e.kind {
        Kind::Net => e.v["dur"]
            .as_u64()
            .map(|d| format!("{d} ms"))
            .unwrap_or_default(),
        _ => e.v["src"].as_str().unwrap_or("").to_string(),
    };
    match s.rsplit_once('/') {
        Some((_, tail)) if !tail.is_empty() => tail.to_string(),
        _ => s,
    }
}

fn one_line(s: &str, max: usize) -> String {
    let mut out: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(max)
        .collect();
    if s.chars().count() > max {
        out.push('…');
    }
    out
}

// ------------------------------------------------------------------ details

/// The words of the details pane, from the app's strings.
pub struct Words {
    pub general: String,
    pub res_headers: String,
    pub req_headers: String,
    pub url: String,
    pub method: String,
    pub status: String,
    pub remote: String,
    pub protocol: String,
    pub initiator: String,
    pub cache: String,
    pub error: String,
    pub stack: String,
    pub cut: String,
    /// `{size}` replaced.
    pub binary: String,
    pub nothing: String,
    /// A dataLayer value the page held before the recording started.
    pub dl_pre: String,
    /// A dataLayer value from a frame inside the page.
    pub dl_frame: String,
}

pub struct Timing {
    pub label: &'static str,
    /// Where the bar starts and how long it is, as parts of the whole request.
    pub a: f32,
    pub w: f32,
    pub ms: String,
}

/// What the pane shows for an entry: tabs (a request only) and which of them have data; the
/// text of the open tab; the timing bars; the body to save, if any.
pub struct Detail {
    pub tabs: bool,
    pub has: [bool; 5],
    pub text: String,
    pub timing: Vec<Timing>,
    pub can_save: bool,
}

pub fn detail(e: &Entry, tab: usize, w: &Words) -> Detail {
    let v = &e.v;
    let mut d = Detail {
        tabs: e.kind == Kind::Net,
        has: [false; 5],
        text: String::new(),
        timing: Vec::new(),
        can_save: false,
    };
    match e.kind {
        Kind::Net => {
            let body = v["body"].as_str();
            d.has = [
                true,
                v["postData"].is_string(),
                body.is_some(),
                body.is_some(),
                v["timing"].is_object() || v["dur"].is_u64(),
            ];
            d.can_save = body.is_some();
            d.text = match tab {
                TAB_PAYLOAD => v["postData"].as_str().map(pretty).unwrap_or_default(),
                TAB_PREVIEW => body.map(|b| preview(v, b, w)).unwrap_or_default(),
                TAB_RESPONSE => body.map(|b| response(v, b, w)).unwrap_or_default(),
                TAB_TIMING => String::new(),
                _ => headers(v, w),
            };
            if tab == TAB_TIMING {
                d.timing = timing(v);
            }
        }
        Kind::Ws => {
            d.text = v["data"].as_str().map(pretty).unwrap_or_default();
        }
        Kind::Nav => d.text = v["url"].as_str().unwrap_or("").to_string(),
        Kind::Dl => {
            let mut s = String::new();
            if v["pre"] == Value::Bool(true) {
                s.push_str(&w.dl_pre);
                s.push_str("\n\n");
            }
            if v["frame"] == Value::Bool(true) {
                s.push_str(&w.dl_frame);
                s.push_str("\n\n");
            }
            match &v["data"] {
                // Cut by the extension's limit: the JSON as it came.
                Value::String(raw) if v["cut"].is_u64() => {
                    s.push_str(raw);
                    s.push_str(&format!(
                        "\n\n… {}",
                        size_text(v["cut"].as_u64().unwrap_or(0) as usize)
                    ));
                }
                data => s.push_str(&serde_json::to_string_pretty(data).unwrap_or_default()),
            }
            d.text = s;
        }
        _ => {
            let mut s = v["text"].as_str().unwrap_or("").to_string();
            // The console's arguments as objects, when they say more than the joined line.
            if let Some(args) = v["args"].as_array()
                && args.len() > 1
            {
                for a in args {
                    s.push_str("\n\n");
                    s.push_str(&a.as_str().map(pretty).unwrap_or_default());
                }
            }
            if let Some(src) = v["src"].as_str().filter(|s| !s.is_empty()) {
                s.push_str(&format!("\n\n{src}"));
            }
            if let Some(st) = v["stack"].as_str().filter(|s| !s.is_empty()) {
                s.push_str(&format!("\n\n{}\n{st}", w.stack));
            } else if let Some(st) = v["stack"].as_array().filter(|a| !a.is_empty()) {
                s.push_str(&format!("\n\n{}", w.stack));
                for f in st {
                    s.push('\n');
                    s.push_str(&frame_line(f));
                }
            }
            d.text = s;
        }
    }
    if d.text.len() > SHOW_MAX {
        let mut cut = SHOW_MAX;
        while !d.text.is_char_boundary(cut) {
            cut -= 1;
        }
        d.text.truncate(cut);
        d.text.push_str("\n\n");
        d.text.push_str(&w.cut);
    }
    if d.text.is_empty() && d.timing.is_empty() {
        d.text = w.nothing.clone();
    }
    d
}

/// A frame of a call stack as DevTools writes it: `at fn (url:line:col)`.
fn frame_line(f: &Value) -> String {
    if let Some(s) = f.as_str() {
        return s.to_string();
    }
    let func = f["fn"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or("(anonymous)");
    let url = f["url"].as_str().unwrap_or("");
    match (f["line"].as_i64(), f["col"].as_i64()) {
        (Some(l), Some(c)) => format!("at {func} ({url}:{l}:{c})"),
        (Some(l), None) => format!("at {func} ({url}:{l})"),
        _ if !url.is_empty() => format!("at {func} ({url})"),
        _ => format!("at {func}"),
    }
}

fn headers(v: &Value, w: &Words) -> String {
    let mut s = format!("{}\n", w.general);
    let mut row = |k: &str, val: String| {
        if !val.is_empty() {
            s.push_str(&format!("  {k}: {val}\n"));
        }
    };
    row(&w.url, v["url"].as_str().unwrap_or("").into());
    row(&w.method, v["method"].as_str().unwrap_or("").into());
    let status = v["status"].as_u64().unwrap_or(0);
    if status > 0 {
        row(
            &w.status,
            format!("{status} {}", v["statusText"].as_str().unwrap_or(""))
                .trim_end()
                .into(),
        );
    }
    row(&w.remote, v["remote"].as_str().unwrap_or("").into());
    row(&w.protocol, v["proto"].as_str().unwrap_or("").into());
    if let Some(i) = v["initiator"].as_object() {
        let mut t = i
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        if let Some(u) = i.get("url").and_then(|u| u.as_str()) {
            t.push(' ');
            t.push_str(u);
            if let Some(l) = i.get("line").and_then(|l| l.as_i64()) {
                t.push_str(&format!(":{}", l + 1));
            }
        }
        row(&w.initiator, t);
    }
    if v["cache"] == Value::Bool(true) {
        row(&w.cache, "✓".into());
    }
    if let Some(r) = v["redirect"].as_str() {
        row("→", r.into());
    }
    row(&w.error, v["err"].as_str().unwrap_or("").into());
    for (title, key) in [
        (&w.res_headers, "resHeaders"),
        (&w.req_headers, "reqHeaders"),
    ] {
        if let Some(h) = v[key].as_object().filter(|h| !h.is_empty()) {
            s.push_str(&format!("\n{title}\n"));
            let mut keys: Vec<_> = h.iter().collect();
            keys.sort_by_key(|(k, _)| k.to_lowercase());
            for (k, val) in keys {
                s.push_str(&format!("  {k}: {}\n", val.as_str().unwrap_or("")));
            }
        }
    }
    s
}

/// JSON pretty-printed; a form's fields one per line; else the text as it is.
pub fn pretty(s: &str) -> String {
    let t = s.trim_start();
    if (t.starts_with('{') || t.starts_with('['))
        && let Ok(j) = serde_json::from_str::<Value>(s)
    {
        return serde_json::to_string_pretty(&j).unwrap_or_else(|_| s.to_string());
    }
    if !s.is_empty()
        && s.contains('=')
        && !s.contains(char::is_whitespace)
        && s.split('&').all(|p| p.contains('='))
    {
        return s
            .split('&')
            .map(|p| {
                let (k, v) = p.split_once('=').unwrap_or((p, ""));
                format!("{}: {}", url_decode(k), url_decode(v))
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    s.to_string()
}

fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => match s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
            {
                Some(x) => {
                    out.push(x);
                    i += 2;
                }
                None => out.push(b'%'),
            },
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn size_text(n: usize) -> String {
    if n >= 1 << 20 {
        format!("{:.1} MB", n as f64 / (1 << 20) as f64)
    } else if n >= 1 << 10 {
        format!("{:.0} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

fn binary_note(v: &Value, b: &str, w: &Words) -> String {
    let n = v["bodyCut"].as_u64().map(|n| n as usize).unwrap_or(b.len()) / 4 * 3;
    w.binary.replace("{size}", &size_text(n))
}

/// Preview: JSON as a tree (pretty), text as it is; binary — its size.
fn preview(v: &Value, b: &str, w: &Words) -> String {
    if v["b64"] == Value::Bool(true) {
        return binary_note(v, b, w);
    }
    pretty(b)
}

/// Response: the body as the server sent it.
fn response(v: &Value, b: &str, w: &Words) -> String {
    if v["b64"] == Value::Bool(true) {
        return binary_note(v, b, w);
    }
    let mut s = b.to_string();
    if let Some(n) = v["bodyCut"].as_u64() {
        s.push_str(&format!("\n\n… {}", size_text(n as usize)));
    }
    s
}

/// The phases of a request from the CDP `ResourceTiming` (ms after `requestTime`; -1 when a
/// phase did not happen) and the whole duration.
fn timing(v: &Value) -> Vec<Timing> {
    let total = v["dur"].as_f64().unwrap_or(0.0).max(1.0);
    let t = &v["timing"];
    let at = |k: &str| t[k].as_f64().filter(|x| *x >= 0.0);
    let mut out = Vec::new();
    let mut push = |label: &'static str, a: f64, b: f64| {
        if b > a {
            out.push(Timing {
                label,
                a: (a / total).clamp(0.0, 1.0) as f32,
                w: ((b - a) / total).clamp(0.002, 1.0) as f32,
                ms: format!("{:.1} ms", b - a),
            });
        }
    };
    let first = [at("dnsStart"), at("connectStart"), at("sendStart")]
        .into_iter()
        .flatten()
        .fold(f64::NAN, f64::min);
    if first.is_finite() {
        push("queue", 0.0, first);
    }
    if let (Some(a), Some(b)) = (at("dnsStart"), at("dnsEnd")) {
        push("dns", a, b);
    }
    if let (Some(a), Some(b)) = (at("connectStart"), at("connectEnd")) {
        push("connect", a, b);
    }
    if let (Some(a), Some(b)) = (at("sslStart"), at("sslEnd")) {
        push("tls", a, b);
    }
    if let (Some(a), Some(b)) = (at("sendStart"), at("sendEnd")) {
        push("send", a, b);
    }
    if let (Some(a), Some(b)) = (at("sendEnd"), at("receiveHeadersEnd")) {
        push("wait", a, b);
    }
    if let Some(a) = at("receiveHeadersEnd") {
        push("download", a, total);
    }
    push("total", 0.0, total);
    out
}

/// The response's bytes for «Save as…»: text as UTF-8, base64 decoded.
pub fn body_bytes(e: &Entry) -> Option<Vec<u8>> {
    let b = e.v["body"].as_str()?;
    if e.v["b64"] == Value::Bool(true) {
        base64_decode(b)
    } else {
        Some(b.as_bytes().to_vec())
    }
}

/// A file name for a saved body: the address's last part, else `response`, with an extension
/// from the MIME type when it has none.
pub fn body_name(e: &Entry) -> String {
    let url = e.v["url"].as_str().unwrap_or("");
    let path = url.split(['?', '#']).next().unwrap_or("");
    let mut name: String = path
        .rsplit('/')
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| !r#"\/:*?"<>|"#.contains(*c))
        .collect();
    if name.is_empty() {
        name = "response".into();
    }
    if !name.contains('.') {
        let mime = e.v["mime"].as_str().unwrap_or("");
        let ext = match mime {
            m if m.contains("json") => "json",
            m if m.contains("html") => "html",
            m if m.contains("javascript") => "js",
            m if m.contains("css") => "css",
            m if m.contains("xml") => "xml",
            m if m.starts_with("text/") => "txt",
            _ => "bin",
        };
        name = format!("{name}.{ext}");
    }
    name
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    };
    let clean: Vec<u8> = s
        .bytes()
        .filter(|c| !c.is_ascii_whitespace() && *c != b'=')
        .collect();
    let mut out = Vec::with_capacity(clean.len() / 4 * 3 + 3);
    for chunk in clean.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        let bytes = n.to_be_bytes();
        out.extend_from_slice(&bytes[1..chunk.len()]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use znimok_format::video::DevEvent;

    fn log() -> DevLog {
        let ev = |ms: i32, j: &str| DevEvent {
            ms,
            json: j.to_string(),
        };
        DevLog {
            wall0_ms: 0,
            events: vec![
                ev(100, r#"{"k":"nav","s":0,"url":"https://example.org/"}"#),
                ev(
                    300,
                    r#"{"k":"net","s":0,"method":"POST","url":"https://example.org/api/save?x=1","status":200,"mime":"application/json","postData":"{\"a\":1}","body":"{\"ok\":true}","dur":40,"timing":{"dnsStart":1,"dnsEnd":3,"sendStart":5,"sendEnd":6,"receiveHeadersEnd":30},"reqHeaders":{"Accept":"*/*"}}"#,
                ),
                ev(
                    500,
                    r#"{"k":"console","s":1,"lvl":"warn","text":"slow","src":"https://example.org/app.js:12"}"#,
                ),
                ev(
                    900,
                    r#"{"k":"error","s":2,"text":"TypeError: x is undefined","src":"https://example.org/app.js:40","stack":"at f (app.js:40)"}"#,
                ),
                ev(
                    700,
                    r#"{"k":"dl","s":0,"ev":"purchase","data":{"event":"purchase","value":42,"user":{"email":"a@example.org"}}}"#,
                ),
                ev(
                    50,
                    r#"{"k":"dl","s":0,"ev":"gtag config G-1","data":["config","G-1"],"pre":true}"#,
                ),
                ev(950, "not json"),
            ],
        }
    }

    #[test]
    fn chips_search_and_the_playhead() {
        let mut p = DevPanel::new(&log());
        // In time order as the log keeps them (the fixture is sorted by DEVT already; here the
        // pre-existing push at 50 ms comes first).
        p.entries.sort_by_key(|e| e.ms);
        p.refilter();
        assert_eq!(p.entries.len(), 6);
        assert_eq!(p.counts(), [6, 1, 1, 1, 2, 1, 2]);
        p.chip = 3;
        p.refilter();
        assert_eq!(p.shown, vec![2]);
        p.chip = 6;
        p.refilter();
        assert_eq!(p.shown, vec![0, 4]);
        // The search reaches into a push's values.
        p.chip = 0;
        p.query = "a@example".into();
        p.refilter();
        assert_eq!(p.shown, vec![4]);
        p.chip = 0;
        p.query = "TYPEERROR".into();
        p.refilter();
        assert_eq!(p.shown, vec![5]);
        // The search reaches into a request's payload too.
        p.query = "\"a\"".into();
        p.refilter();
        assert_eq!(p.shown, vec![2]);
        p.query.clear();
        p.refilter();
        assert_eq!(p.current(10), None);
        assert_eq!(p.current(300), Some(2));
        assert_eq!(p.current(10_000), Some(5));
        assert_eq!(p.next_error(0), Some(900));
        assert_eq!(p.next_error(900), Some(900));
    }

    #[test]
    fn ticks_merge_and_keep_the_worst() {
        let mut p = DevPanel::new(&log());
        p.entries.sort_by_key(|e| e.ms);
        // All the events within a pixel: one tick, red.
        assert_eq!(p.ticks(|ms| ms / 1000, 100), vec![(0, 4)]);
        let t = p.ticks(|ms| ms / 10, 100);
        assert_eq!(t, vec![(5, 2), (10, 1), (30, 0), (50, 3), (70, 2), (90, 4)]);
    }

    #[test]
    fn a_request_in_tabs() {
        let mut p = DevPanel::new(&log());
        p.entries.sort_by_key(|e| e.ms);
        let w = Words {
            general: "General".into(),
            res_headers: "Response headers".into(),
            req_headers: "Request headers".into(),
            url: "URL".into(),
            method: "Method".into(),
            status: "Status".into(),
            remote: "Remote".into(),
            protocol: "Protocol".into(),
            initiator: "Initiator".into(),
            cache: "Cache".into(),
            error: "Error".into(),
            stack: "Stack".into(),
            cut: "cut".into(),
            binary: "binary {size}".into(),
            nothing: "-".into(),
            dl_pre: "before".into(),
            dl_frame: "frame".into(),
        };
        let e = &p.entries[2];
        assert_eq!(label(e), "POST 200");
        assert_eq!(source(e), "40 ms");
        let h = detail(e, TAB_HEADERS, &w);
        assert!(h.tabs && h.has == [true; 5]);
        assert!(h.text.contains("Method: POST") && h.text.contains("Accept: */*"));
        assert_eq!(detail(e, TAB_PAYLOAD, &w).text, "{\n  \"a\": 1\n}");
        assert_eq!(detail(e, TAB_RESPONSE, &w).text, "{\"ok\":true}");
        let t = detail(e, TAB_TIMING, &w).timing;
        let labels: Vec<_> = t.iter().map(|t| t.label).collect();
        assert_eq!(
            labels,
            ["queue", "dns", "send", "wait", "download", "total"]
        );
        assert_eq!(body_name(e), "save.json");
        assert_eq!(body_bytes(e).unwrap(), b"{\"ok\":true}");
        let err = detail(&p.entries[5], 0, &w);
        assert!(!err.tabs && err.text.contains("Stack\nat f (app.js:40)"));
        assert_eq!(source(&p.entries[5]), "app.js:40");
        // dataLayer: the event and its values in the row; the JSON laid out; «before» marked.
        let buy = &p.entries[4];
        assert_eq!(label(buy), "dataLayer");
        assert_eq!(
            message(buy),
            r#"purchase  {"user":{"email":"a@example.org"},"value":42}"#
        );
        assert!(detail(buy, 0, &w).text.contains("\"value\": 42"));
        let pre = detail(&p.entries[0], 0, &w).text;
        assert!(pre.starts_with("before\n\n["), "{pre}");
        // A stack as the extension sends it.
        assert_eq!(
            frame_line(
                &serde_json::json!({"fn": "", "url": "https://x/app.js", "line": 3, "col": 7})
            ),
            "at (anonymous) (https://x/app.js:3:7)"
        );
    }

    #[test]
    fn forms_and_base64() {
        assert_eq!(pretty("a=1&b=x%20y+z"), "a: 1\nb: x y z");
        assert_eq!(pretty("plain text"), "plain text");
        assert_eq!(base64_decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64_decode("aGk").unwrap(), b"hi");
    }
}
