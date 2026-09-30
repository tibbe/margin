//! The command-line interface: opening documents in the app, and the
//! commands coding agents use to read and answer comments.

use anyhow::{Result, bail};
use chrono::{DateTime, Local, Utc};
use clap::{Parser, Subcommand};
use margin_core::comments::anchor::line_col;
use margin_core::comments::export::{for_agent, shell_word};
use margin_core::comments::handoff::Waiter;
use margin_core::comments::{Author, Comments, Store, Thread, all_stores, read_doc};
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};

const AFTER_HELP: &str = "\
Agent workflow:
  margin comments plan.md          read the open threads on a document
  margin thread plan.md 3          read one thread, with all its replies
  margin reply plan.md 3 \"Done.\" --resolve
  margin add plan.md --quote \"retry budget\" \"Is 3 enough?\"
  margin wait plan.md              wait until the writer sends the next round

Threads are numbered per document. Locations are file:line:column, 1-based.
Each message's author is \"user\" (from the editor) or \"agent\" (from margin).";

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
    /// Open documents in the editor (what `margin FILE…` does). Returns
    /// once the editor shows them.
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

    /// Show one thread, open or resolved.
    Thread {
        file: PathBuf,
        id: u64,
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
        /// Exact text to comment on (Markdown source, as in the file),
        /// occurring once in it.
        #[arg(long)]
        quote: String,
        message: String,
    },

    /// Delete a thread.
    Delete { file: PathBuf, id: u64 },

    /// Wait until the writer sends the comments on one of the documents
    /// (Send to Agent in the editor), then print them and exit. Also exits
    /// when the editor doesn't show any of the documents, since then no
    /// comments can come. Either way, it prints what to do next. Run it in
    /// the background if you can, to keep working while you wait.
    Wait {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        #[arg(long)]
        json: bool,
    },
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
    author: Author,
    at: DateTime<Utc>,
    body: &'a str,
}

#[derive(Serialize)]
struct JsonThread<'a> {
    doc: &'a Path,
    id: u64,
    /// `open` or `resolved`.
    status: &'static str,
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
    let range = t
        .anchor
        .range()
        .unwrap_or(t.anchor.start()..t.anchor.start());
    let (l1, c1) = line_col(text, range.start);
    let (l2, c2) = line_col(text, range.end);
    JsonThread {
        doc,
        id: t.id,
        status: if t.is_open() { "open" } else { "resolved" },
        detached: t.anchor.is_detached(),
        start: Pos {
            line: l1,
            column: c1,
        },
        end: Pos {
            line: l2,
            column: c2,
        },
        quote: t.anchor.quote(),
        messages: t
            .messages
            .iter()
            .map(|m| JsonMessage {
                author: m.author,
                at: m.at,
                body: &m.body,
            })
            .collect(),
        resolved_at: t.resolved_at(),
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
    let (line, col) = line_col(text, t.anchor.start());
    let status = match (t.is_open(), t.anchor.is_detached()) {
        (true, false) => "open",
        (true, true) => "open, detached: the commented text was deleted",
        (false, _) => "resolved",
    };
    out.push_str(&format!(
        "#{} {}:{}:{} ({})\n",
        t.id,
        display_path(doc),
        line,
        col,
        status
    ));
    out.push_str(&format!("  on {}\n", quote_line(t.anchor.quote())));
    for m in &t.messages {
        let who = match m.author {
            Author::User => "user",
            Author::Agent => "agent",
        };
        out.push_str(&format!("  {who} · {}\n", when(&m.at)));
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
                if include_resolved {
                    ""
                } else {
                    " (--resolved to show)"
                }
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
    let all: Vec<JsonThread<'_>> = docs
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
                && if open_only {
                    c.open_count() > 0
                } else {
                    !c.threads.is_empty()
                }
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

/// Why `margin wait` stops without a send: no editor shows its documents,
/// since it started or, when `closed`, any more.
fn nothing_to_wait_for(docs: &[&Path], args: &str, closed: bool) -> String {
    let names = docs
        .iter()
        .map(|d| format!("`{}`", display_path(d)))
        .collect::<Vec<_>>()
        .join(", ");
    let (is, it) = if docs.len() == 1 {
        ("is", "it")
    } else {
        ("are", "them")
    };
    if closed {
        format!(
            "{names} {is} no longer open in Margin, so no comments will come. \
             Stop waiting: the writer will ask if they want another review."
        )
    } else {
        format!(
            "{names} {is} not open in Margin, so no comments will come. \
             If the writer wants to review {it}, run `margin open {args}`, then `margin wait {args}`."
        )
    }
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
                    let scope = if all {
                        "anywhere"
                    } else {
                        "under the current directory"
                    };
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
        Command::Thread { file, id, json } => {
            let (store, comments, text) = load(&file)?;
            let Some(t) = comments.thread(id) else {
                bail!("no comment #{id} on {}", store.doc.display());
            };
            if json {
                print(&serde_json::to_string_pretty(&json_thread(
                    &store.doc, &text, t,
                ))?);
            } else {
                let mut out = String::new();
                format_thread(&mut out, &store.doc, &text, t);
                print(&out);
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
                c.reply(id, &message, Author::Agent)?;
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
        Command::Resolve { file, id, message } => {
            let (store, _, text) = load(&file)?;
            store.update(|c| {
                c.sync(&text);
                if let Some(m) = &message {
                    c.reply(id, m, Author::Agent)?;
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
            message,
        } => {
            let (store, _, text) = load(&file)?;
            let range = find_quote(&text, &quote)?;
            let (l, c) = line_col(&text, range.start);
            let id = store.update(|cm| cm.add(&text, range.clone(), &message, Author::Agent))?;
            print(&format!(
                "Added #{id} at {}:{l}:{c}.",
                display_path(&store.doc)
            ));
        }
        Command::Wait { files, json } => {
            let waiters = files
                .iter()
                .map(|f| {
                    let w = Waiter::start(f)?;
                    if !w.doc().exists() {
                        bail!("{} does not exist", w.doc().display());
                    }
                    Ok(w)
                })
                .collect::<Result<Vec<_>>>()?;
            let args: Vec<String> = files
                .iter()
                .map(|f| shell_word(&f.display().to_string()))
                .collect();
            let args = args.join(" ");
            let mut shown_before = false;
            let sent = loop {
                let shown = waiters.iter().any(Waiter::shown);
                // Looked at after `shown`: a window sends before it closes,
                // so a closed window's last send shows up here.
                let sent: Vec<&Waiter> = waiters.iter().filter(|w| w.sent()).collect();
                if !sent.is_empty() {
                    break sent;
                }
                if !shown {
                    let docs: Vec<&Path> = waiters.iter().map(Waiter::doc).collect();
                    let why = nothing_to_wait_for(&docs, &args, shown_before);
                    if json {
                        print("[]");
                        eprintln!("{why}");
                    } else {
                        print(&why);
                    }
                    return Ok(0);
                }
                shown_before = true;
                std::thread::sleep(std::time::Duration::from_millis(250));
            };
            let docs = load_many(
                &sent
                    .iter()
                    .map(|w| w.doc().to_path_buf())
                    .collect::<Vec<_>>(),
            )?;
            if json {
                print(&threads_json(&docs, false)?);
            } else {
                let mut out = String::new();
                for (doc, c, text) in &docs {
                    let open: Vec<Thread> =
                        c.threads.iter().filter(|t| t.is_open()).cloned().collect();
                    out.push_str(&for_agent(doc, text, &open));
                    out.push('\n');
                }
                out.push_str(&format!(
                    "Once you have answered them, run `margin wait {args}` again for the next round."
                ));
                print(&out);
            }
        }
        Command::Delete { file, id } => {
            let (store, _, _) = load(&file)?;
            store.update(|c| c.delete(id))?;
            print(&format!("Deleted #{id}."));
        }
    }
    Ok(0)
}

fn find_quote(text: &str, quote: &str) -> Result<std::ops::Range<usize>> {
    if quote.is_empty() {
        bail!("--quote must not be empty");
    }
    let hits: Vec<usize> = text.match_indices(quote).map(|(i, _)| i).collect();
    match hits.len() {
        0 => bail!(
            "the document does not contain {:?} (quote the Markdown source exactly, including ** and ` characters)",
            quote
        ),
        1 => Ok(hits[0]..hits[0] + quote.len()),
        n => {
            let lines: Vec<String> = hits
                .iter()
                .map(|&h| line_col(text, h).0.to_string())
                .collect();
            bail!(
                "{:?} occurs {n} times (lines {}); quote more of the text around it so it occurs once",
                quote,
                lines.join(", ")
            )
        }
    }
}
