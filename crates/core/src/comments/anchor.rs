//! Keeping comment anchors attached to their text while the document
//! changes underneath them.

use crate::diff::{Piece, diff_pieces};
use serde::{Deserialize, Serialize};
use std::ops::Range;

/// Where a thread's text is, as byte offsets into the document.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Place {
    /// On text with something to comment on.
    On(Span),
    /// The commented text was deleted; this is where it was.
    Detached(usize),
}

impl Place {
    /// On `range` of `text`, or detached at its start when nothing but
    /// whitespace is there (see [`Span::new`]).
    pub fn of(text: &str, range: Range<usize>) -> Place {
        let at = range.start.min(text.len());
        Span::new(text, range).map_or(Place::Detached(at), Place::On)
    }

    /// Where the text starts, or was.
    pub fn start(&self) -> usize {
        match self {
            Place::On(s) => s.0.start,
            Place::Detached(at) => *at,
        }
    }
}

/// A range of a document's text with something on it to comment on: more
/// than whitespace. Only [`Span::new`] makes one, so every [`Place::On`]
/// has text to highlight, in the CLI and in the editors alike.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(transparent)]
pub struct Span(Range<usize>);

impl Span {
    /// `range` of `text`; `None` when the range is not in `text`, or holds
    /// nothing but whitespace, since there is nothing there to comment on.
    pub fn new(text: &str, range: Range<usize>) -> Option<Span> {
        text.get(range.clone())
            .filter(|t| !t.trim().is_empty())
            .map(|_| Span(range))
    }

    pub fn range(&self) -> Range<usize> {
        self.0.clone()
    }
}

/// A thread's place in the document and the text it comments on. The
/// fields are private, so every anchor's quote is the text it was last
/// placed on.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(try_from = "StoredAnchor")]
pub struct Anchor {
    place: Place,
    /// The commented text, as last seen.
    quote: String,
}

/// An anchor as stored, parsed on the way in.
#[derive(Deserialize)]
struct StoredAnchor {
    place: StoredPlace,
    quote: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum StoredPlace {
    On(Range<usize>),
    Detached(usize),
}

impl TryFrom<StoredAnchor> for Anchor {
    type Error = String;

    fn try_from(a: StoredAnchor) -> Result<Anchor, String> {
        let place = match a.place {
            StoredPlace::On(r) if r.is_empty() => {
                return Err(format!("anchor on nothing at {}", r.start));
            }
            // The quote is the text the anchor is on. Earlier versions
            // kept anchors on whitespace alone, which is deleted text.
            StoredPlace::On(r) if a.quote.trim().is_empty() => Place::Detached(r.start),
            StoredPlace::On(r) => Place::On(Span(r)),
            StoredPlace::Detached(at) => Place::Detached(at),
        };
        Ok(Anchor {
            place,
            quote: a.quote,
        })
    }
}

impl Anchor {
    /// On `range` of `text`; `None` when there is nothing there to
    /// comment on (see [`Span::new`]).
    pub fn on(text: &str, range: Range<usize>) -> Option<Anchor> {
        let span = Span::new(text, range)?;
        Some(Anchor {
            quote: text[span.range()].to_string(),
            place: Place::On(span),
        })
    }

    /// Deleted text, remembered by its quote, that was at `at`.
    pub fn detached(at: usize, quote: String) -> Anchor {
        Anchor {
            place: Place::Detached(at),
            quote,
        }
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
            Place::On(s) => Some(s.range()),
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
    /// document now). On text, the quote becomes that text; a detached
    /// anchor keeps its quote.
    pub fn follow(&mut self, text: &str, now: Place) {
        match now {
            Place::On(s) => match text.get(s.range()) {
                Some(q) => {
                    self.quote = q.to_string();
                    self.place = Place::On(s);
                }
                None => self.place = Place::Detached(s.0.start.min(text.len())),
            },
            Place::Detached(at) => self.place = Place::Detached(at.min(text.len())),
        }
    }
}

/// Maps places in an old version of a document to a new version, by
/// what changed between them (see [`diff_pieces`]).
pub struct OffsetMap<'a> {
    pieces: Vec<Piece>,
    new: &'a str,
}

impl<'a> OffsetMap<'a> {
    pub fn new(old: &str, new: &'a str) -> Self {
        OffsetMap {
            pieces: diff_pieces(old, new),
            new,
        }
    }

    /// Maps a place, by these rules:
    /// - text that is kept keeps its place;
    /// - an edit within the text, from edge to edge at most, becomes part
    ///   of it, so replacing the text moves it to the replacement; except
    ///   an insertion at an edge, which stays outside;
    /// - an edit across an edge takes away the part it covers;
    /// - text with nothing but whitespace left is detached where it was.
    pub fn map(&self, p: &Place) -> Place {
        match p {
            Place::On(s) => {
                let r = s.range();
                Place::of(self.new, self.map_start(&r)..self.map_end(&r))
            }
            Place::Detached(at) => Place::Detached(self.map_point(*at)),
        }
    }

    /// Where the text on `r` starts now.
    fn map_start(&self, r: &Range<usize>) -> usize {
        let p = r.start;
        // An insertion at `p` is empty here, so it is passed over.
        for piece in &self.pieces {
            let o = &piece.old;
            if o.start <= p && p < o.end {
                return if piece.kept {
                    piece.new.start + (p - o.start)
                } else if o.start == p && o.end <= r.end {
                    piece.new.start
                } else {
                    piece.new.end
                };
            }
        }
        self.new.len()
    }

    /// Where the text on `r` ends now.
    fn map_end(&self, r: &Range<usize>) -> usize {
        let p = r.end;
        // An insertion at `p` is empty here, so it is passed over.
        for piece in &self.pieces {
            let o = &piece.old;
            if o.start < p && p <= o.end {
                return if piece.kept {
                    piece.new.start + (p - o.start)
                } else if o.end == p && o.start >= r.start {
                    piece.new.end
                } else {
                    piece.new.start
                };
            }
        }
        0
    }

    /// Where a point between characters is now: after text inserted at
    /// it, before an edit that starts at it or around it.
    fn map_point(&self, p: usize) -> usize {
        for piece in &self.pieces {
            let o = &piece.old;
            if o.start <= p && p < o.end {
                return if piece.kept {
                    piece.new.start + (p - o.start)
                } else {
                    piece.new.start
                };
            }
        }
        self.new.len()
    }
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
        match m.map(&Place::of(old, s..s + quote.len())) {
            Place::On(r) => Ok(new[r.range()].to_string()),
            Place::Detached(at) => Err(at),
        }
    }

    /// A paragraph rewritten in one go.
    const OLD_PARAGRAPH: &str = "\
- Send to Agent is insensitive unless an agent is waiting and a thread is
  open in the round; its tooltip says which is missing, or, when the round
  has other documents, how many (\"Send open comments on this and 2 other
  documents to the agent\"). While the agent works on a send, an
  `AdwSpinner` sits before it. A send says \"Sent 2 open comments to the
  agent\" in a toast, \"Sent 5 open comments on 3 documents to the agent\" for
  a round.
";

    const NEW_PARAGRAPH: &str = "\
- Send to Agent is disabled while it can't send (see the spec for when,
  and for its tooltip). While the agent works on a send, an `AdwSpinner`
  sits before it. A send's confirmation is a toast.
";

    #[test]
    fn follows_rewritten_text() {
        let m = |q| mapped(OLD_PARAGRAPH, NEW_PARAGRAPH, q);
        assert_eq!(m("insensitive"), Ok("disabled".into()));
        assert_eq!(m("its tooltip"), Ok("its tooltip".into()));
        assert_eq!(m("AdwSpinner"), Ok("AdwSpinner".into()));
    }

    /// The examples in `docs/spec.md`'s table of how anchors follow edits.
    #[test]
    fn spec_examples() {
        let m = |new| mapped("the quick fox jumps", new, "quick fox");
        assert_eq!(m("then the quick fox jumps"), Ok("quick fox".into()));
        assert_eq!(m("the quick brown fox jumps"), Ok("quick brown fox".into()));
        assert_eq!(m("the lazy dog jumps"), Ok("lazy dog".into()));
        assert_eq!(m("the very quick fox jumps"), Ok("quick fox".into()));
        assert_eq!(m("the quick."), Ok("quick".into()));
        assert_eq!(m("the jumps"), Err(4));
        assert_eq!(
            mapped(
                "the **quick fox** jumps",
                "the **lazy dog** jumps",
                "quick fox"
            ),
            Ok("lazy dog".into())
        );
        assert_eq!(
            mapped(
                "the round; its tooltip",
                "the spec and its tooltip",
                "the round"
            ),
            Ok("the ".into())
        );
    }

    /// Every word deleted, but the line break between two of them kept
    /// for the text after: nothing is left to comment on.
    #[test]
    fn detaches_when_only_whitespace_is_left() {
        assert!(
            mapped(
                "a short message at\n  the bottom of the window that goes away\n",
                "a short message\n  that goes away\n",
                "at\n  the bottom of the window"
            )
            .is_err()
        );
    }

    #[test]
    fn follows_a_replaced_word() {
        assert_eq!(
            mapped("the colour is red", "the color is red", "colour"),
            Ok("color".into())
        );
    }

    #[test]
    fn loses_what_an_edit_across_an_edge_covers() {
        assert_eq!(
            mapped("keep this, lose that", "keep this", "this, lose"),
            Ok("this".into())
        );
        assert_eq!(
            mapped("lose that, keep this", "keep this", "that, keep"),
            Ok("keep".into())
        );
    }

    #[test]
    fn survives_edits_elsewhere() {
        assert_eq!(
            mapped(
                "one two three four",
                "zero one two three four five",
                "two three"
            ),
            Ok("two three".into())
        );
        assert_eq!(mapped("a b c d", "a c d", "c d"), Ok("c d".into()));
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
            mapped(
                "the quick fox jumps",
                "the quick brown fox jumps",
                "quick fox"
            ),
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
        // Not onto the spaces that were around it.
        assert!(
            mapped(
                "Keep this. Gone text here.",
                "Keep this.  here.",
                "Gone text"
            )
            .is_err()
        );
    }

    #[test]
    fn anchors_on_nothing_cannot_be_made() {
        assert!(Anchor::on("abc", 1..1).is_none());
        assert!(Anchor::on("abc", 2..9).is_none());
        assert!(Anchor::on("a \n b", 1..4).is_none());
        // Stored by earlier versions, they load detached.
        let stored: Anchor =
            serde_json::from_str(r#"{"place":{"on":{"start":1,"end":4}},"quote":" \n "}"#).unwrap();
        assert_eq!(
            (stored.quote(), stored.place()),
            (" \n ", &Place::Detached(1))
        );
        let a = Anchor::on("abc", 1..3).unwrap();
        assert_eq!((a.quote(), a.range()), ("bc", Some(1..3)));
        assert!(
            serde_json::from_str::<Anchor>(r#"{"place":{"on":{"start":2,"end":2}},"quote":""}"#)
                .is_err()
        );
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
        b.follow("one two", Place::of("one two", 4..4));
        assert_eq!((b.quote(), b.is_detached()), ("two", true));
        // Found again, it takes the text there.
        a.follow("one TWO", Place::of("one TWO", 4..7));
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
