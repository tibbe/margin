//! What the running editor is showing, published for agents: which
//! documents are open, which one has focus, where the cursor is, what is
//! selected and which comment thread is focused.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct OpenDoc {
    pub doc: PathBuf,
    pub focused: bool,
    /// 1-based cursor line.
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused_thread: Option<u64>,
    pub open_threads: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Status {
    pub pid: u32,
    pub updated: DateTime<Utc>,
    pub docs: Vec<OpenDoc>,
}

pub fn path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    dir.join("margin").join("status.json")
}

pub fn write(docs: Vec<OpenDoc>) -> Result<()> {
    let p = path();
    let dir = p.parent().context("status path")?;
    std::fs::create_dir_all(dir)?;
    let status = Status {
        pid: std::process::id(),
        updated: Utc::now(),
        docs,
    };
    let tmp = p.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_string_pretty(&status)?)?;
    std::fs::rename(&tmp, &p)?;
    Ok(())
}

/// The editor's status, if it is running.
pub fn read() -> Option<Status> {
    let text = std::fs::read_to_string(path()).ok()?;
    let status: Status = serde_json::from_str(&text).ok()?;
    let alive = PathBuf::from(format!("/proc/{}", status.pid)).exists();
    alive.then_some(status)
}
