//! Windows backend of Znimok video (ZK-87): screen recording on the GPU.
//!
//! ```text
//! WGC / Desktop Duplication ──► shared texture (D3D11 → wgpu, NT handle)
//!   ──► record.wgsl (crop / fit, tone, click rings, cursor)  ──► BGRA8 shared texture
//!   ──► D3D11 copy into an IMFVideoSampleAllocatorEx sample ──► IMFSinkWriter
//!       (video processor → NV12, hardware H.264 High; AAC audio) ──► <name>.mp4.part → .mp4
//! ```
//!
//! The loop, CFR, pause, audio timeline and everything that does not touch Windows live in
//! `znimok-video`; this crate implements its traits ([`source::Source`], [`sink::MfSink`],
//! [`decoder::MfDecoder`]) and runs a recording on its own thread ([`recording::Recording`]).
//! The formulas are Little Helpers' (`docs/discovery/inventory_video.md` §2); the two-device
//! bridge is prototype P4's (ZK-17).
//!
//! Nothing here on other systems.

#[cfg(windows)]
pub mod audio;
#[cfg(windows)]
pub mod clock;
#[cfg(windows)]
pub mod decoder;
#[cfg(windows)]
pub mod input;
#[cfg(windows)]
pub mod interop;
#[cfg(windows)]
pub mod mf;
#[cfg(windows)]
pub mod nv12;
#[cfg(windows)]
pub mod recording;
#[cfg(windows)]
pub mod shader;
#[cfg(windows)]
pub mod sink;
#[cfg(windows)]
pub mod source;
#[cfg(windows)]
pub mod synthetic;

#[cfg(windows)]
pub use audio::{AudioDevice, WasapiSource, devices as audio_devices};
#[cfg(windows)]
pub use clock::QpcClock;
#[cfg(windows)]
pub use decoder::{MfDecoder, Nv12Frame};
#[cfg(windows)]
pub use input::{MouseInput, MouseOpts};
#[cfg(windows)]
pub use recording::{Finished, RecordRequest, Recording, Started};
#[cfg(windows)]
pub use shader::{CursorImage, FrameGeometry, Overlay, Ring};
#[cfg(windows)]
pub use source::{Api, Target};
