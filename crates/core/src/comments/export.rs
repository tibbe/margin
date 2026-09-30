//! Open comments as a list to paste into a coding agent, the way
//! tuicr's `y` copies a review.

use super::anchor::line_col;
use super::store::{Author, Message, Thread};
use std::path::Path;

/// How items name the document: relative to its git repository when it is
/// in one (where agents usually run), else the absolute path.
pub fn item_path(doc: &Path) -> String {
    let root = doc.ancestors().skip(1).find(|d| d.join(".git").exists());
    match root.and_then(|r| doc.strip_prefix(r).ok()) {
        Some(rel) => rel.display().to_string(),
        None => doc.display().to_string(),
    }
}

/// `line:col-line:col` of a byte range, 1-based, ending at its last
/// character. Just `line:col` for an empty range.
fn span(text: &str, start: usize, end: usize) -> String {
    let start = start.min(text.len());
    let end = end.clamp(start, text.len());
    let (l1, c1) = line_col(text, start);
    let body = text.get(start..end).unwrap_or("").trim_end_matches('\n');
    match body.chars().last() {
        Some(last) => {
            let (l2, c2) = line_col(text, start + body.len() - last.len_utf8());
            format!("{l1}:{c1}-{l2}:{c2}")
        }
        None => format!("{l1}:{c1}"),
    }
}

fn short_quote(q: &str) -> String {
    let flat = q.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 60 {
        format!("{}…", flat.chars().take(59).collect::<String>())
    } else {
        flat
    }
}

/// Lines after the first, indented to stay inside the list item.
fn continued(body: &str, pad: &str) -> String {
    let mut lines = body.trim().lines();
    let mut out = lines.next().unwrap_or("").to_string();
    for l in lines {
        out.push('\n');
        if !l.trim().is_empty() {
            out.push_str(pad);
            out.push_str(l);
        }
    }
    out
}

/// A word the shell passes through as is, else single-quoted.
pub fn shell_word(s: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "-_./:@%+=,".contains(c);
    if !s.is_empty() && s.chars().all(plain) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// A message as a list item under its thread, after its author.
fn item(m: &Message) -> String {
    let who = match m.author {
        Author::User => "User",
        Author::Agent => "Agent",
    };
    format!("  - {who}: {}\n", continued(&m.body, "    "))
}

/// The threads (anchored against `text`, the document now) as a list in
/// document order, each under its thread number, with where it is. A
/// thread's comment and latest message are given in full, each after its
/// author; the replies between, which the agent has seen on earlier
/// copies, become a `margin thread` command that prints them.
pub fn for_agent(doc: &Path, text: &str, threads: &[Thread]) -> String {
    let name = item_path(doc);
    let mut threads: Vec<&Thread> = threads.iter().collect();
    threads.sort_by_key(|t| (t.anchor.start(), t.id));
    let mut out = format!(
        "I left comments on `{}`. Please address them.\n\n",
        doc.display()
    );
    for t in threads {
        let a = &t.anchor;
        let at = match a.range() {
            Some(r) => format!(
                "`{name}:{}` \"{}\"",
                span(text, r.start, r.end),
                short_quote(a.quote())
            ),
            None => format!(
                "`{name}:{}` (the commented text, \"{}\", was deleted)",
                span(text, a.start(), a.start()),
                short_quote(a.quote())
            ),
        };
        out.push_str(&format!("#{} {at}\n", t.id));
        if let Some(m) = t.messages.first() {
            out.push_str(&item(m));
        }
        let replies = t.messages.get(1..).unwrap_or_default();
        let shown = match replies.split_last() {
            Some((last, earlier)) if !earlier.is_empty() => {
                let n = earlier.len();
                out.push_str(&format!(
                    "  - ({n} earlier repl{}: `margin thread {} {}`)\n",
                    if n == 1 { "y" } else { "ies" },
                    shell_word(&name),
                    t.id
                ));
                std::slice::from_ref(last)
            }
            _ => replies,
        };
        for m in shown {
            out.push_str(&item(m));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comments::{Author, Comments};

    #[test]
    fn numbered_in_document_order_with_line_and_column() {
        let text = "# Plan\n\nSome **bold words** here.\nNext line with a naïve idea.\n";
        let mut c = Comments::new("/nowhere/plan.md".into());
        let naive = text.find("naïve idea").unwrap();
        c.add(
            text,
            naive..naive + "naïve idea".len(),
            "Say more.\n\nWhat changes?",
            Author::Agent,
        )
        .unwrap();
        let bold = text.find("bold words").unwrap();
        let id = c
            .add(text, bold..bold + 10, "Italic instead?", Author::User)
            .unwrap();
        c.reply(id, "Done.", Author::Agent).unwrap();
        assert_eq!(
            for_agent(Path::new("/nowhere/plan.md"), text, &c.threads),
            "I left comments on `/nowhere/plan.md`. Please address them.\n\n\
             #2 `/nowhere/plan.md:3:8-3:17` \"bold words\"\n  \
             - User: Italic instead?\n  \
             - Agent: Done.\n\
             #1 `/nowhere/plan.md:4:18-4:27` \"naïve idea\"\n  \
             - Agent: Say more.\n\n    What changes?\n"
        );
    }

    #[test]
    fn spans_and_deleted_text() {
        let text = "one\ntwo\n";
        assert_eq!(span(text, 0, 8), "1:1-2:3");
        assert_eq!(span(text, 4, 4), "2:1");
        let mut c = Comments::new("/x/a.md".into());
        c.add(text, 4..7, "Why?", Author::User).unwrap();
        c.sync("one\n");
        let out = for_agent(Path::new("/x/a.md"), "one\n", &c.threads);
        assert!(
            out.contains(
                "#1 `/x/a.md:2:1` (the commented text, \"two\", was deleted)\n  - User: Why?"
            ),
            "{out}"
        );
    }

    #[test]
    fn replies_before_the_latest_are_left_to_margin_thread() {
        let text = "one two\n";
        let mut c = Comments::new("/x/my plan.md".into());
        let id = c.add(text, 0..3, "Why?", Author::User).unwrap();
        for (r, a) in [
            ("Because.", Author::Agent),
            ("Not enough.", Author::User),
            ("Rewrote it.", Author::Agent),
            ("Better,\nbut shorter?", Author::User),
        ] {
            c.reply(id, r, a).unwrap();
        }
        let short = c.add(text, 4..7, "Typo?", Author::User).unwrap();
        c.reply(short, "Fixed.", Author::Agent).unwrap();
        assert_eq!(
            for_agent(Path::new("/x/my plan.md"), text, &c.threads),
            "I left comments on `/x/my plan.md`. Please address them.\n\n\
             #1 `/x/my plan.md:1:1-1:3` \"one\"\n  \
             - User: Why?\n  \
             - (3 earlier replies: `margin thread '/x/my plan.md' 1`)\n  \
             - User: Better,\n    but shorter?\n\
             #2 `/x/my plan.md:1:5-1:7` \"two\"\n  \
             - User: Typo?\n  \
             - Agent: Fixed.\n"
        );
    }

    #[test]
    fn shell_words() {
        assert_eq!(shell_word("docs/plan-2.md"), "docs/plan-2.md");
        assert_eq!(shell_word("it's here.md"), "'it'\\''s here.md'");
    }
}
