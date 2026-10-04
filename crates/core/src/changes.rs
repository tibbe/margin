//! What changed in a document since its last commit, by whole lines, for
//! the editors' change bars.

use crate::file_sync::Loaded;
use similar::{Algorithm, DiffTag, TextDiff};
use std::ops::Range;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

/// How lines changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Added,
    Changed,
    Deleted,
}

/// Lines of the text that differ from the base. `lines` index the text's
/// lines as `md::Doc` splits them; a deletion has none, so its range is
/// empty, at the line after the deleted ones.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineChange {
    pub kind: Kind,
    pub lines: Range<usize>,
}

/// The lines of `text` changed from `base`, in order.
pub fn line_changes(base: &str, text: &str) -> Vec<LineChange> {
    // Patience lines paragraphs up by their unique lines, not by the
    // blank lines between them.
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .timeout(Duration::from_millis(300))
        .diff_lines(base, text);
    let mut changes: Vec<LineChange> = Vec::new();
    // Runs of deletions and insertions next to each other are one change.
    let mut run: Option<(bool, Range<usize>)> = None;
    let mut flush = |run: &mut Option<(bool, Range<usize>)>| {
        if let Some((deleted, lines)) = run.take() {
            let kind = match (deleted, lines.is_empty()) {
                (_, true) => Kind::Deleted,
                (true, false) => Kind::Changed,
                (false, false) => Kind::Added,
            };
            changes.push(LineChange { kind, lines });
        }
    };
    for op in diff.ops() {
        let (tag, old, new) = op.as_tag_tuple();
        if tag == DiffTag::Equal {
            flush(&mut run);
            continue;
        }
        let r = run.get_or_insert((false, new.start..new.start));
        r.0 |= !old.is_empty();
        r.1.end = new.end;
    }
    flush(&mut run);
    changes
}

/// The file at `path` as of its git repository's last commit, as the
/// editors hold text (see `file_sync::Loaded`). `None` outside a
/// repository, for a file not yet committed, or without git.
pub fn committed(path: &Path) -> Option<String> {
    let dir = path.parent()?;
    let name = path.file_name()?.to_str()?;
    // Only for files in a repository, so that git, or on macOS its
    // stand-in that offers to install it, runs for nothing else.
    dir.ancestors().find(|d| d.join(".git").exists())?;
    if !has_git() {
        return None;
    }
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["cat-file", "blob", &format!("HEAD:./{name}")])
        // Reading takes no locks that a git command running meanwhile
        // would trip on.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(Loaded::new(String::from_utf8(out.stdout).ok()?).into_text())
}

/// Whether running `git` runs git. On macOS `/usr/bin/git` is a stand-in
/// until the developer tools are installed, and running it asks to install
/// them; `xcode-select -p` fails until they are.
fn has_git() -> bool {
    #[cfg(target_os = "macos")]
    {
        static HAS: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *HAS.get_or_init(|| {
            Command::new("/usr/bin/xcode-select")
                .arg("-p")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        })
    }
    #[cfg(not(target_os = "macos"))]
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

    fn change(kind: Kind, lines: Range<usize>) -> LineChange {
        LineChange { kind, lines }
    }

    #[test]
    fn nothing_changed() {
        assert_eq!(line_changes("a\nb\n", "a\nb\n"), vec![]);
    }

    #[test]
    fn added_lines() {
        assert_eq!(
            line_changes("a\n", "a\n\nb\n"),
            vec![change(Kind::Added, 1..3)]
        );
        assert_eq!(line_changes("", "a\n"), vec![change(Kind::Added, 0..1)]);
    }

    #[test]
    fn changed_lines_are_the_new_ones() {
        assert_eq!(
            line_changes("a\nb\nc\n", "a\nB\nc\n"),
            vec![change(Kind::Changed, 1..2)]
        );
        // Lines replaced by more lines are all changed.
        assert_eq!(
            line_changes("a\nb\nc\n", "a\nB\nB2\nB3\nc\n"),
            vec![change(Kind::Changed, 1..4)]
        );
    }

    #[test]
    fn deletions_sit_at_the_line_after() {
        assert_eq!(
            line_changes("a\nb\nc\n", "a\nc\n"),
            vec![change(Kind::Deleted, 1..1)]
        );
        assert_eq!(
            line_changes("a\nb\n", "b\n"),
            vec![change(Kind::Deleted, 0..0)]
        );
        // At the end: the empty line after the last newline.
        assert_eq!(
            line_changes("a\nb\n", "a\n"),
            vec![change(Kind::Deleted, 1..1)]
        );
        assert_eq!(line_changes("a\n", ""), vec![change(Kind::Deleted, 0..0)]);
    }

    #[test]
    fn paragraphs_match_by_their_text_not_their_blank_lines() {
        // A deleted paragraph, not two changed ones lined up by the blank
        // lines between paragraphs.
        assert_eq!(
            line_changes("One.\n\nTwo.\n\nThree.\n", "One.\n\nThree.\n\nFour.\n"),
            vec![change(Kind::Deleted, 2..2), change(Kind::Added, 3..5)]
        );
    }

    #[test]
    fn several_changes_in_order() {
        assert_eq!(
            line_changes("a\nb\nc\nd\ne\n", "A\nb\nd\ne\nf\n"),
            vec![
                change(Kind::Changed, 0..1),
                change(Kind::Deleted, 2..2),
                change(Kind::Added, 4..5),
            ]
        );
    }

    /// A git repository in a fresh temporary directory.
    struct Repo(PathBuf);

    impl Repo {
        fn new(name: &str) -> Repo {
            let dir =
                std::env::temp_dir().join(format!("margin-changes-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let repo = Repo(dir);
            repo.git(&["init", "-q"]);
            repo
        }

        fn git(&self, args: &[&str]) {
            let mut cmd = Command::new("git");
            // Run from a git hook, git's variables would point these
            // commands at the repository being committed to.
            for (k, _) in std::env::vars_os() {
                if k.to_string_lossy().starts_with("GIT_") {
                    cmd.env_remove(k);
                }
            }
            let ok = cmd
                .arg("-C")
                .arg(&self.0)
                .args(["-c", "user.name=T", "-c", "user.email=t@example.com"])
                .args(["-c", "commit.gpgsign=false"])
                .args(args)
                .status()
                .unwrap()
                .success();
            assert!(ok, "git {args:?}");
        }

        fn write(&self, name: &str, text: &str) -> PathBuf {
            let p = self.0.join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, text).unwrap();
            p
        }

        fn commit(&self) {
            self.git(&["add", "-A"]);
            self.git(&["commit", "-q", "-m", "c"]);
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn committed_is_the_last_commit() {
        let repo = Repo::new("head");
        let p = repo.write("docs/plan.md", "one\n");
        repo.commit();
        repo.write("docs/plan.md", "two\n");
        assert_eq!(committed(&p).as_deref(), Some("one\n"));
        repo.commit();
        assert_eq!(committed(&p).as_deref(), Some("two\n"));
    }

    #[test]
    fn committed_as_the_editors_hold_text() {
        let repo = Repo::new("crlf");
        let p = repo.write("plan.md", "one\r\ntwo");
        repo.commit();
        assert_eq!(committed(&p).as_deref(), Some("one\ntwo\n"));
    }

    #[test]
    fn nothing_committed() {
        let repo = Repo::new("none");
        let p = repo.write("plan.md", "one\n");
        // No commits yet.
        assert_eq!(committed(&p), None);
        repo.write("other.md", "x\n");
        repo.commit();
        // Not yet committed.
        let q = repo.write("new.md", "one\n");
        assert_eq!(committed(&q), None);
    }

    #[test]
    fn outside_a_repository() {
        let dir = std::env::temp_dir().join(format!("margin-changes-out-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("plan.md");
        std::fs::write(&p, "one\n").unwrap();
        assert_eq!(committed(&p), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
