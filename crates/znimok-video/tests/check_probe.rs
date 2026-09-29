//! ZK-99: the checks on decoded content — `vidcheck --bars` (`seqBreaks`, `syncBad`, `lagMax`)
//! and `evprobe check` (slot of every sample, audio level of every frame) — on synthetic decoded
//! streams made the way `evprobe gen` made its files.

use znimok_video::HNS_PER_SEC;
use znimok_video::cfr::{VideoSampleTime, slot_time};
use znimok_video::check::probe::*;
use znimok_video::check::{all_ok, failed};
use znimok_video::export::{KeepSeg, time_of};

fn names_failed(c: &[znimok_video::check::Check]) -> Vec<&'static str> {
    failed(c).map(|c| c.name).collect()
}

/// Decoded frames of a recording: sample `(k, n)` shows content `num` at `slot_time(k)`.
fn decoded(samples: &[(i64, i64, i64)], fps: i64) -> Vec<(i64, i64)> {
    samples
        .iter()
        .map(|&(k, _, num)| (slot_time(k, fps), num))
        .collect()
}

/// A clean recording: every frame shows its own slot.
#[test]
fn bars_clean_recording() {
    let s: Vec<_> = (0..90).map(|k| (k, 1, k)).collect();
    let r = check_bars(&decoded(&s, 30), 30, 1);
    assert_eq!(
        r,
        BarsReport {
            frames: 90,
            last_num: 89,
            seq_breaks: 0,
            sync_bad: 0,
            lag_max: 0
        }
    );
    assert!(all_ok(&r.checks(30.0, true)));
}

/// The loop fell behind: a sample covering 3 slots, then the next frames show content taken
/// late (lag) — allowed up to half a second; the numbers jump (seqBreaks) but none is from the
/// future.
#[test]
fn bars_lag_is_allowed_up_to_half_a_second() {
    // the decoder returns the stored samples; content of slot 10 stayed for 3 slots
    let mut s: Vec<_> = (0..10).map(|k| (k, 1, k)).collect();
    s.push((10, 3, 10));
    s.extend((13..30).map(|k| (k, 1, k)));
    let r = check_bars(&decoded(&s, 30), 30, 1);
    assert_eq!((r.sync_bad, r.lag_max, r.seq_breaks), (0, 0, 1));
    assert_eq!(names_failed(&r.checks(30.0, true)), ["seqBreaks"]);
    assert!(all_ok(&r.checks(30.0, false)));

    // content 15 frames behind its slot: the limit at 30 fps; 16 is over
    for (lag, ok) in [(15, true), (16, false)] {
        let s: Vec<_> = (0..40).map(|k| (k, 1, (k - lag).max(0))).collect();
        let r = check_bars(&decoded(&s, 30), 30, 1);
        assert_eq!(r.lag_max, lag);
        assert_eq!(all_ok(&r.checks(30.0, false)), ok, "{lag}");
    }
    // at 60 fps the limit is 30
    let s: Vec<_> = (0..60).map(|k| (k, 1, (k - 30).max(0))).collect();
    let r = check_bars(&decoded(&s, 60), 60, 1);
    assert!(all_ok(&r.checks(60.0, false)));
}

/// Time compressed (a slot skipped instead of stretched — §7 item 1): content from the future.
#[test]
fn bars_future_content_fails() {
    let mut s: Vec<_> = (0..10).map(|k| (k, 1, k)).collect();
    s.extend((10..20).map(|k| (k, 1, k + 1))); // slot 10 lost, the rest shifted
    let r = check_bars(&decoded(&s, 30), 30, 1);
    assert_eq!(r.sync_bad, 10);
    assert_eq!(r.seq_breaks, 1);
    assert_eq!(names_failed(&r.checks(30.0, false)), ["syncBad"]);
}

/// No frames at all.
#[test]
fn bars_empty() {
    let r = check_bars(&[], 30, 1);
    assert_eq!(r.last_num, -1);
    assert_eq!(r.frames, 0);
}

/// Output of an export with cuts: contiguous new timeline, each sample one frame.
fn cut_output(keep: &[KeepSeg], shown: impl Fn(i64) -> i64) -> Vec<DecodedFrame> {
    let mut out = Vec::new();
    let mut k = 0;
    for s in keep {
        for f in s.a..s.b {
            out.push(DecodedFrame {
                time_hns: time_of(k, 30.0),
                duration_hns: time_of(k + 1, 30.0) - time_of(k, 30.0),
                num: shown(f),
            });
            k += 1;
        }
    }
    out
}

/// `evprobe check out.mp4 30 0-10 20-30`: the export shows exactly the kept frames.
#[test]
fn cut_video_matches_segments() {
    let keep = [KeepSeg::new(0, 10), KeepSeg::new(20, 30)];
    assert_eq!(expected_frames(&keep).len(), 20);
    let r = check_cut_video(&cut_output(&keep, |f| f), 30.0, &keep, false);
    assert_eq!(
        r,
        CutVideoReport {
            slots: 20,
            expect: 20,
            bad: 0
        }
    );
    assert!(r.check().ok);

    // a frame from the cut part leaks in
    let r = check_cut_video(
        &cut_output(&keep, |f| if f == 21 { 11 } else { f }),
        30.0,
        &keep,
        false,
    );
    assert_eq!(r.bad, 1);
    assert!(!r.check().ok);

    // one frame too many at the end
    let mut v = cut_output(&keep, |f| f);
    v.push(DecodedFrame {
        time_hns: time_of(20, 30.0),
        duration_hns: time_of(21, 30.0) - time_of(20, 30.0),
        num: 30,
    });
    let r = check_cut_video(&v, 30.0, &keep, false);
    assert_eq!((r.bad, r.slots), (1, 21));

    // a sample covering two slots at the end: slots 21 ≠ 20
    let mut v = cut_output(&keep, |f| f);
    v.last_mut().unwrap().duration_hns *= 2;
    let r = check_cut_video(&v, 30.0, &keep, false);
    assert_eq!((r.bad, r.slots), (0, 21));
    assert!(!r.check().ok);
}

/// `EV_STRETCH`: the source had samples covering 3 frames at frame 5 of every ten, so frames 6
/// and 7 show 5 — expected with the flag, wrong without it.
#[test]
fn cut_video_stretch_rule() {
    let keep = [KeepSeg::new(3, 28)];
    let shown = |f: i64| {
        if f % 10 == 6 || f % 10 == 7 {
            f - f % 10 + 5
        } else {
            f
        }
    };
    let out = cut_output(&keep, shown);
    assert_eq!(check_cut_video(&out, 30.0, &keep, true).bad, 0);
    assert_eq!(check_cut_video(&out, 30.0, &keep, false).bad, 6);
    assert_eq!(shown_frames(&[5, 6, 7, 8, 16], true), [5, 5, 5, 8, 15]);
}

/// Negative timestamps (priming before zero) are wrong slots, not a panic.
#[test]
fn cut_video_negative_time() {
    let keep = [KeepSeg::new(0, 2)];
    let v = [DecodedFrame {
        time_hns: -HNS_PER_SEC,
        duration_hns: 333_333,
        num: 0,
    }];
    assert_eq!(check_cut_video(&v, 30.0, &keep, false).bad, 1);
}

/// `evprobe gen … audio`: 1 kHz, amplitude `3000·(frame % 8 + 1)`, 48 kHz, audio frames of
/// video frame `f` end at `round((f+1)·48000/fps)`; `lead` silent frames first (AAC priming).
fn tone(frames: &[i64], fps: f64, lead: usize) -> Vec<i16> {
    let rate = 48_000.0;
    let mut pcm = vec![0i16; lead];
    let mut pos = 0i64;
    for (k, &f) in frames.iter().enumerate() {
        let end = ((k as f64 + 1.0) * rate / fps + 0.5) as i64;
        let amp = 3000.0 * ((f % 8) + 1) as f64;
        for i in pos..end {
            pcm.push((amp * (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / rate).sin()) as i16);
        }
        pos = end;
    }
    pcm
}

/// Levels match frame by frame; the AAC priming offset is found within ±30 ms.
#[test]
fn levels_match_with_priming() {
    let expect: Vec<i64> = (0..60).collect();
    for lead in [0, 1024, 1400] {
        let pcm = tone(&expect, 30.0, lead);
        let r = check_levels(&pcm, 0, 48_000, 30.0, &expect, &[]);
        assert_eq!(r.level_bad, 0, "lead {lead}");
        // the window is the middle 40 % of a frame, so every offset within ±0.2 frame (480)
        // scores the same; evprobe keeps the first — the harness only asked for ≤ 1 frame
        assert!(
            (r.offset - lead as i64).abs() <= 480 + 48,
            "lead {lead}: {}",
            r.offset
        );
        assert!(all_ok(&r.checks()), "{:?}", r.checks());
    }
}

/// Audio of an export with cuts: the levels follow the kept frames.
#[test]
fn levels_follow_cuts() {
    let keep = [KeepSeg::new(0, 12), KeepSeg::new(40, 60)];
    let expect = expected_frames(&keep);
    let pcm = tone(&expect, 30.0, 0);
    let r = check_levels(&pcm, 0, 48_000, 30.0, &expect, &[]);
    assert_eq!(r.level_bad, 0);
    // the audio of the uncut source against the cut expectation is wrong
    let src: Vec<i64> = (0..32).collect();
    let r = check_levels(&tone(&src, 30.0, 0), 0, 48_000, 30.0, &expect, &[]);
    assert!(r.level_bad > 0);
}

/// Audio two frames late (67 ms — past the ±30 ms search): levels wrong.
#[test]
fn levels_out_of_sync_fail() {
    let expect: Vec<i64> = (0..60).collect();
    let pcm = tone(&expect, 30.0, 3200);
    let r = check_levels(&pcm, 0, 48_000, 30.0, &expect, &[]);
    assert!(r.level_bad > 10, "{}", r.level_bad);
    assert!(names_failed(&r.checks()).contains(&"levelBad"));
}

/// A first sample stamped later than zero is accounted for (`base`).
#[test]
fn levels_use_the_first_timestamp() {
    let expect: Vec<i64> = (0..60).collect();
    let pcm = tone(&expect, 30.0, 0);
    // drop the first 10 frames of audio and stamp the rest at 10/30 s
    let cut = 16_000;
    let r = check_levels(&pcm[cut..], time_of(10, 30.0), 48_000, 30.0, &expect, &[]);
    assert_eq!(r.level_bad, 0);
    assert!(r.offset.abs() <= 480);
    // but the length is now short by a third of a second
    assert_eq!(names_failed(&r.checks()), ["audioLength"]);
}

/// `EV_SILENT=20-35`: headphones pulled — silence expected inside, one frame at each edge not
/// counted; without the segment it is a failure.
#[test]
fn levels_silent_segments() {
    let expect: Vec<i64> = (0..60).collect();
    let mut pcm = tone(&expect, 30.0, 0);
    let a = (20.0 * 1600.0) as usize;
    let b = (35.0 * 1600.0) as usize;
    pcm[a..b].fill(0);
    let silent = [KeepSeg::new(20, 35)];
    let r = check_levels(&pcm, 0, 48_000, 30.0, &expect, &silent);
    assert_eq!(r.level_bad, 0);
    let r = check_levels(&pcm, 0, 48_000, 30.0, &expect, &[]);
    assert!(r.level_bad >= 13);
}

/// Audio longer than the video by more than 50 ms.
#[test]
fn audio_length_tolerance() {
    let expect: Vec<i64> = (0..30).collect();
    let mut pcm = tone(&expect, 30.0, 0);
    pcm.extend(std::iter::repeat_n(0, 2000)); // +41.7 ms: fine
    assert!(all_ok(
        &check_levels(&pcm, 0, 48_000, 30.0, &expect, &[]).checks()
    ));
    pcm.extend(std::iter::repeat_n(0, 1000)); // +62.5 ms
    let r = check_levels(&pcm, 0, 48_000, 30.0, &expect, &[]);
    assert_eq!(names_failed(&r.checks()), ["audioLength"]);
}

/// The recorder's own sample times feed `check_bars` the way a decoder would: a stretched sample
/// is ONE decoded frame, and the content after it may lag.
#[test]
fn recorder_times_through_bars() {
    let times = [
        VideoSampleTime::new(0, 1, 30),
        VideoSampleTime::new(1, 4, 30),
        VideoSampleTime::new(5, 1, 30),
    ];
    let frames: Vec<(i64, i64)> = times.iter().map(|t| (t.time, t.slot)).collect();
    let r = check_bars(&frames, 30, 1);
    assert_eq!((r.sync_bad, r.lag_max, r.seq_breaks), (0, 0, 1));
}
