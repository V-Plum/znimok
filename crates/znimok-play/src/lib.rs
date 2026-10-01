//! Video playback for the editor (ZK-92), after prototype P4 (ZK-17).
//!
//! ```text
//! Windows: .znimok (the MP4's byte ranges, read in place) → MF Source Reader on D3D11 (DXVA)
//!          → GPU copy into a shared NV12 texture → wgpu (the app's DX12 device)
//! macOS:   MP4 → AVAssetReader (VideoToolbox) → IOSurface planes as Metal textures → wgpu
//!   → nv12.wgsl (BT.709, bilinear chroma, the picture's tone) → RGBA8 texture → the UI shows it
//! ```
//!
//! No frame crosses the CPU on the way to the screen. When the zero-copy path is not there (a
//! device without NV12 textures, a software decoder) the planes are uploaded instead — the same
//! shader, one copy. A paused frame is also read back once (a CPU copy for «Кадр як знімок» and
//! for Hide marks, which sample the picture).
//!
//! The player runs on its own thread. Frames go through a mailbox: the UI is woken once, takes the
//! newest frame, and the texture it shows is never written again until it takes another — so a
//! slow UI skips frames instead of queueing them. Reverse playback decodes the key-frame interval
//! before the frame once into a small cache of textures and plays it backwards (MP4 only decodes
//! forward — a trap from Little Helpers).

pub mod convert;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(windows)]
mod win;

use std::ops::Range;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub use convert::{Converter, Gpu};
use znimok_core::Raster;
use znimok_video::edit::VideoEdit;

/// Where the recording is.
#[derive(Clone, Debug)]
pub enum Source {
    /// Inside a `.znimok`: the MP4's byte ranges in `path` (read in place on Windows; macOS copies
    /// it once into `cache`, AVFoundation only opens whole files).
    InFile {
        path: PathBuf,
        ranges: Vec<Range<u64>>,
        cache: PathBuf,
    },
    /// A plain MP4.
    File(PathBuf),
}

impl Source {
    pub fn of_part(part: &znimok_format::VideoPart, cache: PathBuf) -> Self {
        Source::InFile {
            path: part.source.clone(),
            ranges: part.payload.ranges.clone(),
            cache,
        }
    }

    /// A plain file with the MP4 (copied out of the document into the cache once when needed).
    pub fn as_file(&self) -> Result<PathBuf, String> {
        match self {
            Source::File(p) => Ok(p.clone()),
            Source::InFile {
                path,
                ranges,
                cache,
            } => extract(path, ranges, cache),
        }
    }
}

/// The MP4 inside a document as a file of its own in `cache` (named after the document and the
/// payload's place, so a re-saved document gets a new copy).
fn extract(
    path: &std::path::Path,
    ranges: &[Range<u64>],
    cache: &std::path::Path,
) -> Result<PathBuf, String> {
    use std::io::{Read, Seek, Write};
    std::fs::create_dir_all(cache).map_err(|e| e.to_string())?;
    let payload = znimok_format::video::Payload {
        ranges: ranges.to_vec(),
    };
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "video".into());
    let first = ranges.first().map_or(0, |r| r.start);
    let out = cache.join(format!("{stem}-{first}-{}.mp4", payload.len()));
    if std::fs::metadata(&out).is_ok_and(|m| m.len() == payload.len()) {
        return Ok(out);
    }
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut reader = znimok_format::video::PayloadReader::new(std::io::BufReader::new(f), &payload);
    reader
        .seek(std::io::SeekFrom::Start(0))
        .map_err(|e| e.to_string())?;
    let tmp = out.with_extension("part");
    {
        let mut w =
            std::io::BufWriter::new(std::fs::File::create(&tmp).map_err(|e| e.to_string())?);
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            w.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        }
        w.flush().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, &out).map_err(|e| e.to_string())?;
    Ok(out)
}

/// What an open player knows about the stream.
#[derive(Clone, Debug)]
pub struct Info {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    /// "gpu" — decoded and converted without a CPU copy; "upload" — the planes pass through CPU
    /// memory; "software" — no hardware decoder.
    pub path: &'static str,
}

/// A frame ready to be shown: the texture stays untouched until the next [`Player::take`].
#[derive(Clone)]
pub struct Shown {
    pub frame: i64,
    pub texture: wgpu::Texture,
    pub playing: bool,
}

/// What the player tells the UI (on its own thread: the callback hops to the UI).
pub enum Event {
    Opened(Info),
    Failed(String),
    /// A new frame waits in the mailbox — take it with [`Player::take`].
    Frame,
    /// A CPU copy of the frame shown while paused.
    Still {
        frame: i64,
        raster: Arc<Raster>,
    },
}

/// The decoders of the two systems.
pub(crate) trait Decoder {
    fn size(&self) -> (u32, u32);
    fn fps(&self) -> f64;
    fn path(&self) -> &'static str;
    /// After this, `next` yields frames from the key frame at or before `frame` (Windows) or from
    /// `frame` itself (macOS).
    fn seek(&mut self, frame: i64) -> Result<(), String>;
    /// Decodes the next frame: its first index and how many frame slots it covers; None at the end.
    fn next(&mut self) -> Result<Option<(i64, i64)>, String>;
    /// Queues the conversion of the frame last decoded into `out`.
    fn convert(
        &mut self,
        conv: &Converter,
        out: &wgpu::Texture,
        thumb: bool,
    ) -> Result<wgpu::SubmissionIndex, String>;
}

fn open_decoder(gpu: &Gpu, source: &Source) -> Result<Box<dyn Decoder>, String> {
    #[cfg(windows)]
    {
        Ok(Box::new(win::MfPlayer::open(gpu, source)?))
    }
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(mac::AvPlayer::open(gpu, source)?))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = (gpu, source);
        Err("no video decoder on this system".into())
    }
}

enum Cmd {
    Seek(i64),
    Play {
        from: i64,
        speed: f64,
        looping: bool,
        backward: bool,
    },
    Pause,
    Edit(VideoEdit),
    Tone(Option<Box<[u8; 256]>>),
    Quit,
}

#[derive(Default)]
struct Mail {
    /// The newest frame not yet taken: (pool slot, frame).
    pending: Option<(usize, Shown)>,
    /// The slot the UI shows now.
    shown: Option<usize>,
}

pub struct Player {
    tx: Sender<Cmd>,
    mail: Arc<Mutex<Mail>>,
}

impl Player {
    /// Starts the player of `source` on `gpu` (the UI's device). `frames` is the stream's frame
    /// count from the document; `on_event` is called on the player's thread.
    pub fn open(
        gpu: Gpu,
        source: Source,
        frames: i64,
        on_event: impl Fn(Event) + Send + 'static,
    ) -> Result<Player, String> {
        let (tx, rx) = std::sync::mpsc::channel::<Cmd>();
        let mail: Arc<Mutex<Mail>> = Arc::default();
        let m = mail.clone();
        std::thread::Builder::new()
            .name("znimok-player".into())
            .spawn(move || {
                match Converter::new(&gpu).and_then(|c| Ok((c, open_decoder(&gpu, &source)?))) {
                    Ok((conv, dec)) => {
                        let (w, h) = dec.size();
                        on_event(Event::Opened(Info {
                            width: w,
                            height: h,
                            fps: dec.fps(),
                            path: dec.path(),
                        }));
                        Engine::new(dec, conv, frames, m).run(rx, &on_event);
                    }
                    Err(e) => on_event(Event::Failed(e)),
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Player { tx, mail })
    }

    /// The newest frame, when there is one the UI has not taken yet.
    pub fn take(&self) -> Option<Shown> {
        let mut m = self.mail.lock().ok()?;
        let (slot, s) = m.pending.take()?;
        m.shown = Some(slot);
        Some(s)
    }

    pub fn seek(&self, frame: i64) {
        let _ = self.tx.send(Cmd::Seek(frame));
    }

    pub fn play(&self, from: i64, speed: f64, looping: bool, backward: bool) {
        let _ = self.tx.send(Cmd::Play {
            from,
            speed,
            looping,
            backward,
        });
    }

    pub fn pause(&self) {
        let _ = self.tx.send(Cmd::Pause);
    }

    pub fn set_edit(&self, e: VideoEdit) {
        let _ = self.tx.send(Cmd::Edit(e));
    }

    /// The picture's tone (`znimok_render::develop::tone_lut`), None for none: the frame shown
    /// is converted again.
    pub fn set_tone(&self, lut: Option<[u8; 256]>) {
        let _ = self.tx.send(Cmd::Tone(lut.map(Box::new)));
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
    }
}

/// A converted frame kept on the GPU: its frames `[a, a + n)` and when it was last used.
struct Slot {
    tex: wgpu::Texture,
    frames: Option<(i64, i64)>,
    used: u64,
}

struct Engine {
    dec: Box<dyn Decoder>,
    conv: Converter,
    frames: i64,
    fps: f64,
    mail: Arc<Mutex<Mail>>,
    slots: Vec<Slot>,
    /// How many converted frames may stay on the GPU (reverse needs a key-frame interval).
    cap: usize,
    clock: u64,
    /// The sample the decoder stands on: frames `[a, a + n)`.
    at: Option<(i64, i64)>,
    edit: VideoEdit,
    /// The frame delivered last, and whether a CPU copy of it was sent.
    last: Option<(i64, usize)>,
    still_sent: bool,
}

/// GPU memory the reverse cache may take.
const CACHE_BYTES: u64 = 384 << 20;

impl Engine {
    fn new(dec: Box<dyn Decoder>, conv: Converter, frames: i64, mail: Arc<Mutex<Mail>>) -> Self {
        let (w, h) = dec.size();
        let fps = if dec.fps() > 0.0 { dec.fps() } else { 30.0 };
        let per = u64::from(w.max(1)) * u64::from(h.max(1)) * 4;
        let cap = ((CACHE_BYTES / per) as usize).clamp(4, 40);
        Self {
            dec,
            conv,
            frames: frames.max(1),
            fps,
            mail,
            slots: Vec::new(),
            cap,
            clock: 0,
            at: None,
            edit: VideoEdit::new(frames.max(1)),
            last: None,
            still_sent: false,
        }
    }

    fn protected(&self) -> (Option<usize>, Option<usize>) {
        let m = self.mail.lock().unwrap();
        (m.shown, m.pending.as_ref().map(|(s, _)| *s))
    }

    fn slot_of(&mut self, f: i64) -> Option<usize> {
        let i = self
            .slots
            .iter()
            .position(|s| s.frames.is_some_and(|(a, n)| a <= f && f < a + n))?;
        self.clock += 1;
        self.slots[i].used = self.clock;
        Some(i)
    }

    /// A slot to convert into: a new one while under the cap, else the least recently used one
    /// that is neither shown nor waiting in the mailbox (nor in `keep`).
    fn free_slot(&mut self, keep: Option<Range<i64>>) -> usize {
        let (shown, pending) = self.protected();
        self.clock += 1;
        let (w, h) = self.dec.size();
        // The stream changed its size: the old textures go (the one shown stays alive in the UI).
        if self
            .slots
            .first()
            .is_some_and(|s| (s.tex.width(), s.tex.height()) != (w, h))
        {
            self.slots.clear();
            if let Ok(mut m) = self.mail.lock() {
                m.shown = None;
                m.pending = None;
            }
        }
        if self.slots.len() < self.cap {
            self.slots.push(Slot {
                tex: self.conv.target(w, h),
                frames: None,
                used: self.clock,
            });
            return self.slots.len() - 1;
        }
        let kept = |s: &Slot| {
            keep.as_ref()
                .is_some_and(|k| s.frames.is_some_and(|(a, n)| a < k.end && a + n > k.start))
        };
        let pick = (0..self.slots.len())
            .filter(|i| Some(*i) != shown && Some(*i) != pending)
            .min_by_key(|i| (kept(&self.slots[*i]), self.slots[*i].used))
            .unwrap_or(0);
        self.slots[pick].frames = None;
        self.slots[pick].used = self.clock;
        pick
    }

    /// Converts the sample the decoder stands on into a slot.
    fn convert_current(&mut self, keep: Option<Range<i64>>) -> Result<usize, String> {
        let at = self.at.ok_or("no frame decoded")?;
        let i = self.free_slot(keep);
        let tex = self.slots[i].tex.clone();
        self.dec.convert(&self.conv, &tex, false)?;
        self.slots[i].frames = Some(at);
        Ok(i)
    }

    /// Decodes forward to frame `f` (seeking first when it is behind or far ahead). False at the
    /// end of the stream.
    fn decode_to(&mut self, f: i64) -> Result<bool, String> {
        if let Some((a, n)) = self.at {
            if a <= f && f < a + n {
                return Ok(true);
            }
            if f < a || f > a + n + (self.fps as i64).max(30) {
                self.dec.seek(f)?;
                self.at = None;
            }
        } else {
            self.dec.seek(f)?;
        }
        loop {
            match self.dec.next()? {
                Some((a, n)) => {
                    self.at = Some((a, n));
                    if a + n > f {
                        return Ok(true);
                    }
                }
                None => {
                    self.at = None;
                    return Ok(false);
                }
            }
        }
    }

    /// Frame `f` in a slot: from the cache, else decoded and converted.
    fn frame(&mut self, f: i64) -> Result<Option<usize>, String> {
        if let Some(i) = self.slot_of(f) {
            return Ok(Some(i));
        }
        if !self.decode_to(f)? {
            return Ok(None);
        }
        self.convert_current(None).map(Some)
    }

    /// Reverse: the frames `[lo, f]` decoded from the key frame before `lo` and converted into
    /// the cache in one pass, so they can be shown backwards.
    fn fill_back(&mut self, f: i64) -> Result<(), String> {
        let room = self.cap.saturating_sub(3).max(1) as i64;
        let lo = (f - room + 1).max(0);
        self.dec.seek(lo)?;
        self.at = None;
        loop {
            match self.dec.next()? {
                Some((a, n)) => {
                    self.at = Some((a, n));
                    if a + n > lo && a <= f && self.slot_of(a).is_none() {
                        self.convert_current(Some(lo..f + 1))?;
                    }
                    if a + n > f {
                        return Ok(());
                    }
                }
                None => {
                    self.at = None;
                    return Ok(());
                }
            }
        }
    }

    /// Puts slot `i` (frame `f`) into the mailbox; wakes the UI when the box was empty.
    fn deliver(&mut self, i: usize, f: i64, playing: bool, on_event: &dyn Fn(Event)) {
        let wake = {
            let mut m = self.mail.lock().unwrap();
            let wake = m.pending.is_none();
            m.pending = Some((
                i,
                Shown {
                    frame: f,
                    texture: self.slots[i].tex.clone(),
                    playing,
                },
            ));
            wake
        };
        self.last = Some((f, i));
        self.still_sent = false;
        if wake {
            on_event(Event::Frame);
        }
    }

    /// The UI has not taken the last frame yet (it is busy): a playing player skips a frame.
    fn ui_busy(&self) -> bool {
        self.mail.lock().is_ok_and(|m| m.pending.is_some())
    }

    fn clamp(&self, f: i64) -> i64 {
        f.clamp(0, self.frames - 1)
    }

    /// The frame after `f` in the playing direction, over the cuts; None past the in / out.
    fn after(&self, f: i64, backward: bool) -> Option<i64> {
        let e = &self.edit;
        if backward {
            let p = e.prev_kept(f - 1)?;
            (p >= e.in_point()).then_some(p)
        } else {
            let n = e.next_kept(f + 1)?;
            (n < e.out_point()).then_some(n)
        }
    }

    fn run(mut self, rx: Receiver<Cmd>, on_event: &dyn Fn(Event)) {
        let mut playing = false;
        let mut backward = false;
        let mut speed = 1.0f64;
        let mut looping = false;
        let mut want: Option<i64> = None;
        let mut due = Instant::now();
        // The frame playback stands on (the last one shown).
        let mut pos: i64 = 0;
        loop {
            // Commands: all that wait. Idle: wait for one, or send the paused frame's CPU copy.
            let busy = playing || want.is_some();
            let first = if busy {
                match rx.try_recv() {
                    Ok(c) => Some(c),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => return,
                }
            } else if !self.still_sent && self.last.is_some() {
                match rx.recv_timeout(Duration::from_millis(150)) {
                    Ok(c) => Some(c),
                    Err(RecvTimeoutError::Timeout) => {
                        self.send_still(on_event);
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            } else {
                match rx.recv() {
                    Ok(c) => Some(c),
                    Err(_) => return,
                }
            };
            let mut cmds: Vec<Cmd> = first.into_iter().collect();
            while let Ok(c) = rx.try_recv() {
                cmds.push(c);
            }
            for c in cmds {
                match c {
                    // Only the last seek counts: a dragged playhead is caught up with, not replayed.
                    Cmd::Seek(f) => {
                        playing = false;
                        want = Some(self.clamp(f));
                    }
                    Cmd::Play {
                        from,
                        speed: s,
                        looping: l,
                        backward: b,
                    } => {
                        speed = s.max(0.05);
                        looping = l;
                        backward = b;
                        playing = true;
                        want = Some(self.clamp(from));
                        due = Instant::now();
                    }
                    Cmd::Pause => playing = false,
                    Cmd::Edit(e) => self.edit = e,
                    Cmd::Tone(lut) => {
                        self.conv.set_tone(lut.map(|l| *l));
                        for s in &mut self.slots {
                            s.frames = None;
                        }
                        if !playing && let Some((f, _)) = self.last {
                            want = Some(f);
                        }
                    }
                    Cmd::Quit => return,
                }
            }
            let step = Duration::from_secs_f64(1.0 / (self.fps * speed));
            if let Some(f) = want.take() {
                let got = if backward && playing {
                    match self.slot_of(f) {
                        Some(i) => Ok(Some(i)),
                        None => self.fill_back(f).and_then(|_| self.frame(f)),
                    }
                } else {
                    self.frame(f)
                };
                match got {
                    Ok(Some(i)) => {
                        pos = f;
                        self.deliver(i, f, playing, on_event);
                    }
                    Ok(None) => playing = false,
                    Err(e) => {
                        eprintln!("player: {e}");
                        playing = false;
                    }
                }
                if playing {
                    due = Instant::now() + step;
                }
                continue;
            }
            if !playing {
                continue;
            }
            let mut next = match self.after(pos, backward) {
                Some(n) => n,
                None => {
                    if looping {
                        let e = &self.edit;
                        let start = if backward {
                            e.prev_kept(e.out_point() - 1)
                        } else {
                            e.next_kept(e.in_point())
                        };
                        want = start;
                        if want.is_none() {
                            playing = false;
                        }
                        continue;
                    }
                    // The end: the last frame stays, marked as not playing.
                    playing = false;
                    if let Some((f, i)) = self.last {
                        self.deliver(i, f, false, on_event);
                    }
                    continue;
                }
            };
            let now = Instant::now();
            if due > now {
                std::thread::sleep(due - now);
            }
            // Far behind (a slow decode, a busy UI): the frames in between are skipped.
            let late =
                Instant::now().saturating_duration_since(due).as_secs_f64() * self.fps * speed;
            if late >= 1.0 || self.ui_busy() {
                let skip = (late as i64).max(1);
                for _ in 0..skip {
                    match self.after(next, backward) {
                        Some(n) => next = n,
                        None => break,
                    }
                }
                due = Instant::now();
            }
            let got = if backward {
                match self.slot_of(next) {
                    Some(i) => Ok(Some(i)),
                    None => self.fill_back(next).and_then(|_| self.frame(next)),
                }
            } else {
                self.frame(next)
            };
            match got {
                Ok(Some(i)) => {
                    pos = next;
                    self.deliver(i, next, true, on_event);
                    due += step;
                }
                Ok(None) => playing = false,
                Err(e) => {
                    eprintln!("player: {e}");
                    playing = false;
                }
            }
        }
    }

    /// The paused frame, read back once into CPU memory.
    fn send_still(&mut self, on_event: &dyn Fn(Event)) {
        self.still_sent = true;
        let Some((f, i)) = self.last else { return };
        let tex = self.slots[i].tex.clone();
        match self.conv.read(&tex) {
            Ok(rgba) => on_event(Event::Still {
                frame: f,
                raster: Arc::new(Raster::new(tex.width(), tex.height(), rgba)),
            }),
            Err(e) => eprintln!("player: readback: {e}"),
        }
    }
}

/// A device of its own for work away from the window (export, tests): DX12 on Windows, Metal on
/// macOS, the adapter's limits, NV12 textures where the adapter has them.
pub fn headless_gpu() -> Result<Gpu, String> {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::from_env().unwrap_or(if cfg!(target_os = "macos") {
        wgpu::Backends::METAL
    } else if cfg!(windows) {
        wgpu::Backends::DX12
    } else {
        wgpu::Backends::all()
    });
    let instance = wgpu::Instance::new(desc);
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .or_else(|_| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                force_fallback_adapter: true,
                ..Default::default()
            }))
        })
        .map_err(|e| format!("no GPU adapter: {e}"))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("znimok headless"),
        required_features: adapter.features() & wgpu::Features::TEXTURE_FORMAT_NV12,
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .map_err(|e| format!("no GPU device: {e}"))?;
    Ok(Gpu { device, queue })
}

/// Every frame of a recording in order, as RGBA in CPU memory (ZK-95: export). Decoded and
/// converted on the GPU like the player's, then read back.
pub struct Frames {
    dec: Box<dyn Decoder>,
    conv: Converter,
    out: Option<wgpu::Texture>,
}

impl Frames {
    pub fn open(gpu: &Gpu, source: &Source) -> Result<Self, String> {
        let conv = Converter::new(gpu)?;
        let dec = open_decoder(gpu, source)?;
        Ok(Self {
            dec,
            conv,
            out: None,
        })
    }

    pub fn size(&self) -> (u32, u32) {
        self.dec.size()
    }

    pub fn fps(&self) -> f64 {
        self.dec.fps()
    }

    /// Frames from the key frame at or before `frame` follow (macOS: from `frame`).
    pub fn seek(&mut self, frame: i64) -> Result<(), String> {
        self.dec.seek(frame)
    }

    /// The next decoded frame: its first index, how many frame slots it covers, its pixels.
    pub fn next_frame(&mut self) -> Result<Option<(i64, i64, Raster)>, String> {
        let Some((a, n)) = self.dec.next()? else {
            return Ok(None);
        };
        let (w, h) = self.dec.size();
        if self
            .out
            .as_ref()
            .is_none_or(|t| (t.width(), t.height()) != (w, h))
        {
            self.out = Some(self.conv.target(w, h));
        }
        let out = self.out.clone().unwrap();
        self.dec.convert(&self.conv, &out, false)?;
        let px = self.conv.read(&out)?;
        Ok(Some((a, n, Raster::new(w, h, px))))
    }
}

/// Film-strip thumbnails (ZK-92): the key frame nearest before each wanted frame (fast — no
/// decoding up to the exact frame), averaged down to `height` pixels on the GPU and read back.
/// Runs on its own thread with its own decoder; dropping the handle stops it.
pub struct Thumbs {
    stop: Arc<AtomicBool>,
}

impl Thumbs {
    pub fn start(
        gpu: Gpu,
        source: Source,
        frames: Vec<i64>,
        height: u32,
        tone: Option<[u8; 256]>,
        on_thumb: impl Fn(i64, Raster) + Send + 'static,
    ) -> Thumbs {
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let _ = std::thread::Builder::new()
            .name("znimok-thumbs".into())
            .spawn(move || {
                let mut dec = match open_decoder(&gpu, &source) {
                    Ok(d) => d,
                    Err(e) => {
                        eprintln!("thumbnails: {e}");
                        return;
                    }
                };
                let mut conv = match Converter::new(&gpu) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("thumbnails: {e}");
                        return;
                    }
                };
                conv.set_tone(tone);
                let (w, h) = dec.size();
                let th = height.clamp(8, h.max(8));
                let tw = ((u64::from(w) * u64::from(th)) / u64::from(h.max(1))).max(8) as u32;
                let out = conv.target(tw, th);
                for f in frames {
                    if s.load(Ordering::Relaxed) {
                        return;
                    }
                    let ok = dec.seek(f).and_then(|_| dec.next());
                    match ok {
                        Ok(Some(_)) => {}
                        Ok(None) => continue,
                        Err(e) => {
                            eprintln!("thumbnails: {e}");
                            return;
                        }
                    }
                    if let Err(e) = dec.convert(&conv, &out, true) {
                        eprintln!("thumbnails: {e}");
                        return;
                    }
                    match conv.read(&out) {
                        Ok(px) => on_thumb(f, Raster::new(tw, th, px)),
                        Err(e) => {
                            eprintln!("thumbnails: {e}");
                            return;
                        }
                    }
                }
            });
        Thumbs { stop }
    }
}

impl Drop for Thumbs {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
