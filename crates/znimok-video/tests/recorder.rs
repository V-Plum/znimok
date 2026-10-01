//! The recording loop end to end on synthetic sources and a memory sink, driven by a manual
//! clock. Each test names the item of `docs/discovery/inventory_video.md` it covers.

use std::sync::Arc;
use std::time::Duration;
use znimok_video::audio::{self, TimelineProbe, to_s16};
use znimok_video::clock::{Clock, ManualClock};
use znimok_video::events::{Button, EventQueue, InputEvent};
use znimok_video::export::{AudioCut, KeepSeg, split_video_sample};
use znimok_video::recorder::{
    AudioLayout, AudioWarning, Recorder, RecorderConfig, RecordingControl, RecordingResult, Step,
    commit_part, part_path,
};
use znimok_video::synthetic::*;
use znimok_video::traits::*;
use znimok_video::{Result, VideoError};

const FPS: u32 = 30;

struct Rig {
    clock: ManualClock,
    ctl: RecordingControl,
    probe: Arc<TimelineProbe>,
}

impl Rig {
    fn new() -> Self {
        Self {
            clock: ManualClock::new(),
            ctl: RecordingControl::new(),
            probe: Arc::new(TimelineProbe::default()),
        }
    }

    fn screen(&self) -> SyntheticScreen {
        SyntheticScreen::new(self.clock.clone()).change_every(Duration::from_millis(16))
    }

    fn tone(&self) -> Box<dyn AudioSource> {
        Box::new(
            SyntheticAudio::new(
                AudioKind::System,
                Tone::FrameCoded { fps: FPS },
                self.clock.clone(),
            )
            .probe(self.probe.clone()),
        )
    }

    fn config(&self, layout: AudioLayout) -> RecorderConfig {
        RecorderConfig {
            fps: FPS,
            audio_layout: layout,
            probe: Some(self.probe.clone()),
        }
    }

    fn open(
        &self,
        screen: SyntheticScreen,
        sink: MemorySink,
        audio: Vec<Box<dyn AudioSource>>,
        layout: AudioLayout,
    ) -> Recorder<ManualClock, SyntheticScreen, MemorySink> {
        let mut sink = Some(sink);
        Recorder::open(
            self.clock.clone(),
            screen,
            move |_tracks| Ok(sink.take().unwrap_or_default()),
            audio,
            self.config(layout),
            self.ctl.clone(),
        )
        .unwrap()
    }

    /// Run for `ms` of clock time; `plan(ms since start)` may pause/resume/stop.
    fn run(
        &self,
        mut rec: Recorder<ManualClock, SyntheticScreen, MemorySink>,
        ms: i64,
        mut plan: impl FnMut(i64, &RecordingControl),
    ) -> (RecordingResult, MemorySink) {
        let start = self.clock.ticks();
        let f = self.clock.frequency();
        loop {
            let t = (self.clock.ticks() - start) * 1000 / f;
            plan(t, &self.ctl);
            if t >= ms {
                self.ctl.stop();
            }
            if rec.step() == Step::Stopped {
                break;
            }
        }
        rec.finish()
    }
}

fn frames_of(ms: i64) -> i64 {
    ms * FPS as i64 / 1000
}

/// §2.3, §7 item 1: a steady loop writes one sample per slot, the timeline is continuous and
/// nothing is from the future; the file is finalised and committed.
#[test]
fn steady_loop_is_cfr() {
    let rig = Rig::new();
    let rec = rig.open(
        rig.screen(),
        MemorySink::new(),
        vec![],
        AudioLayout::Separate,
    );
    let (r, sink) = rig.run(rec, 2000, |_, _| {});
    let c = check_sync(&sink.video, FPS);
    assert_eq!(c.sync_bad, 0);
    assert_eq!(c.gaps, 0);
    assert!(
        (r.frames - frames_of(2000)).abs() <= 2,
        "frames {}",
        r.frames
    );
    assert_eq!(c.frames, r.frames);
    assert_eq!(sink.video_duration(), r.duration_hns);
    assert!(sink.video.iter().all(|v| v.time.slots == 1));
    assert!(sink.finalized && r.committed && r.error.is_none());
}

/// §7 item 1 (`VidEmit`): when the loop falls behind (a 250 ms stall), ONE sample covers every
/// slot that has come — the video keeps its real length, content lags but is never from the
/// future (`syncBad = 0`, `lagMax` > 0).
#[test]
fn late_loop_stretches_one_sample() {
    let rig = Rig::new();
    let screen = rig.screen().stall_at(15, Duration::from_millis(250));
    let rec = rig.open(screen, MemorySink::new(), vec![], AudioLayout::Separate);
    let (r, sink) = rig.run(rec, 2000, |_, _| {});
    let long = sink
        .video
        .iter()
        .find(|v| v.time.slots > 1)
        .expect("a stretched sample");
    assert_eq!(long.time.slot, 16);
    assert_eq!(
        long.time.slots, 7,
        "slots 16..=22 came during the 250 ms stall"
    );
    let c = check_sync(&sink.video, FPS);
    assert_eq!(c.sync_bad, 0);
    assert_eq!(c.gaps, 0);
    assert_eq!(c.lag_max, 6);
    assert!(r.samples < r.frames);
    assert!(
        (r.frames - frames_of(2000)).abs() <= 2,
        "real length kept: {}",
        r.frames
    );
}

/// §2.3, §7 item 27: time starts at the first REAL frame; nothing before it counts, and `wall0`
/// is the wall clock of that moment.
#[test]
fn time_starts_at_first_real_frame() {
    let rig = Rig::new();
    let wall_open = rig.clock.wall_ms();
    let screen = rig.screen().first_frame_after(Duration::from_millis(400));
    let rec = rig.open(screen, MemorySink::new(), vec![], AudioLayout::Separate);
    let (r, sink) = rig.run(rec, 1400, |_, _| {});
    assert!((r.frames - frames_of(1000)).abs() <= 2, "{}", r.frames);
    assert_eq!(sink.video[0].time.time, 0);
    let w0 = r.wall0.unwrap();
    assert!(
        (w0 - wall_open - 400.0).abs() < 1.0,
        "wall0 {} after open",
        w0 - wall_open
    );
}

/// §2.4 (CAPS-101): a 2 s pause leaves no gap and no frozen frame — the first frame after resume
/// is the next slot, one slot long; the video is as long as the unpaused time; the pause is
/// logged in wall time.
#[test]
fn pause_shifts_t0_without_gap() {
    let rig = Rig::new();
    let rec = rig.open(
        rig.screen(),
        MemorySink::new(),
        vec![],
        AudioLayout::Separate,
    );
    let (r, sink) = rig.run(rec, 4000, |t, c| c.set_paused((1000..3000).contains(&t)));
    let c = check_sync(&sink.video, FPS);
    assert_eq!((c.sync_bad, c.gaps), (0, 0));
    assert!((r.frames - frames_of(2000)).abs() <= 2, "{}", r.frames);
    assert!(
        sink.video.iter().all(|v| v.time.slots == 1),
        "no frozen (stretched) frame"
    );
    assert_eq!(r.pauses.spans.len(), 1);
    assert!(
        (r.pauses.total_ms() - 2000.0).abs() < 60.0,
        "{}",
        r.pauses.total_ms()
    );
    let w0 = r.wall0.unwrap();
    assert_eq!(
        r.pauses
            .video_ms(w0 + 3500.0, w0, r.duration_ms, false)
            .map(|m| (m - 1500).abs() < 60),
        Some(true)
    );
}

/// §2.4 / CAPS-83: stopping while paused closes the pause span; nothing after the pause started
/// is written.
#[test]
fn stop_while_paused() {
    let rig = Rig::new();
    let rec = rig.open(
        rig.screen(),
        MemorySink::new(),
        vec![],
        AudioLayout::Separate,
    );
    let (r, _) = rig.run(rec, 1500, |t, c| c.set_paused(t >= 1000));
    assert_eq!(r.pauses.spans.len(), 1);
    assert!((r.pauses.total_ms() - 500.0).abs() < 60.0);
    assert!((r.frames - frames_of(1000)).abs() <= 2);
    assert!(r.committed);
}

/// §3: audio zero is the first video frame (earlier audio dropped), the frame-coded tone matches
/// the video frame everywhere, and the tail ends exactly at the end of the video.
#[test]
fn audio_zero_and_tail_match_video() {
    let rig = Rig::new();
    let screen = rig.screen().first_frame_after(Duration::from_millis(300));
    let rec = rig.open(
        screen,
        MemorySink::new(),
        vec![rig.tone()],
        AudioLayout::Separate,
    );
    let (r, sink) = rig.run(rec, 2300, |_, _| {});
    assert_eq!(r.audio_tracks, 1);
    let t = sink.track(0);
    assert_eq!(t[0].0.index, 0);
    let total: i64 = t.iter().map(|(s, _)| s.frames).sum();
    assert_eq!(
        total,
        r.frames * audio::RATE / FPS as i64,
        "audio ends with the video"
    );
    assert!(
        t.windows(2)
            .all(|w| w[0].0.index + w[0].0.frames == w[1].0.index),
        "contiguous"
    );
    let (checked, wrong) = check_audio_sync(&t, FPS, 200);
    assert!(checked > 1000, "{checked}");
    assert_eq!(wrong, 0);
}

/// §2.4 + §3: across a pause the audio of the pause is dropped and later audio is shifted by
/// the same amount — the frame-coded tone still matches the video frame after resume, and the
/// track is exactly as long as the video.
#[test]
fn audio_stays_in_sync_across_pause() {
    let rig = Rig::new();
    let rec = rig.open(
        rig.screen(),
        MemorySink::new(),
        vec![rig.tone()],
        AudioLayout::Separate,
    );
    let (r, sink) = rig.run(rec, 5000, |t, c| c.set_paused((1500..3200).contains(&t)));
    let t = sink.track(0);
    let total: i64 = t.iter().map(|(s, _)| s.frames).sum();
    assert_eq!(total, r.frames * audio::RATE / FPS as i64);
    assert!(
        t.windows(2)
            .all(|w| w[0].0.index + w[0].0.frames == w[1].0.index)
    );
    let (checked, wrong) = check_audio_sync(&t, FPS, 200);
    assert!(checked > 1500);
    assert_eq!(wrong, 0, "tone matches frames after the pause");
}

/// §2.4: a pause shorter than the 300 ms audio lag — the pre-pause audio that had not ripened
/// is written on resume, the track stays contiguous and in sync.
#[test]
fn short_pause_under_the_audio_lag() {
    let rig = Rig::new();
    let rec = rig.open(
        rig.screen(),
        MemorySink::new(),
        vec![rig.tone()],
        AudioLayout::Separate,
    );
    let (r, sink) = rig.run(rec, 2000, |t, c| c.set_paused((1000..1100).contains(&t)));
    let t = sink.track(0);
    let total: i64 = t.iter().map(|(s, _)| s.frames).sum();
    assert_eq!(total, r.frames * audio::RATE / FPS as i64);
    let (_, wrong) = check_audio_sync(&t, FPS, 200);
    assert_eq!(wrong, 0);
}

/// §7 item 41: a source that sends nothing for a while (loopback when nothing plays) becomes
/// silence at the right place — the stamps, not a sample counter, place the audio after the gap.
#[test]
fn packet_gap_becomes_silence_in_place() {
    let rig = Rig::new();
    let start_ms = rig.clock.ticks() / 10_000;
    let src = SyntheticAudio::new(AudioKind::System, Tone::Constant(0.5), rig.clock.clone())
        .gap(start_ms + 800, 500);
    let rec = rig.open(
        rig.screen(),
        MemorySink::new(),
        vec![Box::new(src)],
        AudioLayout::Separate,
    );
    let (r, sink) = rig.run(rec, 2000, |_, _| {});
    let pcm: Vec<i16> = sink
        .track(0)
        .iter()
        .flat_map(|(_, p)| p.iter().copied())
        .collect();
    assert_eq!(pcm.len() as i64, r.frames * audio::RATE / FPS as i64 * 2);
    let at = |ms: i64| pcm[(ms * 48 * 2) as usize];
    assert_eq!(at(400), 16383);
    assert_eq!(at(1000), 0, "gap → silence");
    assert_eq!(at(1600), 16383, "after the gap — where the stamps say");
}

/// §7 item 41: timestamp jitter under 10 ms does not tear the sound (packets stay contiguous);
/// packets flagged silent are not added.
#[test]
fn jitter_absorbed_and_silent_flag() {
    let rig = Rig::new();
    let src = SyntheticAudio::new(AudioKind::System, Tone::Constant(0.25), rig.clock.clone())
        .jitter(vec![0, 300, -200, 470]);
    let mic = SyntheticAudio::new(
        AudioKind::Microphone,
        Tone::Constant(0.5),
        rig.clock.clone(),
    )
    .silent();
    let rec = rig.open(
        rig.screen(),
        MemorySink::new(),
        vec![Box::new(src), Box::new(mic)],
        AudioLayout::Mixed,
    );
    let (_, sink) = rig.run(rec, 1500, |_, _| {});
    let pcm: Vec<i16> = sink
        .track(0)
        .iter()
        .flat_map(|(_, p)| p.iter().copied())
        .collect();
    let body = &pcm[48 * 2 * 20..pcm.len() - 48 * 2 * 40];
    assert!(
        body.iter().all(|&v| v == to_s16(0.25)),
        "no holes, no doubled parts, mic silent"
    );
}

/// §7 item 42: audio is written 300 ms behind now; packets that arrive 200 ms late still make it.
#[test]
fn audio_lags_300ms_and_late_packets_land() {
    let rig = Rig::new();
    let src = SyntheticAudio::new(AudioKind::System, Tone::Constant(0.5), rig.clock.clone())
        .deliver_after(48 * 200);
    let mut rec = rig.open(
        rig.screen(),
        MemorySink::new(),
        vec![Box::new(src)],
        AudioLayout::Separate,
    );
    let start = rig.clock.ticks();
    let mut max_ahead = i64::MIN;
    while (rig.clock.ticks() - start) < 20_000_000 {
        rec.step();
        if let (Some(last), Some(t0)) = (rec.sink().audio.last(), rec.cfr().t0()) {
            let now_idx = (rig.clock.ticks() - t0) * 48 / 10_000;
            max_ahead = max_ahead.max(last.time.index + last.time.frames - (now_idx - audio::LAG));
        }
    }
    assert!(
        max_ahead <= 0,
        "never closer than 300 ms to now: {max_ahead}"
    );
    rig.ctl.stop();
    rec.step();
    let (r, sink) = rec.finish();
    let pcm: Vec<i16> = sink
        .track(0)
        .iter()
        .flat_map(|(_, p)| p.iter().copied())
        .collect();
    assert_eq!(pcm.len() as i64, r.frames * 1600 * 2);
    let body = &pcm[..pcm.len() - 48 * 2 * 250];
    assert!(body.iter().all(|&v| v == 16383), "late packets landed");
}

/// §3: a lost device → silence and a reopen every 500 ms; after it comes back audio continues.
#[test]
fn lost_device_reopens() {
    let rig = Rig::new();
    let start_ms = rig.clock.ticks() / 10_000;
    let src = SyntheticAudio::new(AudioKind::System, Tone::Constant(0.5), rig.clock.clone())
        .lose_device_at_ms(start_ms + 700);
    let rec = rig.open(
        rig.screen(),
        MemorySink::new(),
        vec![Box::new(src)],
        AudioLayout::Separate,
    );
    let (r, sink) = rig.run(rec, 2500, |_, _| {});
    let pcm: Vec<i16> = sink
        .track(0)
        .iter()
        .flat_map(|(_, p)| p.iter().copied())
        .collect();
    assert_eq!(pcm.len() as i64, r.frames * 1600 * 2);
    let at = |ms: i64| pcm[(ms * 48 * 2) as usize];
    assert_eq!(at(300), 16383);
    assert_eq!(at(1000), 0, "lost → silence");
    assert_eq!(at(2000), 16383, "reopened");
}

/// §7 item 43 / §3: a denied microphone gives the MicDenied warning and the recording goes on
/// with the other source; no source at all → no audio track.
#[test]
fn denied_microphone_records_without_it() {
    let rig = Rig::new();
    let mic = SyntheticAudio::new(AudioKind::Microphone, Tone::Quiet440, rig.clock.clone())
        .fail_open(AudioError::PermissionDenied);
    let rec = rig.open(
        rig.screen(),
        MemorySink::new(),
        vec![rig.tone(), Box::new(mic)],
        AudioLayout::Separate,
    );
    let (r, _) = rig.run(rec, 500, |_, _| {});
    assert_eq!(r.warning, Some(AudioWarning::MicDenied));
    assert_eq!(r.audio_tracks, 1);

    let rig = Rig::new();
    let busy = SyntheticAudio::new(AudioKind::System, Tone::Quiet440, rig.clock.clone())
        .fail_open(AudioError::DeviceInUse);
    let rec = rig.open(
        rig.screen(),
        MemorySink::new(),
        vec![Box::new(busy)],
        AudioLayout::Separate,
    );
    let (r, sink) = rig.run(rec, 500, |_, _| {});
    assert_eq!(r.warning, Some(AudioWarning::AudioBusy));
    assert_eq!(r.audio_tracks, 0);
    assert!(sink.audio.is_empty() && r.committed);
}

/// §7 item 43: sources are opened before the sink; a sink that does not take audio is opened
/// again without it — a recording without sound beats no recording.
#[test]
fn sink_without_audio_fallback() {
    let rig = Rig::new();
    let mut asked = Vec::new();
    let rec: Recorder<_, _, MemorySink> = Recorder::open(
        rig.clock.clone(),
        rig.screen(),
        |tracks| {
            asked.push(tracks);
            if tracks > 0 {
                Err(VideoError::Encoder("no AAC".into()))
            } else {
                Ok(MemorySink::new())
            }
        },
        vec![rig.tone()],
        rig.config(AudioLayout::Separate),
        rig.ctl.clone(),
    )
    .unwrap();
    assert_eq!(rec.audio_tracks(), 0);
    let (r, sink) = rig.run(rec, 300, |_, _| {});
    assert_eq!(asked, vec![1, 0]);
    assert_eq!(r.warning, Some(AudioWarning::AudioNone));
    assert!(sink.audio.is_empty() && !sink.video.is_empty());

    let e: Result<Recorder<ManualClock, SyntheticScreen, MemorySink>> = Recorder::open(
        rig.clock.clone(),
        rig.screen(),
        |_| Err(VideoError::Encoder("none".into())),
        vec![],
        rig.config(AudioLayout::Separate),
        RecordingControl::new(),
    );
    assert!(matches!(e, Err(VideoError::Encoder(_))));
}

/// PLAN decision 16 vs §7 item 44: separate tracks keep each source; mixed sums them into one
/// track through the soft limiter (0.5 + 0.5 → limited, not clipped).
#[test]
fn separate_tracks_and_mixed_track() {
    let rig = Rig::new();
    let a = || SyntheticAudio::new(AudioKind::System, Tone::Constant(0.5), rig.clock.clone());
    let b = || {
        SyntheticAudio::new(
            AudioKind::Microphone,
            Tone::Constant(0.5),
            rig.clock.clone(),
        )
    };
    let rec = rig.open(
        rig.screen(),
        MemorySink::new(),
        vec![Box::new(a()), Box::new(b())],
        AudioLayout::Separate,
    );
    let (r, sink) = rig.run(rec, 1000, |_, _| {});
    assert_eq!(r.audio_tracks, 2);
    for tr in 0..2 {
        let t = sink.track(tr);
        assert_eq!(t[3].1[0], 16383);
        let total: i64 = t.iter().map(|(s, _)| s.frames).sum();
        assert_eq!(total, r.frames * 1600);
    }
    // ZK-189: each track's loudness, a byte per 10 ms of what was written: √0.5 · 255 ≈ 180.
    assert_eq!(r.audio_peaks.len(), 2);
    for p in &r.audio_peaks {
        let written: i64 = r.frames * 1600 / 480;
        assert!(
            (p.len() as i64 - written).abs() <= 1,
            "{} of {written}",
            p.len()
        );
        // The tail is the silence that pads the audio to the end of the video.
        let body = &p[..p.len().saturating_sub(3)];
        let odd: Vec<(usize, u8)> = body
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, v)| !(178..=182).contains(v))
            .collect();
        assert!(odd.is_empty(), "{odd:?} of {}", p.len());
    }
    let mixed = audio::mix_tracks_s16(&[&sink.track(0)[3].1[..4], &sink.track(1)[3].1[..4]]);
    assert_eq!(mixed[0], to_s16(2.0 * 16383.0 / 32767.0));

    let rig = Rig::new();
    let a = SyntheticAudio::new(AudioKind::System, Tone::Constant(0.5), rig.clock.clone());
    let b = SyntheticAudio::new(
        AudioKind::Microphone,
        Tone::Constant(0.5),
        rig.clock.clone(),
    );
    let rec = rig.open(
        rig.screen(),
        MemorySink::new(),
        vec![Box::new(a), Box::new(b)],
        AudioLayout::Mixed,
    );
    let (r, sink) = rig.run(rec, 1000, |_, _| {});
    assert_eq!(r.audio_tracks, 1);
    assert_eq!(sink.track(0)[3].1[0], to_s16(1.0));
    assert!(sink.track(0)[3].1[0] < 32767);
}

/// §2.2 / §7 item 11: the encoder holding its pool → retry every 2 ms, up to 250 times; then the
/// recording fails but what was written is finalised (§2.5).
#[test]
fn backpressure() {
    let rig = Rig::new();
    let sink = MemorySink {
        busy: 5,
        ..MemorySink::new()
    };
    let rec = rig.open(rig.screen(), sink, vec![], AudioLayout::Separate);
    let (r, sink) = rig.run(rec, 500, |_, _| {});
    assert_eq!(sink.busy_given, 5);
    assert!(r.error.is_none());
    assert_eq!(check_sync(&sink.video, FPS).sync_bad, 0);

    let rig = Rig::new();
    let sink = MemorySink {
        busy: 1000,
        busy_after: 10,
        ..MemorySink::new()
    };
    let rec = rig.open(rig.screen(), sink, vec![], AudioLayout::Separate);
    let (r, sink) = rig.run(rec, 5000, |_, _| {});
    assert_eq!(r.error, Some(VideoError::Backpressure));
    assert_eq!(sink.busy_given, 251, "one try plus 250 retries");
    assert_eq!(sink.video.len(), 10);
    assert!(sink.finalized && r.committed);
}

/// §2.5 / §7 item 8: a write error mid-recording stops it, and the file is still finalised.
#[test]
fn write_error_still_finalises() {
    let rig = Rig::new();
    let sink = MemorySink {
        fail_at: Some(12),
        ..MemorySink::new()
    };
    let rec = rig.open(rig.screen(), sink, vec![rig.tone()], AudioLayout::Separate);
    let (r, sink) = rig.run(rec, 3000, |_, _| {});
    assert!(matches!(r.error, Some(VideoError::Write(_))));
    assert_eq!(r.samples, 12);
    assert!(sink.finalized && r.committed);
}

/// §7 item 8: finalize failed → no index → the `.part` is deleted, never shown; nothing written
/// → not committed either. A good recording is renamed from `.part`.
#[test]
fn part_file_commit_and_delete() {
    let dir = std::env::temp_dir().join(format!("znimok-video-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let fin = dir.join("rec.mp4");
    let part = part_path(&fin);
    assert_eq!(part.file_name().unwrap(), "rec.mp4.part");

    let rig = Rig::new();
    let rec = rig.open(
        rig.screen(),
        MemorySink {
            fail_finalize: true,
            ..MemorySink::new()
        },
        vec![],
        AudioLayout::Separate,
    );
    let (r, _) = rig.run(rec, 300, |_, _| {});
    assert!(matches!(r.error, Some(VideoError::Finalize(_))));
    std::fs::write(&part, b"x").unwrap();
    assert!(!commit_part(&part, &fin, &r).unwrap());
    assert!(!part.exists() && !fin.exists());

    let rig = Rig::new();
    let rec = rig.open(
        rig.screen(),
        MemorySink::new(),
        vec![],
        AudioLayout::Separate,
    );
    let (r, _) = rig.run(rec, 300, |_, _| {});
    std::fs::write(&part, b"mp4").unwrap();
    assert!(commit_part(&part, &fin, &r).unwrap());
    assert!(!part.exists() && fin.exists());

    let rig = Rig::new();
    let screen = rig.screen().first_frame_after(Duration::from_secs(10));
    let rec = rig.open(screen, MemorySink::new(), vec![], AudioLayout::Separate);
    let (r, sink) = rig.run(rec, 300, |_, _| {});
    assert!(
        !r.committed && !sink.finalized && r.samples == 0,
        "nothing written"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// §2.5: the recorded window closed → the recording stops by itself and is saved.
#[test]
fn closed_window_stops_and_saves() {
    let rig = Rig::new();
    let screen = rig.screen().closes_after(Duration::from_millis(1200));
    let rec = rig.open(screen, MemorySink::new(), vec![], AudioLayout::Separate);
    let (r, _) = rig.run(rec, 60_000, |_, _| {});
    assert!(r.committed && r.error.is_none());
    assert!((r.frames - frames_of(1200)).abs() <= 2, "{}", r.frames);
}

/// §8 "(?)" AVAssetWriter places frames by timestamp: for such a sink a stretched sample is
/// written as one-slot samples of the same frame — the same timeline.
#[test]
fn timestamp_sink_gets_expanded_samples() {
    let rig = Rig::new();
    let screen = rig.screen().stall_at(10, Duration::from_millis(200));
    let rec = rig.open(
        screen,
        MemorySink::by_timestamp(),
        vec![],
        AudioLayout::Separate,
    );
    let (r, sink) = rig.run(rec, 1000, |_, _| {});
    assert!(sink.video.iter().all(|v| v.time.slots == 1));
    let c = check_sync(&sink.video, FPS);
    assert_eq!((c.gaps, c.sync_bad, c.frames), (0, 0, r.frames));
    assert!(c.lag_max >= 5);
    assert_eq!(r.samples, r.frames);
}

/// §2.4 + §4: clicks before the first frame and during a pause are not recorded; after the
/// pause their time is video time (shifted t0).
#[test]
fn clicks_gated_by_t0_and_pause() {
    let rig = Rig::new();
    let q = EventQueue::new();
    let click = |c: &ManualClock| InputEvent {
        ticks: c.ticks(),
        x: 5,
        y: 6,
        button: Button::Left,
        down: true,
    };
    q.push(click(&rig.clock)); // the hotkey itself, before the first frame
    let screen = rig.screen().first_frame_after(Duration::from_millis(100));
    let rec = rig
        .open(screen, MemorySink::new(), vec![], AudioLayout::Separate)
        .with_events(q.clone());
    let clock = rig.clock.clone();
    let mut done = [false; 3];
    let (r, _) = rig.run(rec, 3000, |t, c| {
        c.set_paused((1000..2000).contains(&t));
        for (i, at) in [500, 1500, 2500].into_iter().enumerate() {
            if t >= at && !done[i] {
                done[i] = true;
                q.push(click(&clock));
            }
        }
    });
    let ms: Vec<i64> = r.events.iter().map(|e| e.ms).collect();
    assert_eq!(ms.len(), 2, "{ms:?}");
    assert!((ms[0] - 400).abs() < 40, "{ms:?}");
    assert!(
        (ms[1] - 1400).abs() < 60,
        "after the pause — video time: {ms:?}"
    );
}

/// §7 item 18: a decoder hands back stretched samples; counting frames by DURATION gives the
/// full length, and an export splits them by kept segments into a contiguous timeline.
#[test]
fn decode_and_export_stretched_samples() {
    let rig = Rig::new();
    let screen = rig.screen().stall_at(20, Duration::from_millis(300));
    let rec = rig.open(
        screen,
        MemorySink::new(),
        vec![rig.tone()],
        AudioLayout::Separate,
    );
    let (r, sink) = rig.run(rec, 2000, |_, _| {});
    let mut dec = sink.decoder(FPS);
    assert_eq!(dec.info().duration_hns, r.duration_hns);
    let (mut frames, mut samples) = (0, 0);
    let keep = [KeepSeg::new(0, 22), KeepSeg::new(26, r.frames)];
    let mut out = Vec::new();
    let cut = AudioCut::new(&keep, FPS as f64, audio::RATE, 2, r.frames);
    let mut audio_out = 0;
    while let Some(d) = dec.next().unwrap() {
        match d {
            Decoded::Video {
                time_hns,
                duration_hns,
                ..
            } => {
                let (idx, n) = frames_of_sample(time_hns, duration_hns, FPS as f64);
                assert_eq!(idx, frames);
                frames += n;
                samples += 1;
                out.extend(split_video_sample(&keep, idx, n, FPS as f64));
            }
            Decoded::Audio { time_hns, pcm } => {
                audio_out += cut
                    .cut(time_hns, &pcm)
                    .iter()
                    .map(|p| p.pcm.len() as i64 / 2)
                    .sum::<i64>();
            }
        }
    }
    assert_eq!(frames, r.frames, "frames by duration, not samples");
    assert!(samples < frames);
    assert!(
        out.windows(2)
            .all(|w| w[0].time_hns + w[0].duration_hns == w[1].time_hns)
    );
    assert_eq!(out.iter().map(|o| o.frames).sum::<i64>(), r.frames - 4);
    assert_eq!(
        audio_out,
        cut.total_out(),
        "audio cut by time matches the kept length"
    );
    dec.seek(10_000_000).unwrap();
    assert!(
        matches!(dec.next().unwrap(), Some(Decoded::Video { time_hns, .. }) if time_hns <= 10_000_000)
    );
}
