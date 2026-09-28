//! Open comments as a numbered list to paste into a coding agent, the way
//! tuicr's `y` copies a review.

use super::anchor::line_col;
use super::store::Thread;
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

/// The threads (anchored against `text`, the document now) as a numbered
/// Markdown list in document order, with where each one is.
pub fn for_agent(doc: &Path, text: &str, threads: &[Thread]) -> String {
    let name = item_path(doc);
    let mut threads: Vec<&Thread> = threads.iter().collect();
    threads.sort_by_key(|t| (t.anchor.start, t.id));
    let mut out = format!(
        "I left comments on `{}`. Please address them.\n\n",
        doc.display()
    );
    for (i, t) in threads.iter().enumerate() {
        let n = i + 1;
        let pad = " ".repeat(n.to_string().len() + 2);
        let a = &t.anchor;
        let at = if a.detached {
            format!(
                "`{name}:{}` (the commented text, \"{}\", was deleted)",
                span(text, a.start, a.start),
                short_quote(&a.quote)
            )
        } else {
            format!("`{name}:{}` \"{}\"", span(text, a.start, a.end), short_quote(&a.quote))
        };
        let mut messages = t.messages.iter();
        let body = messages.next().map_or(String::new(), |m| continued(&m.body, &pad));
        out.push_str(&format!("{n}. {at}: {body}\n"));
        for m in messages {
            let reply_pad = format!("{pad}  ");
            out.push_str(&format!("{pad}- Reply: {}\n", continued(&m.body, &reply_pad)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comments::Comments;

    #[test]
    fn numbered_in_document_order_with_line_and_column() {
        let text = "# Plan\n\nSome **bold words** here.\nNext line with a naïve idea.\n";
        let mut c = Comments::new("/nowhere/plan.md".into());
        let naive = text.find("naïve idea").unwrap();
        c.add(text, naive..naive + "naïve idea".len(), "Say more.\n\nWhat changes?");
        let bold = text.find("bold words").unwrap();
        let id = c.add(text, bold..bold + 10, "Italic instead?");
        c.reply(id, "Done.").unwrap();
        assert_eq!(
            for_agent(Path::new("/nowhere/plan.md"), text, &c.threads),
            "I left comments on `/nowhere/plan.md`. Please address them.\n\n\
             1. `/nowhere/plan.md:3:8-3:17` \"bold words\": Italic instead?\n   \
             - Reply: Done.\n\
             2. `/nowhere/plan.md:4:18-4:27` \"naïve idea\": Say more.\n\n   What changes?\n"
        );
    }

    #[test]
    fn spans_and_deleted_text() {
        let text = "one\ntwo\n";
        assert_eq!(span(text, 0, 8), "1:1-2:3");
        assert_eq!(span(text, 4, 4), "2:1");
        let mut c = Comments::new("/x/a.md".into());
        c.add(text, 4..7, "Why?");
        c.sync("one\n");
        let out = for_agent(Path::new("/x/a.md"), "one\n", &c.threads);
        assert!(out.contains("1. `/x/a.md:2:1` (the commented text, \"two\", was deleted): Why?"), "{out}");
    }
}
