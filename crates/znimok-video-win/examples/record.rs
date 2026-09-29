//! Record the screen from the command line and report what the pipeline did — the measuring
//! stick of ZK-87.
//!
//! ```text
//! cargo run --release -p znimok-video-win --example record -- [--display N | --window TEXT |
//!     --region X,Y,W,H] [--seconds S] [--fps 30|60] [--quality 0|1|2] [--software] [--dda]
//!     [--gop N] [-o FILE]
//! ```

#[cfg(windows)]
fn main() {
    use std::time::{Duration, Instant};
    use znimok_platform::Rect;
    use znimok_video::check::mp4::{Mp4Expect, check_mp4, read_mp4_file};
    use znimok_video::settings::Quality;
    use znimok_video_win::{Api, RecordRequest, Recording, Target};

    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let has = |name: &str| args.iter().any(|a| a == name);
    let seconds: f64 = flag("--seconds")
        .and_then(|v| v.parse().ok())
        .unwrap_or(5.0);
    let fps: u32 = flag("--fps").and_then(|v| v.parse().ok()).unwrap_or(30);
    let quality =
        Quality::from_setting(flag("--quality").and_then(|v| v.parse().ok()).unwrap_or(1));
    let out = flag("-o").unwrap_or_else(|| "znimok-record.mp4".into());

    let monitors = znimok_win::raw::monitors();
    if monitors.is_empty() {
        eprintln!("немає дисплеїв");
        std::process::exit(2);
    }
    let display_id = |n: usize| monitors.get(n).map(|m| m.info.id.clone());
    let target = if let Some(t) = flag("--window") {
        use znimok_platform::WindowList;
        let cap = znimok_win::WinCapture::new();
        let wins = cap.windows().unwrap_or_default();
        let Some(w) = wins
            .iter()
            .find(|w| w.title.to_lowercase().contains(&t.to_lowercase()))
        else {
            eprintln!("немає вікна з «{t}»; є:");
            for w in wins.iter().take(30) {
                eprintln!("  {} — {}", w.app, w.title);
            }
            std::process::exit(2);
        };
        eprintln!("вікно: {} — {} ({:?})", w.app, w.title, w.bounds);
        Target::Window { id: w.id }
    } else if let Some(r) = flag("--region") {
        let v: Vec<i32> = r.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        if v.len() != 4 {
            eprintln!("--region X,Y,W,H");
            std::process::exit(2);
        }
        let n: usize = flag("--display").and_then(|v| v.parse().ok()).unwrap_or(0);
        Target::Display {
            id: display_id(n).expect("display"),
            region: Some(Rect::new(
                v[0],
                v[1],
                v[2].max(1) as u32,
                v[3].max(1) as u32,
            )),
        }
    } else {
        let n: usize = flag("--display").and_then(|v| v.parse().ok()).unwrap_or(0);
        Target::Display {
            id: display_id(n).expect("display"),
            region: None,
        }
    };

    let req = RecordRequest {
        fps,
        quality,
        software: has("--software"),
        api: if has("--dda") {
            Api::Duplication
        } else {
            Api::Wgc
        },
        keyframe_interval: flag("--gop").and_then(|v| v.parse().ok()),
        ..RecordRequest::new(target, out.clone().into())
    };
    let cpu0 = cpu_time();
    let t0 = Instant::now();
    let rec = match Recording::start(req) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("не вдалося почати: {e}");
            std::process::exit(1);
        }
    };
    let s = rec.started().clone();
    eprintln!(
        "запис: {} → {} ({}), {}×{}, {} кбіт/с, ключовий кадр кожні {} кадрів, звук: {} доріжок",
        s.api,
        s.encoder,
        s.gpu,
        s.size.0,
        s.size.1,
        s.bitrate / 1000,
        s.keyframe_interval,
        s.audio_tracks
    );
    std::thread::sleep(Duration::from_secs_f64(seconds));
    let fin = rec.stop();
    let wall = t0.elapsed();
    let cpu = cpu_time() - cpu0;
    let r = &fin.result;
    eprintln!(
        "кадрів {} (семплів {}), {:.2} с; CPU {:.1} % машини за час запису; помилка: {:?}",
        r.frames,
        r.samples,
        r.duration_ms / 1000.0,
        cpu.as_secs_f64() / wall.as_secs_f64() / num_cpus() as f64 * 100.0,
        r.error
    );
    let Some(path) = fin.path else {
        eprintln!("файл не записано");
        std::process::exit(1);
    };
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let info = read_mp4_file(&path).expect("mp4");
    let checks = check_mp4(
        &info,
        &Mp4Expect {
            keyframe_interval: Some(s.keyframe_interval),
            duration_s: Some(r.duration_ms / 1000.0),
            duration_tol_s: 0.2,
            ..Default::default()
        },
    );
    for c in &checks {
        eprintln!("{c}");
    }
    eprintln!(
        "{}: {:.1} МБ, {:.1} Мбіт/с",
        path.display(),
        bytes as f64 / 1e6,
        bytes as f64 * 8.0 / (r.duration_ms / 1000.0).max(0.001) / 1e6
    );
    // --dump-frame FILE.pgm: the luma of the frame one second in, to look at.
    if let Some(dump) = flag("--dump-frame") {
        use znimok_video::traits::{Decoded, VideoDecoder};
        use znimok_video_win::MfDecoder;
        let mut d = MfDecoder::open(&path).expect("decoder");
        let mut n = 0;
        while let Some(s) = d.next().expect("decode") {
            if let Decoded::Video { frame, .. } = s {
                n += 1;
                if n == fps.min(r.frames.max(1) as u32) {
                    let mut pgm = format!(
                        "P5
{} {}
255
",
                        frame.width, frame.height
                    )
                    .into_bytes();
                    pgm.extend_from_slice(&frame.y);
                    std::fs::write(&dump, pgm).expect("dump");
                    eprintln!("кадр {n} → {dump}");
                    break;
                }
            }
        }
    }
}

#[cfg(windows)]
fn cpu_time() -> std::time::Duration {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let (mut c, mut x, mut k, mut u) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: out-pointers to locals for the current process.
    unsafe {
        let _ = GetProcessTimes(GetCurrentProcess(), &mut c, &mut x, &mut k, &mut u);
    }
    let t = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
    std::time::Duration::from_nanos((t(k) + t(u)) * 100)
}

#[cfg(windows)]
fn num_cpus() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("лише Windows");
}
