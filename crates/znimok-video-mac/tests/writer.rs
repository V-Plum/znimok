//! The writer through the real VideoToolbox (macOS): frames made from memory and a tone, 2 s at
//! 30 fps → an MP4 whose video is H.264 at the size asked, 60 frames, key frames as asked, no
//! B-frames (no `ctts`), with an AAC track of the same length. Runs without a screen (CI).
#![cfg(target_os = "macos")]

use znimok_video::check::mp4::read_mp4_file;
use znimok_video_mac::writer::{AvWriter, WriterConfig};

#[test]
fn frames_and_sound_into_an_mp4() {
    let dir = std::env::temp_dir().join(format!("znimok-avwriter-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("out.mp4.part");
    let (w, h, fps) = (320u32, 240u32, 30u32);
    let cfg = WriterConfig {
        width: w,
        height: h,
        fps,
        bitrate: 2_000_000,
        keyframe_interval: 8,
        audio_tracks: 1,
        audio_bitrate: 128_000,
        real_time: false,
    };
    let mut wr = AvWriter::create(&path, &cfg).unwrap();
    let frames = 60i64;
    let per_frame = 48_000 / fps as usize;
    let frame = |f: i64| -> Vec<u8> {
        // A moving bar: the encoder has something to encode.
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 0..h as usize {
            for x in 0..w as usize {
                let p = (y * w as usize + x) * 4;
                let on = (x as i64 / 8 + f) % 10 == 0;
                rgba[p..p + 4].copy_from_slice(if on {
                    &[250, 200, 40, 255]
                } else {
                    &[20, 30, 60, 255]
                });
            }
        }
        rgba
    };
    let tone = |f: i64| -> Vec<i16> {
        (0..per_frame)
            .flat_map(|i| {
                let t = (f as usize * per_frame + i) as f32 / 48_000.0;
                let s = ((t * 440.0 * std::f32::consts::TAU).sin() * 8000.0) as i16;
                [s, s]
            })
            .collect()
    };
    // Both inputs are fed as they are ready: AVAssetWriter interleaves, so an input ahead of the
    // other says «not ready» until the other catches up.
    let (mut fv, mut fa, mut idle) = (0i64, 0i64, 0);
    while fv < frames || fa < frames {
        let mut moved = false;
        if fv < frames {
            let pb = wr.pixel_buffer_from_rgba(&frame(fv)).unwrap();
            if wr.append_frame(&pb.0, fv).unwrap() {
                fv += 1;
                moved = true;
            }
        }
        if fa < frames && wr.append_pcm(0, &tone(fa), fa * per_frame as i64).unwrap() {
            fa += 1;
            moved = true;
            if fa == frames {
                wr.end_audio();
            }
        }
        if moved {
            idle = 0;
        } else {
            idle += 1;
            assert!(
                idle < 5000,
                "the writer stopped taking samples at {fv} / {fa}"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    wr.finish().unwrap();
    let done = dir.join("out.mp4");
    std::fs::rename(&path, &done).unwrap();
    let path = done.clone();
    let info = read_mp4_file(&path).unwrap();
    let v = info.video().expect("a video track");
    assert_eq!(v.codec.as_ref().map(|c| &c[..]), Some(&b"avc1"[..]));
    assert_eq!((v.width, v.height), (w, h));
    assert_eq!(v.sample_count, frames as u32);
    assert!(v.ctts.is_empty(), "no B-frames: {:?}", v.ctts);
    let keys = v.stss.clone().unwrap_or_default();
    assert!(keys.len() >= 6, "a key frame every 8 frames: {keys:?}");
    let a = info.audio().expect("an audio track");
    assert_eq!(a.codec.as_ref().map(|c| &c[..]), Some(&b"mp4a"[..]));
    let secs = info.duration_s();
    assert!((secs - 2.0).abs() < 0.1, "{secs} s");
    // The sound back as PCM: 2 s of it, the tone audible.
    let mut readers =
        znimok_video_mac::audio_read::AudioTrackReader::open_all(&dir.join("out.mp4")).unwrap();
    assert_eq!(readers.len(), 1);
    let mut got = Vec::new();
    while let Some((_, pcm)) = readers[0].next_block().unwrap() {
        got.extend(pcm);
    }
    let secs = got.len() as f64 / 2.0 / 48_000.0;
    assert!((secs - 2.0).abs() < 0.1, "{secs} s of sound");
    let loud = got.iter().filter(|v| v.unsigned_abs() > 4000).count();
    assert!(
        loud > got.len() / 4,
        "the tone is there: {loud} of {}",
        got.len()
    );
    // The poster: the first frame back as RGBA, the bar's yellow in it.
    let (pw, ph, rgba) = znimok_video_mac::poster::first_frame(&done).unwrap();
    assert_eq!((pw, ph), (w, h));
    let yellow = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0] > 200 && p[1] > 150 && p[2] < 100)
        .count();
    assert!(yellow > 500, "{yellow} yellow pixels");
    let _ = std::fs::remove_dir_all(&dir);
}
