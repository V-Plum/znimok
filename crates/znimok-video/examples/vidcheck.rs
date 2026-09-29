//! `vidcheck` for Znimok (ZK-99): prints what an MP4 or a GIF actually contains and the verdict
//! of every check; exit code 1 when a check failed or the file cannot be read, 2 on bad usage.
//!
//! ```text
//! cargo run -p znimok-video --example vidcheck -- FILE [--fps N[/D]] [--slots N]
//!     [--duration S] [--tol S] [--frames N[±T]] [--loop 0|none] [--colors N] [--bars]
//! ```
//!
//! MP4: `--fps` expected rate (else inferred), `--slots` output frames, `--duration` seconds
//! (± `--tol`, default 0.6). GIF: `--frames`, `--duration` (sum of delays, ± `--tol` seconds,
//! default 0.02), `--loop`, `--colors` (at most), `--bars` (compose frames and require frame
//! `i` to show slot `i`, as `gifcheck.py --bars` on the synthetic).

use std::fs::File;
use std::io::Read;
use std::process::ExitCode;
use znimok_video::check::gif::{self, GifExpect, GifOptions};
use znimok_video::check::mp4::{self, Fps, Mp4Expect};
use znimok_video::check::{Check, all_ok};

fn usage() -> ExitCode {
    eprintln!(
        "usage: vidcheck FILE [--fps N[/D]] [--slots N] [--duration S] [--tol S] \
         [--frames N[±T]] [--loop 0|none] [--colors N] [--bars]"
    );
    ExitCode::from(2)
}

#[derive(Default)]
struct Args {
    path: String,
    fps: Option<Fps>,
    slots: Option<i64>,
    duration: Option<f64>,
    tol: Option<f64>,
    frames: Option<(usize, usize)>,
    loop_count: Option<Option<u16>>,
    colors: Option<usize>,
    bars: bool,
}

fn parse_args() -> Option<Args> {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(s) = it.next() {
        match s.as_str() {
            "--fps" => {
                let v = it.next()?;
                let (n, d) = v.split_once('/').unwrap_or((&v, "1"));
                a.fps = Some(Fps::new(n.parse().ok()?, d.parse().ok()?));
            }
            "--slots" => a.slots = Some(it.next()?.parse().ok()?),
            "--duration" => a.duration = Some(it.next()?.parse().ok()?),
            "--tol" => a.tol = Some(it.next()?.parse().ok()?),
            "--frames" => {
                let v = it.next()?;
                let (n, t) = v.split_once('±').unwrap_or((&v, "0"));
                a.frames = Some((n.parse().ok()?, t.parse().ok()?));
            }
            "--loop" => {
                a.loop_count = Some(match it.next()?.as_str() {
                    "none" => None,
                    v => Some(v.parse().ok()?),
                })
            }
            "--colors" => a.colors = Some(it.next()?.parse().ok()?),
            "--bars" => a.bars = true,
            _ if s.starts_with("--") || !a.path.is_empty() => return None,
            _ => a.path = s,
        }
    }
    (!a.path.is_empty()).then_some(a)
}

fn print(checks: &[Check]) -> ExitCode {
    for c in checks {
        println!("{c}");
    }
    if all_ok(checks) {
        println!("CHECK OK");
        ExitCode::SUCCESS
    } else {
        println!("CHECK FAIL");
        ExitCode::from(1)
    }
}

fn main() -> ExitCode {
    let Some(a) = parse_args() else {
        return usage();
    };
    let mut magic = [0u8; 6];
    let n = File::open(&a.path)
        .and_then(|mut f| f.read(&mut magic))
        .unwrap_or(0);
    if n == 6 && magic.starts_with(b"GIF8") {
        let opts = GifOptions { bars: a.bars };
        let g = match gif::read_gif_file(a.path.as_ref(), opts) {
            Ok(g) => g,
            Err(e) => {
                println!("error: {e}");
                return ExitCode::from(1);
            }
        };
        let tol_cs = (a.tol.unwrap_or(0.02) * 100.0).round() as i64;
        println!(
            "gif {}x{} frames={} colors={} loop={} bytes={} total_cs={} trans_frames={} full_frames={}",
            g.w,
            g.h,
            g.frames.len(),
            g.colors,
            g.loop_count.map_or("none".into(), |v| v.to_string()),
            g.bytes,
            g.total_cs(),
            g.trans_frames(),
            g.full_frames()
        );
        println!("delays={:?}", g.delays());
        if let Some(b) = &g.bars {
            println!("bars={b:?}");
        }
        let e = GifExpect {
            frames: a.frames,
            total_cs: a.duration.map(|d| ((d * 100.0).round() as i64, tol_cs)),
            loop_count: a.loop_count,
            max_colors: a.colors,
            bars_sequence: a.bars,
            ..GifExpect::default()
        };
        return print(&gif::check_gif(&g, &e));
    }
    let info = match mp4::read_mp4_file(a.path.as_ref()) {
        Ok(i) => i,
        Err(e) => {
            println!("error: {e}");
            return ExitCode::from(1);
        }
    };
    if let Some(s) = info.summary() {
        println!("{s}");
    }
    for t in &info.tracks {
        println!(
            "track {} {} {} timescale={} samples={} duration={:.3} s",
            t.id,
            String::from_utf8_lossy(&t.handler),
            t.codec_name(),
            t.timescale,
            t.sample_count,
            t.duration_s(info.timescale)
        );
    }
    let e = Mp4Expect {
        fps: a.fps,
        keyframe_interval: None,
        slots: a.slots,
        duration_s: a.duration,
        duration_tol_s: a.tol.unwrap_or(mp4::DURATION_TOL_S),
    };
    print(&mp4::check_mp4(&info, &e))
}
