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
}
