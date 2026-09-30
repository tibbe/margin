//! Keeping a document's text and its file in step: autosaving the text,
//! and taking in edits other programs (agents) make to the file meanwhile.
//! No I/O here: the editors read and write the file, run the timers and
//! ask the person, and [`FileSync`] decides what each of those does.

/// A file's contents as the editors hold text: `\n` newlines and a final
/// newline. Whether the file used CRLF is kept, to write it back the same
/// way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loaded {
    text: String,
    crlf: bool,
}

impl Loaded {
    /// From the file's contents as read.
    pub fn new(raw: String) -> Loaded {
        let (mut text, crlf) = if raw.contains('\r') {
            let crlf = raw.contains("\r\n");
            (raw.replace("\r\n", "\n").replace('\r', "\n"), crlf)
        } else {
            (raw, false)
        };
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        Loaded { text, crlf }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn crlf(&self) -> bool {
        self.crlf
    }

    pub fn into_text(self) -> String {
        self.text
    }
}

/// What taking in a change to the file takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reconcile {
    /// The file has our text now: nothing to show.
    CaughtUp,
    /// We had no edits: show the file's text.
    Load(String),
    /// Both changed, on different lines: show the merge, and save it.
    Merge(String),
    /// Both changed the same lines: ask which to keep, then
    /// [`FileSync::keep_mine`] or [`FileSync::load_theirs`]. Nothing is
    /// written until then.
    Conflict,
}

/// A save's next step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Save {
    /// The file has our text.
    Done,
    /// Write this to the file (in the file's newlines), then
    /// [`FileSync::wrote`].
    Write(String),
    /// The file changed first: do what this says, then (unless it is a
    /// conflict) save again.
    Changed(Reconcile),
    /// A conflict waits for the person's answer.
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum State {
    /// The file has our text, as far as we know.
    Saved,
    /// Edited (or merged) since the file was last written; `failed` if
    /// the last save could not read or write it.
    Unsaved { failed: bool },
    /// The file and our text changed the same lines; `disk` is the file's.
    Conflict { disk: String },
}

/// One document's text and its file.
#[derive(Clone, Debug)]
pub struct FileSync {
    /// The file's text as last read or written: what merges start from.
    base: String,
    crlf: bool,
    state: State,
}

impl FileSync {
    /// For a document opened as `text` (as the editors hold text: see
    /// [`Loaded`]) from a file that used CRLF if `crlf`.
    pub fn new(text: &str, crlf: bool) -> FileSync {
        FileSync {
            base: text.to_string(),
            crlf,
            state: State::Saved,
        }
    }

    /// The text was edited.
    pub fn edited(&mut self) {
        if self.state == State::Saved {
            self.state = State::Unsaved { failed: false };
        }
    }

    /// Whether the text may differ from the file: a save is due, or waits
    /// on a conflict.
    pub fn needs_save(&self) -> bool {
        self.state != State::Saved
    }

    /// Whether the last save could not read or write the file.
    pub fn save_failed(&self) -> bool {
        matches!(self.state, State::Unsaved { failed: true })
    }

    pub fn in_conflict(&self) -> bool {
        matches!(self.state, State::Conflict { .. })
    }

    /// Whether the file has `ours` (comparing the texts).
    pub fn has(&self, ours: &str) -> bool {
        !self.in_conflict() && ours == self.base
    }

    /// `ours` in the file's newlines, to write (Save As).
    pub fn file_text(&self, ours: &str) -> String {
        if self.crlf {
            ours.replace('\n', "\r\n")
        } else {
            ours.to_string()
        }
    }

    /// The file was changed, and now reads `disk`, with `ours` in the
    /// editor. `None` when there is nothing to take in: the file has what
    /// we last read or wrote, or a conflict already waits.
    pub fn disk_changed(&mut self, ours: &str, disk: Loaded) -> Option<Reconcile> {
        if self.in_conflict() {
            return None;
        }
        self.crlf = disk.crlf;
        (disk.text != self.base).then(|| self.reconcile(ours, disk.text))
    }

    fn reconcile(&mut self, ours: &str, disk: String) -> Reconcile {
        if ours == disk {
            self.base = disk;
            self.state = State::Saved;
            return Reconcile::CaughtUp;
        }
        if ours == self.base {
            self.base = disk.clone();
            self.state = State::Saved;
            return Reconcile::Load(disk);
        }
        let merge = similar::TextMerge::from_lines(&self.base, ours, &disk);
        if merge.is_conflicted() {
            self.state = State::Conflict { disk };
            return Reconcile::Conflict;
        }
        let merged = merge.to_string();
        self.base = disk;
        self.state = State::Unsaved { failed: false };
        Reconcile::Merge(merged)
    }

    /// Saving `ours`, with the file read just before as `disk`: a file
    /// watcher may not have told us of an outside edit yet, and writing
    /// over it would lose it.
    pub fn save(&mut self, ours: &str, disk: Loaded) -> Save {
        if self.in_conflict() {
            return Save::Blocked;
        }
        self.crlf = disk.crlf;
        if disk.text != self.base {
            return Save::Changed(self.reconcile(ours, disk.text));
        }
        if ours == self.base {
            self.state = State::Saved;
            return Save::Done;
        }
        Save::Write(self.file_text(ours))
    }

    /// The file now has `ours`: it was written (or moved to where `ours`
    /// was written, or is gone and has nothing left to save).
    pub fn wrote(&mut self, ours: &str) {
        self.base = ours.to_string();
        self.state = State::Saved;
    }

    /// Saving could not read or write the file.
    pub fn failed(&mut self) {
        if !self.in_conflict() {
            self.state = State::Unsaved { failed: true };
        }
    }

    /// The person's answer to a conflict: keep their text, which then
    /// needs saving over the file's. False if no conflict waits.
    pub fn keep_mine(&mut self) -> bool {
        let State::Conflict { disk } = &self.state else {
            return false;
        };
        self.base = disk.clone();
        self.state = State::Unsaved { failed: false };
        true
    }

    /// The person's answer to a conflict: load the file's text, which this
    /// returns to show. `None` if no conflict waits.
    pub fn load_theirs(&mut self) -> Option<String> {
        let State::Conflict { disk } = &self.state else {
            return None;
        };
        self.base = disk.clone();
        self.state = State::Saved;
        Some(self.base.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(s: &str) -> Loaded {
        Loaded::new(s.to_string())
    }

    fn opened(s: &str) -> FileSync {
        let l = loaded(s);
        FileSync::new(l.text(), l.crlf())
    }

    #[test]
    fn loading_normalizes_newlines() {
        let l = loaded("a\r\nb");
        assert_eq!((l.text(), l.crlf()), ("a\nb\n", true));
        assert_eq!(loaded("a\rb\n").text(), "a\nb\n");
        assert_eq!(loaded("").text(), "");
    }

    #[test]
    fn saving_writes_edits_in_the_files_newlines() {
        let mut f = opened("one\r\n");
        assert_eq!(f.save("one\n", loaded("one\r\n")), Save::Done);
        f.edited();
        assert!(f.needs_save());
        assert_eq!(
            f.save("one\ntwo\n", loaded("one\r\n")),
            Save::Write("one\r\ntwo\r\n".into())
        );
        f.wrote("one\ntwo\n");
        assert!(!f.needs_save() && f.has("one\ntwo\n"));
    }

    #[test]
    fn a_save_takes_in_an_outside_edit_first() {
        // No edits of ours: show theirs, and there is nothing to write.
        let mut f = opened("a\nb\n");
        assert_eq!(
            f.save("a\nb\n", loaded("a\nB\n")),
            Save::Changed(Reconcile::Load("a\nB\n".into()))
        );
        assert_eq!(f.save("a\nB\n", loaded("a\nB\n")), Save::Done);
        // Edits on other lines merge, then the merge is written.
        let mut f = opened("a\nx\nb\n");
        f.edited();
        assert_eq!(
            f.save("A\nx\nb\n", loaded("a\nx\nB\n")),
            Save::Changed(Reconcile::Merge("A\nx\nB\n".into()))
        );
        assert_eq!(
            f.save("A\nx\nB\n", loaded("a\nx\nB\n")),
            Save::Write("A\nx\nB\n".into())
        );
        // They made our edit too.
        let mut f = opened("a\n");
        f.edited();
        assert_eq!(
            f.save("b\n", loaded("b\n")),
            Save::Changed(Reconcile::CaughtUp)
        );
        assert!(!f.needs_save());
        assert_eq!(f.save("b\n", loaded("b\n")), Save::Done);
    }

    #[test]
    fn nothing_is_written_while_a_conflict_waits() {
        let mut f = opened("a\n");
        f.edited();
        assert_eq!(
            f.save("mine\n", loaded("theirs\n")),
            Save::Changed(Reconcile::Conflict)
        );
        assert_eq!(f.save("mine\n", loaded("theirs\n")), Save::Blocked);
        assert_eq!(f.disk_changed("mine\n", loaded("newer\n")), None);
        assert!(f.needs_save() && !f.has("a\n"));
        // Keeping ours writes it over theirs.
        assert!(f.keep_mine());
        assert_eq!(
            f.save("mine\n", loaded("theirs\n")),
            Save::Write("mine\n".into())
        );
        assert!(!f.keep_mine() && f.load_theirs().is_none());
    }

    #[test]
    fn loading_theirs_ends_a_conflict() {
        let mut f = opened("a\n");
        f.edited();
        assert_eq!(
            f.disk_changed("mine\n", loaded("theirs\n")),
            Some(Reconcile::Conflict)
        );
        assert_eq!(f.load_theirs(), Some("theirs\n".into()));
        assert!(!f.needs_save() && f.has("theirs\n"));
        assert_eq!(f.load_theirs(), None);
    }

    #[test]
    fn outside_changes() {
        let mut f = opened("a\n");
        assert_eq!(f.disk_changed("a\n", loaded("a\r\n")), None);
        assert_eq!(f.file_text("a\n"), "a\r\n");
        assert_eq!(
            f.disk_changed("a\n", loaded("b\n")),
            Some(Reconcile::Load("b\n".into()))
        );
        f.edited();
        assert_eq!(
            f.disk_changed("c\n", loaded("c\n")),
            Some(Reconcile::CaughtUp)
        );
        assert!(!f.needs_save());
    }

    #[test]
    fn a_failed_save_stays_unsaved() {
        let mut f = opened("a\n");
        f.edited();
        f.failed();
        assert!(f.save_failed() && f.needs_save());
        f.edited();
        assert!(f.save_failed());
        f.wrote("b\n");
        assert!(!f.save_failed() && !f.needs_save());
    }
}
