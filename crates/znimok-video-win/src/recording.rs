//! One recording on its own thread (`VidThread`): COM/MF up, the devices, the source, the sink,
//! the loop; the `.part` renamed when the file is complete. Start / pause / stop come through
//! [`RecordingControl`] from any thread.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread::JoinHandle;

use znimok_video::events::EventQueue;
use znimok_video::recorder::{
    Recorder, RecorderConfig, RecordingControl, RecordingResult, commit_part, part_path,
};
use znimok_video::settings::{Quality, encoder_config};
use znimok_video::traits::AudioSource;
use znimok_video::{Result, VideoError};

use crate::interop::{Bridge, Gpu, adapter_of_monitor};
use crate::sink::MfSink;
use crate::source::{Api, OverlayFn, Target, open_source, plan};

pub struct RecordRequest {
    pub target: Target,
    /// The final file (`.mp4`); the recording is written next to it as `.part`.
    pub path: PathBuf,
    /// 30 or 60.
    pub fps: u32,
    pub quality: Quality,
    /// Skip the hardware encoder.
    pub software: bool,
    pub api: Api,
    /// Frames between key frames; `None` = the setting's default (~0.25 s).
    pub keyframe_interval: Option<u32>,
    pub audio: Vec<Box<dyn AudioSource>>,
    pub events: Option<EventQueue>,
    pub overlay: Option<OverlayFn>,
}

impl RecordRequest {
    pub fn new(target: Target, path: PathBuf) -> Self {
        Self {
            target,
            path,
            fps: 30,
            quality: Quality::Normal,
            software: false,
            api: Api::Wgc,
            keyframe_interval: None,
            audio: Vec::new(),
            events: None,
            overlay: None,
        }
    }
}

/// What the recording is made of, known once it started.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Started {
    pub api: &'static str,
    pub gpu: String,
    pub encoder: String,
    pub hardware: bool,
    pub size: (u32, u32),
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
    /// Start; returns once the first frame can be taken (or with the reason it cannot).
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

    /// The loop still runs (it ends by itself when the window closes or the display goes).
    pub fn is_running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    /// Stop and wait for the file.
    pub fn stop(mut self) -> Finished {
        self.control.stop();
        self.finish()
    }

    /// Wait for the loop to end (it ends by itself when the window closes).
    pub fn wait(mut self) -> Finished {
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

fn run(
    req: RecordRequest,
    control: RecordingControl,
    tx: mpsc::Sender<Result<Started>>,
) -> RecordingResult {
    let mut req = req;
    let fail = |e: VideoError| RecordingResult {
        error: Some(e),
        ..Default::default()
    };
    if let Err(e) = crate::mf::startup() {
        let e = VideoError::Encoder(e);
        let _ = tx.send(Err(e.clone()));
        return fail(e);
    }
    let plan = match plan(&req.target) {
        Ok(p) => p,
        Err(e) => {
            let _ = tx.send(Err(e.clone()));
            return fail(e);
        }
    };
    let gpu = match Gpu::new(adapter_of_monitor(plan.monitor)) {
        Ok(g) => Rc::new(g),
        Err(e) => {
            let e = VideoError::Screen(e);
            let _ = tx.send(Err(e.clone()));
            return fail(e);
        }
    };
    let bridge = match Bridge::new(&gpu) {
        Ok(b) => Rc::new(b),
        Err(e) => {
            let e = VideoError::Screen(e);
            let _ = tx.send(Err(e.clone()));
            return fail(e);
        }
    };
    let (source, pool) = match open_source(&gpu, &bridge, &plan, req.api, req.overlay.take()) {
        Ok(v) => v,
        Err(e) => {
            let _ = tx.send(Err(e.clone()));
            return fail(e);
        }
    };
    let api = source.api();
    let part = part_path(&req.path);
    let (w, h) = plan.out;
    let fps = req.fps;
    let quality = req.quality;
    let software = req.software;
    let gop = req.keyframe_interval;
    let (gpu2, bridge2, pool2, part2) = (gpu.clone(), bridge.clone(), pool.clone(), part.clone());
    let mut opened: Option<(String, bool, u32, u32)> = None;
    let open_sink = |tracks: usize| -> Result<MfSink> {
        let mut cfg = encoder_config(w, h, fps, quality, tracks);
        if let Some(g) = gop {
            cfg.keyframe_interval = g.max(1);
        }
        let s = MfSink::open_best(
            gpu2.clone(),
            bridge2.clone(),
            pool2.clone(),
            &part2,
            &cfg,
            !software,
        )
        .map_err(VideoError::Encoder)?;
        opened = Some((
            s.encoder.clone(),
            s.hardware,
            cfg.bitrate,
            cfg.keyframe_interval,
        ));
        Ok(s)
    };
    let rec = Recorder::open(
        crate::clock::QpcClock::new(),
        source,
        open_sink,
        std::mem::take(&mut req.audio),
        RecorderConfig::new(fps),
        control,
    );
    let mut rec = match rec {
        Ok(r) => r,
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            let _ = tx.send(Err(e.clone()));
            return fail(e);
        }
    };
    let (encoder, hardware, bitrate, keyframe_interval) =
        opened.unwrap_or_else(|| ("?".into(), false, 0, 0));
    rec.spawn_audio_thread();
    if let Some(q) = req.events.take() {
        rec = rec.with_events(q);
    }
    // What records (ZK-284): the capture API, the GPU and the encoder go to the log.
    tracing::info!(
        api,
        gpu = %gpu.name,
        encoder = %encoder,
        hardware,
        "recording: {w}×{h}"
    );
    let _ = tx.send(Ok(Started {
        api,
        gpu: gpu.name.clone(),
        encoder,
        hardware,
        size: (w, h),
        bitrate,
        keyframe_interval,
        audio_tracks: rec.audio_tracks(),
    }));
    let (result, sink) = rec.run();
    drop(sink);
    result
}
