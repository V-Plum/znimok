//! P2 prototype (ZK-15): Windows screen/window capture without admin rights and without the yellow
//! border, FP16 on HDR, tone mapping in WGSL, PNG out. Throwaway measurement code — what survives
//! goes into `znimok-win` / `znimok-render` in Phase 3.
//!
//!   znimok-p2 displays                                    monitors as JSON (DPI, colour space, SDR white)
//!   znimok-p2 windows                                     top-level windows as JSON (DWM bounds, DPI)
//!   znimok-p2 shot display <index> [--api wgc|dxgi] [--cpu] [-o out.png]
//!   znimok-p2 shot window <hwnd|title-part> [--cpu] [-o out.png]
//!   znimok-p2 selftest                                    WGSL vs CPU reference on synthetic frames
//!   znimok-p2 compare a.png b.png [--tol N]               e.g. Znimok vs Little Helpers of the same screen

mod gpu;
mod tone;
#[cfg(windows)]
mod win;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let res = match args.first().map(String::as_str) {
        Some("selftest") => selftest(),
        Some("compare") => compare(&args[1..]),
        #[cfg(windows)]
        Some(cmd) => win::run(cmd, &args[1..]),
        #[cfg(not(windows))]
        Some(_) => Err("захоплення в P2 — лише Windows; тут доступні selftest і compare".into()),
        None => {
            Err("команда: displays | windows | shot | selftest | compare (див. main.rs)".into())
        }
    };
    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("помилка: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Tone map a frame on the GPU (or CPU with `cpu`), time it and write PNG.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn save_png(
    frame: &tone::Frame,
    white: f32,
    cpu: bool,
    path: &str,
) -> Result<serde_json::Value, String> {
    let g = if cpu { None } else { Some(gpu::Gpu::new()?) };
    let t = std::time::Instant::now();
    let rgba = match &g {
        Some(g) => g.map(frame, white),
        None => tone::map_cpu(frame, white),
    };
    let tone_ms = t.elapsed().as_secs_f64() * 1000.0;
    let engine = g.map_or_else(|| "cpu".to_string(), |g| g.adapter_name);
    image::save_buffer(
        path,
        &rgba,
        frame.width,
        frame.height,
        image::ColorType::Rgba8,
    )
    .map_err(|e| format!("PNG: {e}"))?;
    Ok(
        serde_json::json!({ "png": path, "tone_engine": engine, "tone_ms": (tone_ms * 10.0).round() / 10.0 }),
    )
}

fn selftest() -> Result<(), String> {
    let g = gpu::Gpu::new()?;
    let mut rows = Vec::new();
    let mut worst_all = 0u8;
    for (frame, white) in tone::synthetic_frames() {
        let a = g.map(&frame, white);
        let b = tone::map_cpu(&frame, white);
        let worst = a
            .iter()
            .zip(&b)
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap_or(0);
        let off = a.iter().zip(&b).filter(|(x, y)| x != y).count();
        worst_all = worst_all.max(worst);
        rows.push(serde_json::json!({ "format": frame.format_name(), "transfer": frame.transfer, "white": white, "max_diff": worst, "differing_channels": off }));
    }
    println!(
        "{}",
        serde_json::json!({ "adapter": g.adapter_name, "cases": rows, "ok": worst_all <= 1 })
    );
    if worst_all <= 1 {
        Ok(())
    } else {
        Err(format!("GPU і CPU розходяться на {worst_all}"))
    }
}

/// Compare two PNGs of the same scene: max and mean channel difference, share of pixels above `tol`.
fn compare(args: &[String]) -> Result<(), String> {
    let (a, b) = match args {
        [a, b, ..] => (a, b),
        _ => return Err("compare a.png b.png [--tol N]".into()),
    };
    let tol: u8 = args
        .iter()
        .position(|s| s == "--tol")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);
    let ia = image::open(a).map_err(|e| format!("{a}: {e}"))?.to_rgba8();
    let ib = image::open(b).map_err(|e| format!("{b}: {e}"))?.to_rgba8();
    if ia.dimensions() != ib.dimensions() {
        return Err(format!(
            "розміри різні: {:?} проти {:?}",
            ia.dimensions(),
            ib.dimensions()
        ));
    }
    let (mut worst, mut sum, mut over) = (0u8, 0u64, 0usize);
    for (pa, pb) in ia.pixels().zip(ib.pixels()) {
        let d = (0..3).map(|c| pa[c].abs_diff(pb[c])).max().unwrap_or(0);
        worst = worst.max(d);
        sum += u64::from(d);
        if d > tol {
            over += 1;
        }
    }
    let n = (ia.width() * ia.height()) as f64;
    println!(
        "{}",
        serde_json::json!({ "size": [ia.width(), ia.height()], "max_diff": worst, "mean_diff": (sum as f64 / n * 1000.0).round() / 1000.0,
                            "tol": tol, "pixels_over_tol": over, "share_over_tol": (over as f64 / n * 10000.0).round() / 10000.0 })
    );
    Ok(())
}
