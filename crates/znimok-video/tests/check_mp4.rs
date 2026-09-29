//! ZK-99: the MP4 container checks on tiny MP4 byte streams written here — a good recording
//! (with and without audio, stretched samples, NTSC rates), every failure the checks know, and
//! hostile inputs that must end in an error, never a panic or an unbounded allocation.

use std::io::{Read, Seek, SeekFrom};
use znimok_video::cfr::VideoSampleTime;
use znimok_video::check::mp4::*;
use znimok_video::check::{CheckError, all_ok, failed};

// ---------------------------------------------------------------------------------------------
// A minimal MP4 writer: ftyp, mdat, moov (moov last, as MF writes it), one sample per chunk.

fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut v = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    v.extend_from_slice(kind);
    v.extend_from_slice(body);
    v
}

fn fullbox(kind: &[u8; 4], version: u8, body: &[u8]) -> Vec<u8> {
    let mut b = vec![version, 0, 0, 0];
    b.extend_from_slice(body);
    bx(kind, &b)
}

fn be32(v: &mut Vec<u8>, x: u32) {
    v.extend_from_slice(&x.to_be_bytes());
}

#[derive(Clone)]
struct TrackSpec {
    handler: [u8; 4],
    codec: [u8; 4],
    timescale: u32,
    stts: Vec<(u32, u32)>,
    ctts: Option<Vec<(u32, u32)>>,
    stss: Option<Vec<u32>>,
    /// (segment duration in movie units, media time)
    edits: Option<Vec<(u32, i32)>>,
    width: u16,
    height: u16,
    sample_bytes: u32,
    /// Overrides.
    mdhd_duration: Option<u32>,
    tkhd_duration: Option<u32>,
    stsz_count: Option<u32>,
}

impl TrackSpec {
    fn video(timescale: u32, stts: Vec<(u32, u32)>) -> Self {
        let n: u32 = stts.iter().map(|e| e.0).sum();
        Self {
            handler: *b"vide",
            codec: *b"avc1",
            timescale,
            stts,
            ctts: None,
            stss: Some((0..n.div_ceil(30)).map(|k| 1 + 30 * k).collect()),
            edits: None,
            width: 1280,
            height: 720,
            sample_bytes: 5,
            mdhd_duration: None,
            tkhd_duration: None,
            stsz_count: None,
        }
    }

    /// AAC at 48 kHz: 1024-frame samples covering `secs`.
    fn audio(secs: f64) -> Self {
        let frames = (secs * 48_000.0).round() as u32;
        let full = frames / 1024;
        let mut stts = vec![(full, 1024)];
        if !frames.is_multiple_of(1024) {
            stts.push((1, frames % 1024));
        }
        Self {
            handler: *b"soun",
            codec: *b"mp4a",
            timescale: 48_000,
            stts,
            ctts: None,
            stss: None,
            edits: None,
            width: 0,
            height: 0,
            sample_bytes: 3,
            mdhd_duration: None,
            tkhd_duration: None,
            stsz_count: None,
        }
    }

    fn samples(&self) -> u32 {
        self.stts.iter().map(|e| e.0).sum()
    }

    fn media_duration(&self) -> u32 {
        self.mdhd_duration.unwrap_or_else(|| {
            self.stts
                .iter()
                .map(|&(c, d)| c as u64 * d as u64)
                .sum::<u64>() as u32
        })
    }
}

#[derive(Clone)]
struct Mp4Spec {
    movie_timescale: u32,
    tracks: Vec<TrackSpec>,
    mvhd_duration: Option<u32>,
    /// Added to every chunk offset (metadata inserted before mdat without fixing stco).
    chunk_shift: i64,
}

impl Mp4Spec {
    fn new(tracks: Vec<TrackSpec>) -> Self {
        Self {
            movie_timescale: 1000,
            tracks,
            mvhd_duration: None,
            chunk_shift: 0,
        }
    }
}

fn tkhd_duration(t: &TrackSpec, movie: u32) -> u32 {
    t.tkhd_duration.unwrap_or_else(|| match &t.edits {
        Some(e) => e.iter().map(|x| x.0).sum(),
        None => (t.media_duration() as u64 * movie as u64 / t.timescale as u64) as u32,
    })
}

fn trak(t: &TrackSpec, id: u32, movie: u32, first_offset: u64, shift: i64) -> Vec<u8> {
    // tkhd v0
    let mut b = Vec::new();
    be32(&mut b, 0);
    be32(&mut b, 0);
    be32(&mut b, id);
    be32(&mut b, 0);
    be32(&mut b, tkhd_duration(t, movie));
    b.extend_from_slice(&[0; 8 + 2 + 2 + 2 + 2 + 36]);
    be32(&mut b, (t.width as u32) << 16);
    be32(&mut b, (t.height as u32) << 16);
    let tkhd = fullbox(b"tkhd", 0, &b);

    let edts = t.edits.as_ref().map(|e| {
        let mut b = Vec::new();
        be32(&mut b, e.len() as u32);
        for &(d, m) in e {
            be32(&mut b, d);
            be32(&mut b, m as u32);
            be32(&mut b, 0x0001_0000);
        }
        bx(b"edts", &fullbox(b"elst", 0, &b))
    });

    let mut b = Vec::new();
    be32(&mut b, 0);
    be32(&mut b, 0);
    be32(&mut b, t.timescale);
    be32(&mut b, t.media_duration());
    be32(&mut b, 0);
    let mdhd = fullbox(b"mdhd", 0, &b);
    let mut b = vec![0; 4];
    b.extend_from_slice(&t.handler);
    b.extend_from_slice(&[0; 13]);
    let hdlr = fullbox(b"hdlr", 0, &b);

    // stsd with one sample entry
    let entry = if &t.handler == b"vide" {
        let mut e = vec![0u8; 78];
        e[24..26].copy_from_slice(&t.width.to_be_bytes());
        e[26..28].copy_from_slice(&t.height.to_be_bytes());
        e
    } else {
        vec![0u8; 28]
    };
    let mut b = Vec::new();
    be32(&mut b, 1);
    b.extend(bx(&t.codec, &entry));
    let stsd = fullbox(b"stsd", 0, &b);

    let mut b = Vec::new();
    be32(&mut b, t.stts.len() as u32);
    for &(c, d) in &t.stts {
        be32(&mut b, c);
        be32(&mut b, d);
    }
    let stts = fullbox(b"stts", 0, &b);

    let ctts = t.ctts.as_ref().map(|c| {
        let mut b = Vec::new();
        be32(&mut b, c.len() as u32);
        for &(k, o) in c {
            be32(&mut b, k);
            be32(&mut b, o);
        }
        fullbox(b"ctts", 0, &b)
    });

    let n = t.samples();
    let mut b = Vec::new();
    be32(&mut b, t.sample_bytes);
    be32(&mut b, t.stsz_count.unwrap_or(n));
    let stsz = fullbox(b"stsz", 0, &b);

    let mut b = Vec::new();
    be32(&mut b, 1);
    be32(&mut b, 1);
    be32(&mut b, 1);
    be32(&mut b, 1);
    let stsc = fullbox(b"stsc", 0, &b);

    let mut b = Vec::new();
    be32(&mut b, n);
    for i in 0..n as u64 {
        let off = (first_offset + i * t.sample_bytes as u64) as i64 + shift;
        be32(&mut b, off as u32);
    }
    let stco = fullbox(b"stco", 0, &b);

    let stss = t.stss.as_ref().map(|s| {
        let mut b = Vec::new();
        be32(&mut b, s.len() as u32);
        for &k in s {
            be32(&mut b, k);
        }
        fullbox(b"stss", 0, &b)
    });

    let mut stbl = [stsd, stts].concat();
    if let Some(c) = ctts {
        stbl.extend(c);
    }
    stbl.extend([stsz, stsc, stco].concat());
    if let Some(s) = stss {
        stbl.extend(s);
    }
    let minf = bx(b"minf", &bx(b"stbl", &stbl));
    let mdia = bx(b"mdia", &[mdhd, hdlr, minf].concat());
    let mut body = tkhd;
    if let Some(e) = edts {
        body.extend(e);
    }
    body.extend(mdia);
    bx(b"trak", &body)
}

fn build(spec: &Mp4Spec) -> Vec<u8> {
    let ftyp = bx(b"ftyp", b"isom\0\0\0\0isomavc1");
    let data: u64 = spec
        .tracks
        .iter()
        .map(|t| t.samples() as u64 * t.sample_bytes as u64)
        .sum();
    let mdat = bx(b"mdat", &vec![0xAB; data as usize]);
    let mut off = (ftyp.len() + 8) as u64;
    let mut traks = Vec::new();
    let mut longest = 0;
    for (i, t) in spec.tracks.iter().enumerate() {
        traks.extend(trak(
            t,
            i as u32 + 1,
            spec.movie_timescale,
            off,
            spec.chunk_shift,
        ));
        off += t.samples() as u64 * t.sample_bytes as u64;
        longest = longest.max(tkhd_duration(t, spec.movie_timescale));
    }
    let mut b = Vec::new();
    be32(&mut b, 0);
    be32(&mut b, 0);
    be32(&mut b, spec.movie_timescale);
    be32(&mut b, spec.mvhd_duration.unwrap_or(longest));
    b.extend_from_slice(&[0; 80]);
    let mvhd = fullbox(b"mvhd", 0, &b);
    let moov = bx(b"moov", &[mvhd, traks].concat());
    [ftyp, mdat, moov].concat()
}

/// `stts` of a recording written by the CFR loop: slots `(k, n)` with MF's 100 ns durations.
fn recorded_stts(samples: &[(i64, i64)], fps: i64) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for &(k, n) in samples {
        let d = VideoSampleTime::new(k, n, fps).duration as u32;
        match out.last_mut() {
            Some(l) if l.1 == d => l.0 += 1,
            _ => out.push((1, d)),
        }
    }
    out
}

fn contiguous(slots: &[i64]) -> Vec<(i64, i64)> {
    let mut k = 0;
    slots
        .iter()
        .map(|&n| {
            let s = (k, n);
            k += n;
            s
        })
        .collect()
}

fn names_failed(c: &[znimok_video::check::Check]) -> Vec<&'static str> {
    failed(c).map(|c| c.name).collect()
}

fn run(spec: &Mp4Spec, e: &Mp4Expect) -> Vec<znimok_video::check::Check> {
    let info = parse_mp4(&build(spec)).expect("parses");
    check_mp4(&info, e)
}

// ---------------------------------------------------------------------------------------------
// Good files

/// A 3 s recording at 30 fps written the way the recorder writes it (100 ns timescale, durations
/// 333333/333334), GOP = fps, with AAC audio of the same length: every check passes and the
/// `vidcheck` summary adds up.
#[test]
fn good_recording_passes() {
    let stts = recorded_stts(&contiguous(&[1; 90]), 30);
    let v = TrackSpec::video(10_000_000, stts);
    let spec = Mp4Spec::new(vec![v, TrackSpec::audio(3.0)]);
    let e = Mp4Expect {
        fps: Some(Fps::new(30, 1)),
        slots: Some(90),
        duration_s: Some(3.0),
        ..Mp4Expect::default()
    };
    let c = run(&spec, &e);
    assert!(all_ok(&c), "{c:#?}");
    let info = parse_mp4(&build(&spec)).unwrap();
    assert!(info.moov_last());
    assert_eq!(&info.major_brand.unwrap(), b"isom");
    let s = info.summary().unwrap();
    assert_eq!(s.codec, "H264");
    assert_eq!((s.width, s.height), (1280, 720));
    assert_eq!(s.fps, Some(Fps::new(30, 1)));
    assert_eq!(s.frames, 90);
    assert_eq!(s.keyframes, 3);
    assert!((s.duration_s - 3.0).abs() < 0.002);
    assert!(s.first_s.abs() < 1e-9);
    assert!((s.last_s - 89.0 / 30.0).abs() < 1e-6);
    assert!((s.max_gap_ms - 33.3).abs() < 0.1);
    let text = s.to_string();
    assert!(
        text.starts_with("codec=H264 1280x720 fps=30/1 duration=3.000 s"),
        "{text}"
    );
    assert!(text.contains("frames=90 first=0.000 last=2.967 maxGap=33.3 ms keyframes=3"));
    assert_eq!(info.audio().unwrap().codec_name(), "AAC");
}

/// A late loop writes ONE sample covering several slots (§7 items 1, 18): still on the grid,
/// counted as stretched, and the output frames (`slots`) include the repeats.
#[test]
fn stretched_samples_stay_cfr() {
    let mut slots = vec![1; 40];
    slots[10] = 3;
    slots[25] = 2;
    let stts = recorded_stts(&contiguous(&slots), 30);
    let v = TrackSpec::video(10_000_000, stts);
    let info = parse_mp4(&build(&Mp4Spec::new(vec![v.clone()]))).unwrap();
    let r = info.video().unwrap().cfr(Fps::new(30, 1));
    assert_eq!(r.off_grid, 0);
    assert_eq!(r.stretched, 2);
    assert_eq!(r.max_frames_per_sample, 3);
    assert_eq!(r.slots, 43);
    // GOP counts samples: 40 samples → key frames 1, 31.
    let e = Mp4Expect {
        slots: Some(43),
        ..Mp4Expect::default()
    };
    let c = check_mp4(&info, &e);
    assert!(all_ok(&c), "{c:#?}");
}

/// Clean timescales: 30 fps at 30000 (delta 1000), 60 fps at 90000, NTSC 29.97 at 30000/1001
/// — the rate is inferred and the grid holds.
#[test]
fn other_timescales_and_ntsc() {
    for (ts, delta, fps, gop) in [
        (30_000, 1000, Fps::new(30, 1), 30),
        (90_000, 1500, Fps::new(60, 1), 60),
        (30_000, 1001, Fps::new(30_000, 1001), 30),
    ] {
        let mut v = TrackSpec::video(ts, vec![(120, delta)]);
        v.stss = Some((0..120u32.div_ceil(gop)).map(|k| 1 + gop * k).collect());
        let info = parse_mp4(&build(&Mp4Spec::new(vec![v]))).unwrap();
        assert_eq!(info.video().unwrap().infer_fps(), Some(fps));
        let c = check_mp4(
            &info,
            &Mp4Expect {
                fps: Some(fps),
                slots: Some(120),
                ..Mp4Expect::default()
            },
        );
        assert!(all_ok(&c), "{ts}: {c:#?}");
    }
}

/// Composition offsets WITH an edit list that removes them: the first frame is at zero.
#[test]
fn ctts_with_edit_list_is_fine() {
    let mut v = TrackSpec::video(30_000, vec![(60, 1000)]);
    v.ctts = Some(vec![(60, 1000)]);
    v.edits = Some(vec![(2000, 1000)]);
    let c = run(&Mp4Spec::new(vec![v]), &Mp4Expect::default());
    assert!(all_ok(&c), "{c:#?}");
}

/// No `stss` = every sample is a key frame: GOP passes.
#[test]
fn no_stss_means_all_key() {
    let mut v = TrackSpec::video(30_000, vec![(45, 1000)]);
    v.stss = None;
    let spec = Mp4Spec::new(vec![v]);
    let c = run(&spec, &Mp4Expect::default());
    assert!(all_ok(&c), "{c:#?}");
    let s = parse_mp4(&build(&spec)).unwrap().summary().unwrap();
    assert_eq!(s.keyframes, 45);
}

/// `read_mp4` over a seekable stream gives the same as over bytes.
#[test]
fn reads_from_a_stream() {
    let bytes = build(&Mp4Spec::new(vec![TrackSpec::video(
        30_000,
        vec![(30, 1000)],
    )]));
    let a = read_mp4(&mut std::io::Cursor::new(bytes.clone())).unwrap();
    assert_eq!(a, parse_mp4(&bytes).unwrap());
    assert_eq!(a.file_len, bytes.len() as u64);
    assert_eq!(a.top.len(), 3);
}

// ---------------------------------------------------------------------------------------------
// Failures, one per check

/// A sample shorter than a frame (a skipped / squeezed slot — what §7 item 1 is about) takes
/// the rest of the timeline off the grid.
#[test]
fn off_grid_sample_fails_cfr() {
    let v = TrackSpec::video(30_000, vec![(10, 1000), (1, 500), (19, 1000)]);
    let e = Mp4Expect {
        fps: Some(Fps::new(30, 1)),
        ..Mp4Expect::default()
    };
    let c = run(&Mp4Spec::new(vec![v]), &e);
    // the shortest delta alone would say 60 fps — hence the expected rate
    assert_eq!(names_failed(&c), ["cfr", "fps"]);
}

/// A drifting rate (1 tick per sample too long) is caught at the end of the run.
#[test]
fn drift_fails_cfr() {
    let v = TrackSpec::video(10_000_000, vec![(90, 333_336)]);
    let info = parse_mp4(&build(&Mp4Spec::new(vec![v]))).unwrap();
    let r = info.video().unwrap().cfr(Fps::new(30, 1));
    assert_eq!(r.off_grid, 90);
    let c = check_mp4(
        &info,
        &Mp4Expect {
            fps: Some(Fps::new(30, 1)),
            ..Mp4Expect::default()
        },
    );
    assert!(names_failed(&c).contains(&"cfr"));
}

/// `mdhd` duration that is not the sum of `stts` (players disagree on the length).
#[test]
fn media_duration_mismatch_fails() {
    let mut v = TrackSpec::video(30_000, vec![(30, 1000)]);
    v.mdhd_duration = Some(31_000);
    let c = run(&Mp4Spec::new(vec![v]), &Mp4Expect::default());
    assert_eq!(names_failed(&c), ["mediaDuration"]);
}

/// `tkhd` / `mvhd` durations off.
#[test]
fn movie_duration_mismatch_fails() {
    let mut v = TrackSpec::video(30_000, vec![(30, 1000)]);
    v.tkhd_duration = Some(1500);
    let mut spec = Mp4Spec::new(vec![v]);
    spec.mvhd_duration = Some(1500);
    assert_eq!(
        names_failed(&run(&spec, &Mp4Expect::default())),
        ["movieDuration"]
    );
    let mut spec = Mp4Spec::new(vec![TrackSpec::video(30_000, vec![(30, 1000)])]);
    spec.mvhd_duration = Some(2000);
    assert_eq!(
        names_failed(&run(&spec, &Mp4Expect::default())),
        ["movieDuration"]
    );
}

/// §7 item 2: B-frames' composition offsets without an edit list — the first frame at 33 ms.
#[test]
fn ctts_without_edit_list_fails() {
    let mut v = TrackSpec::video(30_000, vec![(60, 1000)]);
    v.ctts = Some(vec![(60, 1000)]);
    let spec = Mp4Spec::new(vec![v]);
    let c = run(&spec, &Mp4Expect::default());
    assert_eq!(names_failed(&c), ["firstFrameAtZero"]);
    let s = parse_mp4(&build(&spec)).unwrap().summary().unwrap();
    assert!((s.first_s - 1.0 / 30.0).abs() < 1e-9);
}

/// §7 item 3: a key frame off the GOP = fps grid.
#[test]
fn wrong_gop_fails() {
    let mut v = TrackSpec::video(30_000, vec![(90, 1000)]);
    v.stss = Some(vec![1, 31, 50]);
    let c = run(&Mp4Spec::new(vec![v]), &Mp4Expect::default());
    assert_eq!(names_failed(&c), ["gop"]);
    let d = &c.iter().find(|c| c.name == "gop").unwrap().detail;
    assert!(d.contains("first: (1, 31, 50)"), "{d}");
}

/// §7 item 7: odd sides.
#[test]
fn odd_size_fails() {
    let mut v = TrackSpec::video(30_000, vec![(30, 1000)]);
    v.width = 1281;
    assert_eq!(
        names_failed(&run(&Mp4Spec::new(vec![v]), &Mp4Expect::default())),
        ["evenSize"]
    );
}

/// `evprobe`: audio 200 ms shorter than the video; audio that starts 100 ms late.
#[test]
fn audio_drift_and_late_start_fail() {
    let v = TrackSpec::video(30_000, vec![(90, 1000)]);
    let spec = Mp4Spec::new(vec![v.clone(), TrackSpec::audio(2.8)]);
    assert_eq!(
        names_failed(&run(&spec, &Mp4Expect::default())),
        ["audioDrift"]
    );
    // 40 ms of drift is within the 50 ms evprobe allows.
    let spec = Mp4Spec::new(vec![v.clone(), TrackSpec::audio(2.96)]);
    assert!(all_ok(&run(&spec, &Mp4Expect::default())));
    let mut a = TrackSpec::audio(2.9);
    a.edits = Some(vec![(100, -1), (2900, 0)]);
    let spec = Mp4Spec::new(vec![v, a]);
    assert_eq!(
        names_failed(&run(&spec, &Mp4Expect::default())),
        ["audioStart"]
    );
}

/// Tables that disagree: `stsz` counts fewer samples than `stts`.
#[test]
fn inconsistent_tables_fail() {
    let mut v = TrackSpec::video(30_000, vec![(30, 1000)]);
    v.stsz_count = Some(29);
    let c = run(&Mp4Spec::new(vec![v]), &Mp4Expect::default());
    assert!(names_failed(&c).contains(&"sampleTables"), "{c:#?}");
}

/// §7 item 12: offsets that no longer point into `mdat` (metadata inserted before it).
#[test]
fn chunks_outside_mdat_fail() {
    let mut spec = Mp4Spec::new(vec![TrackSpec::video(30_000, vec![(30, 1000)])]);
    spec.chunk_shift = -100;
    let c = run(&spec, &Mp4Expect::default());
    assert_eq!(names_failed(&c), ["chunksInMdat"]);
}

/// Expectations: fps, output frames, duration (vid_test ±0.6 s).
#[test]
fn expectations_fail_when_different() {
    let spec = Mp4Spec::new(vec![TrackSpec::video(30_000, vec![(90, 1000)])]);
    let e = Mp4Expect {
        fps: Some(Fps::new(25, 1)),
        slots: Some(89),
        duration_s: Some(3.7),
        ..Mp4Expect::default()
    };
    let c = run(&spec, &e);
    let f = names_failed(&c);
    for n in ["fps", "slots", "duration"] {
        assert!(f.contains(&n), "{n}: {c:#?}");
    }
    let e = Mp4Expect {
        duration_s: Some(3.5),
        ..Mp4Expect::default()
    };
    assert!(all_ok(&run(&spec, &e)));
}

/// Audio only: no video track is a failed check, not an error.
#[test]
fn no_video_track_fails() {
    let c = run(
        &Mp4Spec::new(vec![TrackSpec::audio(1.0)]),
        &Mp4Expect::default(),
    );
    assert_eq!(names_failed(&c), ["videoTrack"]);
}

// ---------------------------------------------------------------------------------------------
// Hostile input

fn good_bytes() -> Vec<u8> {
    let mut v = TrackSpec::video(30_000, vec![(45, 1000)]);
    v.ctts = Some(vec![(45, 0)]);
    v.edits = Some(vec![(1500, 0)]);
    build(&Mp4Spec::new(vec![v, TrackSpec::audio(1.5)]))
}

/// §7 item 8: an unfinalised file (no `moov`).
#[test]
fn missing_moov_is_an_error() {
    let b = [bx(b"ftyp", b"isom\0\0\0\0"), bx(b"mdat", &[0; 64])].concat();
    assert_eq!(parse_mp4(&b), Err(CheckError::Missing("moov")));
    assert_eq!(parse_mp4(&[]), Err(CheckError::Missing("moov")));
}

/// Every prefix of a good file: an error or a parse, never a panic.
#[test]
fn truncated_never_panics() {
    let b = good_bytes();
    assert!(parse_mp4(&b).is_ok());
    for n in 0..b.len() {
        if let Ok(i) = parse_mp4(&b[..n]) {
            let _ = check_mp4(&i, &Mp4Expect::default());
        }
    }
}

/// Deterministic byte flips over the whole file: never a panic; whatever parses can be checked.
#[test]
fn corrupted_never_panics() {
    let b = good_bytes();
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    for _ in 0..3000 {
        let mut c = b.clone();
        for _ in 0..3 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let i = (x % c.len() as u64) as usize;
            c[i] = (x >> 32) as u8;
        }
        if let Ok(i) = parse_mp4(&c) {
            let _ = check_mp4(&i, &Mp4Expect::default());
            let _ = i.summary();
        }
    }
}

/// Size 0 = to the end of the file (valid for the last box); size 1 = a 64-bit size.
#[test]
fn box_size_zero_and_one() {
    let b = good_bytes();
    let info = parse_mp4(&b).unwrap();
    // mdat with size 0 as the last box
    let moov_at = info.top[2].offset as usize;
    let mdat_at = info.top[1].offset as usize;
    let mut z = [&b[..mdat_at], &b[moov_at..], &b[mdat_at..moov_at]].concat();
    // The copied mdat now ends the file; its offsets moved, which chunksInMdat reports.
    let mdat2 = z.len() - (moov_at - mdat_at);
    z[mdat2..mdat2 + 4].copy_from_slice(&0u32.to_be_bytes());
    let i = parse_mp4(&z).unwrap();
    assert_eq!(i.top.last().unwrap().size, (moov_at - mdat_at) as u64);
    assert!(!i.moov_last());

    // mdat with a 64-bit size
    let data = &b[mdat_at + 8..moov_at];
    let mut large = 1u32.to_be_bytes().to_vec();
    large.extend_from_slice(b"mdat");
    large.extend_from_slice(&(data.len() as u64 + 16).to_be_bytes());
    large.extend_from_slice(data);
    let l = [&b[..mdat_at], &large[..], &b[moov_at..]].concat();
    let i = parse_mp4(&l).unwrap();
    assert_eq!(i.top[1].header, 16);

    // 64-bit size past the end, and smaller than its own header
    for size in [u64::MAX, 8, 0] {
        let mut h = 1u32.to_be_bytes().to_vec();
        h.extend_from_slice(b"free");
        h.extend_from_slice(&size.to_be_bytes());
        let bad = [&b[..], &h[..]].concat();
        assert!(parse_mp4(&bad).is_err(), "{size}");
    }
}

/// A box whose 32-bit size is smaller than its header would loop forever in a naive reader.
#[test]
fn size_below_header_is_an_error() {
    for size in [2u32, 4, 7] {
        let mut b = size.to_be_bytes().to_vec();
        b.extend_from_slice(b"free");
        b.extend_from_slice(&[0; 16]);
        assert!(
            matches!(parse_mp4(&b), Err(CheckError::Malformed(_))),
            "{size}"
        );
    }
    // the same inside moov
    let bad_child = [4u32.to_be_bytes().to_vec(), b"trak".to_vec()].concat();
    let b = bx(b"moov", &bad_child);
    assert!(matches!(parse_mp4(&b), Err(CheckError::Malformed(_))));
}

/// Thousands of empty top-level boxes: capped.
#[test]
fn too_many_boxes_is_an_error() {
    let b: Vec<u8> = (0..MAX_TOP_BOXES + 1)
        .flat_map(|_| bx(b"free", &[]))
        .collect();
    assert!(matches!(parse_mp4(&b), Err(CheckError::TooLarge(..))));
}

/// Entry counts that the box's bytes cannot back: an error before any allocation.
#[test]
fn huge_entry_counts_are_errors() {
    for kind in [
        b"stts", b"ctts", b"stsc", b"stco", b"co64", b"stss", b"elst",
    ] {
        let mut body = Vec::new();
        be32(&mut body, u32::MAX);
        body.extend_from_slice(&[0; 16]);
        let table = fullbox(kind, 0, &body);
        let stbl = bx(b"stbl", &table);
        let inner = if kind == b"elst" {
            bx(b"edts", &table)
        } else {
            bx(b"mdia", &bx(b"minf", &stbl))
        };
        let mvhd = fullbox(b"mvhd", 0, &[0; 96]);
        let moov = bx(b"moov", &[mvhd, bx(b"trak", &inner)].concat());
        assert!(
            matches!(parse_mp4(&moov), Err(CheckError::Truncated(_))),
            "{}",
            std::str::from_utf8(kind).unwrap()
        );
    }
    // stsz with per-sample sizes it does not have
    let mut body = Vec::new();
    be32(&mut body, 0);
    be32(&mut body, u32::MAX);
    let stbl = bx(b"stbl", &fullbox(b"stsz", 0, &body));
    let mvhd = fullbox(b"mvhd", 0, &[0; 96]);
    let moov = bx(
        b"moov",
        &[mvhd, bx(b"trak", &bx(b"mdia", &bx(b"minf", &stbl)))].concat(),
    );
    assert_eq!(parse_mp4(&moov), Err(CheckError::Truncated("stsz")));
}

/// Run counts of four billion samples cost nothing: nothing is expanded per sample.
#[test]
fn huge_run_counts_are_cheap() {
    let mut small = TrackSpec::video(30_000, vec![(10, 1000)]);
    small.stss = None;
    small.mdhd_duration = Some(0);
    // build() would write 8 billion chunk offsets: build 10 samples, then rewrite the runs.
    let t = {
        let mut b = build(&Mp4Spec::new(vec![small]));
        // patch stts: find "stts" and rewrite its runs
        let p = b.windows(4).position(|w| w == b"stts").unwrap();
        let stts = fullbox(b"stts", 0, &{
            let mut x = Vec::new();
            be32(&mut x, 2);
            for _ in 0..2 {
                be32(&mut x, u32::MAX);
                be32(&mut x, 1000);
            }
            x
        });
        // the original stts is 8 + 4 + 4 + 8 bytes = 24; the new one 32: fix parents' sizes
        let old = 24;
        let grow = stts.len() - old;
        b.splice(p - 4..p - 4 + old, stts);
        for kind in [b"moov", b"trak", b"mdia", b"minf", b"stbl"] {
            let q = b.windows(4).position(|w| w == kind).unwrap();
            let s = u32::from_be_bytes(b[q - 4..q].try_into().unwrap()) as usize + grow;
            b[q - 4..q].copy_from_slice(&(s as u32).to_be_bytes());
        }
        b
    };
    let info = parse_mp4(&t).unwrap();
    let tr = info.video().unwrap();
    assert_eq!(tr.stts_samples(), 2 * u32::MAX as u64);
    let tm = tr.timing();
    assert_eq!(tm.duration, 2 * u32::MAX as u64 * 1000);
    let r = tr.cfr(Fps::new(30, 1));
    assert_eq!(r.off_grid, 0);
    assert_eq!(r.slots, 2 * u32::MAX as i64);
    let c = check_mp4(&info, &Mp4Expect::default());
    assert!(names_failed(&c).contains(&"sampleTables"));
}

/// A stream that claims a huge length with a `moov` over the cap: refused before reading it.
#[test]
fn huge_moov_is_refused_without_allocation() {
    struct Sparse {
        head: Vec<u8>,
        len: u64,
        pos: u64,
    }
    impl Read for Sparse {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let n = buf.len().min((self.len - self.pos) as usize);
            for (i, b) in buf[..n].iter_mut().enumerate() {
                *b = self
                    .head
                    .get((self.pos + i as u64) as usize)
                    .copied()
                    .unwrap_or(0);
            }
            self.pos += n as u64;
            Ok(n)
        }
    }
    impl Seek for Sparse {
        fn seek(&mut self, p: SeekFrom) -> std::io::Result<u64> {
            self.pos = match p {
                SeekFrom::Start(x) => x,
                SeekFrom::End(x) => (self.len as i64 + x) as u64,
                SeekFrom::Current(x) => (self.pos as i64 + x) as u64,
            };
            Ok(self.pos)
        }
    }
    let size = MAX_MOOV + 64;
    let mut head = 1u32.to_be_bytes().to_vec();
    head.extend_from_slice(b"moov");
    head.extend_from_slice(&size.to_be_bytes());
    let mut s = Sparse {
        head,
        len: 1 << 40,
        pos: 0,
    };
    assert_eq!(
        read_mp4(&mut s),
        Err(CheckError::TooLarge("moov", size - 16))
    );
    // A multi-gigabyte mdat is skipped by seeking, not read.
    let moov = good_bytes();
    let info = parse_mp4(&moov).unwrap();
    let m = info.top[2];
    let moov_box = &moov[m.offset as usize..(m.offset + m.size) as usize];
    let big: u64 = 8 << 30;
    let mut head = 1u32.to_be_bytes().to_vec();
    head.extend_from_slice(b"mdat");
    head.extend_from_slice(&big.to_be_bytes());
    let s = Sparse {
        head,
        len: big + moov_box.len() as u64,
        pos: 0,
    };
    // moov at the end: a tail over the sparse stream
    struct Tail<'a>(Sparse, &'a [u8], u64);
    impl Read for Tail<'_> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let start = self.0.pos;
            let n = self.0.read(buf)?;
            for (i, b) in buf[..n].iter_mut().enumerate() {
                let p = start + i as u64;
                if p >= self.2 {
                    *b = self.1[(p - self.2) as usize];
                }
            }
            Ok(n)
        }
    }
    impl Seek for Tail<'_> {
        fn seek(&mut self, p: SeekFrom) -> std::io::Result<u64> {
            self.0.seek(p)
        }
    }
    let mut t = Tail(s, moov_box, big);
    let i = read_mp4(&mut t).unwrap();
    assert_eq!(i.top.len(), 2);
    assert_eq!(i.tracks.len(), 2);
}
