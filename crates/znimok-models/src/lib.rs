//! Model providers for Znimok (ZK-70).
//!
//! - [`ocr`] — text recognition on the device: Windows.Media.Ocr (no package identity needed;
//!   **no Ukrainian** — Microsoft ships 25 languages without it) and Apple Vision (Ukrainian
//!   included). Nothing leaves the machine.
//! - [`anthropic`] — Claude with the user's own API key (BYOK). A request is first built as an
//!   [`anthropic::Outgoing`]: exactly the text and the (already downscaled) images that will be
//!   sent, with a token and cost estimate — the app shows it before sending when the feature is in
//!   «ask» mode. The key comes from the OS store ([`znimok_settings::Vault`]) or, for development,
//!   `ANTHROPIC_API_KEY`.
//! - [`http`] — HTTPS through the OS stack (Windows.Web.Http / NSURLSession): the system's
//!   certificates and proxy settings, and no TLS stack of our own in the dependency tree.
//! - [`pricing`] and [`meter`] — prices per model and a local spending counter per month.

pub mod anthropic;
pub mod http;
pub mod image_prep;
pub mod meter;
pub mod ocr;
pub mod pricing;

/// A picture in memory: straight RGBA8, row after row.
#[derive(Clone, Debug, PartialEq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Rgba {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Option<Self> {
        (width > 0 && height > 0 && pixels.len() == width as usize * height as usize * 4).then_some(
            Self {
                width,
                height,
                pixels,
            },
        )
    }
}
