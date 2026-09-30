//! What agents did to a document's threads between two reads of its store:
//! for the window's announcement and for system notifications.

use super::store::Thread;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A new thread; its comment is the message.
    Added,
    Replied,
    /// Resolved, with the reply that came with it, if any.
    Resolved,
    /// Reopened, with the reply that came with it, if any.
    Reopened,
    Deleted,
}

/// One thing that happened to one thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub id: u64,
    pub kind: Kind,
    /// The commented text, as last seen.
    pub quote: String,
    pub message: Option<String>,
}

/// What changed from `old` to `new`, in thread order, deletions last.
/// Each new reply is its own change, except that the last one goes with a
/// resolve or reopen made at the same time (`margin reply --resolve`).
/// Changes to anchors and edits of existing messages are not activity.
pub fn changes(old: &[Thread], new: &[Thread]) -> Vec<Change> {
    let mut out = Vec::new();
    for t in new {
        let change = |kind, message: Option<&str>| Change {
            id: t.id,
            kind,
            quote: t.anchor.quote().to_string(),
            message: message.map(str::to_string),
        };
        let Some(o) = old.iter().find(|o| o.id == t.id) else {
            out.push(change(
                Kind::Added,
                t.messages.first().map(|m| m.body.as_str()),
            ));
            continue;
        };
        let mut replies: Vec<&str> = t
            .messages
            .iter()
            .skip(o.messages.len())
            .map(|m| m.body.as_str())
            .collect();
        let status = match (o.is_open(), t.is_open()) {
            (true, false) => Some(Kind::Resolved),
            (false, true) => Some(Kind::Reopened),
            _ => None,
        };
        let with_status = if status.is_some() {
            replies.pop()
        } else {
            None
        };
        out.extend(replies.into_iter().map(|r| change(Kind::Replied, Some(r))));
        if let Some(kind) = status {
            out.push(change(kind, with_status));
        }
    }
    for o in old.iter().filter(|o| !new.iter().any(|t| t.id == o.id)) {
        out.push(Change {
            id: o.id,
            kind: Kind::Deleted,
            quote: o.anchor.quote().to_string(),
            message: None,
        });
    }
    out
}

/// The window's announcement: "1 new reply, 2 comments resolved".
/// Empty when nothing changed.
pub fn summary(changes: &[Change]) -> String {
    let count = |f: fn(&Change) -> bool| changes.iter().filter(|c| f(c)).count();
    let replies = count(|c| {
        c.kind == Kind::Replied
            || (c.message.is_some() && matches!(c.kind, Kind::Resolved | Kind::Reopened))
    });
    let parts = [
        (
            count(|c| c.kind == Kind::Added),
            "new comment",
            "new comments",
        ),
        (replies, "new reply", "new replies"),
        (
            count(|c| c.kind == Kind::Resolved),
            "comment resolved",
            "comments resolved",
        ),
        (
            count(|c| c.kind == Kind::Reopened),
            "comment reopened",
            "comments reopened",
        ),
        (
            count(|c| c.kind == Kind::Deleted),
            "comment deleted",
            "comments deleted",
        ),
    ];
    parts
        .iter()
        .filter(|(n, ..)| *n > 0)
        .map(|&(n, one, many)| {
            if n == 1 {
                format!("1 {one}")
            } else {
                format!("{n} {many}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

impl Change {
    /// What happened to which thread, for a notification's subtitle:
    /// `Resolved "quote"`. Just the quote for a reply.
    pub fn headline(&self) -> String {
        let quote = format!(
            "\u{201c}{}\u{201d}",
            self.quote.split_whitespace().collect::<Vec<_>>().join(" ")
        );
        match self.kind {
            Kind::Added => format!("New comment on {quote}"),
            Kind::Replied => quote,
            Kind::Resolved => format!("Resolved {quote}"),
            Kind::Reopened => format!("Reopened {quote}"),
            Kind::Deleted => format!("Deleted {quote}"),
        }
    }

    /// The headline and the message on one line, for platforms whose
    /// notifications have no subtitle: `Resolved "quote": Done.`
    pub fn line(&self) -> String {
        match &self.message {
            Some(m) => format!("{}: {m}", self.headline()),
            None => self.headline(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comments::{Author, Comments};
    use std::path::PathBuf;

    fn doc() -> (Comments, &'static str) {
        let text = "One two three.\n";
        let mut c = Comments::new(PathBuf::from("/d.md"));
        c.add(text, 0..3, "Why?", Author::User).unwrap();
        c.add(text, 4..7, "And this?", Author::User).unwrap();
        (c, text)
    }

    #[test]
    fn nothing_changed() {
        let (c, _) = doc();
        assert!(changes(&c.threads, &c.threads).is_empty());
        assert_eq!(summary(&[]), "");
    }

    #[test]
    fn each_reply_is_a_change() {
        let (old, _) = doc();
        let mut new = old.clone();
        new.reply(1, "Because.", Author::Agent).unwrap();
        new.reply(1, "Also.", Author::Agent).unwrap();
        let cs = changes(&old.threads, &new.threads);
        assert_eq!(
            cs.iter()
                .map(|c| (c.id, c.kind, c.message.as_deref()))
                .collect::<Vec<_>>(),
            [
                (1, Kind::Replied, Some("Because.")),
                (1, Kind::Replied, Some("Also.")),
            ]
        );
        assert_eq!(summary(&cs), "2 new replies");
        assert_eq!(cs[0].headline(), "\u{201c}One\u{201d}");
    }

    #[test]
    fn a_reply_goes_with_its_resolve() {
        let (old, _) = doc();
        let mut new = old.clone();
        new.reply(1, "Done.", Author::Agent).unwrap();
        new.set_resolved(1, true).unwrap();
        new.set_resolved(2, true).unwrap();
        let cs = changes(&old.threads, &new.threads);
        assert_eq!(cs.len(), 2);
        assert_eq!(cs[0].line(), "Resolved \u{201c}One\u{201d}: Done.");
        assert_eq!(cs[1].line(), "Resolved \u{201c}two\u{201d}");
        assert_eq!(summary(&cs), "1 new reply, 2 comments resolved");
    }

    #[test]
    fn added_reopened_and_deleted() {
        let (mut old, text) = doc();
        old.set_resolved(1, true).unwrap();
        let mut new = old.clone();
        new.set_resolved(1, false).unwrap();
        new.delete(2).unwrap();
        new.add(text, 8..13, "Plural?", Author::User).unwrap();
        let cs = changes(&old.threads, &new.threads);
        assert_eq!(
            cs.iter().map(|c| c.headline()).collect::<Vec<_>>(),
            [
                "Reopened \u{201c}One\u{201d}",
                "New comment on \u{201c}three\u{201d}",
                "Deleted \u{201c}two\u{201d}",
            ]
        );
        assert_eq!(cs[1].message.as_deref(), Some("Plural?"));
        assert_eq!(
            summary(&cs),
            "1 new comment, 1 comment reopened, 1 comment deleted"
        );
    }

    #[test]
    fn edits_and_anchors_are_not_activity() {
        let (old, _) = doc();
        let mut new = old.clone();
        new.edit(1, 0, "Why not?").unwrap();
        let text = "One two three.\n";
        new.thread_mut(2)
            .unwrap()
            .anchor
            .follow(text, crate::comments::Place::On(4..13));
        assert!(changes(&old.threads, &new.threads).is_empty());
    }

    #[test]
    fn headlines_keep_quotes_on_one_line() {
        let c = Change {
            id: 1,
            kind: Kind::Replied,
            quote: "a\n  b".into(),
            message: None,
        };
        assert_eq!(c.headline(), "\u{201c}a b\u{201d}");
    }
}
