//! macOS implementations of the `znimok-platform` traits (ZK-37), grown from the P3 prototype
//! (ZK-16, live-tested by the owner): display and region capture through
//! `SCScreenshotManager`, the pointer position, and the Screen Recording permission.
//!
//! Coordinates are macOS "desktop units" — points in the global CoreGraphics space (origin at
//! the top-left of the primary display, y down); `pixels_per_unit` is 2 on a Retina display.
//! Frames are always physical pixels, SDR sRGB: on an EDR display macOS itself maps HDR content
//! to SDR for a screenshot (ZK-129 decision: no own tone mapping on the Mac). The system picker
//! (`picker`) is the fallback without the Screen Recording permission.
//!
//! Binaries that link this crate need the rpath `/usr/lib/swift` (see `crates/znimok-app/build.rs`):
//! screencapturekit's Swift bridge links `@rpath/libswift_Concurrency.dylib`.
//! On other systems the crate is empty.

#[cfg(target_os = "macos")]
mod autostart;
#[cfg(target_os = "macos")]
mod clipboard;
#[cfg(target_os = "macos")]
mod fileassoc;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
pub mod picker;
#[cfg(target_os = "macos")]
mod share;
#[cfg(target_os = "macos")]
pub mod sparkle;

#[cfg(target_os = "macos")]
pub use autostart::MacAutostart;
#[cfg(target_os = "macos")]
pub use clipboard::MacClipboard;
#[cfg(target_os = "macos")]
pub use fileassoc::{BUNDLE_ID, DOCUMENT_UTI, MacFileAssoc};
#[cfg(target_os = "macos")]
pub use share::MacShare;

#[cfg(target_os = "macos")]
pub use mac::MacCapture;
