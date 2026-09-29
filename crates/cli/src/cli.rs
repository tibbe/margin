//! The command-line interface: opening documents in the app, and the
//! commands coding agents use to read and answer comments.

use margin_core::comments::anchor::{line_col, line_start};
use margin_core::comments::{all_stores, read_doc, Comments, Status, Store, Thread};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Local, Utc};
use clap::{Parser, Subcommand};
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};

const AFTER_HELP: &str = "\
Agent workflow:
  margin comments plan.md          read the open threads on a document
  margin reply plan.md 3 \"Done.\" --resolve
  margin add plan.md --quote \"retry budget\" \"Is 3 enough?\"

Threads are numbered per document. Locations are file:line:column, 1-based.";

#[derive(Parser, Debug)]
#[command(
    name = "margin",
    version,
    about = "Markdown editor with Google Docs-style comments that coding agents read from the command line",
    args_conflicts_with_subcommands = true,
    after_help = AFTER_HELP
)]
pub struct Cli {
    /// Markdown files to open in the editor.
    pub files: Vec<PathBuf>,

    /// Keep the editor attached to this terminal instead of returning at once.
    #[arg(long, global = true)]
    pub foreground: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Open documents in the editor (what `margin FILE…` does).
    Open { files: Vec<PathBuf> },

    /// List comment threads. Without files: documents under the current
    /// directory that have open threads.
    Comments {
        files: Vec<PathBuf>,
        /// Include resolved threads.
        #[arg(long)]
        resolved: bool,
        /// Without files: every document, not just ones under the current directory.
        #[arg(long)]
        all: bool,
        #[arg(long)]
        json: bool,
    },

    /// Reply to a thread.
    Reply {
        file: PathBuf,
        id: u64,
        message: String,
        /// Also mark the thread resolved.
        #[arg(long)]
        resolve: bool,
    },

    /// Resolve a thread, optionally with a closing reply.
    Resolve {
        file: PathBuf,
        id: u64,
        message: Option<String>,
    },

    /// Reopen a resolved thread.
    Reopen { file: PathBuf, id: u64 },

    /// Start a thread on some text of the document.
    Add {
        file: PathBuf,
        /// Exact text to comment on (Markdown source, as in the file).
        #[arg(long, conflicts_with = "line")]
        quote: Option<String>,
        /// Which occurrence of --quote, if it appears more than once (1-based).
        #[arg(long)]
        occurrence: Option<usize>,
        /// Comment on a whole line instead (1-based).
        #[arg(long)]
        line: Option<usize>,
        /// Last line of a multi-line range.
        #[arg(long, requires = "line")]
        end_line: Option<usize>,
        message: String,
    },

    /// Delete a thread.
    Delete { file: PathBuf, id: u64 },

}

/// Loads a document's threads, re-anchored against the file as it is now.
fn load(file: &Path) -> Result<(Store, Comments, String)> {
    let store = Store::for_doc(file)?;
    if !store.doc.exists() {
        bail!("{} does not exist", store.doc.display());
    }
    let text = read_doc(&store.doc)?;
    let comments = if store.exists() {
        store.update(|c| {
            c.sync(&text);
            Ok(c.clone())
        })?
    } else {
        Comments::new(store.doc.clone())
    };
    Ok((store, comments, text))
}

fn display_path(p: &Path) -> String {
    if let Ok(cwd) = std::env::current_dir()
        && let Ok(rel) = p.strip_prefix(&cwd)
    {
        return rel.display().to_string();
    }
    p.display().to_string()
}

fn when(at: &DateTime<Utc>) -> String {
    let local = at.with_timezone(&Local);
    if local.date_naive() == Local::now().date_naive() {
        local.format("%H:%M").to_string()
    } else {
        local.format("%Y-%m-%d %H:%M").to_string()
    }
}

#[derive(Serialize)]
struct Pos {
    line: usize,
    column: usize,
}

#[derive(Serialize)]
struct JsonMessage<'a> {
    at: DateTime<Utc>,
    body: &'a str,
}

#[derive(Serialize)]
struct JsonThread<'a> {
    doc: &'a Path,
    id: u64,
    status: Status,
    /// The commented text was deleted from the document.
    detached: bool,
    start: Pos,
    end: Pos,
    quote: &'a str,
    /// The comment, then its replies.
    messages: Vec<JsonMessage<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_at: Option<DateTime<Utc>>,
}

fn json_thread<'a>(doc: &'a Path, text: &str, t: &'a Thread) -> JsonThread<'a> {
    let (l1, c1) = line_col(text, t.anchor.start);
    let (l2, c2) = line_col(text, t.anchor.end);
    JsonThread {
        doc,
        id: t.id,
        status: t.status,
        detached: t.anchor.detached,
        start: Pos { line: l1, column: c1 },
        end: Pos { line: l2, column: c2 },
        quote: &t.anchor.quote,
        messages: t
            .messages
            .iter()
            .map(|m| JsonMessage {
                at: m.at,
                body: &m.body,
            })
            .collect(),
        resolved_at: t.resolved_at,
    }
}

fn indent(s: &str, prefix: &str) -> String {
    s.lines()
        .map(|l| format!("{prefix}{l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn quote_line(q: &str) -> String {
    let flat: String = q.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 160 {
        let cut: String = flat.chars().take(157).collect();
        format!("\"{cut}…\"")
    } else {
        format!("\"{flat}\"")
    }
}

fn format_thread(out: &mut String, doc: &Path, text: &str, t: &Thread) {
    let (line, col) = line_col(text, t.anchor.start);
    let status = match (t.status, t.anchor.detached) {
        (Status::Open, false) => "open".to_string(),
        (Status::Open, true) => "open, detached: the commented text was deleted".to_string(),
        (Status::Resolved, _) => "resolved".to_string(),
    };
    out.push_str(&format!(
        "#{} {}:{}:{} ({})\n",
        t.id,
        display_path(doc),
        line,
        col,
        status
    ));
    out.push_str(&format!("  on {}\n", quote_line(&t.anchor.quote)));
    for (i, m) in t.messages.iter().enumerate() {
        let kind = if i == 0 { "comment" } else { "reply" };
        out.push_str(&format!("  {kind} · {}\n", when(&m.at)));
        out.push_str(&indent(&m.body, "    "));
        out.push('\n');
    }
}

fn threads_text(docs: &[(PathBuf, Comments, String)], include_resolved: bool) -> String {
    let mut out = String::new();
    for (doc, comments, text) in docs {
        let shown: Vec<&Thread> = comments
            .threads
            .iter()
            .filter(|t| include_resolved || t.is_open())
            .collect();
        let open = comments.open_count();
        let resolved = comments.threads.len() - open;
        let mut header = format!(
            "{}: {} open thread{}",
            display_path(doc),
            open,
            if open == 1 { "" } else { "s" }
        );
        if resolved > 0 {
            header.push_str(&format!(
                ", {resolved} resolved{}",
                if include_resolved { "" } else { " (--resolved to show)" }
            ));
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&header);
        out.push('\n');
        for t in shown {
            out.push('\n');
            format_thread(&mut out, doc, text, t);
        }
    }
    out
}

fn threads_json(docs: &[(PathBuf, Comments, String)], include_resolved: bool) -> Result<String> {
    let all: Vec<JsonThread> = docs
        .iter()
        .flat_map(|(doc, c, text)| {
            c.threads
                .iter()
                .filter(move |t| include_resolved || t.is_open())
                .map(move |t| json_thread(doc, text, t))
        })
        .collect();
    Ok(serde_json::to_string_pretty(&all)?)
}

/// Documents under `root` (or anywhere, with `all`) that have threads.
fn discovered_docs(all: bool, open_only: bool) -> Result<Vec<PathBuf>> {
    let cwd = std::env::current_dir()?;
    Ok(all_stores()?
        .into_iter()
        .filter(|(s, c)| {
            (all || s.doc.starts_with(&cwd))
                && s.doc.exists()
                && if open_only { c.open_count() > 0 } else { !c.threads.is_empty() }
        })
        .map(|(s, _)| s.doc)
        .collect())
}

fn load_many(files: &[PathBuf]) -> Result<Vec<(PathBuf, Comments, String)>> {
    files
        .iter()
        .map(|f| {
            let (store, c, text) = load(f)?;
            Ok((store.doc, c, text))
        })
        .collect()
}

fn print(s: &str) {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(s.as_bytes());
    if !s.ends_with('\n') {
        let _ = out.write_all(b"\n");
    }
}

pub fn run(cmd: Command) -> Result<i32> {
    match cmd {
        Command::Open { .. } => unreachable!("handled by main"),
        Command::Comments {
            files,
            resolved,
            all,
            json,
        } => {
            let files = if files.is_empty() {
                let found = discovered_docs(all, !resolved)?;
                if found.is_empty() && !json {
                    let scope = if all { "anywhere" } else { "under the current directory" };
                    print(&format!("No documents with open comments {scope}."));
                    return Ok(0);
                }
                found
            } else {
                files
            };
            let docs = load_many(&files)?;
            if json {
                print(&threads_json(&docs, resolved)?);
            } else {
                print(&threads_text(&docs, resolved));
            }
        }
        Command::Reply {
            file,
            id,
            message,
            resolve,
        } => {
            let (store, _, text) = load(&file)?;
            store.update(|c| {
                c.sync(&text);
                c.reply(id, &message)?;
                if resolve {
                    c.set_resolved(id, true)?;
                }
                Ok(())
            })?;
            print(&format!(
                "Replied to #{id}{}.",
                if resolve { " and resolved it" } else { "" }
            ));
        }
        Command::Resolve {
            file,
            id,
            message,
        } => {
            let (store, _, text) = load(&file)?;
            store.update(|c| {
                c.sync(&text);
                if let Some(m) = &message {
                    c.reply(id, m)?;
                }
                c.set_resolved(id, true)
            })?;
            print(&format!("Resolved #{id}."));
        }
        Command::Reopen { file, id } => {
            let (store, _, text) = load(&file)?;
            store.update(|c| {
                c.sync(&text);
                c.set_resolved(id, false)
            })?;
            print(&format!("Reopened #{id}."));
        }
        Command::Add {
            file,
            quote,
            occurrence,
            line,
            end_line,
            message,
        } => {
            let (store, _, text) = load(&file)?;
            let range = match (quote, line) {
                (Some(q), _) => find_occurrence(&text, &q, occurrence)?,
                (None, Some(l)) => line_range(&text, l, end_line.unwrap_or(l))?,
                (None, None) => bail!("say what to comment on with --quote TEXT or --line N"),
            };
            let (l, c) = line_col(&text, range.start);
            let id = store.update(|cm| Ok(cm.add(&text, range.clone(), &message)))?;
            print(&format!("Added #{id} at {}:{l}:{c}.", display_path(&store.doc)));
        }
        Command::Delete { file, id } => {
            let (store, _, _) = load(&file)?;
            store.update(|c| c.delete(id))?;
            print(&format!("Deleted #{id}."));
        }
    }
    Ok(0)
}

fn find_occurrence(text: &str, quote: &str, occurrence: Option<usize>) -> Result<std::ops::Range<usize>> {
    if quote.is_empty() {
        bail!("--quote must not be empty");
    }
    let hits: Vec<usize> = text.match_indices(quote).map(|(i, _)| i).collect();
    match (hits.len(), occurrence) {
        (0, _) => bail!(
            "the document does not contain {:?} (quote the Markdown source exactly, including ** and ` characters)",
            quote
        ),
        (_, Some(n)) if n >= 1 && n <= hits.len() => Ok(hits[n - 1]..hits[n - 1] + quote.len()),
        (_, Some(n)) => bail!("--occurrence {n} is out of range: found {} occurrences", hits.len()),
        (1, None) => Ok(hits[0]..hits[0] + quote.len()),
        (_, None) => {
            let lines: Vec<String> = hits
                .iter()
                .map(|&h| line_col(text, h).0.to_string())
                .collect();
            bail!(
                "{:?} occurs {} times (lines {}); pick one with --occurrence N",
                quote,
                hits.len(),
                lines.join(", ")
            )
        }
    }
}

fn line_range(text: &str, first: usize, last: usize) -> Result<std::ops::Range<usize>> {
    if last < first {
        bail!("--end-line is before --line");
    }
    let start = line_start(text, first).with_context(|| format!("line {first} is past the end"))?;
    let end_line_start = line_start(text, last).with_context(|| format!("line {last} is past the end"))?;
    let end = text[end_line_start..]
        .find('\n')
        .map_or(text.len(), |i| end_line_start + i);
    // Skip leading indentation so the anchor starts at the text.
    let lead = text[start..end].len() - text[start..end].trim_start().len();
    if start + lead >= end {
        bail!("line {first} is empty");
    }
    Ok(start + lead..end)
}
