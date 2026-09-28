//! P4 prototype (ZK-17): video playback without copying frames through the CPU — Media Foundation
//! Source Reader decodes on the GPU (DXVA, D3D11), frames go through a shared NT handle into wgpu
//! DX12 and a WGSL pass turns NV12 into RGBA. Plus the fallback through CPU memory and exact
//! seeking. Throwaway measurement code — what survives goes into the Phase 8 video crates.
//!
//!   znimok-p4 selftest                            NV12 shader vs CPU formula, barcode round trip
//!   znimok-p4 info                                wgpu adapter, NV12 support, D3D11 on the same LUID
//!   znimok-p4 gen [-o f.mp4] [--size 3840x2160] [--fps 60] [--seconds 20] [--gop N] [--bpp 0.1]
//!   znimok-p4 bench <f.mp4> [--mode zero|cpu|sw] [--paced] [--verify] [--frames N] [--pool N]
//!   znimok-p4 seek <f.mp4> [--mode zero|cpu|sw] [--count N] [--gop N] [--seed N] [--rows]

mod gpu;
#[cfg(target_os = "macos")]
mod mac;
mod pattern;
#[cfg(windows)]
mod win;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let res = match args.first().map(String::as_str) {
        Some("selftest") => selftest(),
        #[cfg(windows)]
        Some(cmd) => win::run(cmd, &args[1..]),
        #[cfg(target_os = "macos")]
        Some(cmd) => mac::run(cmd, &args[1..]),
        #[cfg(not(any(windows, target_os = "macos")))]
        Some(_) => Err("відтворення в P4 — лише Windows і macOS; тут є selftest".into()),
        None => Err("команда: selftest | info | gen | bench | seek (див. main.rs)".into()),
    };
    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("помилка: {e}");
            ExitCode::FAILURE
        }
    }
}

fn selftest() -> Result<(), String> {
    let g = gpu::Gpu::new()?;
    let (w, h) = (1920, 1080);
    let mut f = pattern::Nv12::new(w, h);
    f.draw(4321);
    let planes = g.planes(w, h);
    let (y, uv) = f.data.split_at((w * h) as usize);
    g.upload(&planes, y, w, uv, w);
    let out = g.target(w, h);
    let bind = g.bind(
        &planes.y.create_view(&Default::default()),
        &planes.uv.create_view(&Default::default()),
        &out,
    );
    g.queue.submit([g.convert(&bind, w, h)]);
    let gpu_rgba = g.read_band(&out, h);
    let cpu_rgba = pattern::nv12_to_rgba(&f);
    let worst = gpu_rgba
        .iter()
        .zip(&cpu_rgba)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0);
    let r = pattern::read(&gpu_rgba, w);
    let ok = worst <= 1 && r.index == Some(4321);
    println!(
        "{}",
        serde_json::json!({ "adapter": g.adapter.name, "nv12_textures": g.nv12, "max_diff": worst,
            "barcode": r.index, "patch_max_diff": r.patch_max_diff, "ok": ok })
    );
    if ok {
        Ok(())
    } else {
        Err("selftest не пройшов".into())
    }
}
