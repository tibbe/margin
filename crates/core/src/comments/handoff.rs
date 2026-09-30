//! Sending a document's open comments to the agents waiting on it.
//!
//! An agent waits with `margin wait`, which holds a [`Waiter`]: a record in
//! the document's directory under [`data_dir`]`/waiting/`, locked for as
//! long as the process lives, so a crashed waiter is told from a live one
//! without trusting pids. The editor's [`send`] bumps the document's send
//! count; a waiter returns once the count passes the one it started at.
//! Waiters that started before the last send are on their way out, and
//! don't count as waiting.
//!
//! An editor showing the document holds a [`DocAgents`], which keeps a
//! viewer record there the same way. A waiter whose documents no editor
//! shows has nothing to wait for.

use super::store::{Store, data_dir};
use anyhow::{Context, Result};
use std::fs::{self, File};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const SENT: &str = "sent";
const WAITER: &str = "waiter";
const VIEWER: &str = "viewer";

/// The directory holding a document's waiters and send count.
fn dir(doc: &Path) -> Result<PathBuf> {
    let store = Store::for_doc(doc)?;
    let stem = store
        .path
        .file_stem()
        .context("comment store has no name")?;
    Ok(data_dir().join("waiting").join(stem))
}

/// How many times the document's comments have been sent.
fn sent_count(dir: &Path) -> u64 {
    fs::read_to_string(dir.join(SENT))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// Writes `contents` to `path` through a temporary file, so readers never
/// see half of it. Returns the still-open file.
fn replace(path: &Path, contents: &str, keep: impl FnOnce(&File) -> Result<()>) -> Result<File> {
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    let mut f = File::create(&tmp).with_context(|| format!("writing {}", tmp.display()))?;
    keep(&f)?;
    f.write_all(contents.as_bytes())?;
    f.sync_all()?;
    fs::rename(&tmp, path).with_context(|| format!("replacing {}", path.display()))?;
    Ok(f)
}

/// A record in `dir` holding `contents`, locked for as long as the
/// returned file is open, so that it outlives its process only unlocked.
fn lock_record(dir: &Path, name: &str, kind: &str, contents: &str) -> Result<(PathBuf, File)> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let record = dir.join(format!("{name}.{kind}"));
    // Locked before it appears under its name, so no reader takes it for a
    // dead process's.
    let lock = replace(&record, contents, |f| {
        f.lock().with_context(|| format!("locking {kind} record"))
    })?;
    Ok((record, lock))
}

/// The records of one kind in `dir` whose processes are alive. Removes the
/// records of processes that died.
fn live_records(dir: &Path, kind: &str) -> Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    let mut live = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != kind) {
            continue;
        }
        let Ok(f) = File::open(&path) else { continue };
        match f.try_lock_shared() {
            Ok(()) => {
                // Nobody holds it: its process is gone.
                let _ = fs::remove_file(&path);
            }
            Err(_) => live.push(path),
        }
    }
    Ok(live)
}

/// An agent waiting on one document, for as long as this value lives.
pub struct Waiter {
    doc: PathBuf,
    dir: PathBuf,
    record: PathBuf,
    seen: u64,
    _lock: File,
}

impl Waiter {
    pub fn start(doc: &Path) -> Result<Waiter> {
        let dir = dir(doc)?;
        let seen = sent_count(&dir);
        let (record, lock) = lock_record(
            &dir,
            &std::process::id().to_string(),
            WAITER,
            &seen.to_string(),
        )?;
        Ok(Waiter {
            doc: Store::for_doc(doc)?.doc,
            dir,
            record,
            seen,
            _lock: lock,
        })
    }

    /// The document's canonical path.
    pub fn doc(&self) -> &Path {
        &self.doc
    }

    /// Whether the writer has sent the comments since this waiter started.
    pub fn sent(&self) -> bool {
        sent_count(&self.dir) > self.seen
    }

    /// Whether an editor shows the document, so that it can still send.
    pub fn shown(&self) -> bool {
        // Unsure counts as shown: a waiter gives up only when it knows.
        !matches!(live_records(&self.dir, VIEWER), Ok(v) if v.is_empty())
    }
}

impl Drop for Waiter {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.record);
    }
}

/// How many agents are waiting on the document for its next send. Removes
/// the records of waiters that died.
pub fn waiting(doc: &Path) -> Result<usize> {
    let dir = dir(doc)?;
    let sent = sent_count(&dir);
    Ok(live_records(&dir, WAITER)?
        .iter()
        .filter(|path| {
            let seen: Option<u64> = fs::read_to_string(path)
                .ok()
                .and_then(|s| s.trim().parse().ok());
            seen == Some(sent)
        })
        .count())
}

/// How many editor windows show the document. Removes the records of
/// editors that died.
pub fn viewers(doc: &Path) -> Result<usize> {
    Ok(live_records(&dir(doc)?, VIEWER)?.len())
}

/// An editor window showing one document, for as long as this value lives.
#[derive(Debug)]
struct Viewer {
    record: PathBuf,
    _lock: File,
}

impl Viewer {
    fn start(doc: &Path) -> Result<Viewer> {
        // A process can show a document in more than one window, and
        // replaces a window's viewer before dropping the old one.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let (record, lock) = lock_record(&dir(doc)?, &name, VIEWER, "")?;
        Ok(Viewer {
            record,
            _lock: lock,
        })
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.record);
    }
}

/// Sends the document's comments to the agents waiting on it, returning
/// how many there were. Nothing is sent, or kept for later, when none is.
pub fn send(doc: &Path) -> Result<usize> {
    let n = waiting(doc)?;
    if n > 0 {
        let dir = dir(doc)?;
        replace(&dir.join(SENT), &(sent_count(&dir) + 1).to_string(), |_| {
            Ok(())
        })?;
    }
    Ok(n)
}

/// What the window shows about agents on a document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentState {
    None,
    /// An agent is waiting: the comments can be sent.
    Waiting,
    /// An agent took the last send and hasn't waited again yet.
    Working,
}

/// How long Working lasts without agent activity on the document.
pub const WORKING_TIMEOUT_MS: i64 = 30 * 60 * 1000;

/// The agent state of one open document, from its waiters and the sends
/// and agent activity the window has seen. Times are milliseconds on any
/// clock that only goes forward.
#[derive(Clone, Debug, Default)]
pub struct AgentTracker {
    /// When the last send or agent activity since it happened, while an
    /// agent is working on a send.
    working_since: Option<i64>,
}

impl AgentTracker {
    /// The state, given how many agents are [`waiting`] now.
    pub fn state(&mut self, waiting: usize, now: i64) -> AgentState {
        if waiting > 0 {
            self.working_since = None;
            return AgentState::Waiting;
        }
        match self.working_since {
            Some(t) if now - t < WORKING_TIMEOUT_MS => AgentState::Working,
            _ => {
                self.working_since = None;
                AgentState::None
            }
        }
    }

    /// The comments were sent to `agents` waiting agents.
    pub fn sent(&mut self, agents: usize, now: i64) {
        if agents > 0 {
            self.working_since = Some(now);
        }
    }

    /// An agent changed the document's threads.
    pub fn activity(&mut self, now: i64) {
        if self.working_since.is_some() {
            self.working_since = Some(now);
        }
    }
}

/// The agents on one open document, as its window follows them. While it
/// lives, waiters count the document as shown.
#[derive(Debug)]
pub struct DocAgents {
    doc: PathBuf,
    tracker: AgentTracker,
    /// None when the record couldn't be written; waiters then give up on
    /// the document while the window still shows it.
    _viewer: Option<Viewer>,
}

impl DocAgents {
    pub fn new(doc: &Path) -> DocAgents {
        DocAgents {
            doc: doc.to_path_buf(),
            tracker: AgentTracker::default(),
            _viewer: Viewer::start(doc).ok(),
        }
    }

    /// The state now, looking at the document's waiters.
    pub fn poll(&mut self, now: i64) -> AgentState {
        let n = waiting(&self.doc).unwrap_or(0);
        self.tracker.state(n, now)
    }

    /// Sends the comments; see [`send`].
    pub fn send(&mut self, now: i64) -> Result<usize> {
        let n = send(&self.doc)?;
        self.tracker.sent(n, now);
        Ok(n)
    }

    /// An agent changed the document's threads.
    pub fn activity(&mut self, now: i64) {
        self.tracker.activity(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_send_reaches_the_waiters_of_its_document_only() {
        let root = std::env::temp_dir().join(format!("margin-handoff-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let _env = crate::comments::use_data_dir(&root.join("data"));
        let plan = root.join("plan.md");
        let other = root.join("other.md");
        fs::write(&plan, "plan\n").unwrap();
        fs::write(&other, "other\n").unwrap();

        assert_eq!(waiting(&plan).unwrap(), 0);
        assert_eq!(send(&plan).unwrap(), 0, "nothing to send to");

        let a = Waiter::start(&plan).unwrap();
        let b = Waiter::start(&other).unwrap();
        assert_eq!(waiting(&plan).unwrap(), 1);
        assert!(!a.sent());
        assert_eq!(send(&plan).unwrap(), 1);
        assert!(a.sent());
        assert!(!b.sent());
        // Until it exits, a waiter that got the send isn't waiting.
        assert_eq!(waiting(&plan).unwrap(), 0);
        drop(a);
        let again = Waiter::start(&plan).unwrap();
        assert!(!again.sent());
        assert_eq!(waiting(&plan).unwrap(), 1);
        drop(again);
        assert_eq!(waiting(&plan).unwrap(), 0);
        drop(b);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_dead_waiters_record_is_dropped() {
        let root = std::env::temp_dir().join(format!("margin-handoff-dead-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let _env = crate::comments::use_data_dir(&root.join("data"));
        let plan = root.join("plan.md");
        fs::write(&plan, "plan\n").unwrap();
        let w = Waiter::start(&plan).unwrap();
        let record = w.record.clone();
        // A crash leaves the record but releases the lock.
        std::mem::forget(w);
        let dir = dir(&plan).unwrap();
        fs::write(dir.join("123.waiter"), "0").unwrap();
        assert!(fs::read_dir(&dir).unwrap().count() >= 2);
        // The forgotten waiter still holds its lock in this process.
        assert_eq!(waiting(&plan).unwrap(), 1);
        assert!(!dir.join("123.waiter").exists());
        assert!(record.exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_document_is_shown_while_a_window_follows_its_agents() {
        let root =
            std::env::temp_dir().join(format!("margin-handoff-shown-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let _env = crate::comments::use_data_dir(&root.join("data"));
        let plan = root.join("plan.md");
        let other = root.join("other.md");
        fs::write(&plan, "plan\n").unwrap();
        fs::write(&other, "other\n").unwrap();

        let w = Waiter::start(&plan).unwrap();
        assert!(!w.shown());
        let a = DocAgents::new(&plan);
        let _b = DocAgents::new(&other);
        assert!(w.shown());
        assert_eq!(viewers(&plan).unwrap(), 1);
        // A second window, or a window replacing its own, adds a viewer.
        let again = DocAgents::new(&plan);
        assert_eq!(viewers(&plan).unwrap(), 2);
        drop(a);
        assert!(w.shown());
        drop(again);
        assert!(!w.shown());
        assert_eq!(viewers(&plan).unwrap(), 0);
        // Viewers aren't waiters.
        assert_eq!(waiting(&plan).unwrap(), 1);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn working_lasts_from_a_send_until_an_agent_waits_again() {
        let mut t = AgentTracker::default();
        assert_eq!(t.state(0, 0), AgentState::None);
        assert_eq!(t.state(1, 0), AgentState::Waiting);
        t.sent(1, 10);
        assert_eq!(t.state(0, 20), AgentState::Working);
        t.activity(WORKING_TIMEOUT_MS);
        assert_eq!(t.state(0, WORKING_TIMEOUT_MS + 20), AgentState::Working);
        assert_eq!(t.state(1, WORKING_TIMEOUT_MS + 30), AgentState::Waiting);
        assert_eq!(t.state(0, WORKING_TIMEOUT_MS + 40), AgentState::None);
    }

    #[test]
    fn working_gives_up_without_agent_activity() {
        let mut t = AgentTracker::default();
        t.sent(1, 0);
        assert_eq!(t.state(0, WORKING_TIMEOUT_MS - 1), AgentState::Working);
        assert_eq!(t.state(0, WORKING_TIMEOUT_MS), AgentState::None);
        t.activity(WORKING_TIMEOUT_MS + 1);
        assert_eq!(t.state(0, WORKING_TIMEOUT_MS + 2), AgentState::None);
        t.sent(0, 0);
        assert_eq!(t.state(0, 1), AgentState::None);
    }
}
