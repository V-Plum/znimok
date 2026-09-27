//! Znimok core: document model, objects, undo, commands and queries, library index.
//!
//! No dependency on the OS or the GPU — builds and tests anywhere. For now this is only
//! the placeholder that lets CI check the workspace (ZK-20); the model itself comes in
//! ZK-21/ZK-22.

/// Version of this crate, as written in the workspace manifest.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_set() {
        assert!(!super::VERSION.is_empty());
    }
}
