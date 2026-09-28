//! Znimok settings and profile (ZK-28).
//!
//! - [`Settings`] — everything the user can change, grouped by settings page; every field has a
//!   default, so an old or partial file still loads. The set is the Znimok-relevant part of Little
//!   Helpers (inventory_system.md §3) as a list of functions — there is no migration from the LH
//!   registry (owner, 27.09).
//! - [`Store`] — one `settings.json` in the OS config folder: a lenient load (a wrong or out-of-range
//!   value falls back to its default and is reported, the rest of the file survives), keys this
//!   version does not know are kept for a newer one, saves are atomic (temporary file + rename).
//! - [`Dirs`] — config, data, cache and log folders per OS; `ZNIMOK_HOME` puts all of them under
//!   one folder (tests, portable runs, the self-test).
//! - [`Vault`] — secrets (API keys) in the OS store: Windows Credential Manager, macOS login
//!   Keychain. Never in the JSON file.
//!
//! Autostart is not stored here: its state is read from the OS (`znimok_platform::Autostart`).

mod model;
mod paths;
mod secrets;
mod store;

pub use model::*;
pub use paths::Dirs;
pub use secrets::{Secret, SecretError, Vault};
pub use store::{Problem, Store};
