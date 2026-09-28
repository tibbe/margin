//! Margin's core, shared by every platform's editor and the CLI: Markdown
//! analysis and editing rules (`md`), and comment threads anchored to
//! document text (`comments`). No UI here.

pub mod comments;
pub mod diff;
pub mod md;
