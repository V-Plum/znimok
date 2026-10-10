//! Recording in the app (ZK-180) and its indicators (ZK-91).
//!
//! The recording hotkey (or «Record video» in the tray) opens the capture overlay in its video
//! mode; what is chosen there — a region, a window, the whole screen — becomes the recording's
//! target. While it runs: a thin red edge around the recorded part (outside it, so it is never
//! in the video, the mouse goes through), a control bar with the time, «Pause» and «Stop»
//! (kept out of every capture), the time in the tray. The same hotkey stops it.
//!
//! The MP4 is written to the cache as `.part` by `znimok-video-win` (Windows) or
//! `znimok-video-mac` (macOS, ZK-88); once it is complete it is wrapped into a video document
//! (poster = the first frame, a thumbnail for the library) on a worker thread and moved into the
//! library; the card after a capture says so.
//!
//! On macOS ScreenCaptureKit draws the pointer and its clicks itself (the settings' switches)
//! and leaves this app's own windows out of a display's capture; the clicks are not logged.
#![cfg_attr(
    not(any(windows, target_os = "macos")),
    allow(dead_code, unused_imports, unused_variables)
)]

use std::cell::RefCell;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use slint::ComponentHandle;
use znimok_platform::Rect;

/// What the overlay chose, in desktop units (pixels on Windows).
#[derive(Clone, Debug)]
pub struct Choice {
    /// The display the recording is on (its bounds), for the indicators.
    pub display: Rect,
    /// The part recorded (a region, the window, the whole display).
    pub frame: Rect,
    /// A window chosen by a click (followed while it moves, when the settings say so).
    pub window: Option<u64>,
    /// "region", "window", "screen" — the document's source.
    pub source: &'static str,
}

thread_local! {
    static REC: RefCell<Option<Active>> = const { RefCell::new(None) };
    /// The tray (main keeps it; its menu and tooltip follow the recording).
    pub static TRAY: RefCell<Option<slint::Weak<crate::AppTray>>> = const { RefCell::new(None) };
    /// The recording overlay was asked for: its choice starts a recording, not a shot.
    static VIDEO_MODE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// What an agent's recording (ZK-237) sets itself, whatever the settings say: sound only with
/// its own permission, the browser's log or not.
#[derive(Clone, Copy, Debug)]
pub struct AgentOpts {
    pub system: bool,
    pub microphone: bool,
    pub log: bool,
}

thread_local! {
    /// Set while an agent's recording is being started.
    static AGENT: std::cell::Cell<Option<AgentOpts>> = const { std::cell::Cell::new(None) };
}

fn agent_opts() -> Option<AgentOpts> {
    AGENT.with(|a| a.get())
}

/// The video settings of the recording being started: an agent's own sound choice, and the
/// window it named is the target (not the region it covers now).
fn agent_video(mut p: znimok_settings::Video) -> znimok_settings::Video {
    if let Some(o) = agent_opts() {
        p.audio.system = o.system;
        p.audio.microphone = o.microphone;
        p.follow_window = true;
    }
    p
}

/// Starts a recording for an agent: no overlay, no questions; the reason comes back.
pub fn start_for_agent(choice: Choice, opts: AgentOpts) -> Result<(), String> {
    if is_recording() {
        return Err("a recording is already running".into());
    }
    AGENT.with(|a| a.set(Some(opts)));
    let r = start_inner(&choice);
    AGENT.with(|a| a.set(None));
    if r.is_ok() {
        crate::commands::emit("recordStart");
    }
    r
}

/// How long the running recording is, without its pauses.
pub fn elapsed_ms() -> Option<u64> {
    REC.with(|r| r.borrow().as_ref().map(|a| a.elapsed().as_millis() as u64))
}

struct Active {
    #[cfg(windows)]
    rec: Option<znimok_video_win::Recording>,
    #[cfg(target_os = "macos")]
    rec: Option<znimok_video_mac::Recording>,
    /// The cursor and the clicks (ZK-90): the hook lives as long as the recording.
    #[cfg(windows)]
    mouse: Option<znimok_video_win::MouseInput>,
    /// The MP4 being written (it becomes the document's stream).
    mp4: PathBuf,
    size: (u32, u32),
    fps: u32,
    source: &'static str,
    display: Rect,
    frame: Rect,
    window: Option<u64>,
    started: Instant,
    paused_since: Option<Instant>,
    paused_total: Duration,
    bar: crate::RecBar,
    edges: Vec<crate::RecEdge>,
    timer: slint::Timer,
}

impl Active {
    fn elapsed(&self) -> Duration {
        let now = self.paused_since.unwrap_or_else(Instant::now);
        now.saturating_duration_since(self.started)
            .saturating_sub(self.paused_total)
    }
}

/// The recording is paused.
pub fn is_paused() -> bool {
    REC.with(|r| {
        r.borrow()
            .as_ref()
            .is_some_and(|a| a.paused_since.is_some())
    })
}

pub fn is_recording() -> bool {
    REC.with(|r| r.borrow().is_some())
}

/// The next overlay starts a recording (the hotkey, the tray).
pub fn set_video_mode(on: bool) {
    VIDEO_MODE.with(|m| m.set(on));
}

/// Taken once by the overlay when its choice is made.
pub fn take_video_mode() -> bool {
    VIDEO_MODE.with(|m| m.replace(false))
}

/// The overlay being opened is for a recording (it shows the sound choice, ZK-189).
pub fn video_mode() -> bool {
    VIDEO_MODE.with(|m| m.get())
}

/// The sound of the next recording: 0 none, 1 system, 2 microphone, 3 both.
pub fn sound_mode() -> i32 {
    crate::with_prefs(|p| p.video.audio.system as i32 | (p.video.audio.microphone as i32) << 1)
        .unwrap_or(0)
}

/// The next sound choice, kept in the settings (A in the recording overlay, ZK-189).
pub fn cycle_sound() -> i32 {
    let next = (sound_mode() + 1) % 4;
    if crate::try_with_ctx(|a, ui| a.setting(ui, "rec-sound", next)) {
        next
    } else {
        sound_mode()
    }
}

/// «sound: system» for the overlay's hint strip.
pub fn sound_text(mode: i32) -> String {
    let mut out = String::new();
    // Never a second borrow of the app (ZK-212): busy → no label this time.
    crate::try_with_ctx(|a, _| {
        let key = match mode {
            1 => "rec-system-audio",
            2 => "rec-microphone",
            3 => "vid-sound-both",
            _ => "vid-no-sound",
        };
        out = a.tr.tr_args(
            "capture-sound",
            &crate::app::fargs(&[("mode", a.tr.tr(key).to_lowercase())]),
        );
    });
    out
}

/// The recording hotkey / tray item: stop the running recording, or open the overlay to choose
/// what to record.
pub fn toggle(start_overlay: impl FnOnce()) {
    if is_recording() {
        stop();
        return;
    }
    if cfg!(not(any(windows, target_os = "macos"))) {
        crate::with_ctx(|a, ui| {
            let msg = a.tr.tr("rec-not-here");
            a.toast(ui, msg);
            crate::show_window(ui);
        });
        return;
    }
    if crate::overlay::is_open() || !crate::capture::available() {
        return;
    }
    set_video_mode(true);
    start_overlay();
}

/// Starts recording what the overlay chose.
pub fn start(choice: Choice) {
    if is_recording() {
        return;
    }
    match start_inner(&choice) {
        Ok(()) => crate::commands::emit("recordStart"),
        Err(reason) => {
            crate::commands::emit("failed");
            crate::wins::come_back();
            crate::with_ctx(|a, ui| {
                let msg =
                    a.tr.tr_args("rec-error-start", &crate::app::fargs(&[("reason", reason)]));
                a.toast(ui, msg);
                crate::show_window(ui);
            });
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn start_inner(_choice: &Choice) -> Result<(), String> {
    Err("no recording on this system".into())
}

/// macOS (ZK-88): a display (whole or a region of it, in points) or a window, through
/// ScreenCaptureKit; sound and the pointer as the settings say.
#[cfg(target_os = "macos")]
fn start_inner(choice: &Choice) -> Result<(), String> {
    use znimok_platform::Capture;
    use znimok_video_mac::{RecordRequest, Recording, Target};
    let p = agent_video(crate::with_prefs(|p| p.video.clone()).unwrap_or_default());
    let target = match (choice.window, p.follow_window) {
        (Some(id), true) => Target::Window { id: id as u32 },
        _ => {
            let displays = znimok_mac::MacCapture::new()
                .displays()
                .map_err(|e| e.to_string())?;
            let d = displays
                .iter()
                .find(|d| d.bounds == choice.display)
                .or_else(|| displays.iter().find(|d| d.primary))
                .ok_or_else(|| "no display".to_string())?;
            let id: u32 = d.id.0.parse().map_err(|_| "no display".to_string())?;
            // The whole display, give or take a unit of rounding.
            let near = |a: i64, b: i64| (a - b).abs() <= 1;
            let (f, b) = (choice.frame, d.bounds);
            let whole = near(f.x.into(), b.x.into())
                && near(f.y.into(), b.y.into())
                && near(f.width.into(), b.width.into())
                && near(f.height.into(), b.height.into());
            Target::Display {
                id,
                region: (!whole).then(|| {
                    (
                        f64::from(choice.frame.x - d.bounds.x),
                        f64::from(choice.frame.y - d.bounds.y),
                        f64::from(choice.frame.width),
                        f64::from(choice.frame.height),
                    )
                }),
            }
        }
    };
    let dir = crate::library::cache_dir().join("recordings");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mp4 = dir.join(format!(
        "rec-{}.mp4",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    ));
    let mut req = RecordRequest::new(target, mp4.clone());
    let fps = if p.fps >= 45 { 60 } else { 30 };
    req.fps = fps;
    req.quality = match p.quality {
        znimok_settings::Quality::Small => znimok_video::settings::Quality::Smaller,
        znimok_settings::Quality::Normal => znimok_video::settings::Quality::Normal,
        znimok_settings::Quality::High => znimok_video::settings::Quality::High,
    };
    req.system_audio = p.audio.system;
    req.microphone = p.audio.microphone;
    req.cursor = p.cursor;
    req.clicks = p.clicks;
    let rec = Recording::start(req).map_err(|e| e.to_string())?;
    let size = rec.started().size;
    // What was asked of the system and what came (ZK-295): the facts to read a wrong picture by.
    tracing::info!(started = ?rec.started(), source = choice.source, frame = ?choice.frame,
        display = ?choice.display, "recording: started");
    if rec.started().region_clipped {
        tracing::warn!(frame = ?choice.frame, display = ?choice.display,
            "recording: the region reached outside its display and was clipped to it");
    }
    let bar = crate::RecBar::new().map_err(|e| e.to_string())?;
    let edges = if choice.source == "screen" {
        Vec::new()
    } else {
        (0..4).filter_map(|_| crate::RecEdge::new().ok()).collect()
    };
    bar.on_pause(toggle_pause);
    bar.on_stop(stop);
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(250), tick);
    REC.with(|r| {
        *r.borrow_mut() = Some(Active {
            rec: Some(rec),
            mp4,
            size,
            fps,
            source: choice.source,
            display: choice.display,
            frame: choice.frame,
            window: choice.window.filter(|_| p.follow_window),
            started: Instant::now(),
            paused_since: None,
            paused_total: Duration::ZERO,
            bar,
            edges,
            timer,
        })
    });
    show_indicators();
    // The browser's log (ZK-97): the extension starts writing now (an agent may leave it out).
    if agent_opts().is_none_or(|o| o.log) {
        crate::devlog::hub().start(znimok_devtools::now_ms());
    }
    tick();
    Ok(())
}

#[cfg(windows)]
fn start_inner(choice: &Choice) -> Result<(), String> {
    use znimok_platform::WindowId;
    use znimok_video_win::{RecordRequest, Recording, Target};
    let p = agent_video(crate::with_prefs(|p| p.video.clone()).unwrap_or_default());
    // A clicked window is followed as it moves — or recorded as the region it covers now.
    let target = match (choice.window, p.follow_window) {
        (Some(id), true) => Target::Window { id: WindowId(id) },
        _ => {
            let m = znimok_win::raw::monitors()
                .into_iter()
                .find(|m| m.info.bounds == choice.display)
                .or_else(|| znimok_win::raw::monitors().into_iter().next())
                .ok_or_else(|| "no display".to_string())?;
            let whole = choice.frame == m.info.bounds;
            Target::Display {
                id: m.info.id.clone(),
                region: (!whole).then(|| Rect {
                    x: choice.frame.x - m.info.bounds.x,
                    y: choice.frame.y - m.info.bounds.y,
                    ..choice.frame
                }),
            }
        }
    };
    let dir = crate::library::cache_dir().join("recordings");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mp4 = dir.join(format!(
        "rec-{}.mp4",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    ));
    let mut req = RecordRequest::new(target, mp4.clone());
    req.fps = if p.fps >= 45 { 60 } else { 30 };
    req.quality = match p.quality {
        znimok_settings::Quality::Small => znimok_video::settings::Quality::Smaller,
        znimok_settings::Quality::Normal => znimok_video::settings::Quality::Normal,
        znimok_settings::Quality::High => znimok_video::settings::Quality::High,
    };
    // Sound (ZK-89): each chosen source is a track of its own; a source that cannot open is
    // left out and the recording says why when it ends.
    if p.audio.system {
        req.audio
            .push(Box::new(znimok_video_win::WasapiSource::system(
                p.audio.system_device.clone(),
            )));
    }
    if p.audio.microphone {
        req.audio
            .push(Box::new(znimok_video_win::WasapiSource::microphone(
                p.audio.microphone_device.clone(),
            )));
    }
    // The cursor and the clicks (ZK-90): drawn into the frames, and the clicks logged.
    let mouse = (p.cursor || p.clicks).then(znimok_video_win::MouseInput::start);
    if let Some(m) = &mouse {
        let color = match p.click_color {
            znimok_settings::ClickColor::Yellow => [1.0, 0.82, 0.25],
            znimok_settings::ClickColor::Red => [1.0, 0.35, 0.37],
            znimok_settings::ClickColor::Blue => [0.24, 0.48, 0.96],
        };
        req.events = Some(m.queue());
        req.overlay = Some(m.overlay(znimok_video_win::MouseOpts {
            cursor: p.cursor,
            clicks: p.clicks,
            color,
        }));
    }
    let rec = Recording::start(req).map_err(|e| e.to_string())?;
    let size = rec.started().size;
    let fps = if p.fps >= 45 { 60 } else { 30 };
    let bar = crate::RecBar::new().map_err(|e| e.to_string())?;
    let edges = if choice.source == "screen" {
        Vec::new()
    } else {
        (0..4).filter_map(|_| crate::RecEdge::new().ok()).collect()
    };
    bar.on_pause(toggle_pause);
    bar.on_stop(stop);
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(250), tick);
    REC.with(|r| {
        *r.borrow_mut() = Some(Active {
            rec: Some(rec),
            mouse,
            mp4,
            size,
            fps,
            source: choice.source,
            display: choice.display,
            frame: choice.frame,
            window: choice.window.filter(|_| p.follow_window),
            started: Instant::now(),
            paused_since: None,
            paused_total: Duration::ZERO,
            bar,
            edges,
            timer,
        })
    });
    show_indicators();
    // The browser's log (ZK-97): the extension starts writing now (an agent may leave it out).
    if agent_opts().is_none_or(|o| o.log) {
        crate::devlog::hub().start(znimok_devtools::now_ms());
    }
    // Clicks on the bar (Pause, Stop) are not the recording's.
    REC.with(|r| {
        if let Some(a) = r.borrow().as_ref()
            && let (Some(m), Some(h)) = (a.mouse.as_ref(), hwnd_of(a.bar.window()))
        {
            m.exclude(h);
        }
    });
    tick();
    Ok(())
}

/// A press into the running recording as if the mouse made it (the self-test, ZK-90).
#[cfg(windows)]
pub fn note_click(x: i32, y: i32, down: bool) {
    REC.with(|r| {
        if let Some(m) = r.borrow().as_ref().and_then(|a| a.mouse.as_ref()) {
            m.note(x, y, znimok_video::events::Button::Left, down);
        }
    });
}

/// A top-level window's handle (for the hook to leave its clicks out).
#[cfg(windows)]
fn hwnd_of(w: &slint::Window) -> Option<isize> {
    use slint::winit_030::WinitWindowAccessor;
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    w.with_winit_window(|w| match w.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    })
    .flatten()
}

/// Pause / resume (the bar's button).
pub fn toggle_pause() {
    REC.with(|r| {
        let mut r = r.borrow_mut();
        let Some(a) = r.as_mut() else { return };
        #[cfg(any(windows, target_os = "macos"))]
        if let Some(rec) = &a.rec {
            rec.control().toggle_pause();
        }
        match a.paused_since.take() {
            Some(t) => a.paused_total += t.elapsed(),
            None => a.paused_since = Some(Instant::now()),
        }
        let paused = a.paused_since.is_some();
        crate::commands::emit(if paused {
            "recordPause"
        } else {
            "recordResume"
        });
        crate::devlog::hub().pause(paused);
        a.bar.set_paused(paused);
        for e in &a.edges {
            e.set_paused(paused);
        }
    });
    tick();
}

/// Stop: the indicators go at once; the file is finished and wrapped on a worker thread.
pub fn stop() {
    #[cfg_attr(not(any(windows, target_os = "macos")), allow(unused_mut))]
    let Some(mut a) = REC.with(|r| r.borrow_mut().take()) else {
        return;
    };
    a.timer.stop();
    let _ = a.bar.hide();
    for e in &a.edges {
        let _ = e.hide();
    }
    set_tray(false, String::new());
    // The browser's log ends with the recording (ZK-97).
    let devlog = crate::devlog::hub().stop();
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = devlog;
    #[cfg(any(windows, target_os = "macos"))]
    {
        // The hook goes first: nothing after Stop is the recording's.
        #[cfg(windows)]
        if let Some(mut m) = a.mouse.take() {
            m.stop();
        }
        let Some(rec) = a.rec.take() else { return };
        let lib = crate::with_lib_dir().unwrap_or_else(crate::library::default_dir);
        let name = doc_name();
        let (mp4, size, fps, source, display) = (a.mp4.clone(), a.size, a.fps, a.source, a.display);
        std::thread::spawn(move || {
            let fin = rec.stop();
            LAST_FRAMES.store(fin.result.frames, std::sync::atomic::Ordering::SeqCst);
            let warning = fin.result.warning;
            let r = match fin.path {
                Some(p) => wrap(&p, &lib, &name, size, fps, source, &fin.result, devlog),
                // Nothing came: WGC sends a window's frame only when it is drawn anew, so a
                // window that did not change for the whole recording gives no video.
                None => Err(fin
                    .result
                    .error
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| NOTHING.into())),
            };
            let _ = std::fs::remove_file(&mp4);
            let _ = slint::invoke_from_event_loop(move || {
                saved(r, name, display);
                sound_warning(warning);
            });
        });
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = a;
}

fn doc_name() -> String {
    let now = chrono::Local::now();
    let mut name = String::new();
    crate::with_ctx(|a, _| {
        let (date, time) = (
            now.format("%Y-%m-%d").to_string(),
            now.format("%H.%M.%S").to_string(),
        );
        // The person's own word before the date, when set (ZK-221).
        let prefix = a.prefs().library.video_prefix.trim().to_string();
        name = if prefix.is_empty() {
            a.tr.tr_args(
                "rec-doc-name",
                &crate::app::fargs(&[("date", date), ("time", time)]),
            )
        } else {
            format!("{prefix} {date} {time}")
        };
    });
    name
}

/// The first frame of a finished recording (its poster).
#[cfg(windows)]
fn first_frame(mp4: &std::path::Path) -> Result<znimok_core::Raster, String> {
    use znimok_video::traits::{Decoded, VideoDecoder};
    let mut dec = znimok_video_win::MfDecoder::open(mp4).map_err(|e| e.to_string())?;
    loop {
        match dec.next().map_err(|e| e.to_string())? {
            Some(Decoded::Video { frame, .. }) => return Ok(crate::video::nv12_to_rgba(&frame)),
            Some(_) => continue,
            None => return Err("no frames".into()),
        }
    }
}

/// The first frame of a finished recording (its poster).
#[cfg(target_os = "macos")]
fn first_frame(mp4: &std::path::Path) -> Result<znimok_core::Raster, String> {
    let (w, h, rgba) = znimok_video_mac::poster::first_frame(mp4)?;
    Ok(znimok_core::Raster::new(w, h, rgba))
}

/// The finished MP4 → a video document in the library: the first frame is the poster, a
/// thumbnail for the card; the stream is copied in, never held in memory whole.
#[cfg(any(windows, target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
fn wrap(
    mp4: &std::path::Path,
    lib: &std::path::Path,
    name: &str,
    size: (u32, u32),
    fps: u32,
    source: &str,
    result: &znimok_video::recorder::RecordingResult,
    devlog: Option<znimok_format::video::DevLog>,
) -> Result<(PathBuf, znimok_core::Raster), String> {
    let poster = first_frame(mp4)?;
    let poster = if (poster.width, poster.height) == size {
        poster
    } else {
        znimok_core::Raster::solid(size.0, size.1, znimok_core::Rgb::new(20, 20, 24))
    };
    let mut doc = znimok_core::Document::from_raster(name.to_string(), poster.clone());
    doc.meta.created_ms = chrono::Local::now().timestamp_millis();
    doc.meta.source = source.into();
    let info = znimok_format::VideoInfo {
        width: size.0,
        height: size.1,
        fps_milli: fps * 1000,
        frames: result.frames.max(1) as u32,
        duration_hns: result.duration_hns,
        codec: znimok_format::video::CODEC_H264,
    };
    let mut video = znimok_format::Video::new(info);
    // The tracks as recorded, in the file's order (ZK-89).
    video.audio = result
        .audio_sources
        .iter()
        .map(|(kind, label)| znimok_format::video::AudioTrack {
            source: match kind {
                znimok_video::traits::AudioKind::System => {
                    znimok_format::video::AudioSource::System
                }
                znimok_video::traits::AudioKind::Microphone => {
                    znimok_format::video::AudioSource::Microphone
                }
            },
            label: label.clone(),
            ..Default::default()
        })
        .collect();
    // Their loudness for the timeline (ZK-189).
    for (t, p) in video.audio.iter_mut().zip(&result.audio_peaks) {
        t.peaks = p.clone();
    }
    // The clicks, in video time and video pixels (the MOUS log, ZK-90) — those on the recorded
    // part only: the hook hears the whole desktop, and a click in another window (an agent's
    // permission dialog beside the recorded browser) is not this recording's (ZK-252).
    video.mouse = result
        .events
        .iter()
        .filter(|e| {
            (0..size.0 as i32).contains(&e.event.x) && (0..size.1 as i32).contains(&e.event.y)
        })
        .map(|e| znimok_format::video::MouseEvent {
            ms: e.ms.clamp(0, i32::MAX as i64) as i32,
            x: e.event.x,
            y: e.event.y,
            button: match e.event.button {
                znimok_video::events::Button::Left => znimok_format::video::MouseButton::Left,
                znimok_video::events::Button::Right => znimok_format::video::MouseButton::Right,
                znimok_video::events::Button::Middle => znimok_format::video::MouseButton::Middle,
            },
            down: e.event.down,
        })
        .collect();
    // The browser's log of this recording (ZK-97).
    video.devlog = devlog;
    doc.timeline = Some(video.edit.to_timeline());
    let opts = znimok_format::WriteOptions {
        app_version: format!("Znimok {}", env!("CARGO_PKG_VERSION")),
        thumbnail: Some(thumbnail(&poster)),
        ..Default::default()
    };
    std::fs::create_dir_all(lib).map_err(|e| e.to_string())?;
    let path = crate::library::new_path(lib, &doc.id.simple().to_string());
    let part = path.with_extension("part");
    let len = std::fs::metadata(mp4).map_err(|e| e.to_string())?.len();
    {
        let src = std::io::BufReader::new(std::fs::File::open(mp4).map_err(|e| e.to_string())?);
        let mut out =
            std::io::BufWriter::new(std::fs::File::create(&part).map_err(|e| e.to_string())?);
        let r = {
            let _guard = crate::app::SAVE_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            znimok_format::write_video_to(&mut out, &doc, &video, src, len, &opts)
        };
        if let Err(e) = r {
            drop(out);
            let _ = std::fs::remove_file(&part);
            return Err(e.to_string());
        }
        use std::io::Write;
        out.flush().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
    Ok((path, poster))
}

/// A small copy (≤ 320 × 240) for the library's card.
fn thumbnail(r: &znimok_core::Raster) -> znimok_core::Raster {
    let k = (320.0 / r.width as f64)
        .min(240.0 / r.height as f64)
        .min(1.0);
    let (w, h) = (
        ((r.width as f64 * k).round() as u32).max(1),
        ((r.height as f64 * k).round() as u32).max(1),
    );
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let sy = ((y as f64 + 0.5) / k) as u32;
        for x in 0..w {
            let sx = ((x as f64 + 0.5) / k) as u32;
            let s = ((sy.min(r.height - 1) * r.width + sx.min(r.width - 1)) * 4) as usize;
            let d = ((y * w + x) * 4) as usize;
            out[d..d + 4].copy_from_slice(&r.rgba[s..s + 4]);
        }
    }
    znimok_core::Raster::new(w, h, out)
}

/// Back on the UI thread: the library shows it and the card after a capture says so.
fn saved(r: Result<(PathBuf, znimok_core::Raster), String>, name: String, display: Rect) {
    // An agent's recording goes back to the agent (ZK-237): no editor, no card.
    let agents = crate::agentipc::recording_done(match &r {
        Ok((path, _)) => Ok(path.as_path()),
        Err(e) => Err(e.as_str()),
    });
    crate::with_ctx(|a, ui| match r {
        Ok((path, poster)) => {
            a.refresh_library(ui);
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let heading = a.tr.tr("rec-saved");
            let sub = a.tr.tr_args(
                "rec-saved-details",
                &crate::app::fargs(&[
                    ("width", poster.width.to_string()),
                    ("height", poster.height.to_string()),
                    ("size", human_size(size)),
                ]),
            );
            crate::commands::emit("recordStop");
            LAST_SAVED.with(|l| *l.borrow_mut() = Some(path.clone()));
            // The editor with the recording (ZK-231, the owner: not the card alone), unless
            // the settings keep the card.
            if agents {
                let _ = (poster, name, heading, sub, display);
            } else if a.prefs().video.open_editor {
                a.open_path(ui, &path);
            } else {
                crate::pill::show(poster, path, name, heading, sub, display);
            }
        }
        Err(reason) => {
            let msg = if reason == NOTHING {
                a.tr.tr("rec-nothing")
            } else {
                a.tr.tr_args("rec-error-save", &crate::app::fargs(&[("reason", reason)]))
            };
            crate::commands::emit("failed");
            a.toast(ui, msg);
            crate::show_window(ui);
        }
    });
}

/// A recording that went without some of its sound says why (ZK-89).
#[cfg(any(windows, target_os = "macos"))]
fn sound_warning(w: Option<znimok_video::recorder::AudioWarning>) {
    use znimok_video::recorder::AudioWarning as W;
    let Some(w) = w else { return };
    crate::with_ctx(|a, ui| {
        let key = match w {
            W::MicDenied => "rec-warn-mic-denied",
            W::AudioBusy => "rec-warn-audio-busy",
            W::AudioNone => "rec-warn-audio-none",
        };
        let msg = a.tr.tr(key);
        a.toast(ui, msg);
    });
}

/// The recording ended without a single frame.
const NOTHING: &str = "nothing recorded";

/// Frames of the last recording that ended (the self-test tells an unchanged window from a
/// failure).
pub static LAST_FRAMES: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(-1);

thread_local! {
    /// The last recording saved (the self-test opens it).
    pub static LAST_SAVED: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

fn human_size(b: u64) -> String {
    if b >= 1 << 20 {
        format!("{:.1} MB", b as f64 / (1u64 << 20) as f64)
    } else {
        format!("{} KB", (b / 1024).max(1))
    }
}

// ------------------------------------------------------------------ indicators

/// Thickness of the red edge and its gap from the recorded part, pixels.
const EDGE: i32 = 3;
const GAP: i32 = 2;

fn show_indicators() {
    REC.with(|r| {
        let r = r.borrow();
        let Some(a) = r.as_ref() else { return };
        for e in &a.edges {
            let _ = e.show();
            no_touch(e.window());
        }
        let _ = a.bar.show();
        crate::frame::round_window(a.bar.window());
        keep_out_of_capture(a.bar.window());
        place(a);
    });
}

/// The edges around the recorded part and the bar under it (above when there is no room
/// below, inside at the bottom when neither fits).
fn place(a: &Active) {
    let f = a.frame;
    let area = crate::system::work_area(a.display);
    let (x0, y0, w, h) = edge_box(f, area, cfg!(target_os = "macos"));
    let rects = [
        (x0, y0, w, EDGE),
        (x0, y0 + h - EDGE, w, EDGE),
        (x0, y0, EDGE, h),
        (x0 + w - EDGE, y0, EDGE, h),
    ];
    for (e, (x, y, w, h)) in a.edges.iter().zip(rects) {
        set_rect(e.window(), x, y, w.max(1) as u32, h.max(1) as u32);
    }
    // The bar's size in desktop units: pixels on Windows, points on macOS (ZK-292).
    let k = if cfg!(target_os = "macos") {
        1.0
    } else {
        a.bar.window().scale_factor()
    };
    let (bw, bh) = ((272.0 * k) as i32, (52.0 * k) as i32);
    let cx = (f.x + f.width as i32 / 2 - bw / 2).clamp(
        area.x + 8,
        (area.x + area.width as i32 - bw - 8).max(area.x + 8),
    );
    let below = f.y + f.height as i32 + GAP + EDGE + 10;
    let above = f.y - GAP - EDGE - 10 - bh;
    let y = if below + bh <= area.y + area.height as i32 - 8 {
        below
    } else if above >= area.y + 8 {
        above
    } else {
        area.y + area.height as i32 - bh - 24
    };
    set_pos(a.bar.window(), cx, y);
}

/// The box the four edges go round: the recorded part with a gap around it (x, y, width,
/// height). With `keep_visible` (macOS) it stays within the visible part of the display: a
/// maximised window's edges would lie off screen and under the menu bar, and macOS moves such
/// windows on screen by itself — the edges came out some 40 points inside the window (ZK-303).
fn edge_box(f: Rect, area: Rect, keep_visible: bool) -> (i32, i32, i32, i32) {
    let out = GAP + EDGE;
    let (mut x0, mut y0) = (f.x - out, f.y - out);
    let (mut x1, mut y1) = (f.x + f.width as i32 + out, f.y + f.height as i32 + out);
    if keep_visible {
        let (ax1, ay1) = (area.x + area.width as i32, area.y + area.height as i32);
        let (cx0, cy0, cx1, cy1) = (x0.max(area.x), y0.max(area.y), x1.min(ax1), y1.min(ay1));
        // Only when something of the box is left (a part on another display keeps its own).
        if cx1 - cx0 > 2 * EDGE && cy1 - cy0 > 2 * EDGE {
            (x0, y0, x1, y1) = (cx0, cy0, cx1, cy1);
        }
    }
    (x0, y0, x1 - x0, y1 - y0)
}

/// Desktop units are the system's: physical pixels on Windows, points on macOS, where the
/// recorded window's and the display's rectangles come in points (ZK-292: set as pixels, the
/// frame came out half its size in the corner and the bar halfway up the screen).
fn set_pos(w: &slint::Window, x: i32, y: i32) {
    if cfg!(target_os = "macos") {
        w.set_position(slint::LogicalPosition::new(x as f32, y as f32));
    } else {
        w.set_position(slint::PhysicalPosition::new(x, y));
    }
}

fn set_rect(w: &slint::Window, x: i32, y: i32, width: u32, height: u32) {
    set_pos(w, x, y);
    if cfg!(target_os = "macos") {
        w.set_size(slint::LogicalSize::new(width as f32, height as f32));
    } else {
        w.set_size(slint::PhysicalSize::new(width, height));
    }
}

/// The mouse goes through the edge, and it stays out of captures.
fn no_touch(w: &slint::Window) {
    use slint::winit_030::WinitWindowAccessor;
    w.with_winit_window(|w| {
        let _ = w.set_cursor_hittest(false);
    });
    keep_out_of_capture(w);
}

/// Never in a screenshot or a recording (the whole screen, or a window under it).
fn keep_out_of_capture(w: &slint::Window) {
    #[cfg(windows)]
    {
        use slint::winit_030::WinitWindowAccessor;
        use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
        };
        w.with_winit_window(|w| {
            if let Ok(h) = w.window_handle()
                && let RawWindowHandle::Win32(h) = h.as_raw()
            {
                // SAFETY: the window's own handle, alive while `w` is.
                let _ = unsafe {
                    SetWindowDisplayAffinity(HWND(h.hwnd.get() as _), WDA_EXCLUDEFROMCAPTURE)
                };
            }
        });
    }
    #[cfg(not(windows))]
    let _ = w;
}

/// Every 250 ms: the time on the bar and in the tray; a followed window's new place; the end
/// when the recording stopped by itself (the window closed, the display went away).
fn tick() {
    let ended = REC.with(|r| {
        let mut r = r.borrow_mut();
        let Some(a) = r.as_mut() else { return false };
        #[cfg(target_os = "macos")]
        {
            if a.rec.as_ref().is_some_and(|rec| !rec.is_running()) {
                return true;
            }
            // The frame follows a recorded window as it moves.
            if let Some(b) = a.window.and_then(|id| znimok_mac::window_bounds(id as u32))
                && b != a.frame
            {
                a.frame = b;
                place(a);
            }
        }
        #[cfg(windows)]
        {
            if a.rec.as_ref().is_some_and(|rec| !rec.is_running()) {
                return true;
            }
            if let Some(id) = a.window {
                let h = znimok_win::raw::hwnd(znimok_platform::WindowId(id));
                if let Some(b) = znimok_win::raw::dwm_bounds(h)
                    && b != a.frame
                {
                    a.frame = b;
                    place(a);
                }
            }
        }
        let t = fmt(a.elapsed());
        a.bar.set_time(t.clone().into());
        set_tray(true, t);
        false
    });
    if ended {
        stop();
    }
}

fn fmt(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn set_tray(recording: bool, time: String) {
    TRAY.with(|t| {
        if let Some(t) = t.borrow().as_ref().and_then(|w| w.upgrade()) {
            t.set_recording(recording);
            t.set_rec_time(time.into());
        }
    });
}

/// For the self-test: the recording's control bar, while one runs.
#[allow(dead_code)]
pub fn bar_time() -> Option<String> {
    REC.with(|r| r.borrow().as_ref().map(|a| a.bar.get_time().to_string()))
}

/// For the self-test: the recorded part and how many edges show it.
#[allow(dead_code)]
pub fn indicators() -> Option<(Rect, usize, bool)> {
    REC.with(|r| {
        r.borrow().as_ref().map(|a| {
            (
                a.frame,
                a.edges.iter().filter(|e| e.window().is_visible()).count(),
                a.bar.window().is_visible(),
            )
        })
    })
}

/// For the self-test: `f` on the control bar's window, while a recording runs.
#[allow(dead_code)]
pub fn with_bar(f: impl FnOnce(&slint::Window)) {
    REC.with(|r| {
        if let Some(a) = r.borrow().as_ref() {
            f(a.bar.window());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ZK-303: a maximised window on a MacBook (1800 × 1169 pt, the menu bar 33 pt): the edges
    /// stay on the visible part instead of off screen; elsewhere they go round with a gap.
    #[test]
    fn the_edges_stay_on_the_visible_part() {
        let area = Rect::new(0, 33, 1800, 1136);
        let maximised = Rect::new(0, 33, 1800, 1136);
        assert_eq!(edge_box(maximised, area, true), (0, 33, 1800, 1136));
        // Windows: as before, round it (the system does not move them).
        assert_eq!(edge_box(maximised, area, false), (-5, 28, 1810, 1146));
        // A smaller window: round it, a gap away, on both systems.
        let small = Rect::new(200, 150, 800, 600);
        assert_eq!(edge_box(small, area, true), (195, 145, 810, 610));
        // Touching the left side only: that side on the visible part, the rest round it.
        let left = Rect::new(0, 200, 900, 600);
        assert_eq!(edge_box(left, area, true), (0, 195, 905, 610));
    }
}
