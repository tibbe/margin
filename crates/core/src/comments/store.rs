//! Comment threads for a document, persisted outside the document itself.
//!
//! Each document's threads live in one JSON file under
//! [`data_dir`]`/docs/`, keyed by the document's canonical path.
//! The app and the CLI both edit it, so every write happens under a file
//! lock as read-modify-write, and files are replaced atomically.

use super::anchor::{find_quote, OffsetMap};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Open,
    Resolved,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub at: DateTime<Utc>,
    pub body: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    /// Byte offsets into [`Comments::snapshot`].
    pub start: usize,
    pub end: usize,
    /// The commented text, as last seen.
    pub quote: String,
    /// The commented text was deleted; `start == end` marks where it was.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub detached: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Thread {
    pub id: u64,
    pub status: Status,
    pub anchor: Anchor,
    /// The comment, then its replies. Authors are not recorded: one person
    /// comments, and agents answer.
    pub messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<DateTime<Utc>>,
}

impl Thread {
    pub fn is_open(&self) -> bool {
        self.status == Status::Open
    }
}

/// Everything stored for one document.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Comments {
    pub version: u32,
    pub doc: PathBuf,
    pub next_id: u64,
    pub threads: Vec<Thread>,
    /// The document text that anchor offsets refer to.
    #[serde(default)]
    pub snapshot: String,
}

impl Comments {
    pub fn new(doc: PathBuf) -> Self {
        Comments {
            version: 1,
            doc,
            next_id: 1,
            threads: Vec::new(),
            snapshot: String::new(),
        }
    }

    pub fn thread(&self, id: u64) -> Option<&Thread> {
        self.threads.iter().find(|t| t.id == id)
    }

    pub fn thread_mut(&mut self, id: u64) -> Result<&mut Thread> {
        match self.threads.iter_mut().find(|t| t.id == id) {
            Some(t) => Ok(t),
            None => bail!("no comment #{id} on {}", self.doc.display()),
        }
    }

    /// Re-anchors every thread against `text`, the document as it is now.
    /// Returns whether anything changed.
    pub fn sync(&mut self, text: &str) -> bool {
        if self.snapshot == text {
            return false;
        }
        if self.snapshot.is_empty() && !self.threads.is_empty() {
            // No snapshot to diff against: fall back to finding the quotes.
            for t in &mut self.threads {
                match find_quote(text, &t.anchor.quote, t.anchor.start) {
                    Some(r) => {
                        t.anchor.start = r.start;
                        t.anchor.end = r.end;
                        t.anchor.detached = false;
                    }
                    None => {
                        t.anchor.detached = true;
                        t.anchor.start = t.anchor.start.min(text.len());
                        t.anchor.end = t.anchor.start;
                    }
                }
            }
        } else {
            let map = OffsetMap::new(&self.snapshot, text);
            for t in &mut self.threads {
                let a = &mut t.anchor;
                if a.detached {
                    let p = map.map_start(a.start).min(text.len());
                    a.start = p;
                    a.end = p;
                    continue;
                }
                match map.map_range(a.start..a.end) {
                    Ok(r) => {
                        a.quote = text[r.clone()].to_string();
                        a.start = r.start;
                        a.end = r.end;
                    }
                    Err(p) => {
                        a.detached = true;
                        a.start = p;
                        a.end = p;
                    }
                }
            }
        }
        self.snapshot = text.to_string();
        true
    }

    /// Adds a thread anchored to `range` of `text` (the current document).
    pub fn add(&mut self, text: &str, range: Range<usize>, body: &str) -> u64 {
        self.sync(text);
        let id = self.next_id;
        self.next_id += 1;
        self.threads.push(Thread {
            id,
            status: Status::Open,
            anchor: Anchor {
                start: range.start,
                end: range.end,
                quote: text[range].to_string(),
                detached: false,
            },
            messages: vec![Message {
                at: Utc::now(),
                body: body.to_string(),
            }],
            resolved_at: None,
        });
        id
    }

    pub fn reply(&mut self, id: u64, body: &str) -> Result<()> {
        let t = self.thread_mut(id)?;
        t.messages.push(Message {
            at: Utc::now(),
            body: body.to_string(),
        });
        Ok(())
    }

    pub fn set_resolved(&mut self, id: u64, resolved: bool) -> Result<()> {
        let t = self.thread_mut(id)?;
        if resolved {
            t.status = Status::Resolved;
            t.resolved_at = Some(Utc::now());
        } else {
            t.status = Status::Open;
            t.resolved_at = None;
        }
        Ok(())
    }

    pub fn delete(&mut self, id: u64) -> Result<()> {
        let before = self.threads.len();
        self.threads.retain(|t| t.id != id);
        if self.threads.len() == before {
            bail!("no comment #{id} on {}", self.doc.display());
        }
        Ok(())
    }

    /// Replaces the body of message `index` (0 is the comment).
    pub fn edit(&mut self, id: u64, index: usize, body: &str) -> Result<()> {
        if body.trim().is_empty() {
            bail!("a comment can't be empty; delete it instead");
        }
        let t = self.thread_mut(id)?;
        let Some(m) = t.messages.get_mut(index) else {
            bail!("comment #{id} has no message {index}");
        };
        m.body = body.to_string();
        Ok(())
    }

    /// Deletes message `index`; deleting the comment (0) deletes the thread.
    pub fn delete_message(&mut self, id: u64, index: usize) -> Result<()> {
        if index == 0 {
            return self.delete(id);
        }
        let t = self.thread_mut(id)?;
        if index >= t.messages.len() {
            bail!("comment #{id} has no message {index}");
        }
        t.messages.remove(index);
        Ok(())
    }

    /// Puts a deleted reply back at `index` (Undo).
    pub fn insert_message(&mut self, id: u64, index: usize, message: Message) -> Result<()> {
        let t = self.thread_mut(id)?;
        let i = index.clamp(1, t.messages.len());
        t.messages.insert(i, message);
        Ok(())
    }

    pub fn open_count(&self) -> usize {
        self.threads.iter().filter(|t| t.is_open()).count()
    }
}

/// The comment store for one document.
#[derive(Clone, Debug)]
pub struct Store {
    pub doc: PathBuf,
    pub path: PathBuf,
}

/// Where comments and drafts live: `MARGIN_DATA_DIR`, else
/// `$XDG_DATA_HOME/margin`, else the platform's place for app data.
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("MARGIN_DATA_DIR") {
        return PathBuf::from(d);
    }
    if let Some(base) = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return base.join("margin");
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Margin")
    } else {
        home.join(".local/share/margin")
    }
}

/// Absolute, symlink-free path of a document that may not exist yet.
pub fn canonical_doc_path(p: &Path) -> Result<PathBuf> {
    if let Ok(c) = fs::canonicalize(p) {
        return Ok(c);
    }
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()?.join(p)
    };
    let parent = abs.parent().context("document path has no parent")?;
    let name = abs.file_name().context("document path has no file name")?;
    let parent = fs::canonicalize(parent)
        .with_context(|| format!("directory {} does not exist", parent.display()))?;
    Ok(parent.join(name))
}

impl Store {
    pub fn for_doc(doc: &Path) -> Result<Store> {
        let doc = canonical_doc_path(doc)?;
        let hash = Sha256::digest(doc.as_os_str().as_encoded_bytes());
        let hex: String = hash[..8].iter().map(|b| format!("{b:02x}")).collect();
        let name: String = doc
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' })
            .collect();
        let path = data_dir().join("docs").join(format!("{name}-{hex}.json"));
        Ok(Store { doc, path })
    }

    fn lock_path(&self) -> PathBuf {
        self.path.with_extension("lock")
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    /// Deletes the stored threads under the same lock used by updates. The
    /// lock file stays in place so another process cannot lock a new inode.
    pub fn remove(&self) -> Result<()> {
        let dir = self.path.parent().context("store has no directory")?;
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.lock_path())
            .context("opening store lock")?;
        lock.lock().context("locking comment store")?;
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).with_context(|| format!("removing {}", self.path.display())),
        }
    }

    /// Loads the threads, or an empty set if there are none yet.
    pub fn load(&self) -> Result<Comments> {
        match fs::read_to_string(&self.path) {
            Ok(s) => serde_json::from_str(&s)
                .with_context(|| format!("corrupt comment store {}", self.path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Comments::new(self.doc.clone())),
            Err(e) => Err(e).with_context(|| format!("reading {}", self.path.display())),
        }
    }

    /// Runs `f` on the stored threads under an exclusive lock and saves the
    /// result if it changed.
    pub fn update<T>(&self, f: impl FnOnce(&mut Comments) -> Result<T>) -> Result<T> {
        let dir = self.path.parent().context("store has no directory")?;
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.lock_path())
            .context("opening store lock")?;
        lock.lock().context("locking comment store")?;
        let mut comments = self.load()?;
        let before = comments.clone();
        let out = f(&mut comments)?;
        if comments != before {
            self.write(&comments)?;
        }
        drop(lock);
        Ok(out)
    }

    fn write(&self, c: &Comments) -> Result<()> {
        let json = serde_json::to_string_pretty(c)?;
        let tmp = self.path.with_extension(format!("tmp{}", std::process::id()));
        {
            let mut f = fs::File::create(&tmp)
                .with_context(|| format!("writing {}", tmp.display()))?;
            f.write_all(json.as_bytes())?;
            f.write_all(b"\n")?;
            f.sync_all()?;
        }
        fs::rename(&tmp, &self.path).with_context(|| format!("replacing {}", self.path.display()))?;
        Ok(())
    }
}

/// Every document that has a comment store.
pub fn all_stores() -> Result<Vec<(Store, Comments)>> {
    let dir = data_dir().join("docs");
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else { continue };
        let Ok(c) = serde_json::from_str::<Comments>(&text) else { continue };
        let store = Store {
            doc: c.doc.clone(),
            path,
        };
        out.push((store, c));
    }
    out.sort_by(|a, b| a.0.doc.cmp(&b.0.doc));
    Ok(out)
}

/// Reads a document as UTF-8 text; a missing file reads as empty.
pub fn read_doc(path: &Path) -> Result<String> {
    match fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|_| anyhow::anyhow!("{} is not UTF-8 text", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_sync_and_detach() {
        let mut c = Comments::new("/tmp/x.md".into());
        let text = "Deploy with blue/green everywhere.\n";
        let s = text.find("blue/green").unwrap();
        let id = c.add(text, s..s + 10, "Canary instead?");
        assert_eq!(id, 1);
        let new = "Intro.\n\nDeploy with canary everywhere.\n";
        assert!(c.sync(new));
        let a = &c.thread(1).unwrap().anchor;
        assert_eq!(&new[a.start..a.end], "canary");
        assert_eq!(a.quote, "canary");
        let gone = "Intro.\n";
        c.sync(gone);
        assert!(c.thread(1).unwrap().anchor.detached);
    }

    #[test]
    fn edit_and_delete_messages() {
        let mut c = Comments::new("/tmp/x.md".into());
        let text = "hello world\n";
        let id = c.add(text, 6..11, "Why?");
        c.reply(id, "Because.").unwrap();
        c.reply(id, "Fixed.").unwrap();
        c.edit(id, 0, "Why not?").unwrap();
        assert_eq!(c.thread(id).unwrap().messages[0].body, "Why not?");
        assert!(c.edit(id, 0, "  ").is_err());
        assert!(c.edit(id, 3, "x").is_err());
        let reply = c.thread(id).unwrap().messages[1].clone();
        c.delete_message(id, 1).unwrap();
        let bodies = |c: &Comments| c.thread(id).unwrap().messages.iter().map(|m| m.body.clone()).collect::<Vec<_>>();
        assert_eq!(bodies(&c), ["Why not?", "Fixed."]);
        c.insert_message(id, 1, reply).unwrap();
        assert_eq!(bodies(&c), ["Why not?", "Because.", "Fixed."]);
        c.delete_message(id, 0).unwrap();
        assert!(c.thread(id).is_none());
    }

    #[test]
    fn stores_written_with_authors_still_load() {
        let json = r#"{"version":1,"doc":"/tmp/x.md","next_id":2,"threads":[{"id":1,
            "status":"resolved","anchor":{"start":0,"end":5,"quote":"hello"},
            "messages":[{"author":"tibbe","at":"2026-09-27T10:00:00Z","body":"hi"}],
            "resolved_by":"Claude","resolved_at":"2026-09-27T11:00:00Z"}],"snapshot":"hello\n"}"#;
        let c: Comments = serde_json::from_str(json).unwrap();
        assert_eq!(c.threads[0].messages[0].body, "hi");
        let out = serde_json::to_string(&c).unwrap();
        assert!(!out.contains("author") && !out.contains("resolved_by"));
    }

    #[test]
    fn store_roundtrip_with_lock() {
        let dir = std::env::temp_dir().join(format!("margin-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        unsafe { std::env::set_var("MARGIN_DATA_DIR", dir.join("data")) };
        let doc = dir.join("doc.md");
        fs::write(&doc, "hello world\n").unwrap();
        let store = Store::for_doc(&doc).unwrap();
        let id = store
            .update(|c| Ok(c.add("hello world\n", 6..11, "hi")))
            .unwrap();
        store.update(|c| c.reply(id, "done")).unwrap();
        store.update(|c| c.set_resolved(id, true)).unwrap();
        let c = store.load().unwrap();
        assert_eq!(c.threads[0].messages.len(), 2);
        assert_eq!(c.threads[0].status, Status::Resolved);
        assert_eq!(all_stores().unwrap().len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }
}
