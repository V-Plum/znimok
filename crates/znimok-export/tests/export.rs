//! Exports of a real recording (Windows): the synthetic source (its frame number as a barcode)
//! recorded through the real pipeline, then exported with a cut and a mark in time — the MP4's
//! frames are the kept ones in order, the mark is on its frames only; with a frame and a size the
//! MP4 has that size; the GIF has its frames and size; an untouched video is copied byte for
//! byte. Needs a DX12 adapter and a Media Foundation H.264 encoder; without them it says so and
//! passes.
#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use znimok_core::{Data, Document, IRect, Object, Raster, Rgb, Timeline, TimelinePart};
use znimok_export::{Job, Kind, Progress, run};
use znimok_format::video::{CODEC_H264, Video, VideoInfo};
use znimok_play::Source;
use znimok_video::clock::{Clock, ManualClock};
use znimok_video::recorder::{
    AudioLayout, Recorder, RecorderConfig, RecordingControl, Step, commit_part, part_path,
};
use znimok_video::settings::{Quality, encoder_config};
use znimok_video::traits::{Decoded, VideoDecoder};
use znimok_video_win::decoder::MfDecoder;
use znimok_video_win::interop::{Bridge, Gpu as RecGpu};
use znimok_video_win::sink::MfSink;
use znimok_video_win::source::{FramePool, PoolFormat, Source as RecSource};
use znimok_video_win::synthetic::{SyntheticSource, read_index};

const FPS: u32 = 30;
const W: u32 = 640;
const H: u32 = 360;
const SECONDS: f64 = 3.0;

fn record(dir: &Path) -> Option<PathBuf> {
    record_with(dir, false)
}

/// The synthetic recording, with a synthetic tone as its audio track when `audio`.
fn record_with(dir: &Path, audio: bool) -> Option<PathBuf> {
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
    let pool = FramePool::new(gpu.clone(), bridge.clone(), W, H, PoolFormat::Bgra8).ok()?;
    let src = SyntheticSource::new(pool.clone(), W, H, 1.0).ok()?;
    let clock = ManualClock::new();
    let ctl = RecordingControl::new();
    let final_path = dir.join(if audio { "clip-audio.mp4" } else { "clip.mp4" });
    let tracks: Vec<Box<dyn znimok_video::traits::AudioSource>> = if audio {
        vec![Box::new(znimok_video::synthetic::SyntheticAudio::new(
            znimok_video::traits::AudioKind::System,
            znimok_video::synthetic::Tone::FrameCoded { fps: FPS },
            clock.clone(),
        ))]
    } else {
        Vec::new()
    };
    let part = part_path(&final_path);
    let (g2, b2, p2, part2) = (gpu.clone(), bridge.clone(), pool.clone(), part.clone());
    let mut rec = Recorder::open(
        clock.clone(),
        RecSource::Synthetic(src),
        |tracks| {
            let cfg = encoder_config(W, H, FPS, Quality::Normal, tracks);
            MfSink::open_best(g2.clone(), b2.clone(), p2.clone(), &part2, &cfg, true)
                .map_err(znimok_video::VideoError::Encoder)
        },
        tracks,
        RecorderConfig {
            fps: FPS,
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
        if (clock.ticks() - start) as f64 / f as f64 >= SECONDS {
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
    Some(final_path)
}

/// Every frame of an MP4: (its barcode number, the pixel at a point).
fn frames_of(path: &Path, at: (u32, u32)) -> (u32, u32, Vec<(Option<u32>, u8)>) {
    let mut d = MfDecoder::open(path).unwrap();
    let (w, h) = (d.info().width, d.info().height);
    let mut out = Vec::new();
    while let Some(s) = d.next().unwrap() {
        if let Decoded::Video { frame, .. } = s {
            let luma = frame.luma(at.0.min(w - 1), at.1.min(h - 1));
            out.push((read_index(&frame), luma));
        }
    }
    (w, h, out)
}

#[test]
fn exports_cut_marked_framed_and_as_gif() {
    let dir = std::env::temp_dir().join(format!("znimok-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let Some(mp4) = record(&dir) else { return };
    let gpu = match znimok_play::headless_gpu() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("skipped: {e}");
            return;
        }
    };
    let frames = (SECONDS * f64::from(FPS)) as i64;
    let info = VideoInfo {
        width: W,
        height: H,
        fps_milli: FPS * 1000,
        frames: frames as u32,
        duration_hns: (SECONDS * 1e7) as i64,
        codec: CODEC_H264,
    };
    // The cut: frames 30..60 go. A white box over the lower part, on frames 70..80 only.
    let mut doc = Document::from_raster("v", Raster::solid(W, H, Rgb::new(40, 40, 40)));
    let mut mark = Object::new(
        IRect::new(20, 300, 120, 40),
        Data::Hide {
            mode: znimok_core::HideMode::Plate,
            strength: 50,
        },
    );
    mark.style.color = Rgb::WHITE;
    doc.push(mark);
    let id = doc.objects[0].id;
    let mut t = Timeline::whole(frames);
    t.parts = vec![
        TimelinePart {
            a: 0,
            b: 30,
            off: false,
        },
        TimelinePart {
            a: 30,
            b: 60,
            off: true,
        },
        TimelinePart {
            a: 60,
            b: frames,
            off: false,
        },
    ];
    t.marks.insert(id, (70, 80));
    doc.timeline = Some(t);
    let video = Video::new(info);
    let source = Source::File(mp4.clone());
    let job = |kind: Kind, dest: &str, doc: &Document, video: &Video| Job {
        source: source.clone(),
        doc: doc.clone(),
        video: video.clone(),
        kind,
        dest: dir.join(dest),
    };

    // MP4 with the cut and the mark.
    let p = Progress::default();
    let o = run(
        &job(Kind::Mp4 { sound: true }, "cut.mp4", &doc, &video),
        &gpu,
        &p,
    )
    .unwrap();
    assert!(!o.copied);
    let (w, h, got) = frames_of(&dir.join("cut.mp4"), (80, 320));
    assert_eq!((w, h), (W, H));
    assert_eq!(got.len(), 60, "30 frames cut out of 90");
    let nums: Vec<Option<u32>> = got.iter().map(|g| g.0).collect();
    let want: Vec<Option<u32>> = (0..30).chain(60..90).map(Some).collect();
    assert_eq!(nums, want, "the kept frames, in order");
    for (k, (n, luma)) in got.iter().enumerate() {
        let on = n.is_some_and(|n| (70..80).contains(&n));
        assert_eq!(
            *luma > 200,
            on,
            "output frame {k} (source {n:?}): the mark only on 70..80, luma {luma}"
        );
    }
    assert_eq!(p.done.load(std::sync::atomic::Ordering::Relaxed), 1000);

    // A frame and a size: 320 × 180 of the frame's centre, out at 160 × 90.
    let mut framed = doc.clone();
    framed.crop = Some(IRect::new(160, 90, 320, 180));
    let mut v2 = video.clone();
    v2.out_size = Some((160, 90));
    run(
        &job(Kind::Mp4 { sound: false }, "framed.mp4", &framed, &v2),
        &gpu,
        &Progress::default(),
    )
    .unwrap();
    let (w, h, got) = frames_of(&dir.join("framed.mp4"), (0, 0));
    assert_eq!((w, h, got.len()), (160, 90, 60));

    // GIF at 10 fps, 320 wide: two seconds kept → 20 frames.
    run(
        &job(
            Kind::Gif {
                width: 320,
                fps: 10.0,
                dither: true,
            },
            "cut.gif",
            &doc,
            &video,
        ),
        &gpu,
        &Progress::default(),
    )
    .unwrap();
    let bytes = std::fs::read(dir.join("cut.gif")).unwrap();
    let mut opts = gif::DecodeOptions::new();
    opts.set_color_output(gif::ColorOutput::RGBA);
    let mut d = opts.read_info(&bytes[..]).unwrap();
    assert_eq!((d.width(), d.height()), (320, 180));
    let mut total_cs = 0u32;
    let mut n = 0;
    while let Some(f) = d.read_next_frame().unwrap() {
        total_cs += u32::from(f.delay);
        n += 1;
    }
    assert!((2..=20).contains(&n), "{n} frames");
    assert_eq!(total_cs, 200, "two seconds in hundredths");
    let est = znimok_export::estimate_gif(
        &job(
            Kind::Gif {
                width: 320,
                fps: 10.0,
                dither: true,
            },
            "x.gif",
            &doc,
            &video,
        ),
        &gpu,
        320,
        10.0,
        true,
    )
    .unwrap();
    let real = bytes.len() as f64;
    assert!(
        (est as f64) > real * 0.3 && (est as f64) < real * 3.0,
        "estimate {est} vs {real}"
    );

    // Untouched: the MP4 as it is.
    let plain = Document::from_raster("v", Raster::solid(W, H, Rgb::new(40, 40, 40)));
    let mut plain = plain;
    plain.timeline = Some(Timeline::whole(frames));
    let o = run(
        &job(Kind::Mp4 { sound: true }, "same.mp4", &plain, &video),
        &gpu,
        &Progress::default(),
    )
    .unwrap();
    assert!(o.copied);
    assert_eq!(
        std::fs::read(dir.join("same.mp4")).unwrap(),
        std::fs::read(&mp4).unwrap()
    );

    // Cancelled: no file is left.
    let p = Progress::default();
    p.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    let r = run(
        &job(Kind::Mp4 { sound: false }, "gone.mp4", &doc, &video),
        &gpu,
        &p,
    );
    assert_eq!(r.err().as_deref(), Some(znimok_export::CANCELLED));
    assert!(!dir.join("gone.mp4").exists() && !dir.join("gone.mp4.part").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The sound: the track cut with the video (two of three seconds kept), left out when muted or
/// when the export has no sound.
#[test]
fn the_sound_follows_the_cuts() {
    use znimok_video::check::mp4::read_mp4_file;
    let dir = std::env::temp_dir().join(format!("znimok-export-a-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let Some(mp4) = record_with(&dir, true) else {
        return;
    };
    let Ok(gpu) = znimok_play::headless_gpu() else {
        return;
    };
    let frames = (SECONDS * f64::from(FPS)) as i64;
    let info = VideoInfo {
        width: W,
        height: H,
        fps_milli: FPS * 1000,
        frames: frames as u32,
        duration_hns: (SECONDS * 1e7) as i64,
        codec: CODEC_H264,
    };
    let mut doc = Document::from_raster("v", Raster::solid(W, H, Rgb::new(40, 40, 40)));
    let mut t = Timeline::whole(frames);
    t.parts = vec![
        TimelinePart {
            a: 0,
            b: 30,
            off: false,
        },
        TimelinePart {
            a: 30,
            b: 60,
            off: true,
        },
        TimelinePart {
            a: 60,
            b: frames,
            off: false,
        },
    ];
    doc.timeline = Some(t);
    let mut video = Video::new(info);
    video.audio = vec![znimok_format::video::AudioTrack::default()];
    let run_to = |name: &str, video: &Video, sound: bool| {
        let dest = dir.join(name);
        run(
            &Job {
                source: Source::File(mp4.clone()),
                doc: doc.clone(),
                video: video.clone(),
                kind: Kind::Mp4 { sound },
                dest: dest.clone(),
            },
            &gpu,
            &Progress::default(),
        )
        .unwrap();
        read_mp4_file(&dest).unwrap()
    };
    let i = run_to("sound.mp4", &video, true);
    let a = i.audio().expect("an audio track");
    let ad = a.duration_s(i.timescale);
    assert!((ad - 2.0).abs() < 0.35, "audio {ad} s for 2 s kept");
    assert!(
        (i.duration_s() - 2.0).abs() < 0.35,
        "video {} s",
        i.duration_s()
    );
    assert!(run_to("silent.mp4", &video, false).audio().is_none());
    let mut muted = video.clone();
    muted.audio[0].muted = true;
    assert!(run_to("muted.mp4", &muted, true).audio().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
