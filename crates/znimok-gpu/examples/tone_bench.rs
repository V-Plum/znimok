//! 4K HDR frame → SDR: the CPU reference against the GPU path (ZK-38).
//! `cargo run --release -p znimok-gpu --example tone_bench`

use std::time::Instant;
use znimok_gpu::ToneMapper;
use znimok_platform::{ColorInfo, Frame, PixelFormat, Rect, Transfer};

fn main() {
    let (w, h) = (3840u32, 2160u32);
    // scRGB 0…12.5 across the frame (a half of 1.0 = 0x3C00, scaled per column).
    let mut data = Vec::with_capacity((w * h * 8) as usize);
    for _y in 0..h {
        for x in 0..w {
            let v = (x as f32 / w as f32 * 12.5).to_bits();
            let half = (((v >> 16) & 0x8000)
                | ((((v >> 23) & 0xFF).saturating_sub(112) & 0x1F) << 10)
                | ((v >> 13) & 0x3FF)) as u16;
            for c in [half, half, half, 0x3C00] {
                data.extend_from_slice(&c.to_le_bytes());
            }
        }
    }
    let f = Frame {
        width: w,
        height: h,
        stride: w * 8,
        format: PixelFormat::Rgba16Float,
        color: ColorInfo {
            transfer: Transfer::ScRgb,
            sdr_white_nits: 240.0,
            hdr: true,
        },
        source: Rect::new(0, 0, w, h),
        scale: 1.0,
        data,
    };
    let t = Instant::now();
    let cpu = f.to_srgb8();
    println!("CPU: {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
    let t = Instant::now();
    let Some(g) = ToneMapper::shared() else {
        println!("no GPU device");
        return;
    };
    println!(
        "GPU device: {:.0} ms ({})",
        t.elapsed().as_secs_f64() * 1000.0,
        g.adapter
    );
    for i in 0..3 {
        let t = Instant::now();
        let gpu = g.map(&f).unwrap();
        println!("GPU run {i}: {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
        let worst = gpu
            .iter()
            .zip(&cpu)
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap();
        assert!(worst <= 1, "off by {worst}");
    }
}
