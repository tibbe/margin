//! What changed between two versions of a text, so that an editor applying
//! the edits keeps its cursor and marks on unchanged text, and comment
//! anchors can follow their text (see `comments::anchor::OffsetMap`).

use crate::md::edit::Change;
use similar::{Algorithm, DiffTag, TextDiff};
use std::ops::Range;
use std::time::Duration;
use unicode_segmentation::UnicodeSegmentation;

/// A stretch of the old text and the stretch of the new text it became:
/// the same text, kept, or an edit. An edit with nothing in `old` is an
/// insertion; with nothing in `new`, a deletion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    pub kept: bool,
    pub old: Range<usize>,
    pub new: Range<usize>,
}

/// How `old` becomes `new`, as pieces covering both in order, kept text
/// and edits taking turns. Lines are compared first, then the words of the
/// lines that changed, whole words only, as Unicode splits them: letters
/// and digits that run together stay together and punctuation stands
/// alone, so the `**` around a changed word is kept, while a rewritten word
/// is replaced whole, never matched letter by letter with whatever replaced
/// it. Both steps use git's histogram diff, which matches rare lines and
/// words first.
pub fn diff_pieces(old: &str, new: &str) -> Vec<Piece> {
    let old_lines: Vec<&str> = old.split_inclusive('\n').collect();
    let new_lines: Vec<&str> = new.split_inclusive('\n').collect();
    let mut pieces = Vec::new();
    for p in diff_tokens(&old_lines, &new_lines, Duration::from_millis(300)) {
        let (o, n) = (&p.old, &p.new);
        if p.kept || o.is_empty() || n.is_empty() || o.len() > 8192 || n.len() > 8192 {
            push(&mut pieces, p);
            continue;
        }
        let old_words: Vec<&str> = old[o.clone()].split_word_bounds().collect();
        let new_words: Vec<&str> = new[n.clone()].split_word_bounds().collect();
        let refined = diff_tokens(&old_words, &new_words, Duration::from_millis(50));
        for w in refined {
            push(
                &mut pieces,
                Piece {
                    kept: w.kept,
                    old: o.start + w.old.start..o.start + w.old.end,
                    new: n.start + w.new.start..n.start + w.new.end,
                },
            );
        }
    }
    pieces
}

/// The replacements turning `old` into `new`, as byte ranges of `old`: the
/// edits of [`diff_pieces`].
pub fn diff_changes(old: &str, new: &str) -> Vec<Change> {
    diff_pieces(old, new)
        .into_iter()
        .filter(|p| !p.kept)
        .map(|p| Change::replace(p.old, &new[p.new]))
        .collect()
}

/// Diffs two texts split into tokens, as pieces with byte ranges.
fn diff_tokens(old: &[&str], new: &[&str], timeout: Duration) -> Vec<Piece> {
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Histogram)
        .timeout(timeout)
        .diff_slices(old, new);
    let so = offsets(old);
    let sn = offsets(new);
    let mut pieces = Vec::new();
    for op in diff.ops() {
        let (t, a, b) = op.as_tag_tuple();
        push(
            &mut pieces,
            Piece {
                kept: t == DiffTag::Equal,
                old: so[a.start]..so[a.end],
                new: sn[b.start]..sn[b.end],
            },
        );
    }
    pieces
}

/// Adds `p` to `pieces`, joining it to the last if both are kept text or
/// both edits, so that they take turns.
fn push(pieces: &mut Vec<Piece>, p: Piece) {
    if p.old.is_empty() && p.new.is_empty() {
        return;
    }
    match pieces.last_mut() {
        Some(last) if last.kept == p.kept => {
            last.old.end = p.old.end;
            last.new.end = p.new.end;
        }
        _ => pieces.push(p),
    }
}

/// Where each token starts, and where the last ends.
fn offsets(tokens: &[&str]) -> Vec<usize> {
    let mut v = vec![0];
    for t in tokens {
        v.push(v.last().unwrap() + t.len());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewritten_words_are_replaced_whole() {
        // Not letter by letter: "insensitive" and "disabled" share "i",
        // "s" and "e", which must not keep their places.
        assert_eq!(
            diff_changes("is insensitive unless\n", "is disabled while\n"),
            [
                Change::replace(3..14, "disabled"),
                Change::replace(15..21, "while")
            ]
        );
    }
}
