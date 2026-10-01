//! ZK-96: video documents — round trips, defaults, the stream kept out of memory, and hostile
//! video blocks.

use super::*;
use std::io::{Cursor, Read, Seek, SeekFrom};

use crate::video::{CODEC_H264, PAYLOAD_CHUNK};

const W: u32 = 64;
const H: u32 = 48;

/// Not a real MP4: the format never looks inside the stream.
fn fake_mp4(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 31 % 251) as u8).collect()
}

fn info() -> VideoInfo {
    VideoInfo {
        width: W,
        height: H,
        fps_milli: 30_000,
        frames: 300,
        duration_hns: 100_000_000,
        codec: CODEC_H264,
    }
}

fn poster() -> Document {
    let mut doc = Document::from_raster("Запис — крок 1", Raster::solid(W, H, Rgb::WHITE));
    doc.meta.created_ms = 1_790_558_000_123;
    doc.meta.source = "region".into();
    doc.push(Object::new(IRect::new(1, 2, 20, 10), Data::Rect));
    doc.push(Object::new(IRect::new(5, 5, 8, 8), Data::Ellipse));
    doc.push(Object::new(IRect::new(9, 9, 4, 4), Data::Mark));
    doc
}

fn rich() -> (Document, Video) {
    let mut doc = poster();
    doc.crop = Some(IRect::new(4, 6, 40, 30));
    let mut v = Video::new(info());
    v.edit = Edit {
        parts: vec![
            Part {
                a: 0,
                b: 30,
                off: false,
            },
            Part {
                a: 30,
                b: 90,
                off: true,
            },
            Part {
                a: 90,
                b: 300,
                off: false,
            },
        ],
        in_frame: 10,
        out_frame: 280,
    };
    v.out_size = Some((20, 16));
    v.audio = vec![
        AudioTrack::default(),
        AudioTrack {
            source: AudioSource::Microphone,
            label: "Мікрофон (USB)".into(),
            volume: 150,
            muted: true,
            offset_ms: -40,
            peaks: vec![0, 12, 255, 7],
        },
    ];
    v.mouse = vec![
        MouseEvent {
            ms: 0,
            x: 10,
            y: 12,
            button: MouseButton::Move,
            down: false,
        },
        MouseEvent {
            ms: 250,
            x: 11,
            y: 13,
            button: MouseButton::Left,
            down: true,
        },
        MouseEvent {
            ms: 400,
            x: 11,
            y: 13,
            button: MouseButton::Left,
            down: false,
        },
        MouseEvent {
            ms: 900,
            x: -3,
            y: 70,
            button: MouseButton::Right,
            down: true,
        },
    ];
    v.devlog = Some(DevLog {
        wall0_ms: 1_790_558_000_456,
        events: vec![
            DevEvent {
                ms: 0,
                json: r#"{"t":1,"k":"tab","url":"https://example.com"}"#.into(),
            },
            DevEvent {
                ms: 1234,
                json: r#"{"t":2,"k":"console","text":"привіт"}"#.into(),
            },
        ],
    });
    v.mark_spans.insert(doc.objects[0].id, (0, 90));
    v.mark_spans.insert(doc.objects[2].id, (120, 121));
    (doc, v)
}

fn loaded_video(l: Loaded) -> VideoDocument {
    match l {
        Loaded::Video(v) => v,
        Loaded::Image(_) => panic!("expected a video"),
    }
}

#[test]
fn video_round_trips_every_block() {
    let (doc, v) = rich();
    let mp4 = fake_mp4(5000);
    let bytes = write_video(&doc, &v, &mp4, &WriteOptions::default());
    let back = loaded_video(read_any(&bytes).unwrap());
    assert_eq!(back.video, v);
    assert_eq!(back.payload.bytes(&bytes).unwrap(), mp4);
    assert_eq!(back.payload.ranges.len(), 1);
    assert_eq!(back.doc.id, doc.id);
    assert_eq!(back.doc.name, doc.name);
    assert_eq!(back.doc.meta, doc.meta);
    assert_eq!(back.doc.crop, doc.crop);
    assert_eq!(back.doc.objects, doc.objects);
    // Deterministic: writing what was read gives the same bytes.
    let again = write_video(
        &back.doc,
        &back.video,
        &back.payload.bytes(&bytes).unwrap(),
        &WriteOptions::default(),
    );
    assert_eq!(again, bytes);
    // The same through a seekable source: the stream is located, not loaded.
    let from = loaded_video(read_from(&mut Cursor::new(&bytes), &Limits::default()).unwrap());
    assert_eq!(from.video, v);
    assert_eq!(from.payload, back.payload);
}

#[test]
fn version_kind_and_block_order() {
    let (doc, v) = rich();
    let bytes = write_video(
        &doc,
        &v,
        &fake_mp4(100),
        &WriteOptions {
            thumbnail: Some(Raster::solid(32, 24, Rgb::BLUE)),
            ..Default::default()
        },
    );
    // Video documents are 1.1, screenshots stay 1.0.
    assert_eq!(&bytes[8..12], &[1, 0, 1, 0]);
    assert_eq!(&write(&doc, &WriteOptions::default())[8..12], &[1, 0, 0, 0]);
    let pos = |t: &[u8]| bytes.windows(4).position(|w| w == t).unwrap();
    // Descriptive blocks before the poster, the stream last.
    assert!(pos(b"INFO") < pos(b"VINF") && pos(b"VINF") < pos(b"THMB"));
    assert!(pos(b"THMB") < pos(b"SRC "));
    for t in [b"GEOM", b"CUTS", b"AUDI", b"MOUS", b"DEVT"] {
        assert!(pos(b"OBJS") < pos(t) && pos(t) < pos(b"MP4 "));
    }
    // Neither the recipe nor CROP: the frame is in GEOM.
    assert!(!bytes.windows(4).any(|w| w == b"CROP" || w == b"RCPE"));
    // Peek from the head only: kind, stream parameters, export size.
    let p = peek(&bytes[..pos(b"SRC ") + 8]).unwrap();
    assert_eq!(p.kind, DocKind::Video);
    assert_eq!(p.video, Some(info()));
    assert_eq!((p.width, p.height), (20, 16));
    assert!(p.thumbnail_png.is_some());
    // This video carries a browser log: the head says so, for its file icon (ZK-150).
    assert!(v.devlog.is_some() && p.devtools);
    // A screenshot peeks as an image without video info.
    let p = peek(&write(&doc, &WriteOptions::default())).unwrap();
    assert_eq!((p.kind, p.video, p.devtools), (DocKind::Image, None, false));
    // `read` gives the poster document with the marks.
    let d = read(&bytes).unwrap();
    assert_eq!(d.objects.len(), 3);
    assert_eq!(d.crop, doc.crop);
    assert_eq!(extension_for(DocKind::Video), extension_for(DocKind::Image));
}

#[test]
fn fresh_recording_writes_no_default_blocks() {
    let doc = Document::from_raster("", Raster::solid(W, H, Rgb::BLACK));
    let v = Video::new(info());
    let bytes = write_video(&doc, &v, &fake_mp4(10), &WriteOptions::default());
    let has = |t: &[u8]| bytes.windows(4).any(|w| w == t);
    for absent in [
        b"GEOM", b"CUTS", b"AUDI", b"MOUS", b"DEVT", b"CROP", b"RCPE", b"vspn",
    ] {
        assert!(
            !has(absent),
            "{} should be absent",
            String::from_utf8_lossy(absent)
        );
    }
    for present in [b"META", b"INFO", b"VINF", b"SRC ", b"OBJS", b"MP4 "] {
        assert!(has(present));
    }
    let back = loaded_video(read_any(&bytes).unwrap());
    assert_eq!(back.video, v);
    assert_eq!(back.doc.crop, None);
    // Only a resize: GEOM without a crop, and the crop stays None.
    let mut v2 = v.clone();
    v2.out_size = Some((32, 24));
    let b2 = write_video(&doc, &v2, &fake_mp4(10), &WriteOptions::default());
    let back = loaded_video(read_any(&b2).unwrap());
    assert_eq!((back.video.out_size, back.doc.crop), (Some((32, 24)), None));
}

#[test]
fn stream_in_several_chunks_reads_as_one() {
    let (doc, v) = rich();
    let mut bytes = write_video(&doc, &v, b"first-", &WriteOptions::default());
    // A later writer may split the stream (the writer does at PAYLOAD_CHUNK, 1 GiB) — and put a
    // future block between.
    let mut w = Writer::default();
    w.record(b"FUTR", |w| w.u32(7));
    w.record(b"MP4 ", |w| w.bytes(b""));
    w.record(b"MP4 ", |w| w.bytes(b"second"));
    bytes.extend_from_slice(&w.buf);
    let back = loaded_video(read_any(&bytes).unwrap());
    assert_eq!(back.payload.ranges.len(), 3);
    assert_eq!(back.payload.len(), 12);
    assert_eq!(back.payload.bytes(&bytes).unwrap(), b"first-second");
    // The reader plays the ranges as one stream, with seeks.
    let mut r = PayloadReader::new(Cursor::new(&bytes), &back.payload);
    assert_eq!(r.len(), 12);
    let mut all = Vec::new();
    r.read_to_end(&mut all).unwrap();
    assert_eq!(all, b"first-second");
    r.seek(SeekFrom::Start(4)).unwrap();
    let mut four = [0u8; 4];
    r.read_exact(&mut four).unwrap();
    assert_eq!(&four, b"t-se");
    r.seek(SeekFrom::End(-3)).unwrap();
    let mut rest = Vec::new();
    r.read_to_end(&mut rest).unwrap();
    assert_eq!(rest, b"ond");
    assert!(r.seek(SeekFrom::Current(-100)).is_err());
    assert!(PAYLOAD_CHUNK < u32::MAX as u64);
}

#[test]
fn save_open_and_save_again_from_the_same_file() {
    let dir = std::env::temp_dir().join(format!("znimok-video-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rec.znimok");
    let (doc, mut v) = rich();
    let mp4 = fake_mp4(70_000);
    save_video(
        &path,
        &doc,
        &v,
        &mp4[..],
        mp4.len() as u64,
        &WriteOptions::default(),
    )
    .unwrap();
    let first = loaded_video(open(&path).unwrap());
    assert_eq!(first.video, v);
    // Edit and save the project over itself, streaming the stream out of the old file.
    v.edit = Edit::whole(v.info.frames);
    let src = PayloadReader::new(std::fs::File::open(&path).unwrap(), &first.payload);
    let len = src.len();
    save_video(&path, &first.doc, &v, src, len, &WriteOptions::default()).unwrap();
    assert!(!dir.join("rec.znimok.part").exists());
    let second = loaded_video(open(&path).unwrap());
    assert_eq!(second.video.edit, Edit::whole(300));
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(second.payload.bytes(&bytes).unwrap(), mp4);
    // A stream shorter than promised fails and leaves the file as it was.
    let r = save_video(
        &path,
        &doc,
        &v,
        &mp4[..10],
        mp4.len() as u64,
        &WriteOptions::default(),
    );
    assert!(matches!(r, Err(FormatError::Io(m)) if m.contains("ended")));
    // A poster that is not a frame of the video is refused before anything is written.
    let small = Document::from_raster("", Raster::solid(W / 2, H, Rgb::WHITE));
    let r = save_video(
        &path,
        &small,
        &v,
        &mp4[..],
        mp4.len() as u64,
        &WriteOptions::default(),
    );
    assert!(matches!(r, Err(FormatError::Io(m)) if m.contains("poster")));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(!dir.join("rec.znimok.part").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn edit_list_rules() {
    let e = rich().1.edit;
    assert!(e.is_valid(300));
    assert!(!e.is_valid(301), "must end at N");
    assert_eq!(e.keep_segments(), vec![10..30, 90..280]);
    let mut merged = Edit::whole(100);
    merged.parts = vec![
        Part {
            a: 0,
            b: 50,
            off: false,
        },
        Part {
            a: 50,
            b: 100,
            off: false,
        },
    ];
    assert_eq!(merged.keep_segments(), vec![0..100], "adjacent parts merge");
    for bad in [
        Edit {
            parts: vec![],
            in_frame: 0,
            out_frame: 100,
        },
        Edit {
            parts: vec![Part {
                a: 1,
                b: 100,
                off: false,
            }],
            in_frame: 0,
            out_frame: 100,
        },
        Edit {
            in_frame: 50,
            out_frame: 50,
            ..Edit::whole(100)
        },
        Edit {
            out_frame: 101,
            ..Edit::whole(100)
        },
    ] {
        assert!(!bad.is_valid(100), "{bad:?}");
    }
}

/// Builds a video document by hand from blocks, for hostile inputs.
fn frame(minor: u16, blocks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut w = Writer::default();
    w.bytes(&MAGIC);
    w.u16(1);
    w.u16(minor);
    for (t, b) in blocks {
        w.record(t, |w| w.bytes(b));
    }
    w.buf
}

fn info_block(kind: u8) -> Vec<u8> {
    let mut w = Writer::default();
    w.u32(W);
    w.u32(H);
    w.u32(0);
    w.u8(kind);
    w.buf
}

fn vinf_block(i: &VideoInfo) -> Vec<u8> {
    let mut w = Writer::default();
    video::write_vinf(&mut w, i, false);
    w.buf[8..].to_vec()
}

fn src_block(w: u32, h: u32) -> Vec<u8> {
    encode_png(&Raster::solid(w, h, Rgb::BLACK), png::Compression::Fast)
}

fn base(extra: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut blocks: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"INFO", info_block(1)),
        (b"VINF", vinf_block(&info())),
        (b"SRC ", src_block(W, H)),
    ];
    blocks.extend(extra.iter().cloned());
    blocks.push((b"MP4 ", vec![1, 2, 3]));
    frame(1, &blocks)
}

fn corrupt(data: &[u8], needle: &str) {
    match read_any(data) {
        Err(FormatError::Corrupt(m)) => assert!(m.contains(needle), "{m:?} lacks {needle:?}"),
        other => panic!(
            "expected Corrupt({needle}), got {:?}",
            other.map(|l| l.kind())
        ),
    }
}

fn le(parts: &[u32]) -> Vec<u8> {
    parts.iter().flat_map(|v| v.to_le_bytes()).collect()
}

#[test]
fn hostile_video_blocks_fail_cleanly() {
    assert!(read_any(&base(&[])).is_ok());
    // The kind says video, but VINF, the stream or the right poster size is missing.
    corrupt(
        &frame(
            1,
            &[
                (b"INFO", info_block(1)),
                (b"SRC ", src_block(W, H)),
                (b"MP4 ", vec![1]),
            ],
        ),
        "VINF",
    );
    corrupt(
        &frame(
            1,
            &[
                (b"INFO", info_block(1)),
                (b"VINF", vinf_block(&info())),
                (b"SRC ", src_block(W, H)),
            ],
        ),
        "stream",
    );
    corrupt(
        &frame(
            1,
            &[
                (b"INFO", info_block(1)),
                (b"VINF", vinf_block(&info())),
                (b"SRC ", src_block(W + 2, H)),
                (b"MP4 ", vec![1]),
            ],
        ),
        "poster",
    );
    // VINF out of range.
    for bad in [
        VideoInfo { width: 0, ..info() },
        VideoInfo {
            height: 40_000,
            ..info()
        },
        VideoInfo {
            fps_milli: 0,
            ..info()
        },
        VideoInfo {
            frames: 0,
            ..info()
        },
        VideoInfo {
            frames: u32::MAX,
            ..info()
        },
        VideoInfo {
            duration_hns: -1,
            ..info()
        },
    ] {
        let d = frame(
            1,
            &[
                (b"INFO", info_block(1)),
                (b"VINF", vinf_block(&bad)),
                (b"SRC ", src_block(W, H)),
                (b"MP4 ", vec![1]),
            ],
        );
        assert!(
            matches!(read_any(&d), Err(FormatError::Corrupt(_))),
            "{bad:?}"
        );
    }
    // Counts beyond the limits or beyond the block: refused before allocating.
    corrupt(&base(&[(b"CUTS", le(&[u32::MAX]))]), "limit");
    corrupt(&base(&[(b"CUTS", le(&[1000, 0, 1]))]), "run past");
    corrupt(&base(&[(b"MOUS", le(&[u32::MAX, 13]))]), "limit");
    corrupt(&base(&[(b"MOUS", le(&[1000, 13]))]), "run past");
    corrupt(&base(&[(b"MOUS", le(&[1, 12, 0, 0, 0]))]), "record of 12");
    corrupt(&base(&[(b"MOUS", le(&[1, u32::MAX]))]), "record of");
    corrupt(&base(&[(b"AUDI", le(&[17]))]), "limit");
    corrupt(&base(&[(b"AUDI", le(&[2]))]), "run past");
    let mut devt = vec![1u8];
    devt.extend_from_slice(&0i64.to_le_bytes());
    devt.extend_from_slice(&u32::MAX.to_le_bytes());
    corrupt(&base(&[(b"DEVT", devt.clone())]), "limit");
    devt.truncate(9);
    devt.extend_from_slice(&5u32.to_le_bytes());
    corrupt(&base(&[(b"DEVT", devt)]), "run past");
    // A string length bomb inside DEVT.
    let mut devt = vec![1u8];
    devt.extend_from_slice(&0i64.to_le_bytes());
    devt.extend_from_slice(&le(&[1, 0, u32::MAX]));
    corrupt(&base(&[(b"DEVT", devt)]), "string");
    // Overflow: a huge count with a large record size, under generous limits.
    let lim = Limits {
        max_mouse_events: usize::MAX,
        ..Default::default()
    };
    let d = base(&[(b"MOUS", le(&[u32::MAX, 256]))]);
    assert!(matches!(
        read_any_with_limits(&d, &lim),
        Err(FormatError::Corrupt(m)) if m.contains("run past")
    ));
    // Not a TRK record inside AUDI.
    let mut audi = le(&[1]);
    audi.extend_from_slice(b"XXXX");
    audi.extend_from_slice(&le(&[0]));
    corrupt(&base(&[(b"AUDI", audi)]), "TRK");
    // Too many stream chunks.
    let lim = Limits {
        max_payload_chunks: 1,
        ..Default::default()
    };
    let d = base(&[(b"MP4 ", vec![9])]);
    assert!(matches!(
        read_any_with_limits(&d, &lim),
        Err(FormatError::Corrupt(m)) if m.contains("chunks")
    ));
    assert!(matches!(
        read_from(&mut Cursor::new(&d), &lim),
        Err(FormatError::Corrupt(m)) if m.contains("chunks")
    ));
}

#[test]
fn lenient_where_the_video_still_plays() {
    // An edit list that does not fit the stream is dropped: the video opens uncut.
    let cuts = {
        let mut w = Writer::default();
        w.u32(1);
        w.u32(0);
        w.u32(299); // not N = 300
        w.u8(0);
        w.u32(0);
        w.u32(299);
        w.buf
    };
    let v = loaded_video(read_any(&base(&[(b"CUTS", cuts)])).unwrap());
    assert_eq!(v.video.edit, Edit::whole(300));
    // Unknown mouse buttons are skipped (not turned into left clicks); longer records of a
    // newer minor are read by their size; out-of-range audio fields are clamped; unknown TRK
    // fields and an unknown source code fall back.
    let mut mous = le(&[3, 14]);
    for (ms, code) in [(1i32, 0x10u8), (2, 0x07), (3, 0x0F)] {
        mous.extend_from_slice(&ms.to_le_bytes());
        mous.extend_from_slice(&le(&[5, 6]));
        mous.push(code);
        mous.push(0xEE); // the 14th byte of a future record
    }
    let mut w = Writer::default();
    w.u32(1);
    w.record(b"TRK ", |w| {
        w.record(b"srce", |w| w.u8(9));
        w.record(b"volm", |w| w.u8(255));
        w.record(b"offs", |w| w.i32(i32::MIN));
        w.record(b"zzzz", |w| w.u32(1));
    });
    let v = loaded_video(read_any(&base(&[(b"MOUS", mous), (b"AUDI", w.buf)])).unwrap());
    assert_eq!(
        v.video.mouse,
        vec![
            MouseEvent {
                ms: 1,
                x: 5,
                y: 6,
                button: MouseButton::Left,
                down: true
            },
            MouseEvent {
                ms: 3,
                x: 5,
                y: 6,
                button: MouseButton::Move,
                down: false
            },
        ]
    );
    assert_eq!(
        v.video.audio,
        vec![AudioTrack {
            source: AudioSource::System,
            volume: 200,
            offset_ms: -60_000,
            ..Default::default()
        }]
    );
    // An unknown document kind reads as an image (the poster), and video blocks in an image are
    // ignored.
    let d = frame(
        1,
        &[
            (b"INFO", info_block(7)),
            (b"VINF", vinf_block(&info())),
            (b"SRC ", src_block(W, H)),
            (b"MP4 ", vec![1]),
        ],
    );
    assert_eq!(read_any(&d).unwrap().kind(), DocKind::Image);
    assert_eq!(peek(&d).unwrap().video, None);
    // A GEOM crop is clamped to the video; an empty span is ignored.
    let mut geom = le(&[0, 0]);
    geom.extend_from_slice(&[(-10i32), -10, 1000, 1000].map(i32::to_le_bytes).concat());
    let v = loaded_video(read_any(&base(&[(b"GEOM", geom)])).unwrap());
    assert_eq!(v.doc.crop, Some(IRect::new(0, 0, W as i32, H as i32)));
    assert_eq!(v.video.out_size, None);
}

#[test]
fn every_truncation_of_a_video_fails_cleanly() {
    let (doc, v) = rich();
    let bytes = write_video(&doc, &v, &fake_mp4(64), &WriteOptions::default());
    for n in 0..bytes.len() {
        let cut = &bytes[..n];
        let a = read_any(cut);
        let b = read_from(&mut Cursor::new(cut), &Limits::default());
        let _ = peek(cut);
        assert!(
            a.is_err() && b.is_err(),
            "prefix of {n} bytes read as a document"
        );
    }
}

#[test]
fn streaming_reader_bounds() {
    let (doc, v) = rich();
    let bytes = write_video(&doc, &v, &fake_mp4(4096), &WriteOptions::default());
    // The stream does not count against max_file; the rest does.
    let head = bytes.len() - 4096;
    let lim = Limits {
        max_file: head,
        ..Default::default()
    };
    assert!(read_from(&mut Cursor::new(&bytes), &lim).is_ok());
    let lim = Limits {
        max_file: head - 20,
        ..Default::default()
    };
    assert!(matches!(
        read_from(&mut Cursor::new(&bytes), &lim),
        Err(FormatError::Corrupt(m)) if m.contains("limit")
    ));
    // A chunk claiming more than the file has.
    let mut bad = bytes.clone();
    let at = bad.windows(4).position(|w| w == b"MP4 ").unwrap();
    bad[at + 4..at + 8].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        read_from(&mut Cursor::new(&bad), &Limits::default()),
        Err(FormatError::Corrupt(m)) if m.contains("past the end")
    ));
    assert_eq!(
        read_from(
            &mut Cursor::new(b"not a document at all"),
            &Limits::default()
        )
        .unwrap_err(),
        FormatError::NotZnimok
    );
}

/// The `VINF` flags byte (ZK-150): a video without a browser log peeks without it, and a `VINF`
/// written before the byte existed (it ends at the codec) reads as «no log».
#[test]
fn devtools_flag_in_the_head() {
    let (doc, mut v) = rich();
    v.devlog = None;
    let bytes = write_video(&doc, &v, &fake_mp4(100), &WriteOptions::default());
    let p = peek(&bytes).unwrap();
    assert_eq!((p.kind, p.devtools), (DocKind::Video, false));

    let mut full = vinf_block(&info());
    assert_eq!(full.pop(), Some(0), "the flags byte closes VINF");
    let limits = Limits::default();
    let mut r = crate::codec::Reader::new(&full, &limits);
    let back = video::read_vinf(&mut r, &limits).unwrap();
    assert_eq!(back, info());
    assert_eq!(video::read_vinf_flags(&mut r).unwrap(), 0);
}

/// ZK-94: the marks' times open into the document's timeline, and a save writes the timeline's
/// times (only for marks that are there, inside the video).
#[test]
fn marks_times_live_in_the_documents_timeline() {
    let (doc, v) = rich();
    let dir = std::env::temp_dir().join(format!("znimok-vspn-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("v.znimok");
    std::fs::write(
        &path,
        write_video(&doc, &v, &fake_mp4(5000), &WriteOptions::default()),
    )
    .unwrap();
    let vd = loaded_video(open(&path).unwrap());
    let t = vd.doc.timeline.clone().unwrap();
    assert_eq!(t.marks.get(&doc.objects[0].id), Some(&(0, 90)));
    assert_eq!(t.marks.get(&doc.objects[2].id), Some(&(120, 121)));
    // The editor moves one, gives another a time, and one points at a mark that is gone.
    let mut d2 = vd.doc.clone();
    let mut t2 = t.clone();
    t2.marks.insert(doc.objects[0].id, (30, 60));
    t2.marks.insert(doc.objects[1].id, (5, 6));
    t2.marks.insert(9999, (1, 2));
    d2.timeline = Some(t2);
    let part = VideoPart {
        video: vd.video.clone(),
        payload: vd.payload.clone(),
        source: path.clone(),
    };
    save_same_kind(&path, &d2, Some(&part), &WriteOptions::default()).unwrap();
    let back = loaded_video(open(&path).unwrap());
    assert_eq!(
        back.video.mark_spans.get(&doc.objects[0].id),
        Some(&(30, 60))
    );
    assert_eq!(back.video.mark_spans.get(&doc.objects[1].id), Some(&(5, 6)));
    assert!(!back.video.mark_spans.contains_key(&9999));
    assert_eq!(
        back.doc.timeline.unwrap().marks.get(&doc.objects[0].id),
        Some(&(30, 60))
    );
    let _ = std::fs::remove_dir_all(&dir);
}
