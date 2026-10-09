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
//! A send covers a round: the documents of the agents waiting on the
//! document, and of the agents waiting on those, and so on. Each waiter's
//! record lists the documents its agent waits on, and [`send`] bumps the
//! send count of every document in the round.
//!
//! A send gives the agent what changed since the last one: every open
//! thread, and the threads the writer resolved since (see [`to_send`]).
//! Each document keeps when its last send was, or, before any, when an
//! agent first waited on it.
//!
//! An editor showing the document holds a [`DocAgents`], which keeps a
//! viewer record there the same way. A waiter whose documents no editor
//! shows has nothing to wait for.

use super::store::{Author, Store, Thread, data_dir};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const SENT: &str = "sent";
const SINCE: &str = "since";
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

/// When the document's last send was, or, before any, when an agent first
/// waited on it.
fn since_in(dir: &Path) -> Option<DateTime<Utc>> {
    fs::read_to_string(dir.join(SINCE))
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn mark_since(dir: &Path, at: DateTime<Utc>) -> Result<()> {
    replace(&dir.join(SINCE), &at.to_rfc3339(), |_| Ok(()))?;
    Ok(())
}

/// Whether the writer resolved the thread after `since`.
fn resolved_since(t: &Thread, since: Option<DateTime<Utc>>) -> bool {
    t.resolved_by() == Some(Author::User)
        && since.is_some_and(|s| t.resolved_at().is_some_and(|at| at > s))
}

/// The threads a send gives the agent on one document: the open ones, and
/// the ones the writer resolved after `since`, the document's last send.
/// Threads the agent resolved it knows about already.
pub fn to_send(threads: &[Thread], since: Option<DateTime<Utc>>) -> Vec<Thread> {
    threads
        .iter()
        .filter(|t| t.is_open() || resolved_since(t, since))
        .cloned()
        .collect()
}

/// How many threads a send would give the agent on a document.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pending {
    pub open: usize,
    /// Resolved by the writer since the last send.
    pub resolved: usize,
}

impl Pending {
    /// What a send would give on a document with `threads`, last sent at
    /// `since`.
    pub fn of(threads: &[Thread], since: Option<DateTime<Utc>>) -> Pending {
        Pending {
            open: threads.iter().filter(|t| t.is_open()).count(),
            resolved: threads.iter().filter(|t| resolved_since(t, since)).count(),
        }
    }

    /// What a send would give on the document now.
    fn now(doc: &Path) -> Pending {
        let since = dir(doc).ok().and_then(|d| since_in(&d));
        Store::for_doc(doc)
            .and_then(|s| s.load())
            .map_or_else(|_| Pending::default(), |c| Pending::of(&c.threads, since))
    }

    pub fn is_empty(&self) -> bool {
        self.open == 0 && self.resolved == 0
    }
}

impl std::ops::Add for Pending {
    type Output = Pending;
    fn add(self, o: Pending) -> Pending {
        Pending {
            open: self.open + o.open,
            resolved: self.resolved + o.resolved,
        }
    }
}

/// What a send gave, for its announcement: "2 open comments", "2 open and
/// 1 resolved comment on 2 documents", "1 resolved comment".
pub fn sent_summary(sent: Pending, docs: usize) -> String {
    let n = |k: usize| if k == 1 { "comment" } else { "comments" };
    let mut s = match (sent.open, sent.resolved) {
        (o, 0) => format!("{o} open {}", n(o)),
        (0, r) => format!("{r} resolved {}", n(r)),
        (o, r) => format!("{o} open and {r} resolved {}", n(r)),
    };
    if docs > 1 {
        s.push_str(&format!(" on {docs} documents"));
    }
    s
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

/// A waiter's record: the send count it started at, then the canonical
/// paths of every document its agent waits on, one per line.
fn waiter_record(seen: u64, round: &[PathBuf]) -> String {
    let mut s = seen.to_string();
    for doc in round {
        s.push('\n');
        s.push_str(&doc.display().to_string());
    }
    s
}

/// An agent waiting on `dir`'s document for its next send, and the
/// documents it waits on.
struct Waiting {
    /// The record's name, the same in every document's directory for one
    /// agent.
    agent: String,
    docs: Vec<PathBuf>,
}

/// The agents waiting on `dir`'s document for its next send. Records of
/// waiters that started before the last send don't count.
fn waiting_in(dir: &Path) -> Result<Vec<Waiting>> {
    let sent = sent_count(dir);
    Ok(live_records(dir, WAITER)?
        .iter()
        .filter_map(|path| {
            let record = fs::read_to_string(path).ok()?;
            let mut lines = record.lines();
            let seen: u64 = lines.next()?.trim().parse().ok()?;
            let agent = path.file_stem()?.to_string_lossy().into_owned();
            (seen == sent).then(|| Waiting {
                agent,
                docs: lines.map(PathBuf::from).collect(),
            })
        })
        .collect())
}

/// The documents a send on `doc` covers, `doc` first, and the agents
/// waiting on them.
pub fn round(doc: &Path) -> Result<(Vec<PathBuf>, BTreeSet<String>)> {
    let doc = Store::for_doc(doc)?.doc;
    let mut docs = vec![doc];
    let mut agents = BTreeSet::new();
    let mut i = 0;
    while i < docs.len() {
        // A document elsewhere that is gone has no directory to look in.
        if let Ok(waiting) = dir(&docs[i]).and_then(|d| waiting_in(&d)) {
            for w in waiting {
                agents.insert(w.agent);
                for d in w.docs {
                    if !docs.contains(&d) {
                        docs.push(d);
                    }
                }
            }
        }
        i += 1;
    }
    Ok((docs, agents))
}

/// An agent waiting on one document, for as long as this value lives.
pub struct Waiter {
    doc: PathBuf,
    dir: PathBuf,
    record: PathBuf,
    seen: u64,
    since: DateTime<Utc>,
    _lock: File,
}

impl Waiter {
    /// Waits on `doc`, one of the documents in `round`, the canonical
    /// paths of all the documents the agent waits on.
    pub fn start(doc: &Path, round: &[PathBuf]) -> Result<Waiter> {
        let dir = dir(doc)?;
        fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        // Before the send count: should a send come in between, the next
        // one gives too much rather than too little.
        let since = match since_in(&dir) {
            Some(t) => t,
            None => {
                let now = Utc::now();
                mark_since(&dir, now)?;
                now
            }
        };
        let seen = sent_count(&dir);
        let (record, lock) = lock_record(
            &dir,
            &std::process::id().to_string(),
            WAITER,
            &waiter_record(seen, round),
        )?;
        Ok(Waiter {
            doc: Store::for_doc(doc)?.doc,
            dir,
            record,
            seen,
            since,
            _lock: lock,
        })
    }

    /// The document's last send before this waiter started, or, before
    /// any, when an agent first waited on it: what the writer resolved
    /// after it goes with the next send.
    pub fn since(&self) -> DateTime<Utc> {
        self.since
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
    Ok(waiting_in(&dir(doc)?)?.len())
}

/// How many times the document has been sent, 0 when that can't be read.
fn sends(doc: &Path) -> u64 {
    dir(doc).map(|d| sent_count(&d)).unwrap_or(0)
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

/// Sends the comments on the document's round to the agents waiting on
/// it, returning how many agents there were. Nothing is sent, or kept for
/// later, when none is.
pub fn send(doc: &Path) -> Result<usize> {
    let (docs, agents) = round(doc)?;
    if agents.is_empty() {
        return Ok(0);
    }
    for (i, d) in docs.iter().enumerate() {
        match dir(d).and_then(|dir| bump(&dir)) {
            Ok(()) => {}
            // The document itself must be sent; one elsewhere that is gone
            // has nothing to send.
            Err(e) if i == 0 => return Err(e),
            Err(_) => {}
        }
    }
    Ok(agents.len())
}

/// Counts one more send of `dir`'s document, made now.
fn bump(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    mark_since(dir, Utc::now())?;
    replace(&dir.join(SENT), &(sent_count(dir) + 1).to_string(), |_| {
        Ok(())
    })?;
    Ok(())
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
    /// The document's send count when last looked at, to tell when a send
    /// from another document's window covered it.
    sends: u64,
    /// The document's last send, or first wait, as of the last poll.
    since: Option<DateTime<Utc>>,
    /// The other documents a send from here covers, and what it would give
    /// on each, as of the last poll.
    others: Vec<(PathBuf, Pending)>,
    /// None when the record couldn't be written; waiters then give up on
    /// the document while the window still shows it.
    _viewer: Option<Viewer>,
}

impl DocAgents {
    pub fn new(doc: &Path) -> DocAgents {
        DocAgents {
            doc: doc.to_path_buf(),
            tracker: AgentTracker::default(),
            sends: sends(doc),
            since: None,
            others: Vec::new(),
            _viewer: Viewer::start(doc).ok(),
        }
    }

    /// The state now, looking at the document's waiters, and at sends that
    /// covered it from other windows.
    pub fn poll(&mut self, now: i64) -> AgentState {
        self.saw_sends(now);
        let (docs, agents) = round(&self.doc).unwrap_or_default();
        self.since = dir(&self.doc).ok().and_then(|d| since_in(&d));
        self.others = docs
            .into_iter()
            .skip(1)
            .map(|d| {
                let p = Pending::now(&d);
                (d, p)
            })
            .collect();
        // The round has agents only when one waits here.
        self.tracker.state(agents.len(), now)
    }

    /// What a send from here would give on the document, with `threads`
    /// as the window has them now and its last send as of the last
    /// [`poll`](Self::poll).
    pub fn pending(&self, threads: &[Thread]) -> Pending {
        Pending::of(threads, self.since)
    }

    /// The other documents a send from here covers, as of the last
    /// [`poll`](Self::poll), in the order agents named them, each with what
    /// a send would give on it.
    pub fn others(&self) -> &[(PathBuf, Pending)] {
        &self.others
    }

    /// Sends the comments on the round; see [`send`].
    pub fn send(&mut self, now: i64) -> Result<usize> {
        let n = send(&self.doc)?;
        self.tracker.sent(n, now);
        self.sends = sends(&self.doc);
        Ok(n)
    }

    /// Starts working when a send covered the document since last looked.
    fn saw_sends(&mut self, now: i64) {
        let sends = sends(&self.doc);
        if sends > self.sends {
            self.tracker.sent(1, now);
        }
        self.sends = sends;
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

        let a = Waiter::start(&plan, &[]).unwrap();
        let b = Waiter::start(&other, &[]).unwrap();
        assert_eq!(waiting(&plan).unwrap(), 1);
        assert!(!a.sent());
        assert_eq!(send(&plan).unwrap(), 1);
        assert!(a.sent());
        assert!(!b.sent());
        // Until it exits, a waiter that got the send isn't waiting.
        assert_eq!(waiting(&plan).unwrap(), 0);
        drop(a);
        let again = Waiter::start(&plan, &[]).unwrap();
        assert!(!again.sent());
        assert_eq!(waiting(&plan).unwrap(), 1);
        drop(again);
        assert_eq!(waiting(&plan).unwrap(), 0);
        drop(b);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_send_covers_every_document_its_agents_wait_on() {
        let root =
            std::env::temp_dir().join(format!("margin-handoff-round-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let _env = crate::comments::use_data_dir(&root.join("data"));
        let [plan, spec, notes, other] = ["plan.md", "spec.md", "notes.md", "other.md"].map(|n| {
            fs::write(root.join(n), "text\n").unwrap();
            Store::for_doc(&root.join(n)).unwrap().doc
        });

        // This agent waits on plan and spec; another, on spec and notes.
        let both = [plan.clone(), spec.clone()];
        let on_plan = Waiter::start(&plan, &both).unwrap();
        let on_spec = Waiter::start(&spec, &both).unwrap();
        let elsewhere = Waiter::start(&other, &[]).unwrap();
        let _another = [&spec, &notes].map(|d| {
            lock_record(
                &dir(d).unwrap(),
                "another",
                WAITER,
                &waiter_record(0, &[spec.clone(), notes.clone()]),
            )
            .unwrap()
        });
        let (docs, agents) = round(&plan).unwrap();
        assert_eq!(docs, [plan.clone(), spec.clone(), notes.clone()]);
        assert_eq!(agents.len(), 2);

        // A window on notes follows the round; one on other doesn't.
        let mut notes_window = DocAgents::new(&notes);
        let mut other_window = DocAgents::new(&other);
        assert_eq!(notes_window.poll(0), AgentState::Waiting);
        assert_eq!(
            notes_window.others(),
            [
                (spec.clone(), Pending::default()),
                (plan.clone(), Pending::default())
            ]
        );

        assert_eq!(send(&plan).unwrap(), 2);
        assert!(on_plan.sent());
        assert!(on_spec.sent());
        assert!(!elsewhere.sent(), "other is in no agent's round");
        assert_eq!(sends(&notes), 1);
        // A send from another window counts as one here.
        assert_eq!(notes_window.poll(10), AgentState::Working);
        assert_eq!(other_window.poll(10), AgentState::Waiting);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_send_gives_what_the_writer_resolved_since_the_last() {
        let root = std::env::temp_dir().join(format!("margin-handoff-news-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let _env = crate::comments::use_data_dir(&root.join("data"));
        let plan = root.join("plan.md");
        let text = "one two three\n";
        fs::write(&plan, text).unwrap();
        let store = Store::for_doc(&plan).unwrap();
        let [one, two, three] = [0..3, 4..7, 8..13].map(|r| {
            store
                .update(|c| c.add(text, r, "Why?", Author::User))
                .unwrap()
        });
        let resolve = |id, by| store.update(|c| c.set_resolved(id, true, by)).unwrap();
        // Resolved before any agent waited: not news to it.
        resolve(one, Author::User);
        let ids = |w: &Waiter| -> Vec<u64> {
            to_send(&store.load().unwrap().threads, Some(w.since()))
                .iter()
                .map(|t| t.id)
                .collect()
        };

        let w = Waiter::start(&plan, &[]).unwrap();
        let mut window = DocAgents::new(&plan);
        window.poll(0);
        let pending = |w: &DocAgents| w.pending(&store.load().unwrap().threads);
        assert_eq!(
            pending(&window),
            Pending {
                open: 2,
                resolved: 0
            }
        );
        resolve(two, Author::User);
        resolve(three, Author::Agent);
        assert_eq!(
            pending(&window),
            Pending {
                open: 0,
                resolved: 1
            }
        );
        assert_eq!(send(&plan).unwrap(), 1);
        assert_eq!(ids(&w), [two], "the agent knows what it resolved");
        drop(w);

        // The next send starts from this one.
        window.poll(0);
        assert!(pending(&window).is_empty());
        let w = Waiter::start(&plan, &[]).unwrap();
        assert!(ids(&w).is_empty());
        store
            .update(|c| c.set_resolved(two, false, Author::User))
            .unwrap();
        resolve(two, Author::User);
        assert_eq!(ids(&w), [two]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn announcements_say_what_went() {
        let p = |open, resolved| Pending { open, resolved };
        assert_eq!(sent_summary(p(1, 0), 1), "1 open comment");
        assert_eq!(sent_summary(p(2, 0), 2), "2 open comments on 2 documents");
        assert_eq!(sent_summary(p(2, 1), 1), "2 open and 1 resolved comment");
        assert_eq!(sent_summary(p(0, 3), 1), "3 resolved comments");
    }

    #[test]
    fn a_dead_waiters_record_is_dropped() {
        let root = std::env::temp_dir().join(format!("margin-handoff-dead-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let _env = crate::comments::use_data_dir(&root.join("data"));
        let plan = root.join("plan.md");
        fs::write(&plan, "plan\n").unwrap();
        let w = Waiter::start(&plan, &[]).unwrap();
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

        let w = Waiter::start(&plan, &[]).unwrap();
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
