//! Comment threads anchored to document text, shared by the app and the CLI.

pub mod activity;
pub mod anchor;
pub mod export;
pub mod handoff;
pub mod store;

pub use store::*;

/// Held by tests that point `MARGIN_DATA_DIR` somewhere, since tests run in
/// parallel and the environment is shared.
#[cfg(test)]
pub(crate) static DATA_DIR_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
