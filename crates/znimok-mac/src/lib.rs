//! macOS implementations of the `znimok-platform` traits (ZK-37), grown from the P3 prototype
//! (ZK-16, live-tested by the owner): display and region capture through
//! `SCScreenshotManager`, the pointer position, and the Screen Recording permission.
//!
//! Coordinates are macOS "desktop units" — points in the global CoreGraphics space (origin at
//! the top-left of the primary display, y down); `pixels_per_unit` is 2 on a Retina display.
//! Frames are always physical pixels. Not yet here: window capture, the system picker, EDR/HDR
//! frames (SDR sRGB for now), work area without the menu bar and Dock.
//!
//! Binaries that link this crate need the rpath `/usr/lib/swift` (see `crates/znimok-app/build.rs`):
//! screencapturekit's Swift bridge links `@rpath/libswift_Concurrency.dylib`.
//! On other systems the crate is empty.

#[cfg(target_os = "macos")]
mod mac;

#[cfg(target_os = "macos")]
pub use mac::MacCapture;
