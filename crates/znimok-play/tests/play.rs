//! The player on a real recording (Windows): the synthetic source (its frame number as a
//! barcode) recorded through the real pipeline into an MP4, then played by `znimok-play` on a wgpu
//! device of its own — exact seeks, forward and reverse playback, the MP4 read in place from the
//! byte ranges of a bigger file, thumbnails, the tone table, the paused frame's CPU copy.
//! Needs a DX12 adapter (WARP will do) and a Media Foundation H.264 encoder; without them the
//! test says so and passes.
#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use znimok_play::{Converter, Event, Gpu, Player, Source, Thumbs};
use znimok_video::clock::{Clock, ManualClock};
use znimok_video::recorder::{
    AudioLayout, Recorder, RecorderConfig, RecordingControl, Step, commit_part, part_path,
};
use znimok_video::settings::{Quality, encoder_config};
use znimok_video_win::interop::{Bridge, Gpu as RecGpu};
use znimok_video_win::sink::MfSink;
use znimok_video_win::source::{FramePool, PoolFormat, Source as RecSource};
use znimok_video_win::synthetic::{SyntheticSource, block};

const FPS: u32 = 30;
const W: u32 = 640;
const H: u32 = 360;
const SECONDS: f64 = 3.0;

/// Records the synthetic source; None when this machine cannot.
fn record(dir: &Path) -> Option<PathBuf> {
    record_clip(dir, W, H, FPS, SECONDS)
}

/// Records the synthetic source at any size; None when this machine cannot.
fn record_clip(dir: &Path, w: u32, h: u32, fps: u32, seconds: f64) -> Option<PathBuf> {
    znimok_video_win::mf::startup().unwrap();
    let gpu = Rc::new(
        RecGpu::new(None)
            .map_err(|e| eprintln!("skipped: {e}"))
            .ok()?,
    );
    let bridge = Rc::new(
        Bridge::new(&gpu)
            .map_err(|e| eprintln!("skipped: {e}"))
            .ok()?,
    );
    let pool = FramePool::new(gpu.clone(), bridge.clone(), w, h, PoolFormat::Bgra8).ok()?;
    let src = SyntheticSource::new(pool.clone(), w, h, 1.0).ok()?;
    let clock = ManualClock::new();
    let ctl = RecordingControl::new();
    let final_path = dir.join("clip.mp4");
    let part = part_path(&final_path);
    let (g2, b2, p2, part2) = (gpu.clone(), bridge.clone(), pool.clone(), part.clone());
    let mut rec = Recorder::open(
        clock.clone(),
        RecSource::Synthetic(src),
        |tracks| {
            let cfg = encoder_config(w, h, fps, Quality::Normal, tracks);
            MfSink::open_best(g2.clone(), b2.clone(), p2.clone(), &part2, &cfg, true)
                .map_err(znimok_video::VideoError::Encoder)
        },
        Vec::new(),
        RecorderConfig {
            fps,
            audio_layout: AudioLayout::Separate,
            probe: None,
        },
        ctl.clone(),
    )
    .map_err(|e| eprintln!("skipped (no encoder): {e}"))
    .ok()?;
    let start = clock.ticks();
    let f = clock.frequency();
    loop {
        if (clock.ticks() - start) as f64 / f as f64 >= seconds {
            ctl.stop();
        }
        if rec.step() == Step::Stopped {
            break;
        }
        clock.advance(Duration::from_millis(5));
    }
    let (result, sink) = rec.finish();
    drop(sink);
    assert!(result.error.is_none(), "{:?}", result.error);
    assert!(commit_part(&part, &final_path, &result).unwrap());
    // ZNIMOK_PLAY_KEEP=<file>: the clip is kept there (the macOS test plays it, tests/clip.rs).
    if let Some(keep) = std::env::var_os("ZNIMOK_PLAY_KEEP") {
        std::fs::copy(&final_path, keep).unwrap();
    }
    Some(final_path)
}

/// A device like the app's: DX12, with NV12 textures when the adapter has them.
fn device(nv12: bool) -> Option<Gpu> {
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::DX12;
    let instance = wgpu::Instance::new(desc);
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .or_else(|_| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                force_fallback_adapter: true,
                ..Default::default()
            }))
        })
        .ok()?;
    let features = if nv12 {
        adapter.features() & wgpu::Features::TEXTURE_FORMAT_NV12
    } else {
        wgpu::Features::empty()
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: features,
        ..Default::default()
    }))
    .ok()?;
    eprintln!(
        "wgpu: {} (NV12 textures: {})",
        adapter.get_info().name,
        !features.is_empty()
    );
    Some(Gpu { device, queue })
}

/// The barcode's number in an RGBA frame.
fn read_rgba(px: &[u8], w: u32, h: u32) -> Option<u32> {
    let blk = block(w);
    let mut n = 0u32;
    for i in 0..32u32 {
        let (x, y) = (blk + i * blk + blk / 2, blk + blk / 2);
        if x >= w || y >= h {
            return None;
        }
        let o = ((y * w + x) * 4) as usize;
        let on = (u32::from(px[o]) + u32::from(px[o + 1]) + u32::from(px[o + 2])) / 3 > 125;
        if i < 16 {
            n |= u32::from(on) << i;
        } else if on == ((n >> (i - 16)) & 1 == 1) {
            return None;
        }
    }
    Some(n)
}

struct Rig {
    conv: Converter,
    player: Player,
    rx: mpsc::Receiver<Event>,
}

fn open(gpu: &Gpu, source: Source, frames: i64) -> Rig {
    let (tx, rx) = mpsc::channel();
    let player = Player::open(gpu.clone(), source, frames, move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
        Event::Opened(i) => {
            eprintln!(
                "player: {}×{} at {} fps, path {}",
                i.width, i.height, i.fps, i.path
            );
            let nv12 = gpu
                .device
                .features()
                .contains(wgpu::Features::TEXTURE_FORMAT_NV12);
            // Without a D3D11 video device (WARP on CI) the decoder is the software one.
            let want = if nv12 { "gpu" } else { "upload" };
            assert!(i.path == want || i.path == "software", "{} instead of {want}", i.path);
        }
        Event::Failed(e) => panic!("player failed: {e}"),
        _ => panic!("no Opened first"),
    }
    Rig {
        conv: Converter::new(gpu).unwrap(),
        player,
        rx,
    }
}

impl Rig {
    /// The next frame the player shows: its number and the number its pixels carry.
    fn shown(&self) -> Option<(i64, Option<u32>, bool)> {
        loop {
            match self.rx.recv_timeout(Duration::from_secs(10)).ok()? {
                Event::Frame => {
                    let s = self.player.take()?;
                    let px = self.conv.read(&s.texture).unwrap();
                    return Some((s.frame, read_rgba(&px, W, H), s.playing));
                }
                Event::Still { .. } => continue,
                Event::Failed(e) => panic!("{e}"),
                Event::Opened(_) => continue,
            }
        }
    }

    fn seek(&self, f: i64) -> (i64, Option<u32>) {
        self.player.seek(f);
        let (got, bar, _) = self.shown().expect("a frame after a seek");
        (got, bar)
    }
}

#[test]
fn plays_a_recording_on_the_gpu() {
    let dir = std::env::temp_dir().join(format!("znimok-play-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let Some(mp4) = record(&dir) else { return };
    let Some(gpu) = device(true) else {
        eprintln!("skipped: no wgpu device");
        return;
    };
    let frames = (SECONDS * f64::from(FPS)) as i64;
    let r = open(&gpu, Source::File(mp4.clone()), frames);

    // Exact seeks, anywhere, backwards too.
    for f in [0, 45, 89, 10, 11, 60, 7] {
        let (got, bar) = r.seek(f);
        assert_eq!(got, f);
        assert_eq!(bar, Some(f as u32), "seek to {f}");
    }

    // The paused frame's CPU copy comes after a moment of rest.
    let still = loop {
        match r.rx.recv_timeout(Duration::from_secs(5)).expect("a still") {
            Event::Still { frame, raster } => break (frame, raster),
            _ => continue,
        }
    };
    assert_eq!(still.0, 7);
    assert_eq!(read_rgba(&still.1.rgba, W, H), Some(7));

    // Forward at 2×: the frames come in order, each carrying its own number.
    r.player.play(20, 2.0, false, false);
    let mut last = -1;
    let mut seen = 0;
    while let Some((f, bar, playing)) = r.shown() {
        assert!(f > last || !playing, "forward: {f} after {last}");
        assert_eq!(bar, Some(f as u32), "forward frame {f}");
        last = f;
        seen += 1;
        if !playing {
            break;
        }
    }
    assert_eq!(last, frames - 1, "played to the end");
    assert!(seen >= 10, "only {seen} frames shown");

    // Reverse from 70: descending, exact.
    r.player.play(70, 1.0, false, true);
    let mut last = i64::MAX;
    let mut seen = 0;
    while let Some((f, bar, playing)) = r.shown() {
        if !playing {
            break;
        }
        assert!(f < last, "reverse: {f} after {last}");
        assert_eq!(bar, Some(f as u32), "reverse frame {f}");
        last = f;
        seen += 1;
        if f < 40 {
            r.player.pause();
            break;
        }
    }
    assert!(seen >= 10, "only {seen} reverse frames");

    // Whatever the reverse run still delivered is taken first.
    while r.rx.recv_timeout(Duration::from_millis(400)).is_ok() {
        let _ = r.player.take();
    }
    // The tone table is applied: a negative inverts every bit of the barcode, which then reads
    // as the complement of the number (the pattern's two halves swap roles).
    assert_eq!(r.seek(50), (50, Some(50)));
    let neg: [u8; 256] = std::array::from_fn(|i| 255 - i as u8);
    r.player.set_tone(Some(neg));
    let (f, bar, _) = r.shown().expect("re-converted after the tone");
    assert_eq!((f, bar), (50, Some(!50u32 & 0xFFFF)), "the negative");
    drop(r);

    // In place: the MP4 as two byte ranges of a bigger file.
    let data = std::fs::read(&mp4).unwrap();
    let cut = data.len() / 3;
    let mut doc = b"ZNIMOK-TEST-HEADER".to_vec();
    let a0 = doc.len() as u64;
    doc.extend_from_slice(&data[..cut]);
    doc.extend_from_slice(b"--between--");
    let b0 = doc.len() as u64;
    doc.extend_from_slice(&data[cut..]);
    doc.extend_from_slice(b"TRAILER");
    let file = dir.join("doc.bin");
    std::fs::write(&file, &doc).unwrap();
    let src = Source::InFile {
        path: file,
        ranges: vec![a0..a0 + cut as u64, b0..b0 + (data.len() - cut) as u64],
        cache: dir.join("cache"),
    };
    let r = open(&gpu, src.clone(), frames);
    for f in [33, 3, 88] {
        assert_eq!(r.seek(f), (f, Some(f as u32)), "in place, frame {f}");
    }
    drop(r);

    // Thumbnails: every one arrives, small, from a key frame at or before the wanted one.
    let (tx, rx) = mpsc::channel();
    let _t = Thumbs::start(
        gpu.clone(),
        src,
        vec![0, 30, 60, 89],
        32,
        None,
        move |f, r| {
            let _ = tx.send((f, r.width, r.height));
        },
    );
    let mut got = Vec::new();
    while let Ok(t) = rx.recv_timeout(Duration::from_secs(10)) {
        got.push(t);
        if got.len() == 4 {
            break;
        }
    }
    assert_eq!(got.len(), 4, "{got:?}");
    assert!(got.iter().all(|(_, w, h)| *h == 32 && *w == 56), "{got:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A device without NV12 textures: the planes go through CPU memory, the same shader converts.
#[test]
fn plays_through_the_upload_path() {
    let dir = std::env::temp_dir().join(format!("znimok-play-up-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let Some(mp4) = record(&dir) else { return };
    let Some(gpu) = device(false) else {
        eprintln!("skipped: no wgpu device");
        return;
    };
    let r = open(&gpu, Source::File(mp4), (SECONDS * f64::from(FPS)) as i64);
    for f in [0, 61, 12] {
        assert_eq!(r.seek(f), (f, Some(f as u32)), "upload path, frame {f}");
    }
    drop(r);
    let _ = std::fs::remove_dir_all(&dir);
}

/// CPU time of this process (all threads).
fn cpu_time() -> Duration {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let (mut a, mut b, mut k, mut u) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: out-pointers to locals.
    unsafe {
        let _ = GetProcessTimes(GetCurrentProcess(), &mut a, &mut b, &mut k, &mut u);
    }
    let t = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
    Duration::from_nanos((t(k) + t(u)) * 100)
}

/// 4K at 60 fps: how many frames reach the UI at 1× and what it costs, and how fast seeks are.
/// `cargo test --release -p znimok-play --test play bench_4k -- --ignored --nocapture`
#[test]
#[ignore]
fn bench_4k() {
    let dir = std::env::temp_dir().join(format!("znimok-play-4k-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (w, h, fps, secs) = (3840, 2160, 60, 6.0);
    let Some(mp4) = record_clip(&dir, w, h, fps, secs) else {
        return;
    };
    let Some(gpu) = device(true) else { return };
    let frames = (secs * f64::from(fps)) as i64;
    let (tx, rx) = mpsc::channel();
    let player = Player::open(gpu.clone(), Source::File(mp4), frames, move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
        Event::Opened(i) => eprintln!(
            "4K: {}×{} at {} fps, path {}",
            i.width, i.height, i.fps, i.path
        ),
        Event::Failed(e) => panic!("{e}"),
        _ => {}
    }
    // Seeks: 30 random frames, the time from the command to the frame in the mailbox.
    let mut seeks = Vec::new();
    for i in 1..=30i64 {
        // 30 different frames scattered over the clip (37 and 360 share no divisor).
        let f = (i * 37 + 5) % frames;
        let t = std::time::Instant::now();
        player.seek(f);
        loop {
            match rx.recv_timeout(Duration::from_secs(5)).expect("frame") {
                Event::Frame => {
                    if player.take().is_some_and(|s| s.frame == f) {
                        break;
                    }
                }
                _ => continue,
            }
        }
        seeks.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    seeks.sort_by(f64::total_cmp);
    // Playback at 1× from the start: frames shown and the CPU it took.
    player.seek(0);
    while let Ok(e) = rx.recv_timeout(Duration::from_millis(500)) {
        if matches!(e, Event::Frame) {
            let _ = player.take();
        }
    }
    let cpu0 = cpu_time();
    let t0 = std::time::Instant::now();
    player.play(0, 1.0, false, false);
    let mut shown = 0;
    let mut last = 0;
    loop {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Event::Frame) => {
                if let Some(s) = player.take() {
                    shown += 1;
                    last = s.frame;
                    if !s.playing {
                        break;
                    }
                }
            }
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    let wall = t0.elapsed().as_secs_f64();
    let cpu = (cpu_time() - cpu0).as_secs_f64();
    eprintln!(
        "4K60 1x: {shown} of {frames} frames shown (last {last}) in {wall:.2} s = {:.1} fps; CPU {:.1} % of one core",
        shown as f64 / wall,
        cpu / wall * 100.0
    );
    eprintln!(
        "seek: median {:.1} ms, p90 {:.1} ms, max {:.1} ms",
        seeks[seeks.len() / 2],
        seeks[seeks.len() * 9 / 10],
        seeks[seeks.len() - 1]
    );
    drop(player);
    let _ = std::fs::remove_dir_all(&dir);
}
