//! Znimok core: document model, objects, undo, commands and queries, library index.
//!
//! No dependency on the OS or the GPU — builds and tests anywhere.

pub mod model;

pub use model::*;

/// Version of this crate, as written in the workspace manifest.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
