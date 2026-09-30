//! Comment threads anchored to document text, shared by the app and the CLI.

pub mod activity;
pub mod anchor;
pub mod export;
pub mod handoff;
pub mod store;

pub use anchor::{Anchor, Place};
pub use store::*;

/// Points `MARGIN_DATA_DIR` at `dir` for a test, until the guard drops.
/// Tests run in parallel and share the environment, so the guard keeps
/// other tests that call this waiting.
#[cfg(test)]
pub(crate) fn use_data_dir(dir: &std::path::Path) -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    #[expect(
        unsafe_code,
        reason = "only tests set the environment, one at a time under LOCK, and \
                  everything else reads it through `std::env`, as `set_var` requires"
    )]
    unsafe {
        std::env::set_var("MARGIN_DATA_DIR", dir)
    };
    guard
}
