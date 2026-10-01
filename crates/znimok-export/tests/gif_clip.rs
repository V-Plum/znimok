//! GIF export on a clip recorded elsewhere (macOS has no recorder yet — ZK-88): run with
//! ZNIMOK_PLAY_CLIP=<the barcode clip of znimok-play's tests> (30 fps, 640 × 360, 90 frames);
//! without it the test passes quietly. MP4 export on macOS waits for its encoder.

use znimok_core::{Document, Raster, Rgb, Timeline, TimelinePart};
use znimok_export::{Job, Kind, Progress, run};
use znimok_format::video::{CODEC_H264, Video, VideoInfo};
use znimok_play::Source;

#[test]
fn a_gif_of_the_kept_clip() {
    let Some(clip) = std::env::var_os("ZNIMOK_PLAY_CLIP") else {
        eprintln!("skipped: ZNIMOK_PLAY_CLIP is not set");
        return;
    };
    let Ok(gpu) = znimok_play::headless_gpu() else {
        eprintln!("skipped: no GPU");
        return;
    };
    let dir = std::env::temp_dir().join(format!("znimok-export-gif-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut doc = Document::from_raster("v", Raster::solid(640, 360, Rgb::new(0, 0, 0)));
    let mut t = Timeline::whole(90);
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
            b: 90,
            off: false,
        },
    ];
    doc.timeline = Some(t);
    let video = Video::new(VideoInfo {
        width: 640,
        height: 360,
        fps_milli: 30_000,
        frames: 90,
        duration_hns: 30_000_000,
        codec: CODEC_H264,
    });
    let dest = dir.join("clip.gif");
    let o = run(
        &Job {
            source: Source::File(clip.into()),
            doc,
            video,
            kind: Kind::Gif {
                width: 320,
                fps: 10.0,
                dither: false,
            },
            dest: dest.clone(),
        },
        &gpu,
        &Progress::default(),
    )
    .unwrap();
    let bytes = std::fs::read(&dest).unwrap();
    let mut opts = gif::DecodeOptions::new();
    opts.set_color_output(gif::ColorOutput::RGBA);
    let mut d = opts.read_info(&bytes[..]).unwrap();
    assert_eq!((d.width(), d.height()), (320, 180));
    let mut cs = 0u32;
    while let Some(f) = d.read_next_frame().unwrap() {
        cs += u32::from(f.delay);
    }
    assert_eq!(cs, 200, "two seconds kept");
    assert!(o.frames >= 2);
    let _ = std::fs::remove_dir_all(&dir);
}
