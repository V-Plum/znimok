//! One recording on its own thread, as on Windows (`VidThread`): the content, the stream, the
//! writer, the loop; the `.part` renamed when the file is complete. Start / pause / stop come
//! through [`RecordingControl`] from any thread.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;

use screencapturekit::cg::{CGPoint, CGRect, CGSize};
use screencapturekit::prelude::*;
use screencapturekit::shareable_content::SCShareableContentInfo;
use screencapturekit::stream::configuration::SCCaptureResolutionType;
use screencapturekit::stream::configuration::pixel_format::PixelFormat;
use znimok_video::events::EventQueue;
use znimok_video::recorder::{
    Recorder, RecorderConfig, RecordingControl, RecordingResult, commit_part, part_path,
};
use znimok_video::settings::{Quality, encoder_config};
use znimok_video::traits::AudioSource;
use znimok_video::{Result, VideoError};

use crate::audio::ScAudio;
use crate::clock::MachClock;
use crate::sink::AvSink;
use crate::source::{ScSource, Shared};

/// What to record.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// A display (`CGDirectDisplayID`), whole or a part of it in points from its top-left corner.
    Display {
        id: u32,
        region: Option<(f64, f64, f64, f64)>,
    },
    /// A window (`CGWindowID`) on its own, whatever covers it; followed as it moves.
    Window { id: u32 },
}

pub struct RecordRequest {
    pub target: Target,
    /// The final file (`.mp4`); the recording is written next to it as `.part`.
    pub path: PathBuf,
    /// 30 or 60.
    pub fps: u32,
    pub quality: Quality,
    /// Frames between key frames; `None` = the setting's default (~0.25 s).
    pub keyframe_interval: Option<u32>,
    pub system_audio: bool,
    pub microphone: bool,
    /// The pointer drawn by the system, and its clicks (macOS 15+).
    pub cursor: bool,
    pub clicks: bool,
    pub events: Option<EventQueue>,
}

impl RecordRequest {
    pub fn new(target: Target, path: PathBuf) -> Self {
        Self {
            target,
            path,
            fps: 30,
            quality: Quality::Normal,
            keyframe_interval: None,
            system_audio: false,
            microphone: false,
            cursor: true,
            clicks: true,
            events: None,
        }
    }
}

/// What the recording is made of, known once it started.
#[derive(Clone, Debug, PartialEq)]
pub struct Started {
    /// The video.
    pub size: (u32, u32),
    /// What ScreenCaptureKit was asked for: the content's own pixels (ZK-295).
    pub capture: (u32, u32),
    /// The first frame as it came (None: the recording started before one did).
    pub first: Option<crate::source::FrameFacts>,
    pub bitrate: u32,
    pub keyframe_interval: u32,
    pub audio_tracks: usize,
}

pub struct Recording {
    control: RecordingControl,
    started: Started,
    path: PathBuf,
    thread: Option<JoinHandle<RecordingResult>>,
}

/// Outcome of a finished recording.
#[derive(Clone, Debug)]
pub struct Finished {
    pub result: RecordingResult,
    /// The final file, when the recording was committed.
    pub path: Option<PathBuf>,
}

impl Recording {
    /// Start; returns once the stream runs (or with the reason it cannot).
    pub fn start(req: RecordRequest) -> Result<Self> {
        if req.fps == 0 {
            return Err(VideoError::Invalid("fps = 0".into()));
        }
        let control = RecordingControl::new();
        let ctl = control.clone();
        let (tx, rx) = mpsc::channel::<Result<Started>>();
        let path = req.path.clone();
        let part = part_path(&path);
        let thread = std::thread::Builder::new()
            .name("znimok-record".into())
            .spawn(move || run(req, ctl, tx))
            .map_err(|e| VideoError::Io(e.to_string()))?;
        match rx.recv() {
            Ok(Ok(started)) => Ok(Self {
                control,
                started,
                path,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                let _ = std::fs::remove_file(&part);
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(VideoError::Screen(
                    "потік запису завершився до старту".into(),
                ))
            }
        }
    }

    pub fn control(&self) -> &RecordingControl {
        &self.control
    }

    pub fn started(&self) -> &Started {
        &self.started
    }

    /// The loop still runs (it ends by itself when the window closes).
    pub fn is_running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    /// Stop and wait for the file.
    pub fn stop(mut self) -> Finished {
        self.control.stop();
        self.finish()
    }

    fn finish(&mut self) -> Finished {
        let result = match self.thread.take().map(|t| t.join()) {
            Some(Ok(r)) => r,
            _ => RecordingResult {
                error: Some(VideoError::Write("потік запису впав".into())),
                ..Default::default()
            },
        };
        let part = part_path(&self.path);
        let path = match commit_part(&part, &self.path, &result) {
            Ok(true) => Some(self.path.clone()),
            _ => None,
        };
        Finished { result, path }
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.control.stop();
            self.finish();
        }
    }
}

/// The stream ended by itself (the window closed, the display went away).
struct Watch(Arc<Shared>);

impl SCStreamDelegateTrait for Watch {
    fn did_stop_with_error(&self, _error: SCError) {
        self.0.stopped.store(true, Ordering::Release);
    }
}

/// The video's size for content of `w` × `h` pixels: even sides within what H.264 takes
/// everywhere (4096 × 2304 — a 5K display, or a MacBook's «More Space» mode, is scaled down).
fn out_size(w: f64, h: f64) -> (u32, u32) {
    let k = (4096.0 / w).min(2304.0 / h).min(1.0);
    let even = |v: f64| ((v * k / 2.0).round() as u32 * 2).max(64);
    (even(w), even(h))
}

/// What ScreenCaptureKit is asked for: the content's own pixels, to the pixel. Never the
/// video's size when that is smaller (ZK-295): asked to scale a display down to the encoder's
/// limit, macOS 27 drew it at half the size in a corner of the frame (and scalesToFit with the
/// best resolution changed nothing) — the writer scales instead, as it does any frame that is
/// not the video's size.
fn capture_size(w: f64, h: f64) -> (u32, u32) {
    let side = |v: f64| (v.round() as u32).clamp(2, 16_384);
    (side(w), side(h))
}

fn screen_error(e: impl std::fmt::Display) -> VideoError {
    let m = e.to_string();
    if m.contains("TCC") || m.contains("declined") || m.contains("permission") {
        VideoError::Screen("немає дозволу «Запис екрана» (Системні параметри → Приватність)".into())
    } else {
        VideoError::Screen(m)
    }
}

fn run(
    req: RecordRequest,
    control: RecordingControl,
    tx: mpsc::Sender<Result<Started>>,
) -> RecordingResult {
    let mut req = req;
    let fail = |e: VideoError, tx: &mpsc::Sender<Result<Started>>| {
        let _ = tx.send(Err(e.clone()));
        RecordingResult {
            error: Some(e),
            ..Default::default()
        }
    };
    let content = match SCShareableContent::get() {
        Ok(c) => c,
        Err(e) => return fail(screen_error(e), &tx),
    };
    // The filter, the stream's output size and, for a region, its rectangle in points.
    let me = std::process::id() as i32;
    let picked = match &req.target {
        Target::Display { id, region } => {
            let displays = content.displays();
            let Some(d) = displays.iter().find(|d| d.display_id() == *id) else {
                return fail(VideoError::Screen(format!("дисплей {id} не знайдено")), &tx);
            };
            // Without this process's own windows (the recording bar, the frame).
            let own: Vec<SCWindow> = content
                .windows()
                .into_iter()
                .filter(|w| w.owning_application().is_some_and(|a| a.process_id() == me))
                .collect();
            let own: Vec<&SCWindow> = own.iter().collect();
            let filter = match SCContentFilter::create()
                .with_display(d)
                .with_excluding_windows(&own)
                .build()
            {
                Ok(f) => f,
                Err(e) => return fail(screen_error(e), &tx),
            };
            let f = d.frame();
            let info = SCShareableContentInfo::for_filter(&filter);
            let scale = info
                .as_ref()
                .map(|i| f64::from(i.point_pixel_scale()))
                .filter(|s| *s > 0.0)
                .unwrap_or(2.0);
            let (pw, ph, rect) = match region {
                Some((x, y, w, h)) => (
                    w * scale,
                    h * scale,
                    Some(CGRect {
                        origin: CGPoint { x: *x, y: *y },
                        size: CGSize {
                            width: *w,
                            height: *h,
                        },
                    }),
                ),
                // The whole display: its pixels as the system counts them.
                None => match info.as_ref().map(|i| i.pixel_size()) {
                    Some((pw, ph)) if pw > 0 && ph > 0 => (f64::from(pw), f64::from(ph), None),
                    _ => (f.size.width * scale, f.size.height * scale, None),
                },
            };
            (filter, capture_size(pw, ph), out_size(pw, ph), rect)
        }
        Target::Window { id } => {
            let windows = content.windows();
            let Some(w) = windows.iter().find(|w| w.window_id() == *id) else {
                return fail(VideoError::Screen(format!("вікно {id} не знайдено")), &tx);
            };
            let filter = match SCContentFilter::create().with_window(w).build() {
                Ok(f) => f,
                Err(e) => return fail(screen_error(e), &tx),
            };
            let (pw, ph) = SCShareableContentInfo::for_filter(&filter)
                .map(|i| i.pixel_size())
                .unwrap_or((
                    (w.frame().size.width * 2.0) as u32,
                    (w.frame().size.height * 2.0) as u32,
                ));
            let (pw, ph) = (f64::from(pw), f64::from(ph));
            (filter, capture_size(pw, ph), out_size(pw, ph), None)
        }
    };
    let (filter, capture, (w, h), rect) = picked;
    let fps = req.fps;

    let mut cfg = SCStreamConfiguration::new()
        .with_width(capture.0)
        .with_height(capture.1)
        .with_pixel_format(PixelFormat::BGRA)
        .with_fps(fps)
        .with_queue_depth(5)
        .with_shows_cursor(req.cursor)
        .with_captures_audio(req.system_audio)
        .with_sample_rate(48_000)
        .with_channel_count(2)
        .with_excludes_current_process_audio(true);
    if let Some(r) = rect {
        cfg = cfg.with_source_rect(r);
    }
    // The full pixel resolution (macOS 27 may default to the nominal 1×, ZK-292); the size asked
    // for is the content's own, so nothing is left to scale — see `capture_size`.
    cfg = cfg.with_scales_to_fit(true);
    cfg = cfg
        .clone()
        .with_capture_resolution_type(SCCaptureResolutionType::Best)
        .unwrap_or(cfg);
    if req.clicks && req.cursor {
        cfg = cfg.clone().with_shows_mouse_clicks(true).unwrap_or(cfg);
    }
    if req.microphone {
        cfg = match cfg.clone().with_captures_microphone(true) {
            Ok(c) => c,
            Err(_) => {
                req.microphone = false;
                cfg
            }
        };
    }

    let shared = Arc::new(Shared::default());
    let mut stream = match SCStream::new_with_delegate(&filter, &cfg, Watch(shared.clone())) {
        Ok(s) => s,
        Err(e) => return fail(screen_error(e), &tx),
    };
    let mut kinds = vec![SCStreamOutputType::Screen];
    if req.system_audio {
        kinds.push(SCStreamOutputType::Audio);
    }
    if req.microphone {
        kinds.push(SCStreamOutputType::Microphone);
    }
    for kind in kinds {
        let s = shared.clone();
        let handler = move |sample: CMSampleBuffer, t: SCStreamOutputType| s.on_sample(&sample, t);
        if let Err(e) = stream.add_output_handler(handler, kind) {
            return fail(screen_error(e), &tx);
        }
    }
    if let Err(e) = stream.start_capture() {
        return fail(screen_error(e), &tx);
    }

    let mut audio: Vec<Box<dyn AudioSource>> = Vec::new();
    if req.system_audio {
        let mut a = ScAudio::system();
        a.attach(shared.clone(), String::new());
        audio.push(Box::new(a));
    }
    if req.microphone {
        let mut a = ScAudio::microphone();
        a.attach(shared.clone(), String::new());
        audio.push(Box::new(a));
    }
    let part = part_path(&req.path);
    let quality = req.quality;
    let gop = req.keyframe_interval;
    let mut opened: Option<(u32, u32)> = None;
    let open_sink = |tracks: usize| -> Result<AvSink> {
        let mut c = encoder_config(w, h, fps, quality, tracks);
        if let Some(g) = gop {
            c.keyframe_interval = g.max(1);
        }
        let s = AvSink::open(&part, &c).map_err(VideoError::Encoder)?;
        opened = Some((c.bitrate, c.keyframe_interval));
        Ok(s)
    };
    let rec = Recorder::open(
        MachClock::new(),
        ScSource::new(shared.clone()),
        open_sink,
        audio,
        RecorderConfig::new(fps),
        control,
    );
    let mut rec = match rec {
        Ok(r) => r,
        Err(e) => {
            let _ = stream.stop_capture();
            let _ = std::fs::remove_file(&part);
            return fail(e, &tx);
        }
    };
    let (bitrate, keyframe_interval) = opened.unwrap_or((0, 0));
    rec.spawn_audio_thread();
    if let Some(q) = req.events.take() {
        rec = rec.with_events(q);
    }
    let _ = tx.send(Ok(Started {
        size: (w, h),
        capture,
        first: shared.first.lock().ok().and_then(|f| *f),
        bitrate,
        keyframe_interval,
        audio_tracks: rec.audio_tracks(),
    }));
    let (result, sink) = rec.run();
    drop(sink);
    let _ = stream.stop_capture();
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn sizes_are_even_and_within_h264() {
        assert_eq!(super::out_size(2940.0, 1912.0), (2940, 1912));
        assert_eq!(super::out_size(5120.0, 2880.0), (4096, 2304));
        assert_eq!(super::out_size(301.0, 99.0), (302, 100));
    }

    /// ZK-295: the screen is asked for its own pixels whatever the video's size is.
    #[test]
    fn the_capture_is_never_scaled_by_the_system() {
        // A MacBook Pro 14 in «More Space»: 1800 x 1169 points, 3600 x 2338 pixels.
        assert_eq!(super::capture_size(3600.0, 2338.0), (3600, 2338));
        assert_eq!(super::out_size(3600.0, 2338.0), (3548, 2304));
        // The default mode fits the encoder as it is.
        assert_eq!(super::capture_size(3024.0, 1964.0), (3024, 1964));
        assert_eq!(super::out_size(3024.0, 1964.0), (3024, 1964));
        // An odd side stays the content's (the writer makes the video even).
        assert_eq!(super::capture_size(301.0, 99.0), (301, 99));
    }
}
