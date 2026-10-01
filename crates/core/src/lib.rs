//! Margin's core, shared by every platform's editor and the CLI: Markdown
//! analysis and editing rules (`md`), comment threads anchored to document
//! text (`comments`), and keeping the text and its file in step
//! (`file_sync`). No UI here.

pub mod changes;
pub mod comments;
pub mod diff;
pub mod file_sync;
pub mod md;
