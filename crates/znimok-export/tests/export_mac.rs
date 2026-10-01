//! Exports on macOS (ZK-205): a synthetic recording with a tone made by the real VideoToolbox,
//! exported with a cut and a mark — the MP4 has the kept frames and the sound of the kept time;
//! the HTML page and the developer report carry it. Needs a Metal device (any Apple Silicon Mac,
//! a CI runner too); without one it says so and passes.
#![cfg(target_os = "macos")]

use znimok_core::{Data, Document, IRect, Object, Raster, Rgb, Timeline, TimelinePart};
use znimok_export::{Job, Kind, Progress, run};
use znimok_format::video::{AudioTrack, CODEC_H264, Video, VideoInfo};
use znimok_play::Source;
use znimok_video::check::mp4::read_mp4_file;
use znimok_video_mac::writer::{AvWriter, WriterConfig};

const W: u32 = 320;
const H: u32 = 240;
const FPS: u32 = 30;
const FRAMES: i64 = 90;

/// 3 s: a moving bar, a 440 Hz tone.
fn source(path: &std::path::Path) {
    let mut wr = AvWriter::create(
        path,
        &WriterConfig {
            width: W,
            height: H,
            fps: FPS,
            bitrate: 2_000_000,
            keyframe_interval: 8,
            audio_tracks: 1,
            audio_bitrate: 128_000,
            real_time: false,
        },
    )
    .unwrap();
    let per = (48_000 / FPS) as usize;
    let (mut fv, mut fa) = (0i64, 0i64);
    while fv < FRAMES || fa < FRAMES {
        let mut moved = false;
        if fv < FRAMES {
            let mut rgba = vec![0u8; (W * H * 4) as usize];
            for (i, p) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let x = (i as u32 % W) as i64;
                *p = if (x / 8 + fv) % 10 == 0 {
                    [250, 200, 40, 255]
                } else {
                    [20, 30, 60, 255]
                };
            }
            let pb = wr.pixel_buffer_from_rgba(&rgba).unwrap();
            if wr.append_frame(&pb.0, fv).unwrap() {
                fv += 1;
                moved = true;
            }
        }
        if fa < FRAMES {
            let pcm: Vec<i16> = (0..per)
                .flat_map(|i| {
                    let t = (fa as usize * per + i) as f32 / 48_000.0;
                    let s = ((t * 440.0 * std::f32::consts::TAU).sin() * 8000.0) as i16;
                    [s, s]
                })
                .collect();
            if wr.append_pcm(0, &pcm, fa * per as i64).unwrap() {
                fa += 1;
                moved = true;
                if fa == FRAMES {
                    wr.end_audio();
                }
            }
        }
        if !moved {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    wr.finish().unwrap();
}

#[test]
fn mp4_html_and_report_with_a_cut() {
    let gpu = match znimok_play::headless_gpu() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("skipped: {e}");
            return;
        }
    };
    let dir = std::env::temp_dir().join(format!("znimok-export-mac-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("src.mp4");
    source(&src);
    // Frames 30..60 cut out; a box on frames 70..80.
    let mut doc = Document::from_raster("v", Raster::solid(W, H, Rgb::new(40, 40, 40)));
    doc.push(Object::new(IRect::new(20, 20, 80, 40), Data::Rect));
    let id = doc.objects[0].id;
    let mut t = Timeline::whole(FRAMES);
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
            b: FRAMES,
            off: false,
        },
    ];
    t.marks.insert(id, (70, 80));
    doc.timeline = Some(t);
    let mut video = Video::new(VideoInfo {
        width: W,
        height: H,
        fps_milli: FPS * 1000,
        frames: FRAMES as u32,
        duration_hns: 30_000_000,
        codec: CODEC_H264,
    });
    video.audio = vec![AudioTrack::default()];
    let job = |kind: Kind, dest: &str| Job {
        source: Source::File(src.clone()),
        doc: doc.clone(),
        video: video.clone(),
        kind,
        dest: dir.join(dest),
    };

    let o = run(
        &job(Kind::Mp4 { sound: true }, "cut.mp4"),
        &gpu,
        &Progress::default(),
    )
    .unwrap();
    assert_eq!(o.frames, 60, "90 frames, 30 cut out");
    let info = read_mp4_file(&dir.join("cut.mp4")).unwrap();
    let v = info.video().unwrap();
    assert_eq!((v.width, v.height, v.sample_count), (W, H, 60));
    assert!(v.ctts.is_empty());
    assert!(info.audio().is_some(), "the sound is exported");
    let secs = info.duration_s();
    assert!((secs - 2.0).abs() < 0.1, "{secs} s");
    // The sound of the kept time only: 2 s of it, the tone in it.
    let mut r =
        znimok_video_mac::audio_read::AudioTrackReader::open_all(&dir.join("cut.mp4")).unwrap();
    let mut pcm = Vec::new();
    while let Some((_, b)) = r[0].next_block().unwrap() {
        pcm.extend(b);
    }
    let s = pcm.len() as f64 / 2.0 / 48_000.0;
    assert!((s - 2.0).abs() < 0.1, "{s} s of sound");

    // HTML and the report: the MP4 inside, the mark a layer.
    run(&job(Kind::Html, "page.html"), &gpu, &Progress::default()).unwrap();
    let page = std::fs::read_to_string(dir.join("page.html")).unwrap();
    assert!(page.contains("data:video/mp4;base64,") && page.contains("class=\"m\""));
    let report = Kind::Report(Box::new(znimok_export::ReportOptions {
        zip: true,
        mask: None,
        meta: Default::default(),
        strings: Default::default(),
    }));
    run(&job(report, "bug.zreport"), &gpu, &Progress::default()).unwrap();
    let back =
        znimok_report::read_zreport(&std::fs::read(dir.join("bug.zreport")).unwrap()).unwrap();
    assert_eq!((back.width, back.height), (W, H));
    let _ = std::fs::remove_dir_all(&dir);
}
