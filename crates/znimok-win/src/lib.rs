//! Windows implementations of `znimok-platform` (ZK-36: capture and window list; ZK-77: autostart).
//!
//! Carried over from P2 (ZK-15) and Little Helpers (inventory_screenshots.md §7):
//! - the process is **Per-Monitor-v2** DPI aware ([`init_process`]), so desktop units are physical
//!   pixels everywhere and mixed-DPI desks stay continuous;
//! - WGC with `RequestAccessAsync(Borderless)` → `IsBorderRequired = false` (no yellow border on
//!   Windows 11 26100+ without a prompt), cursor off (Znimok draws its own);
//! - pool format **R16G16B16A16Float on an HDR display** (BGRA8 there is washed out — no BitBlt in HDR),
//!   **B8G8R8A8 on SDR** (exact colours, no conversion); copy `ContentSize` only; honour `RowPitch`;
//! - DXGI Desktop Duplication as the other API: all adapters walked, first (black) frame skipped,
//!   `DuplicateOutput1` with FP16 / 10-bit / BGRA8;
//! - SDR white = DisplayConfig request **11** × 80 / 1000;
//! - window bounds = `DWMWA_EXTENDED_FRAME_BOUNDS` (no invisible shadow), cloaked windows skipped;
//! - a region never spans two displays (they can differ in colour space and white level).
//!
//! On other OSes the crate is empty.

#[cfg(windows)]
mod win;

#[cfg(windows)]
pub use win::autostart::{BACKGROUND_ARG, RunKeyAutostart};
#[cfg(windows)]
pub use win::{Api, WinCapture, init_process};
