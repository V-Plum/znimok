//! GPU work outside the canvas. For now one job (ZK-38): HDR → SDR tone mapping of a captured
//! frame in WGSL ([`tone`]), with the formulas of Little Helpers. The CPU version in
//! `znimok_platform::frame::tone` stays the reference and the fallback — the tests keep both
//! within ±1 of each other on every format and transfer.
//!
//! The frame still comes from CPU memory, as every capture does today; taking it straight from
//! the capture's texture (shared handle / IOSurface) is the video work (ZK-87/88).

pub mod tone;

pub use tone::{ToneMapper, to_srgb8};

#[derive(Debug)]
pub enum GpuError {
    /// No wgpu adapter or device (a machine without a usable GPU driver).
    NoDevice(String),
    /// The frame is larger than the device allows.
    TooLarge { width: u32, height: u32, max: u32 },
    /// The GPU did not finish or the result could not be read back.
    Failed(String),
}

impl std::fmt::Display for GpuError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GpuError::NoDevice(e) => write!(f, "no GPU device: {e}"),
            GpuError::TooLarge { width, height, max } => {
                write!(f, "frame {width}×{height} is larger than {max}")
            }
            GpuError::Failed(e) => write!(f, "GPU: {e}"),
        }
    }
}

impl std::error::Error for GpuError {}
