//! Locate the app containing the macOS CLI.

use std::path::{Path, PathBuf};

/// The containing macOS app, if this executable lives in its `MacOS` or
/// `Helpers` directory. Standalone Cargo builds have no containing app, so
/// they can't open documents.
pub(super) fn app_bundle() -> Option<PathBuf> {
    bundle_for_executable(&std::env::current_exe().ok()?.canonicalize().ok()?)
}

fn bundle_for_executable(executable: &Path) -> Option<PathBuf> {
    let directory = executable.parent()?;
    if !matches!(directory.file_name()?.to_str()?, "MacOS" | "Helpers") {
        return None;
    }
    let contents = directory.parent()?;
    let bundle = contents.parent()?;
    (contents.file_name()? == "Contents" && bundle.extension()? == "app")
        .then(|| bundle.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_bundled_and_standalone_executables() {
        let bundle = PathBuf::from("/worktrees/with spaces/Margin.app");
        for executable in ["Contents/MacOS/Margin", "Contents/Helpers/margin"] {
            assert_eq!(
                bundle_for_executable(&bundle.join(executable)),
                Some(bundle.clone())
            );
        }
        for executable in [
            "/repo/target/debug/margin",
            "/repo/Margin.app/margin",
            "/repo/Margin.app/Contents/Resources/margin",
        ] {
            assert_eq!(bundle_for_executable(Path::new(executable)), None);
        }
    }
}
