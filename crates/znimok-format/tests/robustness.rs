//! ZK-27, the part that runs in every CI build: thousands of damaged variants of a real
//! document must be rejected cleanly — no panic, no runaway allocation, bounded time. The
//! open-ended search runs under cargo-fuzz (`crates/znimok-format/fuzz`).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

use znimok_format::{
    AudioSource, AudioTrack, DevEvent, DevLog, Edit, FormatError, Limits, Loaded, MouseButton,
    MouseEvent, Part, Video, VideoInfo, WriteOptions, peek, read, read_any, read_from, write,
    write_video,
};

/// Small deterministic generator (xorshift64*), so failures reproduce from the seed.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn sample() -> Vec<u8> {
    let doc = znimok_render::reference::reference_document(160, 100);
    let thumb = znimok_core::Raster::solid(16, 10, znimok_core::Rgb::BLUE);
    write(
        &doc,
        &WriteOptions {
            thumbnail: Some(thumb),
            ..Default::default()
        },
    )
}

/// A video document with every video block (ZK-96); the stream is a stand-in, the format does
/// not look inside it.
fn video_sample() -> Vec<u8> {
    let mut doc = znimok_render::reference::reference_document(160, 100);
    doc.crop = Some(znimok_core::IRect::new(8, 6, 120, 80));
    let info = VideoInfo {
        width: 160,
        height: 100,
        fps_milli: 30_000,
        frames: 90,
        duration_hns: 30_000_000,
        codec: znimok_format::video::CODEC_H264,
    };
    let mut v = Video::new(info);
    v.edit = Edit {
        parts: vec![
            Part {
                a: 0,
                b: 20,
                off: false,
            },
            Part {
                a: 20,
                b: 40,
                off: true,
            },
            Part {
                a: 40,
                b: 90,
                off: false,
            },
        ],
        in_frame: 2,
        out_frame: 88,
    };
    v.out_size = Some((60, 40));
    v.audio = vec![
        AudioTrack::default(),
        AudioTrack {
            source: AudioSource::Microphone,
            label: "mic".into(),
            volume: 80,
            muted: false,
            offset_ms: 15,
        },
    ];
    v.mouse = (0..20)
        .map(|i| MouseEvent {
            ms: i * 50,
            x: i * 3,
            y: 100 - i,
            button: if i % 3 == 0 {
                MouseButton::Left
            } else {
                MouseButton::Move
            },
            down: i % 2 == 0,
        })
        .collect();
    v.devlog = Some(DevLog {
        wall0_ms: 1_790_558_000_000,
        events: vec![DevEvent {
            ms: 10,
            json: r#"{"t":1,"k":"net","url":"https://example.com/"}"#.into(),
        }],
    });
    for o in doc.objects.iter().take(3) {
        v.mark_spans.insert(o.id, (5, 60));
    }
    let mp4: Vec<u8> = (0..2048u32).map(|i| (i * 7) as u8).collect();
    write_video(&doc, &v, &mp4, &WriteOptions::default())
}

fn mutate(rng: &mut Rng, base: &[u8]) -> Vec<u8> {
    let mut v = base.to_vec();
    for _ in 0..=rng.below(4) {
        match rng.below(7) {
            0 => {
                let i = rng.below(v.len());
                v[i] ^= 1 << rng.below(8);
            }
            1 => {
                let i = rng.below(v.len());
                v[i] = rng.next() as u8;
            }
            2 => v.truncate(rng.below(v.len())),
            3 => {
                let i = rng.below(v.len());
                let n = rng.below(64);
                let junk: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
                v.splice(i..i, junk);
            }
            4 => {
                // Overwrite a 32-bit field with a huge or boundary value (lengths, counts).
                if v.len() > 16 {
                    let i = 12 + rng.below(v.len() - 16);
                    let val: u32 =
                        [u32::MAX, 0x7FFF_FFFF, 0x8000_0000, 1 << 20, 0, 65535][rng.below(6)];
                    v[i..i + 4].copy_from_slice(&val.to_le_bytes());
                }
            }
            5 => {
                // Duplicate a slice somewhere else.
                let a = rng.below(v.len());
                let n = rng.below(256).min(v.len() - a);
                let chunk = v[a..a + n].to_vec();
                let at = rng.below(v.len());
                v.splice(at..at, chunk);
            }
            _ => {
                let a = rng.below(v.len());
                let b = (a + rng.below(128)).min(v.len());
                v.drain(a..b);
            }
        }
        if v.len() < 12 {
            break;
        }
    }
    v
}

#[test]
fn damaged_files_never_panic() {
    let base = sample();
    assert!(read(&base).is_ok());
    let mut rng = Rng(0x5EED_2026_0928);
    let started = Instant::now();
    let mut outcomes = [0usize; 3];
    for i in 0..4000 {
        let v = mutate(&mut rng, &base);
        let t = Instant::now();
        let r = catch_unwind(AssertUnwindSafe(|| (read(&v), peek(&v))));
        let (full, _) = r.unwrap_or_else(|_| panic!("panic on case {i} (seed 0x5EED_2026_0928)"));
        assert!(
            t.elapsed() < Duration::from_secs(2),
            "case {i} took {:?}",
            t.elapsed()
        );
        match full {
            Ok(_) => outcomes[0] += 1,
            Err(FormatError::Corrupt(_)) => outcomes[1] += 1,
            Err(_) => outcomes[2] += 1,
        }
    }
    eprintln!(
        "4000 damaged files in {:?}: {} still readable, {} corrupt, {} other",
        started.elapsed(),
        outcomes[0],
        outcomes[1],
        outcomes[2]
    );
    assert!(outcomes[1] > 1000, "most damage must be detected");
}

/// ZK-96: the same for a video document, through the in-memory and the streaming reader.
#[test]
fn damaged_videos_never_panic() {
    let base = video_sample();
    assert!(matches!(read_any(&base), Ok(Loaded::Video(_))));
    let mut rng = Rng(0x5EED_2026_0929);
    let mut outcomes = [0usize; 3];
    for i in 0..4000 {
        let v = mutate(&mut rng, &base);
        let t = Instant::now();
        let r = catch_unwind(AssertUnwindSafe(|| {
            let _ = peek(&v);
            let a = read_any(&v);
            let b = read_from(&mut std::io::Cursor::new(&v), &Limits::default());
            // Both readers agree on whether the file is readable.
            assert_eq!(a.is_ok(), b.is_ok(), "case {i}");
            a
        }));
        let full = r.unwrap_or_else(|_| panic!("panic on case {i} (seed 0x5EED_2026_0929)"));
        assert!(
            t.elapsed() < Duration::from_secs(2),
            "case {i} took {:?}",
            t.elapsed()
        );
        match full {
            Ok(_) => outcomes[0] += 1,
            Err(FormatError::Corrupt(_)) => outcomes[1] += 1,
            Err(_) => outcomes[2] += 1,
        }
    }
    eprintln!(
        "4000 damaged videos: {} still readable, {} corrupt, {} other",
        outcomes[0], outcomes[1], outcomes[2]
    );
    assert!(outcomes[1] > 1000, "most damage must be detected");
}

#[test]
fn length_bombs_fail_fast_without_allocating() {
    use znimok_format::MAGIC;
    let frame = |blocks: &[(&[u8; 4], Vec<u8>)]| {
        let mut v = MAGIC.to_vec();
        v.extend_from_slice(&[1, 0, 0, 0]);
        for (t, b) in blocks {
            v.extend_from_slice(*t);
            v.extend_from_slice(&(b.len() as u32).to_le_bytes());
            v.extend_from_slice(b);
        }
        v
    };
    // Object count and pen point count far beyond the limits.
    let objs = frame(&[(b"OBJS", u32::MAX.to_le_bytes().to_vec())]);
    assert!(matches!(read(&objs), Err(FormatError::Corrupt(m)) if m.contains("limit")));
    // A block claiming more bytes than the file has.
    let mut short = frame(&[]);
    short.extend_from_slice(b"SRC ");
    short.extend_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(read(&short), Err(FormatError::Corrupt(_))));
    // A PNG header announcing 30000×30000 pixels: refused from the header.
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&30000u32.to_be_bytes());
    ihdr.extend_from_slice(&30000u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    png.extend_from_slice(&(ihdr.len() as u32).to_be_bytes());
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&ihdr);
    let mut crc_in = b"IHDR".to_vec();
    crc_in.extend_from_slice(&ihdr);
    png.extend_from_slice(&crc32(&crc_in).to_be_bytes());
    let t = Instant::now();
    let big = frame(&[(b"SRC ", png)]);
    assert!(matches!(read(&big), Err(FormatError::Corrupt(_))));
    assert!(t.elapsed() < Duration::from_millis(500));
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

/// Writes the seed corpus for cargo-fuzz when `ZNIMOK_FUZZ_SEED=<dir>` is set.
#[test]
fn write_fuzz_seed_if_asked() {
    if let Ok(dir) = std::env::var("ZNIMOK_FUZZ_SEED") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join("reference.znimok"),
            sample(),
        )
        .unwrap();
        let small = znimok_core::Document::from_raster(
            "s",
            znimok_core::Raster::solid(3, 2, znimok_core::Rgb::RED),
        );
        std::fs::write(
            std::path::Path::new(&dir).join("small.znimok"),
            write(&small, &WriteOptions::default()),
        )
        .unwrap();
        // ZK-96: a video with every video block, and a fresh recording with none of the optional
        // ones.
        std::fs::write(
            std::path::Path::new(&dir).join("video.znimok"),
            video_sample(),
        )
        .unwrap();
        let info = VideoInfo {
            width: 3,
            height: 2,
            fps_milli: 60_000,
            frames: 1,
            duration_hns: 166_667,
            codec: znimok_format::video::CODEC_HEVC,
        };
        std::fs::write(
            std::path::Path::new(&dir).join("video-small.znimok"),
            write_video(&small, &Video::new(info), b"mp4", &WriteOptions::default()),
        )
        .unwrap();
    }
}

/// Inputs that once crashed the reader (found by cargo-fuzz). Each must now read or fail
/// cleanly — and whatever reads must survive a write/read round trip.
#[test]
fn fuzz_regressions() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fuzz-regressions");
    let mut n = 0;
    for e in std::fs::read_dir(&dir).unwrap() {
        let data = std::fs::read(e.unwrap().path()).unwrap();
        let _ = peek(&data);
        if let Ok(Loaded::Video(v)) = read_any(&data) {
            let mp4 = v.payload.bytes(&data).unwrap();
            let again = write_video(&v.doc, &v.video, &mp4, &WriteOptions::default());
            let Ok(Loaded::Video(w)) = read_any(&again) else {
                panic!("re-read of a written video");
            };
            assert_eq!(w.video, v.video);
        }
        if let Ok(doc) = read(&data) {
            for o in &doc.objects {
                let _ = o.bounds();
            }
            let again = read(&write(&doc, &WriteOptions::default())).unwrap();
            assert_eq!(again.objects.len(), doc.objects.len());
        }
        n += 1;
    }
    assert!(n >= 1);
}

/// ZK-145: a video opened for editing and saved again stays a video — twice to the same path
/// (the stream is read from the file being replaced, and its offsets move), stream intact.
#[test]
fn a_video_saved_again_stays_a_video() {
    let dir = std::env::temp_dir().join(format!("znimok-resave-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("video.znimok");
    let bytes = video_sample();
    std::fs::write(&path, &bytes).unwrap();
    let mp4_before = match znimok_format::read_any(&bytes).unwrap() {
        znimok_format::Loaded::Video(v) => v.payload.bytes(&bytes).unwrap(),
        _ => panic!("sample is a video"),
    };
    let (mut doc, mut part) = znimok_format::open_parts(&path).unwrap();
    assert!(part.is_some());
    for name in ["Перша назва", "Друга, довша назва відео, щоб зсунути потік"]
    {
        doc.name = name.into();
        doc.objects.truncate(doc.objects.len().saturating_sub(1));
        part = znimok_format::save_same_kind(&path, &doc, part.as_ref(), &WriteOptions::default())
            .unwrap();
    }
    let after = std::fs::read(&path).unwrap();
    match znimok_format::read_any(&after).unwrap() {
        znimok_format::Loaded::Video(v) => {
            assert_eq!(v.doc.name, "Друга, довша назва відео, щоб зсунути потік");
            assert_eq!(v.payload.bytes(&after).unwrap(), mp4_before);
        }
        _ => panic!("the video became a screenshot"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// ZK-144: a video's edits come in as the document's timeline and go out from it.
#[test]
fn the_timeline_is_the_edit_list() {
    let dir = std::env::temp_dir().join(format!("znimok-timeline-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("video.znimok");
    std::fs::write(&path, video_sample()).unwrap();
    let (mut doc, part) = znimok_format::open_parts(&path).unwrap();
    let t = doc.timeline.clone().expect("a video has a timeline");
    assert_eq!(t.frames(), 90);
    assert_eq!((t.in_point, t.out_point), (2, 88));
    let mut cut = znimok_core::Timeline::whole(90);
    cut.parts = vec![
        znimok_core::TimelinePart {
            a: 0,
            b: 45,
            off: false,
        },
        znimok_core::TimelinePart {
            a: 45,
            b: 90,
            off: true,
        },
    ];
    doc.timeline = Some(cut.clone());
    znimok_format::save_same_kind(&path, &doc, part.as_ref(), &WriteOptions::default()).unwrap();
    let (again, _) = znimok_format::open_parts(&path).unwrap();
    assert_eq!(again.timeline, Some(cut));
    let _ = std::fs::remove_dir_all(&dir);
}
