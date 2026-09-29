//! The whole Windows path on a synthetic source: pool → shader → encoder → MP4 → decoder, driven
//! by a manual clock so every slot is written exactly once. Needs a DX12 adapter (WARP will do)
//! and a Media Foundation H.264 encoder; without them the tests say so and pass.
#![cfg(windows)]

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use znimok_video::check::mp4::{Fps, Mp4Expect, check_mp4, read_mp4_file};
use znimok_video::check::probe::check_bars;
use znimok_video::clock::{Clock, ManualClock};
use znimok_video::recorder::{
    AudioLayout, Recorder, RecorderConfig, RecordingControl, Step, commit_part, part_path,
};
use znimok_video::settings::{Quality, encoder_config};
use znimok_video::synthetic::{SyntheticAudio, Tone};
use znimok_video::traits::{AudioKind, AudioSource, Decoded, VideoDecoder};
use znimok_video_win::decoder::MfDecoder;
use znimok_video_win::interop::{Bridge, Gpu};
use znimok_video_win::shader::{
    CursorImage, FrameGeometry, OutFormat, Overlay, Ring, Stage, local_output, read_back,
};
use znimok_video_win::sink::MfSink;
use znimok_video_win::source::{FramePool, PoolFormat, SharedPool, Source};
use znimok_video_win::synthetic::{Pattern, SyntheticSource, f16, patch_error, read_index};

const FPS: u32 = 30;
const W: u32 = 640;
const H: u32 = 360;

struct Rig {
    gpu: Rc<Gpu>,
    bridge: Rc<Bridge>,
    dir: PathBuf,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn rig(tag: &str) -> Option<Rig> {
    znimok_video_win::mf::startup().unwrap();
    let gpu = match Gpu::new(None) {
        Ok(g) => Rc::new(g),
        Err(e) => {
            eprintln!("пропущено: {e}");
            return None;
        }
    };
    let bridge = Rc::new(Bridge::new(&gpu).unwrap());
    let dir = std::env::temp_dir().join(format!("znimok-video-win-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    eprintln!("GPU: {} / {}", gpu.name, bridge.adapter);
    Some(Rig { gpu, bridge, dir })
}

fn pool(r: &Rig, fmt: PoolFormat) -> SharedPool {
    FramePool::new(r.gpu.clone(), r.bridge.clone(), W, H, fmt).unwrap()
}

/// Record `seconds` of the synthetic source; `None` when no encoder could be opened.
fn record(
    r: &Rig,
    fmt: PoolFormat,
    white: f32,
    hardware: bool,
    audio: Vec<Box<dyn AudioSource>>,
    seconds: f64,
    name: &str,
) -> Option<(znimok_video::recorder::RecordingResult, PathBuf, bool)> {
    let pool = pool(r, fmt);
    let src = SyntheticSource::new(pool.clone(), W, H, white).unwrap();
    let clock = ManualClock::new();
    let ctl = RecordingControl::new();
    let final_path = r.dir.join(format!("{name}.mp4"));
    let part = part_path(&final_path);
    let (gpu, bridge, pool2, part2) = (r.gpu.clone(), r.bridge.clone(), pool.clone(), part.clone());
    let mut hw = None;
    let rec = Recorder::open(
        clock.clone(),
        Source::Synthetic(src),
        |tracks| {
            let cfg = encoder_config(W, H, FPS, Quality::Normal, tracks);
            let s = MfSink::open_best(
                gpu.clone(),
                bridge.clone(),
                pool2.clone(),
                &part2,
                &cfg,
                hardware,
            )
            .map_err(znimok_video::VideoError::Encoder)?;
            eprintln!("кодувальник: {}", s.encoder);
            hw = Some(s.hardware);
            Ok(s)
        },
        audio,
        RecorderConfig {
            fps: FPS,
            audio_layout: AudioLayout::Separate,
            probe: None,
        },
        ctl.clone(),
    );
    let mut rec = match rec {
        Ok(r) => r,
        Err(e) => {
            eprintln!("пропущено (немає кодувальника): {e}");
            return None;
        }
    };
    let hw = hw.unwrap_or(false);
    let start = clock.ticks();
    let f = clock.frequency();
    loop {
        let t = (clock.ticks() - start) as f64 / f as f64;
        if t >= seconds {
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
    assert!(result.committed);
    assert!(commit_part(&part, &final_path, &result).unwrap());
    Some((result, final_path, hw))
}

/// Decode every frame: (timestamp, bar number) and the worst patch error.
fn decode(path: &std::path::Path) -> (Vec<(i64, i64)>, u8, f64) {
    let mut d = MfDecoder::open(path).unwrap();
    let fps = d.info().fps;
    let mut frames = Vec::new();
    let mut worst = 0u8;
    while let Some(s) = d.next().unwrap() {
        if let Decoded::Video {
            time_hns, frame, ..
        } = s
        {
            if frames.is_empty()
                && let Some(dir) = std::env::var_os("ZNIMOK_DUMP")
            {
                let mut pgm = format!("P5\n{} {}\n255\n", frame.width, frame.height).into_bytes();
                pgm.extend_from_slice(&frame.y);
                let name = path.file_stem().unwrap().to_string_lossy().to_string();
                std::fs::write(std::path::Path::new(&dir).join(format!("{name}.pgm")), pgm)
                    .unwrap();
            }
            let n = read_index(&frame).map_or(-1, i64::from);
            if frames.is_empty() {
                for (i, c) in znimok_video_win::synthetic::PATCHES.iter().enumerate() {
                    eprintln!(
                        "  пляма {i} {c:?}: хочу {:?}, є {:?}",
                        znimok_video_win::synthetic::rgb_to_ycbcr(*c),
                        znimok_video_win::synthetic::patch_ycc(&frame, i)
                    );
                }
            }
            worst = worst.max(patch_error(&frame));
            frames.push((time_hns, n));
        }
    }
    (frames, worst, fps)
}

fn check_file(path: &std::path::Path, slots: i64, seconds: f64) {
    let info = read_mp4_file(path).unwrap();
    let checks = check_mp4(
        &info,
        &Mp4Expect {
            fps: Some(Fps { num: FPS, den: 1 }),
            keyframe_interval: Some(znimok_video::settings::keyframe_interval(FPS)),
            slots: Some(slots),
            duration_s: Some(seconds),
            duration_tol_s: 0.2,
        },
    );
    for c in &checks {
        eprintln!("{c}");
    }
    assert!(znimok_video::check::all_ok(&checks));
    let (frames, worst, fps) = decode(path);
    {
        let v = info.video().unwrap();
        eprintln!(
            "перші: {:?}; ctts {} записів, edits {:?}",
            &frames[..frames.len().min(5)],
            v.ctts.len(),
            v.edits
        );
    }
    assert!(frames.len() as i64 >= slots - 2, "{} кадрів", frames.len());
    let bars = check_bars(&frames, FPS, 1);
    eprintln!("{bars:?}, найгірша пляма {worst}, fps {fps}");
    assert_eq!(bars.sync_bad, 0);
    assert_eq!(bars.seq_breaks, 0, "{frames:?}");
    assert!(bars.lag_max <= 1);
    assert!(frames.iter().all(|(_, n)| *n >= 0), "штрихкод не читається");
    assert!(worst <= 12, "кольори плям розійшлися на {worst}");
}

#[test]
fn bgra8_frames_reach_the_file_in_order() {
    let Some(r) = rig("bgra") else { return };
    let Some((res, path, hw)) = record(&r, PoolFormat::Bgra8, 80.0, true, vec![], 2.0, "bgra")
    else {
        return;
    };
    eprintln!(
        "апаратний: {hw}, кадрів {}, семплів {}",
        res.frames, res.samples
    );
    assert!((res.frames - 60).abs() <= 2);
    check_file(&path, res.frames, 2.0);
}

/// scRGB FP16 at an SDR white of 240 nits: the tone maps it back to the same sRGB colours.
#[test]
fn scrgb_frames_are_tone_mapped() {
    let Some(r) = rig("scrgb") else { return };
    let Some((res, path, _)) = record(&r, PoolFormat::Rgba16F, 240.0, true, vec![], 1.5, "scrgb")
    else {
        return;
    };
    check_file(&path, res.frames, 1.5);
}

#[test]
fn software_encoder_is_the_fallback() {
    let Some(r) = rig("sw") else { return };
    let Some((res, path, hw)) = record(&r, PoolFormat::Bgra8, 80.0, false, vec![], 1.0, "sw")
    else {
        return;
    };
    assert!(!hw);
    check_file(&path, res.frames, 1.0);
}

#[test]
fn an_audio_track_is_muxed() {
    let Some(r) = rig("audio") else { return };
    let clock = ManualClock::new();
    let tone = Box::new(SyntheticAudio::new(
        AudioKind::System,
        Tone::FrameCoded { fps: FPS },
        clock.clone(),
    ));
    // The recorder's clock is its own manual clock; the tone's clock only needs to move, which
    // the recorder's sleeps do not do here — advance both from the loop below instead.
    let pool = pool(&r, PoolFormat::Bgra8);
    let src = SyntheticSource::new(pool.clone(), W, H, 80.0).unwrap();
    let ctl = RecordingControl::new();
    let final_path = r.dir.join("audio.mp4");
    let part = part_path(&final_path);
    let (gpu, bridge, pool2, part2) = (r.gpu.clone(), r.bridge.clone(), pool.clone(), part.clone());
    let rec = Recorder::open(
        clock.clone(),
        Source::Synthetic(src),
        |tracks| {
            let cfg = encoder_config(W, H, FPS, Quality::Normal, tracks);
            MfSink::open_best(
                gpu.clone(),
                bridge.clone(),
                pool2.clone(),
                &part2,
                &cfg,
                true,
            )
            .map_err(znimok_video::VideoError::Encoder)
        },
        vec![tone],
        RecorderConfig::new(FPS),
        ctl.clone(),
    );
    let mut rec = match rec {
        Ok(r) => r,
        Err(e) => {
            eprintln!("пропущено: {e}");
            return;
        }
    };
    assert_eq!(rec.audio_tracks(), 1, "the AAC track was not added");
    let start = clock.ticks();
    let f = clock.frequency();
    loop {
        if (clock.ticks() - start) as f64 / f as f64 >= 2.0 {
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
    let info = read_mp4_file(&final_path).unwrap();
    let a = info.audio().expect("an audio track in the file");
    let ad = a.duration_s(info.timescale);
    eprintln!(
        "звук: {} семплів, {ad:.2} с; відео {:.2} с",
        a.sample_count,
        info.duration_s()
    );
    assert!(
        (ad - info.duration_s()).abs() < 0.35,
        "audio {ad} vs video {}",
        info.duration_s()
    );
}

/// The overlay: click rings and the cursor are drawn after the tone, exactly where asked.
#[test]
fn rings_and_cursor_are_drawn() {
    let Some(r) = rig("overlay") else { return };
    let gpu = &r.gpu;
    let pool = pool(&r, PoolFormat::Bgra8);
    let mut pat = Pattern::new(W, H);
    pat.draw(7);
    let (slot, ready) = pool.borrow_mut().put_cpu(&pat.to_bgra8(), W * 4, W, H);
    let mut stage = Stage::new(gpu, OutFormat::Rgba8).unwrap();
    let out = local_output(gpu, OutFormat::Rgba8, W, H);
    let view = out.create_view(&Default::default());
    let geo = FrameGeometry {
        mode: 0,
        white: 80.0,
        crop: (0, 0, W, H),
        out: (W, H),
    };
    let src = pool.borrow().slots[slot].tex.view.clone();
    let render = |stage: &mut Stage, ov: &Overlay| {
        r.bridge.wait_in_wgpu(gpu, ready).unwrap();
        stage.render(gpu, &src, &view, &geo, ov);
        read_back(gpu, &out)
    };
    let plain = render(&mut stage, &Overlay::default());
    let px = |img: &[u8], x: u32, y: u32| {
        let i = ((y * W + x) * 4) as usize;
        [img[i], img[i + 1], img[i + 2]]
    };
    // The plain render is the pattern (RGBA8 out of BGRA8 in).
    assert_eq!(px(&plain, 300, 200), {
        let i = ((200 * W + 300) * 3) as usize;
        [pat.rgb[i], pat.rgb[i + 1], pat.rgb[i + 2]]
    });
    // An inverting 4×4 cursor at (100, 100): a = −1, rgb = 1.
    let mut cur = Vec::new();
    for _ in 0..16 {
        for v in [1.0f32, 1.0, 1.0, -1.0] {
            cur.extend_from_slice(&f16(v).to_le_bytes());
        }
    }
    let ov = Overlay {
        rings: vec![Ring {
            x: 400.0,
            y: 250.0,
            radius: 20.0,
            alpha: 1.0,
            kind: 0,
        }],
        cursor: Some(CursorImage {
            x: 100,
            y: 100,
            width: 4,
            height: 4,
            data: std::sync::Arc::new(cur),
            generation: 1,
        }),
        scale: 1.0,
        ring_color: [1.0, 0.0, 0.0],
    };
    let with = render(&mut stage, &ov);
    let inv = px(&plain, 101, 101).map(|v| 255 - v);
    let got = px(&with, 101, 101);
    assert!(
        (0..3).all(|c| got[c].abs_diff(inv[c]) <= 2),
        "cursor inverts: {got:?} vs {inv:?}"
    );
    assert_eq!(
        px(&with, 106, 101),
        px(&plain, 106, 101),
        "outside the cursor"
    );
    // On the ring: red; at its centre: the dot (red); well outside: untouched.
    let on_ring = px(&with, 420, 250);
    assert!(
        on_ring[0] > 200 && on_ring[1] < 60 && on_ring[2] < 60,
        "{on_ring:?}"
    );
    let centre = px(&with, 400, 250);
    assert!(centre[0] > 150, "{centre:?}");
    assert_eq!(px(&with, 460, 250), px(&plain, 460, 250));
}
