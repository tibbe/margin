//! Keeping comment anchors attached to their text while the document
//! changes underneath them.

use serde::{Deserialize, Serialize};
use similar::{Algorithm, DiffTag, TextDiff};
use std::ops::Range;
use std::time::Duration;

/// Where a thread's text is, as byte offsets into the document.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Place {
    /// On this range of the text. Never empty in an [`Anchor`].
    On(Range<usize>),
    /// The commented text was deleted; this is where it was.
    Detached(usize),
}

impl Place {
    /// Where the text starts, or was.
    pub fn start(&self) -> usize {
        match self {
            Place::On(r) => r.start,
            Place::Detached(at) => *at,
        }
    }
}

/// A thread's place in the document and the text it comments on. The
/// fields are private, so every anchor keeps two rules: one on text has a
/// non-empty range, and its quote is the text it was last placed on.
/// (Loading a stored anchor checks only the first.)
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(try_from = "StoredAnchor")]
pub struct Anchor {
    place: Place,
    /// The commented text, as last seen.
    quote: String,
}

/// An anchor as stored, checked on the way in.
#[derive(Deserialize)]
struct StoredAnchor {
    place: Place,
    quote: String,
}

impl TryFrom<StoredAnchor> for Anchor {
    type Error = String;

    fn try_from(a: StoredAnchor) -> Result<Anchor, String> {
        match a.place {
            Place::On(r) if r.is_empty() => Err(format!("anchor on nothing at {}", r.start)),
            place => Ok(Anchor { place, quote: a.quote }),
        }
    }
}

impl Anchor {
    /// On `range` of `text`; `None` when the range is empty (or not in
    /// `text`), since there is nothing there to comment on.
    pub fn on(text: &str, range: Range<usize>) -> Option<Anchor> {
        let quote = text.get(range.clone()).filter(|q| !q.is_empty())?.to_string();
        Some(Anchor { place: Place::On(range), quote })
    }

    /// Deleted text, remembered by its quote, that was at `at`.
    pub fn detached(at: usize, quote: String) -> Anchor {
        Anchor { place: Place::Detached(at), quote }
    }

    /// At `place` in `text`, with `quote` remembered in case it is
    /// detached.
    pub fn at(text: &str, place: Place, quote: String) -> Anchor {
        let mut a = Anchor::detached(place.start(), quote);
        a.follow(text, place);
        a
    }

    pub fn place(&self) -> &Place {
        &self.place
    }

    pub fn quote(&self) -> &str {
        &self.quote
    }

    /// The text's range, or `None` if it was deleted.
    pub fn range(&self) -> Option<Range<usize>> {
        match &self.place {
            Place::On(r) => Some(r.clone()),
            Place::Detached(_) => None,
        }
    }

    /// Where the text starts, or was.
    pub fn start(&self) -> usize {
        self.place.start()
    }

    pub fn is_detached(&self) -> bool {
        matches!(self.place, Place::Detached(_))
    }

    /// Moves the anchor to `now`, where its text is in `text` (the
    /// document now). On text, the quote becomes that text; an empty range
    /// detaches it, and a detached anchor keeps its quote.
    pub fn follow(&mut self, text: &str, now: Place) {
        match now {
            Place::On(r) => match text.get(r.clone()).filter(|q| !q.is_empty()) {
                Some(q) => {
                    self.quote = q.to_string();
                    self.place = Place::On(r);
                }
                None => self.place = Place::Detached(r.start.min(text.len())),
            },
            Place::Detached(at) => self.place = Place::Detached(at.min(text.len())),
        }
    }
}

/// Maps byte offsets in an old version of a document to a new version.
pub struct OffsetMap {
    /// Diff operations as byte ranges: (tag, old range, new range).
    ops: Vec<(DiffTag, Range<usize>, Range<usize>)>,
    new_len: usize,
}

impl OffsetMap {
    pub fn new(old: &str, new: &str) -> Self {
        let diff = TextDiff::configure()
            .algorithm(Algorithm::Myers)
            .timeout(Duration::from_millis(250))
            .diff_words(old, new);
        let old_offsets = offsets(diff.iter_old_slices().map(str::len));
        let new_offsets = offsets(diff.iter_new_slices().map(str::len));
        let mut ops = Vec::new();
        for op in diff.ops() {
            let (tag, o, n) = op.as_tag_tuple();
            let o = old_offsets[o.start]..old_offsets[o.end];
            let n = new_offsets[n.start]..new_offsets[n.end];
            // Refine replaced words character by character, so that
            // `**blue**` → `**green**` keeps the `**` as unchanged text.
            if tag == DiffTag::Replace && o.len() <= 4096 && n.len() <= 4096 {
                let sub = TextDiff::configure()
                    .algorithm(Algorithm::Myers)
                    .timeout(Duration::from_millis(50))
                    .diff_chars(&old[o.clone()], &new[n.clone()]);
                let so = offsets(sub.iter_old_slices().map(str::len));
                let sn = offsets(sub.iter_new_slices().map(str::len));
                for sop in sub.ops() {
                    let (t, a, b) = sop.as_tag_tuple();
                    ops.push((t, o.start + so[a.start]..o.start + so[a.end], n.start + sn[b.start]..n.start + sn[b.end]));
                }
            } else {
                ops.push((tag, o, n));
            }
        }
        OffsetMap {
            ops,
            new_len: new.len(),
        }
    }

    /// Maps the start of a range. Text inserted exactly at `p` is excluded;
    /// text that replaced what started at `p` is included.
    pub fn map_start(&self, p: usize) -> usize {
        for (tag, o, n) in &self.ops {
            if *tag == DiffTag::Insert {
                continue;
            }
            if o.start <= p && p < o.end {
                return match tag {
                    DiffTag::Equal => n.start + (p - o.start),
                    _ => n.start,
                };
            }
        }
        self.new_len
    }

    /// Maps the end of a range. Text inserted exactly at `p` is excluded;
    /// text that replaced what ended at `p` is included.
    pub fn map_end(&self, p: usize) -> usize {
        for (tag, o, n) in &self.ops {
            if *tag == DiffTag::Insert {
                continue;
            }
            if o.start < p && p <= o.end {
                return match tag {
                    DiffTag::Equal => n.start + (p - o.start),
                    _ => n.end,
                };
            }
        }
        0
    }

    /// Maps a place: text all of which was deleted is detached where it
    /// was.
    pub fn map(&self, p: &Place) -> Place {
        match p {
            Place::On(r) => {
                let start = self.map_start(r.start);
                let end = self.map_end(r.end);
                if start < end { Place::On(start..end) } else { Place::Detached(start.min(self.new_len)) }
            }
            Place::Detached(at) => Place::Detached(self.map_start(*at).min(self.new_len)),
        }
    }
}

fn offsets(lens: impl Iterator<Item = usize>) -> Vec<usize> {
    let mut v = vec![0];
    let mut acc = 0;
    for l in lens {
        acc += l;
        v.push(acc);
    }
    v
}

/// Finds `quote` in `text`, preferring the occurrence nearest `hint`.
pub fn find_quote(text: &str, quote: &str, hint: usize) -> Option<Range<usize>> {
    if quote.is_empty() {
        return None;
    }
    text.match_indices(quote)
        .map(|(i, _)| i)
        .min_by_key(|i| i.abs_diff(hint))
        .map(|i| i..i + quote.len())
}

/// 1-based line and column (in characters) of a byte offset.
pub fn line_col(text: &str, pos: usize) -> (usize, usize) {
    let pos = floor_char_boundary(text, pos.min(text.len()));
    let before = &text[..pos];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let col = text[line_start..pos].chars().count() + 1;
    (line, col)
}

/// Byte offset of a 1-based line's start.
pub fn line_start(text: &str, line: usize) -> Option<usize> {
    if line == 0 {
        return None;
    }
    if line == 1 {
        return Some(0);
    }
    text.match_indices('\n').nth(line - 2).map(|(i, _)| i + 1)
}

pub fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapped(old: &str, new: &str, quote: &str) -> Result<String, usize> {
        let s = old.find(quote).unwrap();
        let m = OffsetMap::new(old, new);
        match m.map(&Place::On(s..s + quote.len())) {
            Place::On(r) => Ok(new[r].to_string()),
            Place::Detached(at) => Err(at),
        }
    }

    #[test]
    fn survives_edits_elsewhere() {
        assert_eq!(
            mapped("one two three four", "zero one two three four five", "two three"),
            Ok("two three".into())
        );
        assert_eq!(
            mapped("a b c d", "a c d", "c d"),
            Ok("c d".into())
        );
    }

    #[test]
    fn follows_replacement() {
        assert_eq!(
            mapped("use blue/green deploys", "use canary deploys", "blue/green"),
            Ok("canary".into())
        );
    }

    #[test]
    fn follows_replacement_inside_markup() {
        assert_eq!(
            mapped("use **blue/green** now", "use **canary** now", "blue/green"),
            Ok("canary".into())
        );
    }

    #[test]
    fn grows_with_inner_insertions() {
        assert_eq!(
            mapped("the quick fox jumps", "the quick brown fox jumps", "quick fox"),
            Ok("quick brown fox".into())
        );
    }

    #[test]
    fn excludes_insertions_at_edges() {
        assert_eq!(mapped("a b c", "a X b Y c", "b"), Ok("b".into()));
    }

    #[test]
    fn detaches_when_deleted() {
        assert!(mapped("keep this gone text", "keep text", "this gone").is_err());
    }

    #[test]
    fn anchors_on_nothing_cannot_be_made() {
        assert!(Anchor::on("abc", 1..1).is_none());
        assert!(Anchor::on("abc", 2..9).is_none());
        let a = Anchor::on("abc", 1..3).unwrap();
        assert_eq!((a.quote(), a.range()), ("bc", Some(1..3)));
        assert!(serde_json::from_str::<Anchor>(r#"{"place":{"on":{"start":2,"end":2}},"quote":""}"#).is_err());
        let back: Anchor = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
        assert_eq!(back, a);
    }

    #[test]
    fn following_keeps_the_quote_of_deleted_text() {
        let mut a = Anchor::on("one two", 4..7).unwrap();
        a.follow("one ", Place::Detached(4));
        assert_eq!((a.quote(), a.place()), ("two", &Place::Detached(4)));
        // An empty range is deleted text too.
        let mut b = Anchor::on("one two", 4..7).unwrap();
        b.follow("one two", Place::On(4..4));
        assert_eq!((b.quote(), b.is_detached()), ("two", true));
        // Found again, it takes the text there.
        a.follow("one TWO", Place::On(4..7));
        assert_eq!((a.quote(), a.range()), ("TWO", Some(4..7)));
    }

    #[test]
    fn line_and_column() {
        let t = "ab\ncdé\nf";
        assert_eq!(line_col(t, 0), (1, 1));
        assert_eq!(line_col(t, 4), (2, 2));
        assert_eq!(line_col(t, t.len()), (3, 2));
        assert_eq!(line_start(t, 2), Some(3));
        assert_eq!(line_start(t, 3), Some(8));
        assert_eq!(line_start(t, 4), None);
    }

    #[test]
    fn quote_search_prefers_nearest() {
        let t = "x foo y foo z";
        assert_eq!(find_quote(t, "foo", 9), Some(8..11));
        assert_eq!(find_quote(t, "foo", 0), Some(2..5));
        assert_eq!(find_quote(t, "bar", 0), None);
    }
}
