//! Znimok core: document model, objects, undo, commands and queries.
//!
//! No dependency on the OS or the GPU — builds and tests anywhere. The GUI, the CLI, MCP
//! agents and tests all change a document through [`Editor::apply`] with a [`Command`] and read
//! it through [`Editor::query`] (PLAN §5.3–5.4).

pub mod command;
pub mod editor;
pub mod history;
pub mod hit;
pub mod model;
pub mod pixels;

pub use command::{
    Applied, Change, Command, CoreError, ObjectPatch, Query, QueryResult, SCHEMA_VERSION,
    StylePatch,
};
pub use editor::Editor;
pub use history::MergeKey;
pub use model::*;

/// Version of this crate, as written in the workspace manifest.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
