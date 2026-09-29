//! A real recording of the primary display (an interactive desktop: a developer machine or the
//! GitHub Windows runner). Where the screen cannot be captured the test says so and passes.
#![cfg(windows)]

use std::time::Duration;

use znimok_video::check::mp4::{Mp4Expect, check_mp4, read_mp4_file};
use znimok_video_win::{Api, RecordRequest, Recording, Target};

#[test]
fn records_the_primary_display() {
    let Some(m) = znimok_win::raw::monitors().into_iter().next() else {
        eprintln!("пропущено: немає дисплеїв");
        return;
    };
    let dir = std::env::temp_dir().join(format!("znimok-live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("display.mp4");
    let req = RecordRequest {
        fps: 30,
        api: Api::Wgc,
        ..RecordRequest::new(
            Target::Display {
                id: m.info.id.clone(),
                region: None,
            },
            path.clone(),
        )
    };
    let rec = match Recording::start(req) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("пропущено (екран недоступний у цьому сеансі): {e}");
            return;
        }
    };
    let started = rec.started().clone();
    eprintln!("{started:?}");
    std::thread::sleep(Duration::from_millis(1500));
    let fin = rec.stop();
    eprintln!(
        "кадрів {}, семплів {}, {:.0} мс, помилка {:?}",
        fin.result.frames, fin.result.samples, fin.result.duration_ms, fin.result.error
    );
    if fin.result.frames == 0 {
        // No frame within the wait: a desktop that never presents (a detached RDP session).
        eprintln!("пропущено: жодного кадру");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    assert!(fin.result.error.is_none());
    let out = fin.path.expect("the file was committed");
    let info = read_mp4_file(&out).unwrap();
    let checks = check_mp4(
        &info,
        // Not `slots`: the MP4 muxer gives the LAST sample the previous delta as its duration,
        // so a stretched last sample (a late loop at the stop) counts one slot short.
        &Mp4Expect {
            keyframe_interval: Some(started.keyframe_interval),
            duration_s: Some(fin.result.duration_ms / 1000.0),
            duration_tol_s: 0.2,
            ..Default::default()
        },
    );
    for c in &checks {
        eprintln!("{c}");
    }
    assert!(znimok_video::check::all_ok(&checks));
    let _ = std::fs::remove_dir_all(&dir);
}
