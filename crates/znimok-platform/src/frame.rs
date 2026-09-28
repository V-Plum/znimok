//! A captured frame: pixels in CPU memory plus what is needed to show them right (colour space,
//! SDR white). GPU-resident frames (shared handles, IOSurface) come with `znimok-gpu`; until then
//! every capture ends in CPU memory, as in LH.

use crate::geom::Rect;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PixelFormat {
    /// 8-bit B, G, R, A — SDR desktop format on both OSes.
    Bgra8,
    /// 8-bit R, G, B, A.
    Rgba8,
    /// 16-bit float R, G, B, A (`half`, little endian) — scRGB on Windows HDR, extended linear sRGB on macOS EDR.
    Rgba16Float,
    /// 10-bit R, G, B + 2-bit A packed in u32 (DXGI R10G10B10A2) — usually PQ (HDR10).
    Rgb10A2,
}

impl PixelFormat {
    pub const fn bytes_per_pixel(self) -> u32 {
        match self {
            Self::Bgra8 | Self::Rgba8 | Self::Rgb10A2 => 4,
            Self::Rgba16Float => 8,
        }
    }
}

/// How the pixel values are to be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Transfer {
    /// sRGB-encoded values, 1.0 = SDR white.
    Srgb,
    /// Linear, BT.709 primaries, 1.0 = 80 nits (Windows scRGB).
    ScRgb,
    /// Linear, BT.709 primaries, 1.0 = SDR white, values above 1.0 are HDR headroom (macOS EDR).
    ExtendedLinear,
    /// SMPTE ST 2084 (PQ), BT.2020 primaries.
    Pq,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ColorInfo {
    pub transfer: Transfer,
    /// Brightness of SDR white on this display in nits (Windows "SDR content brightness"; 80 on SDR).
    pub sdr_white_nits: f32,
    /// Whether the display was in HDR mode when the frame was taken.
    pub hdr: bool,
}

impl ColorInfo {
    pub const SDR: ColorInfo = ColorInfo {
        transfer: Transfer::Srgb,
        sdr_white_nits: 80.0,
        hdr: false,
    };
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// Size in physical pixels.
    pub width: u32,
    pub height: u32,
    /// Bytes between the starts of two rows (`>= width × bytes_per_pixel`).
    pub stride: u32,
    pub format: PixelFormat,
    pub color: ColorInfo,
    /// What was captured, in desktop units (display bounds, window bounds or the region).
    pub source: Rect,
    /// Pixels per desktop unit of the captured area.
    pub scale: f32,
    pub data: Vec<u8>,
}

impl Frame {
    /// Check that `data` holds `height` rows of `stride` bytes (implementations call this before
    /// handing a frame out).
    pub fn validate(self) -> Result<Self, crate::PlatformError> {
        let row = self.width as usize * self.format.bytes_per_pixel() as usize;
        let need = self.stride as usize * self.height.saturating_sub(1) as usize + row;
        if self.width == 0
            || self.height == 0
            || (self.stride as usize) < row
            || self.data.len() < need
        {
            return Err(crate::PlatformError::Other(format!(
                "кадр {}×{} {:?}: stride {}, байтів {}",
                self.width,
                self.height,
                self.format,
                self.stride,
                self.data.len()
            )));
        }
        Ok(self)
    }

    /// Bytes of row `y` without the padding.
    pub fn row(&self, y: u32) -> &[u8] {
        let o = y as usize * self.stride as usize;
        &self.data[o..o + self.width as usize * self.format.bytes_per_pixel() as usize]
    }

    /// 8-bit RGBA of an SDR frame (`Bgra8`/`Rgba8` only; HDR goes through tone mapping, ZK-38).
    pub fn to_rgba8(&self) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity((self.width * self.height * 4) as usize);
        for y in 0..self.height {
            let r = self.row(y);
            match self.format {
                PixelFormat::Rgba8 => out.extend_from_slice(r),
                PixelFormat::Bgra8 => {
                    for p in r.as_chunks::<4>().0 {
                        out.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
                    }
                }
                _ => return None,
            }
        }
        Some(out)
    }

    /// 8-bit sRGB RGBA of any frame — SDR as is, HDR tone mapped on the CPU with the formulas of
    /// Little Helpers (ported from P2 `tone.rs`, verified there against the WGSL version to ±1):
    /// everything up to SDR white stays, above is clipped ("white stays white", right for
    /// interfaces), then the sRGB curve. The GPU path is ZK-38; this is the fallback and the
    /// reference. Alpha is 255 for HDR formats.
    pub fn to_srgb8(&self) -> Vec<u8> {
        if let Some(v) = self.to_rgba8() {
            return v;
        }
        let white = if self.color.sdr_white_nits > 0.0 {
            self.color.sdr_white_nits
        } else {
            80.0
        };
        let mut out = Vec::with_capacity((self.width * self.height * 4) as usize);
        for y in 0..self.height {
            let r = self.row(y);
            match self.format {
                PixelFormat::Rgba16Float => {
                    for p in r.as_chunks::<8>().0 {
                        let h = |i: usize| half_to_f32(u16::from_le_bytes([p[i], p[i + 1]]));
                        let [a, b, c] = tone::map(self.color.transfer, [h(0), h(2), h(4)], white);
                        out.extend_from_slice(&[a, b, c, 255]);
                    }
                }
                PixelFormat::Rgb10A2 => {
                    for p in r.as_chunks::<4>().0 {
                        let w = u32::from_le_bytes(*p);
                        let n = |s: u32| ((w >> s) & 1023) as f32 / 1023.0;
                        let [a, b, c] = tone::map(self.color.transfer, [n(0), n(10), n(20)], white);
                        out.extend_from_slice(&[a, b, c, 255]);
                    }
                }
                PixelFormat::Bgra8 | PixelFormat::Rgba8 => unreachable!("handled by to_rgba8"),
            }
        }
        out
    }
}

/// IEEE 754 binary16 → f32 (no dependency needed for this one conversion).
fn half_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = ((h >> 10) & 0x1F) as i32;
    let man = (h & 0x3FF) as f32;
    match exp {
        0 => sign * man * 2f32.powi(-24),
        31 => {
            if man == 0.0 {
                sign * f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => sign * (1.0 + man / 1024.0) * 2f32.powi(exp - 15),
    }
}

/// HDR → SDR maths of Little Helpers (`CapConvert`).
pub mod tone {
    use super::Transfer;

    pub fn srgb_encode(linear: f32) -> f32 {
        let c = if linear.is_nan() {
            0.0
        } else {
            linear.clamp(0.0, 1.0)
        };
        if c <= 0.003_130_8 {
            12.92 * c
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        }
    }

    /// ST 2084 EOTF: encoded 0..1 → nits.
    pub fn pq_to_nits(e: f32) -> f32 {
        const M1: f32 = 0.159_301_76;
        const M2: f32 = 78.843_75;
        const C1: f32 = 0.835_937_5;
        const C2: f32 = 18.851_563;
        const C3: f32 = 18.687_5;
        let p = e.max(0.0).powf(1.0 / M2);
        10_000.0 * ((p - C1).max(0.0) / (C2 - C3 * p)).powf(1.0 / M1)
    }

    /// BT.2020 → BT.709, linear light.
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

    /// One pixel, normalised channel values → sRGB 8-bit.
    pub fn map(transfer: Transfer, rgb: [f32; 3], white_nits: f32) -> [u8; 3] {
        let enc = match transfer {
            Transfer::ScRgb => {
                let k = white_nits / 80.0;
                rgb.map(|v| srgb_encode(v / k))
            }
            Transfer::ExtendedLinear => rgb.map(srgb_encode),
            Transfer::Pq => {
                let n = rgb.map(|v| pq_to_nits(v) / white_nits);
                bt2020_to_709(n).map(srgb_encode)
            }
            Transfer::Srgb => rgb.map(|v| v.clamp(0.0, 1.0)),
        };
        enc.map(quant)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn sdr_white_maps_to_white_and_above_clips() {
            // scRGB: SDR white of 240 nits = 3.0.
            assert_eq!(
                map(Transfer::ScRgb, [3.0, 3.0, 3.0], 240.0),
                [255, 255, 255]
            );
            assert_eq!(
                map(Transfer::ScRgb, [6.0, 6.0, 6.0], 240.0),
                [255, 255, 255]
            );
            assert_eq!(map(Transfer::ScRgb, [0.0, 0.0, 0.0], 240.0), [0, 0, 0]);
            // Mid grey: 18 % of white → sRGB ≈ 118.
            let g = map(Transfer::ScRgb, [0.54, 0.54, 0.54], 240.0)[0];
            assert!((116..=120).contains(&g), "{g}");
            // PQ with 203-nit white: just above white clips to white.
            assert_eq!(
                map(Transfer::Pq, [0.6, 0.6, 0.6], 203.0),
                [255, 255, 255]
            );
        }

        #[test]
        fn half_floats_decode() {
            assert_eq!(super::super::half_to_f32(0x3C00), 1.0);
            assert_eq!(super::super::half_to_f32(0xC000), -2.0);
            assert_eq!(super::super::half_to_f32(0x0000), 0.0);
            assert!((super::super::half_to_f32(0x3555) - 0.3333).abs() < 1e-3);
        }
    }
}
