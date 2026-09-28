//! HDR → SDR tone mapping — the formulas of Little Helpers (`CapConvert`, `kVidHlsl`), ported 1:1.
//!
//! The rule from LH: tone is mapped ONCE, on input. Everything up to SDR white stays as is, everything
//! above is clipped ("white stays white" — right for screenshots of user interfaces), then the sRGB
//! curve. This module is the CPU reference; `gpu.rs` runs the same maths in WGSL and the tests keep
//! both within ±1 of each other.

/// How the captured pixels are encoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum Transfer {
    /// scRGB: linear BT.709, 1.0 = 80 nits (WGC FP16 on any display, DXGI FP16 on HDR).
    ScRgb,
    /// HDR10: PQ (ST 2084), BT.2020 primaries (DXGI R10G10B10A2 on an HDR display).
    Pq2020,
    /// Already sRGB-encoded SDR (BGRA8, or 10-bit SDR).
    Srgb,
}

/// Captured pixels, row-packed (no row pitch), RGBA order for the float and 10-bit layouts.
#[derive(Clone, Debug)]
pub enum Pixels {
    /// R16G16B16A16_FLOAT as raw half bits.
    F16(Vec<u16>),
    /// R10G10B10A2_UNORM packed words (r in bits 0..10).
    Rgb10a2(Vec<u32>),
    /// B8G8R8A8 bytes.
    Bgra8(Vec<u8>),
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Pixels,
    pub transfer: Transfer,
}

impl Frame {
    pub fn format_name(&self) -> &'static str {
        match self.pixels {
            Pixels::F16(_) => "R16G16B16A16_FLOAT",
            Pixels::Rgb10a2(_) => "R10G10B10A2_UNORM",
            Pixels::Bgra8(_) => "B8G8R8A8_UNORM",
        }
    }
}

/// Shader mode, shared with `tone.wgsl`.
pub fn mode(transfer: Transfer) -> u32 {
    match transfer {
        Transfer::ScRgb => 1,
        Transfer::Pq2020 => 2,
        Transfer::Srgb => 3,
    }
}

/// SDR white used when the system does not report one (LH `kCapHdrFallback` / SDR reference).
#[cfg_attr(not(windows), allow(dead_code))]
pub const FALLBACK_WHITE_HDR: f32 = 200.0;
#[cfg_attr(not(windows), allow(dead_code))]
pub const FALLBACK_WHITE_SDR: f32 = 80.0;

pub fn srgb_encode(linear: f32) -> f32 {
    let c = linear.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// ST 2084 EOTF: encoded value 0..1 → nits.
pub fn pq_to_nits(e: f32) -> f32 {
    const M1: f32 = 0.159_301_76;
    const M2: f32 = 78.843_75;
    const C1: f32 = 0.835_937_5;
    const C2: f32 = 18.851_563;
    const C3: f32 = 18.687_5;
    let p = e.max(0.0).powf(1.0 / M2);
    10_000.0 * ((p - C1).max(0.0) / (C2 - C3 * p)).powf(1.0 / M1)
}

/// BT.2020 → BT.709 in linear light (the same rows as LH's shader).
pub fn bt2020_to_709(n: [f32; 3]) -> [f32; 3] {
    [
        1.6605 * n[0] - 0.5876 * n[1] - 0.0728 * n[2],
        -0.1246 * n[0] + 1.1329 * n[1] - 0.0083 * n[2],
        -0.0182 * n[0] - 0.1006 * n[1] + 1.1187 * n[2],
    ]
}

fn quant(v: f32) -> u8 {
    (v * 255.0 + 0.5).floor().clamp(0.0, 255.0) as u8
}

/// One pixel: normalised channel values (as the GPU sees them after loading the texture) → sRGB 8-bit.
pub fn map_pixel(transfer: Transfer, rgb: [f32; 3], white_nits: f32) -> [u8; 3] {
    let enc = match transfer {
        Transfer::ScRgb => {
            let k = white_nits / 80.0;
            rgb.map(|v| srgb_encode(v / k))
        }
        Transfer::Pq2020 => {
            let n = rgb.map(|v| pq_to_nits(v) / white_nits);
            bt2020_to_709(n).map(srgb_encode)
        }
        Transfer::Srgb => rgb.map(|v| v.clamp(0.0, 1.0)),
    };
    enc.map(quant)
}

/// Normalised RGB of pixel `i` (what a texture load would return).
pub fn load(p: &Pixels, i: usize) -> [f32; 3] {
    match p {
        Pixels::F16(v) => [
            half::f16::from_bits(v[i * 4]).to_f32(),
            half::f16::from_bits(v[i * 4 + 1]).to_f32(),
            half::f16::from_bits(v[i * 4 + 2]).to_f32(),
        ],
        Pixels::Rgb10a2(v) => {
            let w = v[i];
            [
                (w & 1023) as f32 / 1023.0,
                ((w >> 10) & 1023) as f32 / 1023.0,
                ((w >> 20) & 1023) as f32 / 1023.0,
            ]
        }
        Pixels::Bgra8(v) => [
            v[i * 4 + 2] as f32 / 255.0,
            v[i * 4 + 1] as f32 / 255.0,
            v[i * 4] as f32 / 255.0,
        ],
    }
}

/// CPU reference: whole frame → RGBA8 (alpha 255).
pub fn map_cpu(frame: &Frame, white_nits: f32) -> Vec<u8> {
    let n = (frame.width * frame.height) as usize;
    let mut out = Vec::with_capacity(n * 4);
    for i in 0..n {
        let [r, g, b] = map_pixel(frame.transfer, load(&frame.pixels, i), white_nits);
        out.extend_from_slice(&[r, g, b, 255]);
    }
    out
}

/// Synthetic frames that cover each format's whole range (for `selftest` and the GPU test).
pub fn synthetic_frames() -> Vec<(Frame, f32)> {
    let (w, h) = (256u32, 64u32);
    let (mut f16, mut pq, mut b8) = (Vec::new(), Vec::new(), Vec::new());
    for y in 0..h {
        for x in 0..w {
            // scRGB 0..12.5 (up to 1000 nits) across x, channels decorrelated by y
            let v = x as f32 / 255.0 * 12.5;
            for ch in [v, v * (y as f32 / 63.0), v * 0.3, 1.0] {
                f16.push(half::f16::from_f32(ch).to_bits());
            }
            let e = |t: f32| ((t * 1023.0).round() as u32).min(1023);
            pq.push(
                e(x as f32 / 255.0)
                    | (e(y as f32 / 63.0) << 10)
                    | (e(1.0 - x as f32 / 255.0) << 20)
                    | (3 << 30),
            );
            b8.extend_from_slice(&[(255 - x) as u8, (y * 4) as u8, x as u8, 255]);
        }
    }
    vec![
        (
            Frame {
                width: w,
                height: h,
                pixels: Pixels::F16(f16.clone()),
                transfer: Transfer::ScRgb,
            },
            80.0,
        ),
        (
            Frame {
                width: w,
                height: h,
                pixels: Pixels::F16(f16),
                transfer: Transfer::ScRgb,
            },
            240.0,
        ),
        (
            Frame {
                width: w,
                height: h,
                pixels: Pixels::Rgb10a2(pq.clone()),
                transfer: Transfer::Pq2020,
            },
            203.0,
        ),
        (
            Frame {
                width: w,
                height: h,
                pixels: Pixels::Rgb10a2(pq),
                transfer: Transfer::Srgb,
            },
            80.0,
        ),
        (
            Frame {
                width: w,
                height: h,
                pixels: Pixels::Bgra8(b8),
                transfer: Transfer::Srgb,
            },
            80.0,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f16(v: f32) -> u16 {
        half::f16::from_f32(v).to_bits()
    }

    #[test]
    fn scrgb_sdr_white_stays_white() {
        // SDR display: white 80 nits, scRGB 1.0 → 255; above is clipped.
        assert_eq!(
            map_pixel(Transfer::ScRgb, [1.0, 1.0, 1.0], 80.0),
            [255, 255, 255]
        );
        assert_eq!(
            map_pixel(Transfer::ScRgb, [4.0, 0.0, 0.0], 80.0),
            [255, 0, 0]
        );
    }

    #[test]
    fn scrgb_hdr_white_level_scales() {
        // HDR display with SDR white 240 nits: scRGB 3.0 is that white; 1.5 is linear 0.5 → sRGB 188.
        assert_eq!(
            map_pixel(Transfer::ScRgb, [3.0, 3.0, 3.0], 240.0),
            [255, 255, 255]
        );
        assert_eq!(
            map_pixel(Transfer::ScRgb, [1.5, 1.5, 1.5], 240.0),
            [188, 188, 188]
        );
    }

    #[test]
    fn pq_reference_points() {
        // PQ 0.5081 ≈ 100 nits; with white 100 → linear 1 → 255 (grey, BT.2020→709 keeps grey grey).
        let e = 0.508_08;
        assert!((pq_to_nits(e) - 100.0).abs() < 0.5, "{}", pq_to_nits(e));
        assert_eq!(
            map_pixel(Transfer::Pq2020, [e, e, e], 100.0),
            [255, 255, 255]
        );
        assert_eq!(
            map_pixel(Transfer::Pq2020, [0.0, 0.0, 0.0], 100.0),
            [0, 0, 0]
        );
    }

    #[test]
    fn frame_roundtrip_layouts() {
        let f = Frame {
            width: 1,
            height: 1,
            pixels: Pixels::F16(vec![f16(1.0), f16(0.0), f16(0.0), f16(1.0)]),
            transfer: Transfer::ScRgb,
        };
        assert_eq!(map_cpu(&f, 80.0), vec![255, 0, 0, 255]);
        let b = Frame {
            width: 1,
            height: 1,
            pixels: Pixels::Bgra8(vec![10, 20, 30, 255]),
            transfer: Transfer::Srgb,
        };
        assert_eq!(map_cpu(&b, 80.0), vec![30, 20, 10, 255]);
        let w = 1023u32 | (512 << 10) | (3 << 30);
        let t = Frame {
            width: 1,
            height: 1,
            pixels: Pixels::Rgb10a2(vec![w]),
            transfer: Transfer::Srgb,
        };
        assert_eq!(map_cpu(&t, 80.0), vec![255, 128, 0, 255]);
    }
}
