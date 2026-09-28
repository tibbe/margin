//! Keeping comment anchors attached to their text while the document
//! changes underneath them.

use similar::{Algorithm, DiffTag, TextDiff};
use std::ops::Range;
use std::time::Duration;

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

    /// Maps a range; `None` if all of its text was deleted.
    pub fn map_range(&self, r: Range<usize>) -> Result<Range<usize>, usize> {
        let start = self.map_start(r.start);
        let end = self.map_end(r.end);
        if start < end { Ok(start..end) } else { Err(start.min(self.new_len)) }
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
        m.map_range(s..s + quote.len()).map(|r| new[r].to_string())
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
