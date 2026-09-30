//! The recording loop (`VidRecord` in LH, §2.2–§2.5, §3) over the platform traits.
//!
//! One loop owns the frame source and the sink: it waits for the next slot while pulling the
//! source, writes one sample per due slot run ([`crate::cfr`]), writes ripe audio into the same
//! file, handles pause by shifting `t0`, and on stop writes the audio tail up to the end of the
//! video and finalises the file. [`Recorder::step`] is one turn of that loop, so tests drive it
//! with a [`crate::clock::ManualClock`]; [`Recorder::run`] loops until stopped.
//!
//! Start-up order (§7 item 43): audio sources are opened BEFORE the sink, and a track is added only
//! for a source that opened; if the sink cannot be opened with audio, it is opened without —
//! a recording without sound beats no recording.

use crate::audio::{self, AudioBuffer, AudioStep, AudioTimeline, PacketPlacer, TimelineProbe};
use crate::cfr::{Cfr, Slot, VideoSampleTime};
use crate::clock::{Clock, ticks_to_hns};
use crate::events::{EventGate, EventQueue, TimedEvent};
use crate::pause::PauseLog;
use crate::traits::{
    AudioError, AudioKind, AudioPacket, AudioSource, FrameSource, Pulled, SinkError, VideoSink,
};
use crate::{Result, VideoError};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

/// How long the loop waits for the source before the first frame and while paused.
pub const IDLE_PULL: Duration = Duration::from_millis(50);
/// Back-pressure: retries while the encoder holds its whole pool, and the pause between them
/// (250 × 2 ms ≈ 0.5 s, then the recording fails — §2.2, §7 item 11).
pub const BUSY_RETRIES: u32 = 250;
pub const BUSY_SLEEP: Duration = Duration::from_millis(2);

/// How sources map onto audio tracks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AudioLayout {
    /// One track per source; mixing happens at export (PLAN decision 16; on macOS the sources
    /// even run on different clocks).
    #[default]
    Separate,
    /// All sources summed into one track with the soft limiter — LH behaviour (§7 item 44).
    Mixed,
}

#[derive(Clone, Debug)]
pub struct RecorderConfig {
    /// 30 or 60 ([`crate::settings::fps_from_setting`]).
    pub fps: u32,
    pub audio_layout: AudioLayout,
    /// Share the published audio timeline with observers created before the recorder (the
    /// synthetic frame-coded tone, a level meter); a fresh one when `None`.
    pub probe: Option<Arc<TimelineProbe>>,
}

impl RecorderConfig {
    pub fn new(fps: u32) -> Self {
        Self {
            fps,
            audio_layout: AudioLayout::default(),
            probe: None,
        }
    }
}

/// Start/stop/pause from other threads (hotkey, tray, pill, browser extension).
#[derive(Clone, Debug, Default)]
pub struct RecordingControl {
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
}

impl RecordingControl {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
    /// Pause or resume. There is no pause hotkey (owner's decision); the pill and the tray call
    /// this.
    pub fn set_paused(&self, p: bool) {
        self.pause.store(p, Ordering::SeqCst);
    }
    pub fn toggle_pause(&self) {
        self.pause.fetch_xor(true, Ordering::SeqCst);
    }
    pub fn paused(&self) -> bool {
        self.pause.load(Ordering::SeqCst)
    }
}

/// Why a recording went without (some) sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioWarning {
    /// Privacy settings deny the microphone.
    MicDenied,
    /// A device is held exclusively by another program.
    AudioBusy,
    /// A source could not be opened, or the encoder did not take audio.
    AudioNone,
}

/// What came out of a recording.
#[derive(Clone, Debug, Default)]
pub struct RecordingResult {
    /// Slots on the timeline (`k`): the video is `frames / fps` long.
    pub frames: i64,
    /// Samples handed to the sink (fewer than `frames` when the loop stretched samples).
    pub samples: i64,
    pub duration_ms: f64,
    pub duration_hns: i64,
    /// Wall clock of the first frame, ms since the epoch (browser-log sync).
    pub wall0: Option<f64>,
    pub pauses: PauseLog,
    /// Clicks with video time.
    pub events: Vec<TimedEvent>,
    pub audio_tracks: usize,
    /// What each written track is, in track order (separate layout): the source's kind and its
    /// device name (ZK-89). Empty when the tracks are mixed or there are none.
    pub audio_sources: Vec<(AudioKind, String)>,
    pub warning: Option<AudioWarning>,
    /// The first error; the file is still finalised when anything was written.
    pub error: Option<VideoError>,
    /// The file is complete (something written and finalised) and may be shown — rename the
    /// `.part`; otherwise delete it ([`commit_part`]).
    pub committed: bool,
}

// ---------------------------------------------------------------------------------------------
// Audio capture

/// One audio source with its position on the timeline and its reopen timer.
pub struct AudioInput {
    source: Box<dyn AudioSource>,
    placer: PacketPlacer,
    open: bool,
    retry_at_ms: i64,
    track: usize,
    packets: Vec<AudioPacket>,
}

impl AudioInput {
    pub fn kind(&self) -> AudioKind {
        self.source.kind()
    }

    /// `VidAudPoll`: every packet at its own timestamp; a lost device → silence and a reopen
    /// every 500 ms.
    fn poll(&mut self, now_ms: i64, anchor_hns: i64, buffers: &mut [AudioBuffer]) {
        if !self.open {
            if now_ms >= self.retry_at_ms {
                self.retry_at_ms = now_ms + audio::REOPEN_MS as i64;
                if self.source.open().is_ok() {
                    self.open = true;
                    self.placer.reset();
                }
            }
            return;
        }
        self.packets.clear();
        let r = self.source.read(&mut self.packets);
        for p in self.packets.drain(..) {
            let idx = audio::index_of(p.time_hns, anchor_hns);
            let at = self.placer.place(idx, p.frames());
            if !p.silent {
                buffers[self.track].add(at, &p.data);
            }
        }
        if r.is_err() {
            self.source.close();
            self.open = false;
            self.retry_at_ms = now_ms + audio::REOPEN_MS as i64;
        }
    }
}

/// The audio sources of a recording and the buffers they add into.
pub struct AudioCapture {
    inputs: Vec<AudioInput>,
    anchor_hns: i64,
    buffers: Arc<Mutex<Vec<AudioBuffer>>>,
}

impl AudioCapture {
    /// Read every source once.
    pub fn poll(&mut self, clock: &dyn Clock) {
        let f = clock.frequency();
        let now_ms = crate::clock::ticks_to_ms(clock.ticks(), f);
        let mut b = lock(&self.buffers);
        for i in &mut self.inputs {
            i.poll(now_ms, self.anchor_hns, &mut b);
        }
    }

    fn close(&mut self) {
        for i in &mut self.inputs {
            if i.open {
                i.source.close();
                i.open = false;
            }
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

struct AudioThread {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<AudioCapture>,
}

struct AudioSide {
    capture: Option<AudioCapture>,
    thread: Option<AudioThread>,
    buffers: Arc<Mutex<Vec<AudioBuffer>>>,
    timeline: AudioTimeline,
    anchor_hns: i64,
    probe: Arc<TimelineProbe>,
}

// ---------------------------------------------------------------------------------------------
// The loop

/// One recording in progress.
pub struct Recorder<C: Clock, S: FrameSource, K: VideoSink<Frame = S::Frame>> {
    clock: C,
    source: S,
    sink: K,
    control: RecordingControl,
    cfr: Cfr,
    audio: Option<AudioSide>,
    events: Option<(EventQueue, EventGate)>,
    pause_wall: f64,
    closed: bool,
    result: RecordingResult,
}

/// Which warning a failed audio source gives (`VidRecord` 32217–32219).
fn warning_for(kind: AudioKind, e: &AudioError) -> AudioWarning {
    match e {
        AudioError::PermissionDenied if kind == AudioKind::Microphone => AudioWarning::MicDenied,
        AudioError::DeviceInUse => AudioWarning::AudioBusy,
        _ => AudioWarning::AudioNone,
    }
}

impl<C: Clock, S: FrameSource, K: VideoSink<Frame = S::Frame>> Recorder<C, S, K> {
    /// Open a recording: audio sources first, then the sink via `open_sink(audio_tracks)` (it
    /// builds the [`crate::traits::EncoderConfig`] and does the hardware → software fallback
    /// itself). If the sink fails with audio, it is asked again without.
    pub fn open(
        clock: C,
        source: S,
        mut open_sink: impl FnMut(usize) -> Result<K>,
        audio_sources: Vec<Box<dyn AudioSource>>,
        config: RecorderConfig,
        control: RecordingControl,
    ) -> Result<Self> {
        if config.fps == 0 {
            return Err(VideoError::Invalid("fps = 0".into()));
        }
        let f = clock.frequency();
        let mut result = RecordingResult::default();
        let anchor_hns = ticks_to_hns(clock.ticks(), f);
        let mut inputs = Vec::new();
        for mut s in audio_sources {
            match s.open() {
                Ok(()) => inputs.push(AudioInput {
                    source: s,
                    placer: PacketPlacer::default(),
                    open: true,
                    retry_at_ms: 0,
                    track: 0,
                    packets: Vec::new(),
                }),
                Err(e) => result.warning = Some(warning_for(s.kind(), &e)),
            }
        }
        let tracks = match config.audio_layout {
            AudioLayout::Separate => inputs.len(),
            AudioLayout::Mixed => inputs.len().min(1),
        };
        if config.audio_layout == AudioLayout::Separate {
            for (i, a) in inputs.iter_mut().enumerate() {
                a.track = i;
            }
        }
        let (sink, tracks) = match open_sink(tracks) {
            Ok(k) => (k, tracks),
            Err(e) if tracks == 0 => return Err(e),
            Err(_) => {
                result.warning = Some(AudioWarning::AudioNone);
                (open_sink(0)?, 0)
            }
        };
        let audio = if tracks > 0 {
            let buffers = Arc::new(Mutex::new(vec![AudioBuffer::new(); tracks]));
            let probe = config.probe.clone().unwrap_or_default();
            probe.set_anchor(anchor_hns);
            Some(AudioSide {
                capture: Some(AudioCapture {
                    inputs,
                    anchor_hns,
                    buffers: buffers.clone(),
                }),
                thread: None,
                buffers,
                timeline: AudioTimeline::new(),
                anchor_hns,
                probe,
            })
        } else {
            for mut i in inputs {
                i.source.close();
            }
            None
        };
        result.audio_tracks = tracks;
        if config.audio_layout == AudioLayout::Separate
            && let Some(c) = audio.as_ref().and_then(|a| a.capture.as_ref())
        {
            result.audio_sources = c
                .inputs
                .iter()
                .map(|i| (i.source.kind(), i.source.label()))
                .collect();
        }
        Ok(Self {
            cfr: Cfr::new(config.fps, f),
            clock,
            source,
            sink,
            control,
            audio,
            events: None,
            pause_wall: 0.0,
            closed: false,
            result,
        })
    }

    /// Record clicks from this queue (the OS hook pushes into it).
    pub fn with_events(mut self, q: EventQueue) -> Self {
        self.events = Some((q, EventGate::default()));
        self
    }

    /// The published audio timeline (for the synthetic frame-coded tone and meters).
    pub fn timeline_probe(&self) -> Option<Arc<TimelineProbe>> {
        self.audio.as_ref().map(|a| a.probe.clone())
    }

    pub fn control(&self) -> &RecordingControl {
        &self.control
    }

    pub fn cfr(&self) -> &Cfr {
        &self.cfr
    }

    pub fn sink(&self) -> &K {
        &self.sink
    }

    /// Audio tracks in the file.
    pub fn audio_tracks(&self) -> usize {
        self.result.audio_tracks
    }

    fn audio_index(&self, ticks: i64) -> i64 {
        let a = self.audio.as_ref().map_or(0, |a| a.anchor_hns);
        audio::index_of(ticks_to_hns(ticks, self.clock.frequency()), a)
    }

    fn apply_audio(&mut self, step: AudioStep) {
        let Some(a) = self.audio.as_ref() else {
            return;
        };
        let mut b = lock(&a.buffers);
        match step {
            AudioStep::Discard { upto } => {
                for buf in b.iter_mut() {
                    buf.take_f32(upto);
                }
            }
            AudioStep::Emit { upto, sample } => {
                for (track, buf) in b.iter_mut().enumerate() {
                    let pcm = buf.take_s16(upto);
                    if !pcm.is_empty() {
                        // LH ignores audio write errors: the video goes on.
                        let _ = self.sink.write_audio(track, &pcm, sample);
                    }
                }
            }
        }
    }

    fn poll_audio_inline(&mut self) {
        if let Some(c) = self.audio.as_mut().and_then(|a| a.capture.as_mut()) {
            c.poll(&self.clock);
        }
    }

    fn pull(&mut self, wait: Duration) -> Result<()> {
        if self.source.pull(wait)? == Pulled::Closed {
            self.closed = true;
        }
        Ok(())
    }

    fn fail(&mut self, e: VideoError) -> Step {
        if self.result.error.is_none() {
            self.result.error = Some(e);
        }
        Step::Stopped
    }

    fn write_video(&mut self, t: VideoSampleTime) -> std::result::Result<i64, VideoError> {
        let frame = self.source.frame_for_slot(t.slot)?;
        let fps = self.cfr.fps();
        let parts: Vec<VideoSampleTime> = if self.sink.caps().carries_duration {
            vec![t]
        } else {
            t.expand(fps).collect()
        };
        for p in &parts {
            let mut tries = 0;
            loop {
                match self.sink.write_video(frame, *p) {
                    Ok(()) => break,
                    Err(SinkError::Busy) if tries < BUSY_RETRIES => {
                        tries += 1;
                        self.clock.sleep(BUSY_SLEEP);
                    }
                    Err(SinkError::Busy) => return Err(VideoError::Backpressure),
                    Err(SinkError::Failed(m)) => return Err(VideoError::Write(m)),
                }
            }
        }
        Ok(parts.len() as i64)
    }

    /// One turn of the loop.
    pub fn step(&mut self) -> Step {
        match self.step_inner() {
            Ok(s) => s,
            Err(e) => self.fail(e),
        }
    }

    fn step_inner(&mut self) -> Result<Step> {
        if self.control.stopped() || self.closed {
            return Ok(Step::Stopped);
        }
        self.poll_audio_inline();
        // Before the first real frame time does not run.
        if !self.cfr.started() {
            self.pull(IDLE_PULL)?;
            if self.source.has_frame() {
                let now = self.clock.ticks();
                self.cfr.start(now);
                self.result.wall0 = Some(self.clock.wall_ms());
                let idx0 = self.audio_index(now);
                if let Some(a) = self.audio.as_mut() {
                    let s = a.timeline.start(idx0);
                    a.probe.set_zero(idx0);
                    self.apply_audio(s);
                }
            }
            return Ok(Step::Continue);
        }
        // CAPS-101: pause.
        let want = self.control.paused();
        if want != self.cfr.paused() {
            let now = self.clock.ticks();
            let idx = self.audio_index(now);
            if want {
                self.cfr.pause(now);
                self.pause_wall = self.clock.wall_ms();
                if let Some(a) = self.audio.as_mut() {
                    a.timeline.pause(idx);
                }
            } else {
                self.cfr.resume(now);
                self.result
                    .pauses
                    .push(self.pause_wall, self.clock.wall_ms());
                if let Some(a) = self.audio.as_mut() {
                    let steps = a.timeline.resume(idx);
                    a.probe.set_shift(a.timeline.shift());
                    for s in steps {
                        self.apply_audio(s);
                    }
                }
                if let Some((q, g)) = self.events.as_mut() {
                    g.skip_queued(q);
                }
            }
        }
        if self.cfr.paused() {
            // The source stays alive; no frame is written; audio from before the pause ripens.
            self.pull(IDLE_PULL)?;
            let idx = self.audio_index(self.clock.ticks());
            if let Some(s) = self
                .audio
                .as_mut()
                .and_then(|a| a.timeline.ripe_paused(idx))
            {
                self.apply_audio(s);
            }
            return Ok(Step::Continue);
        }
        let now = self.clock.ticks();
        match self.cfr.poll(now) {
            Slot::Wait { wait_ms } => {
                self.pull(Duration::from_millis(wait_ms as u64))?;
                Ok(Step::Continue)
            }
            Slot::Due(t) => {
                if let (Some((q, g)), Some(t0)) = (self.events.as_mut(), self.cfr.t0()) {
                    let f = self.clock.frequency();
                    self.result.events.extend(g.drain(q, t0, f));
                }
                let written = self.write_video(t)?;
                self.result.samples += written;
                self.cfr.commit(&t);
                let idx = self.audio_index(self.clock.ticks());
                if let Some(s) = self.audio.as_mut().and_then(|a| a.timeline.ripe(idx)) {
                    self.apply_audio(s);
                }
                Ok(Step::Continue)
            }
            Slot::NotStarted | Slot::Paused => Ok(Step::Continue),
        }
    }

    /// Loop until stopped (by the control, a closed source or an error), then finish.
    pub fn run(mut self) -> (RecordingResult, K) {
        while self.step() == Step::Continue {}
        self.finish()
    }

    /// Stop: close the pause, write the audio tail up to the end of the video, finalise the file
    /// when anything was written.
    pub fn finish(mut self) -> (RecordingResult, K) {
        if self.cfr.paused() {
            self.result
                .pauses
                .push(self.pause_wall, self.clock.wall_ms());
        }
        let frames = self.cfr.frames();
        self.result.frames = frames;
        self.result.duration_ms = self.cfr.duration_ms();
        self.result.duration_hns = self.cfr.duration_hns();
        let mut tail = None;
        if let Some(a) = self.audio.as_mut() {
            if let Some(t) = a.thread.take() {
                t.stop.store(true, Ordering::SeqCst);
                a.capture = t.handle.join().ok();
            }
            if let Some(c) = a.capture.as_mut() {
                c.poll(&self.clock); // the last that came in
                c.close();
            }
            if self.cfr.started() && self.result.samples > 0 {
                // The tail ends exactly at the end of the video.
                tail = a.timeline.finish(frames, self.cfr.fps());
            }
        }
        if let Some(s) = tail {
            self.apply_audio(s);
        }
        // Even after an error: what was written stays whole. Not finalised → no index → no
        // player opens it → not committed.
        if self.result.samples > 0 {
            match self.sink.finalize() {
                Ok(()) => self.result.committed = true,
                Err(e) => {
                    if self.result.error.is_none() {
                        self.result.error = Some(VideoError::Finalize(e.to_string()));
                    }
                }
            }
        }
        (self.result, self.sink)
    }
}

impl<C, S, K> Recorder<C, S, K>
where
    C: Clock + Clone + 'static,
    S: FrameSource,
    K: VideoSink<Frame = S::Frame>,
{
    /// Poll audio on its own thread every 10 ms (as LH does) instead of inside [`Self::step`].
    pub fn spawn_audio_thread(&mut self) {
        let clock = self.clock.clone();
        let Some(a) = self.audio.as_mut() else {
            return;
        };
        let Some(mut cap) = a.capture.take() else {
            return;
        };
        let stop = Arc::new(AtomicBool::new(false));
        let st = stop.clone();
        let handle = std::thread::Builder::new()
            .name("znimok-audio".into())
            .spawn(move || {
                while !st.load(Ordering::SeqCst) {
                    cap.poll(&clock);
                    clock.sleep(Duration::from_millis(audio::POLL_MS));
                }
                cap
            });
        match handle {
            Ok(handle) => a.thread = Some(AudioThread { stop, handle }),
            // No thread — fall back to polling inside the loop. The capture moved into the
            // failed closure is gone; audio for this recording is silence.
            Err(_) => a.capture = None,
        }
    }
}

/// Outcome of one [`Recorder::step`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Continue,
    Stopped,
}

/// Where a recording is written until it is complete: `<final>.part` (§7 item 8).
pub fn part_path(final_path: &Path) -> PathBuf {
    let mut s = final_path.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// Make the recording visible (rename `.part` over the final name) when it is complete;
/// otherwise delete the `.part` — without `moov` no player would open it. Returns whether the
/// final file exists.
pub fn commit_part(part: &Path, final_path: &Path, result: &RecordingResult) -> Result<bool> {
    if !result.committed {
        let _ = std::fs::remove_file(part);
        return Ok(false);
    }
    std::fs::rename(part, final_path).map_err(|e| VideoError::Io(e.to_string()))?;
    Ok(true)
}
