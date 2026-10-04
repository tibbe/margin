//! The display map: what an editor shows of a document, paragraph by
//! paragraph, and where each shown character comes from.
//!
//! The source is the document; this is a projection of it, as CodeMirror's
//! decorations or Zed's display map are. Markdown syntax is hidden, a soft
//! line break shows as a space while reflowing (joining the paragraph's
//! lines into one shown paragraph), a table cell's padding as the gap to its
//! column, and an image or diagram as one object. Editors lay out the shown
//! paragraphs, and map every position, both ways, through here.
//!
//! Positions are source byte offsets; offsets in a shown paragraph are byte
//! offsets in its shown text.

use super::doc::{Doc, LineKind, Style};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

/// What decides the projection besides the document.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Reflow Paragraphs: soft line breaks show as spaces, so a paragraph's
    /// lines show as one paragraph.
    pub reflow: bool,
    /// Show Markdown: every character shows as it is, and nothing joins.
    pub source_mode: bool,
    /// Hidden syntax shown all the same: the blank line the cursor is on,
    /// the fences of the code block it is in (see [`reveal_at`]).
    pub reveal: Vec<Range<usize>>,
}

/// How a stretch of source shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    /// As it is.
    Shown(Range<usize>),
    /// Not at all.
    Hidden(Range<usize>),
    /// As other text: a soft line break as a space, a table cell's gap as
    /// a tab, which the editor sets at the cell's column.
    Replaced(Range<usize>, &'static str),
    /// As an image or a diagram: one object, one shown character
    /// ([`OBJECT`]), that the cursor never rests inside.
    Object(Range<usize>),
}

/// The shown character standing for an object.
pub const OBJECT: char = '\u{FFFC}';

impl Piece {
    pub fn source(&self) -> &Range<usize> {
        match self {
            Piece::Shown(r) | Piece::Hidden(r) | Piece::Replaced(r, _) | Piece::Object(r) => r,
        }
    }

    /// How many bytes it shows as.
    fn shown_len(&self) -> usize {
        match self {
            Piece::Shown(r) => r.len(),
            Piece::Hidden(_) => 0,
            Piece::Replaced(_, t) => t.len(),
            Piece::Object(_) => OBJECT.len_utf8(),
        }
    }
}

/// One shown paragraph: one source line, or several joined by soft breaks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paragraph {
    /// The source lines it shows, as indices into [`Doc::lines`]; empty for
    /// the empty paragraph after a final line break.
    pub lines: Range<usize>,
    /// Its source, without the line break that ends it, and with any
    /// collapsed lines before it (blank lines, fences) as hidden pieces.
    pub source: Range<usize>,
    /// Covering `source`, in order.
    pub pieces: Vec<Piece>,
}

impl Paragraph {
    /// What it shows.
    pub fn text(&self, src: &str) -> String {
        let mut t = String::new();
        for p in &self.pieces {
            match p {
                Piece::Shown(r) => t.push_str(&src[r.clone()]),
                Piece::Hidden(_) => {}
                Piece::Replaced(_, s) => t.push_str(s),
                Piece::Object(_) => t.push(OBJECT),
            }
        }
        t
    }

    pub fn shown_len(&self) -> usize {
        self.pieces.iter().map(Piece::shown_len).sum()
    }
}

/// A document as shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Display {
    pub paragraphs: Vec<Paragraph>,
    /// Per source line: where the cursor's stops on it start (past its
    /// block syntax, which shows as a marker or not at all; in Show
    /// Markdown, at its start), and whether it is collapsed (every
    /// character, line break included, hidden), so neither shows nor takes
    /// the cursor.
    stops_from: Vec<usize>,
    collapsed: Vec<bool>,
    line_starts: Vec<usize>,
    /// Positions inside table rows' padding and `|` (sorted, disjoint),
    /// which the cursor passes: it rests in cells' text.
    no_stops: Vec<Range<usize>>,
    len: usize,
}

/// A shown position: a paragraph and a byte offset in its shown text. Only
/// the map makes them, from a source position ([`Display::to_shown`]) or
/// from an offset the editor gives ([`Display::position`]), which it puts
/// between characters, so a shown position never splits one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shown {
    paragraph: usize,
    offset: usize,
}

impl Shown {
    pub fn paragraph(&self) -> usize {
        self.paragraph
    }

    pub fn offset(&self) -> usize {
        self.offset
    }
}

/// What Transpose does: it moves `moved`, a shown character's own source,
/// to `to`, as if deleted and typed there, and leaves the cursor at
/// `cursor`; all in the source as it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transpose {
    pub moved: Range<usize>,
    pub to: usize,
    pub cursor: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Deco {
    Hide,
    Replace(&'static str),
    Object,
}

/// Projects `doc` (of `src`) as `opts` says.
pub fn project(src: &str, doc: &Doc, opts: &Options) -> Display {
    let len = src.len();
    let decos = decorations(src, doc, opts);
    let mut paragraphs: Vec<Paragraph> = Vec::new();
    let mut pieces: Vec<Piece> = Vec::new();
    let mut start = 0;
    // Whether the paragraph being built shows anything yet.
    let mut showing = false;
    let mut di = 0;
    let mut pos = 0;
    let bytes = src.as_bytes();
    let line_of = |p: usize| {
        doc.lines
            .partition_point(|l| l.start <= p)
            .saturating_sub(1)
    };
    let finish = |paragraphs: &mut Vec<Paragraph>, pieces: Vec<Piece>, start: usize, end: usize| {
        let first = pieces
            .iter()
            .find(|p| !matches!(p, Piece::Hidden(_)))
            .map_or(end.max(start), |p| p.source().start);
        let lines = if doc.lines.is_empty() {
            0..0
        } else {
            line_of(first)..line_of(end.max(first)) + 1
        };
        paragraphs.push(Paragraph {
            lines,
            source: start..end,
            pieces,
        });
    };
    while pos < len {
        while di < decos.len() && decos[di].0.end <= pos {
            di += 1;
        }
        let deco = decos.get(di).filter(|(r, _)| r.start <= pos);
        let (end, kind) = match deco {
            Some((r, k)) => (r.end, Some(*k)),
            None => {
                // Up to the next decoration or line break, whichever first.
                let next = decos.get(di).map_or(len, |(r, _)| r.start);
                let nl = bytes[pos..next]
                    .iter()
                    .position(|&b| b == b'\n')
                    .map(|i| pos + i);
                match nl {
                    Some(n) if n == pos => (pos + 1, None),
                    Some(n) => (n, None),
                    None => (next, None),
                }
            }
        };
        let r = pos..end;
        match kind {
            None if bytes[pos] == b'\n' => {
                // A shown line break ends the paragraph, even an empty one.
                finish(&mut paragraphs, std::mem::take(&mut pieces), start, pos);
                start = pos + 1;
                showing = false;
            }
            None => {
                pieces.push(Piece::Shown(r));
                showing = true;
            }
            Some(Deco::Replace(t)) => {
                pieces.push(Piece::Replaced(r, t));
                showing = true;
            }
            Some(Deco::Object) => {
                pieces.push(Piece::Object(r));
                showing = true;
            }
            Some(Deco::Hide) => {
                // A hidden line break ends a paragraph that shows something;
                // one on a collapsed line runs into the next paragraph.
                let mut a = pos;
                for (i, &b) in bytes[pos..end].iter().enumerate() {
                    if b == b'\n' && showing {
                        let n = pos + i;
                        if n > a {
                            pieces.push(Piece::Hidden(a..n));
                        }
                        finish(&mut paragraphs, std::mem::take(&mut pieces), start, n);
                        start = n + 1;
                        a = n + 1;
                        showing = false;
                    }
                }
                if end > a {
                    pieces.push(Piece::Hidden(a..end));
                }
            }
        }
        pos = end;
    }
    if showing || paragraphs.is_empty() || src.ends_with('\n') && !pieces.is_empty() {
        finish(&mut paragraphs, std::mem::take(&mut pieces), start, len);
    } else if !pieces.is_empty() {
        // Collapsed lines at the end belong to the paragraph before them.
        let last = paragraphs.last_mut().expect("a paragraph");
        last.source.end = len;
        last.pieces.extend(pieces);
    } else if start == len && src.ends_with('\n') {
        // After a final line break: an empty line, where the cursor can be.
        paragraphs.push(Paragraph {
            lines: doc.lines.len()..doc.lines.len(),
            source: len..len,
            pieces: Vec::new(),
        });
    }

    let hidden_at = |p: usize| {
        let i = decos.partition_point(|(r, _)| r.end <= p);
        decos
            .get(i)
            .is_some_and(|(r, k)| r.start <= p && *k == Deco::Hide)
    };
    let collapsed = doc
        .lines
        .iter()
        .map(|l| {
            let end = (l.end + 1).min(len);
            end > l.start && (l.start..end).all(&hidden_at)
        })
        .collect();
    let mut no_stops = Vec::new();
    if !opts.source_mode {
        for t in &doc.tables {
            for row in &t.rows {
                for (j, cell) in row.cells.iter().enumerate() {
                    // A row's start doesn't take the cursor, nor does the
                    // padding between cells: the end of the cell before
                    // and the start of this one do.
                    let from = if j == 0 {
                        cell.lead.start
                    } else {
                        cell.lead.start + 1
                    };
                    if from < cell.lead.end {
                        no_stops.push(from..cell.lead.end);
                    }
                }
                if row.trail.end > row.trail.start {
                    no_stops.push(row.trail.start + 1..row.trail.end + 1);
                }
            }
        }
    }
    let no_stops = merge(no_stops);
    Display {
        paragraphs,
        no_stops,
        stops_from: doc
            .lines
            .iter()
            .map(|l| {
                if opts.source_mode {
                    l.start
                } else {
                    l.content_start
                }
            })
            .collect(),
        collapsed,
        line_starts: doc.lines.iter().map(|l| l.start).collect(),
        len,
    }
}

/// The decorations, sorted and disjoint: objects first, then what is
/// hidden, then what is replaced, each over what is left.
fn decorations(src: &str, doc: &Doc, opts: &Options) -> Vec<(Range<usize>, Deco)> {
    if opts.source_mode {
        return Vec::new();
    }
    let len = src.len();
    let line_end = |li: usize| doc.lines[li].end;
    let mut objects: Vec<Range<usize>> = doc
        .images
        .iter()
        .map(|i| i.range.start..line_end(i.line))
        .chain(
            doc.diagrams
                .iter()
                .map(|d| d.range.start..line_end(d.last_line)),
        )
        .collect();
    objects.sort_by_key(|r| r.start);

    let mut hidden: Vec<Range<usize>> = doc
        .spans
        .iter()
        .filter(|s| s.style == Style::Hidden)
        .map(|s| s.range.clone())
        .collect();
    let mut replaced: Vec<(Range<usize>, &'static str)> = Vec::new();
    for t in &doc.tables {
        let d = &doc.lines[t.delimiter_line];
        hidden.push(d.start..(d.end + 1).min(len));
        for row in &t.rows {
            for cell in &row.cells {
                if cell.lead.end > cell.lead.start {
                    let gap = cell.lead.end - 1;
                    hidden.push(cell.lead.start..gap);
                    replaced.push((gap..gap + 1, "\t"));
                }
            }
            hidden.push(row.trail.clone());
        }
    }
    let hidden = subtract(&merge(hidden), &merge(opts.reveal.clone()));
    if opts.reflow {
        replaced.extend(doc.soft_breaks.iter().map(|&b| (b..b + 1, " ")));
    }

    let mut out: Vec<(Range<usize>, Deco)> =
        objects.iter().map(|r| (r.clone(), Deco::Object)).collect();
    for r in subtract(&hidden, &objects) {
        out.push((r, Deco::Hide));
    }
    let taken = merge(out.iter().map(|(r, _)| r.clone()).collect());
    for (r, t) in replaced {
        if subtract(std::slice::from_ref(&r), &taken) == [r.clone()] {
            out.push((r, Deco::Replace(t)));
        }
    }
    out.retain(|(r, _)| !r.is_empty());
    out.sort_by_key(|(r, _)| r.start);
    out
}

fn merge(mut rs: Vec<Range<usize>>) -> Vec<Range<usize>> {
    rs.retain(|r| !r.is_empty());
    rs.sort_by_key(|r| r.start);
    let mut out: Vec<Range<usize>> = Vec::with_capacity(rs.len());
    for r in rs {
        match out.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}

/// `a` without `b`; both sorted and disjoint.
fn subtract(a: &[Range<usize>], b: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut j = 0;
    for r in a {
        let mut start = r.start;
        while j < b.len() && b[j].end <= start {
            j += 1;
        }
        let mut k = j;
        while k < b.len() && b[k].start < r.end {
            if b[k].start > start {
                out.push(start..b[k].start);
            }
            start = start.max(b[k].end);
            k += 1;
        }
        if start < r.end {
            out.push(start..r.end);
        }
    }
    out
}

/// The syntax the cursor at `cursor` needs to see: the blank line it is on,
/// and the fences of the code block it is in (not a diagram's, which shows
/// as such).
pub fn reveal_at(doc: &Doc, cursor: usize) -> Vec<Range<usize>> {
    if doc.lines.is_empty() {
        return Vec::new();
    }
    let len = doc.len;
    let li = doc.line_index(cursor.min(len));
    let mut out = Vec::new();
    let line = &doc.lines[li];
    if line.kind == LineKind::Blank {
        out.push(line.start..(line.end + 1).min(len));
    }
    for cb in &doc.code_blocks {
        let first = cb.open_line.unwrap_or(cb.first_line);
        let last = cb.close_line.unwrap_or(cb.last_line);
        let diagram = doc.diagrams.iter().any(|d| d.first_line == cb.first_line);
        if li < first || li > last || diagram {
            continue;
        }
        for fl in [cb.open_line, cb.close_line].into_iter().flatten() {
            let l = &doc.lines[fl];
            out.push(l.content_start..(l.end + 1).min(len));
        }
    }
    out.retain(|r| !r.is_empty());
    out
}

impl Display {
    /// The shown paragraph holding source position `pos`, and where in it:
    /// hidden syntax shows where it starts, and a position inside an
    /// object, before it.
    pub fn to_shown(&self, pos: usize) -> Shown {
        let pos = pos.min(self.len);
        let ps = &self.paragraphs;
        // The last paragraph starting at or before `pos`; its line break,
        // just past its source, is its end.
        let i = ps
            .partition_point(|p| p.source.start <= pos)
            .saturating_sub(1);
        let p = &ps[i];
        let mut x = 0;
        for piece in &p.pieces {
            let r = piece.source();
            if pos < r.start {
                break;
            }
            match piece {
                Piece::Shown(r) if pos <= r.end => return self.at(i, x + pos - r.start),
                Piece::Hidden(r) if pos <= r.end => return self.at(i, x),
                Piece::Replaced(r, t) if pos <= r.end => {
                    return self.at(i, if pos == r.start { x } else { x + t.len() });
                }
                Piece::Object(r) if pos < r.end => return self.at(i, x),
                Piece::Object(r) if pos == r.end => return self.at(i, x + OBJECT.len_utf8()),
                _ => {}
            }
            x += piece.shown_len();
        }
        self.at(i, x)
    }

    fn at(&self, paragraph: usize, offset: usize) -> Shown {
        Shown { paragraph, offset }
    }

    /// The shown position at `offset` of `paragraph`, as an editor gives
    /// one (where a click lands, a laid-out line's end): in the document,
    /// and between characters (grapheme clusters), at the start of one it
    /// falls inside, so no position the editor maps back splits one.
    pub fn position(&self, src: &str, paragraph: usize, offset: usize) -> Shown {
        let paragraph = paragraph.min(self.paragraphs.len() - 1);
        let text = self.paragraphs[paragraph].text(src);
        let offset = text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .chain([text.len()])
            .take_while(|&i| i <= offset)
            .last()
            .unwrap_or(0);
        Shown { paragraph, offset }
    }

    /// Where the cursor goes for a shown position: the first source
    /// position showing there that may take the cursor. Not inside hidden
    /// syntax, not on a collapsed line, not in a line's block syntax (`- `,
    /// `# `, `> `): so at the left edge of inline syntax, and inside bold
    /// that ends there (`**bold|**`).
    pub fn to_source(&self, at: Shown) -> usize {
        let Some(p) = self.paragraphs.get(at.paragraph) else {
            return self.len;
        };
        let candidates = self.candidates(p, at.offset);
        self.stop_in(&candidates)
            .or_else(|| candidates.last().copied())
            .unwrap_or(p.source.end)
    }

    /// The source of the shown text from `start` to `end`, in `start`'s
    /// paragraph (an `end` past it reads as its end): from its first
    /// character's own position (past hidden syntax before it) to just
    /// after its last character (before hidden syntax after it), so a
    /// replacement keeps the formatting around it.
    pub fn source_range(&self, start: Shown, end: Shown) -> Range<usize> {
        let Some(p) = self.paragraphs.get(start.paragraph) else {
            return self.len..self.len;
        };
        let end = if end.paragraph == start.paragraph {
            end.offset
        } else {
            p.shown_len()
        };
        let a = self
            .candidates(p, start.offset)
            .last()
            .copied()
            .unwrap_or(p.source.end);
        let b = self
            .candidates(p, end)
            .first()
            .copied()
            .unwrap_or(p.source.end);
        a..b.max(a)
    }

    /// The first of `candidates` that takes the cursor.
    fn stop_in(&self, candidates: &[usize]) -> Option<usize> {
        candidates.iter().copied().find(|&c| self.takes_cursor(c))
    }

    /// Whether a shown position has a source position that takes the
    /// cursor (a table row's leading gap doesn't).
    fn is_stop(&self, at: Shown) -> bool {
        self.paragraphs
            .get(at.paragraph)
            .is_some_and(|p| self.stop_in(&self.candidates(p, at.offset)).is_some())
    }

    /// The source positions showing at `offset` of `p`, in order.
    fn candidates(&self, p: &Paragraph, offset: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut x = 0;
        for piece in &p.pieces {
            let n = piece.shown_len();
            match piece {
                Piece::Shown(r) if x <= offset && offset <= x + n => {
                    let at = r.start + offset - x;
                    if out.last() != Some(&at) {
                        out.push(at);
                    }
                    if offset < x + n {
                        return out;
                    }
                }
                Piece::Hidden(r) if x == offset => {
                    for c in r.start..=r.end {
                        if out.last() != Some(&c) {
                            out.push(c);
                        }
                    }
                }
                Piece::Replaced(r, _) | Piece::Object(r) if x == offset => {
                    if out.last() != Some(&r.start) {
                        out.push(r.start);
                    }
                    return out;
                }
                Piece::Replaced(r, _) | Piece::Object(r) if x + n == offset => {
                    out.push(r.end);
                }
                _ => {}
            }
            x += n;
            if x > offset {
                break;
            }
        }
        if out.is_empty() {
            out.push(p.source.end);
        }
        out
    }

    /// Whether the cursor may rest at source position `c`.
    fn takes_cursor(&self, c: usize) -> bool {
        if self.line_starts.is_empty() {
            return true;
        }
        let li = self
            .line_starts
            .partition_point(|&s| s <= c)
            .saturating_sub(1);
        let i = self.no_stops.partition_point(|r| r.end <= c);
        let passed = self.no_stops.get(i).is_some_and(|r| r.start <= c);
        !self.collapsed[li] && c >= self.stops_from[li] && !passed
    }

    /// Where the cursor at `pos` goes with one press of Right (`forward`)
    /// or Left: one shown character over, never resting in hidden syntax
    /// or where nothing takes the cursor; an object is one character; from
    /// a paragraph's end, the next one's start.
    pub fn step(&self, src: &str, pos: usize, forward: bool) -> usize {
        let mut at = self.to_shown(pos);
        loop {
            match self.next_shown(src, at, forward) {
                Some(next) if self.is_stop(next) => return self.to_source(next),
                Some(next) => at = next,
                None => {
                    return if forward {
                        self.len
                    } else {
                        self.to_source(at)
                    };
                }
            }
        }
    }

    /// The shown position one character after (or before) `at`.
    fn next_shown(&self, src: &str, at: Shown, forward: bool) -> Option<Shown> {
        let text = self.paragraphs[at.paragraph].text(src);
        if forward {
            if at.offset >= text.len() {
                return (at.paragraph + 1 < self.paragraphs.len()).then_some(Shown {
                    paragraph: at.paragraph + 1,
                    offset: 0,
                });
            }
            let next = text[at.offset..]
                .grapheme_indices(true)
                .nth(1)
                .map_or(text.len(), |(i, _)| at.offset + i);
            Some(Shown {
                paragraph: at.paragraph,
                offset: next,
            })
        } else {
            if at.offset == 0 {
                let prev = at.paragraph.checked_sub(1)?;
                return Some(Shown {
                    paragraph: prev,
                    offset: self.paragraphs[prev].shown_len(),
                });
            }
            let prev = text[..at.offset]
                .grapheme_indices(true)
                .next_back()
                .map_or(0, |(i, _)| i);
            Some(Shown {
                paragraph: at.paragraph,
                offset: prev,
            })
        }
    }

    /// Where Option-Right (`forward`) or Option-Left takes the cursor at
    /// `pos`: to the end of the word it is in or the next one, or to the
    /// start of the word it is in or the one before, in the shown text, as
    /// Unicode's word boundaries have words; from a paragraph's edge, to
    /// the next paragraph's.
    pub fn word(&self, src: &str, pos: usize, forward: bool) -> usize {
        let at = self.to_shown(pos);
        let i = at.paragraph;
        let text = self.paragraphs[i].text(src);
        let mut words = text
            .split_word_bound_indices()
            .filter(|(_, w)| w.chars().any(char::is_alphanumeric))
            .map(|(a, w)| a..a + w.len());
        let offset = if forward {
            if at.offset >= text.len() {
                return if i + 1 < self.paragraphs.len() {
                    self.to_source(self.at(i + 1, 0))
                } else {
                    self.len
                };
            }
            words
                .find(|w| w.end > at.offset)
                .map_or(text.len(), |w| w.end)
        } else {
            if at.offset == 0 {
                return match i.checked_sub(1) {
                    Some(prev) => self.to_source(self.at(prev, self.paragraphs[prev].shown_len())),
                    None => self.to_source(at),
                };
            }
            words
                .take_while(|w| w.start < at.offset)
                .last()
                .map_or(0, |w| w.start)
        };
        self.to_source(self.at(i, offset))
    }

    /// What Transpose (Ctrl+T) does at `pos`: the shown characters on
    /// either side of it swap, or at a paragraph's end the two before it,
    /// and the cursor ends after them. The first moves past the second; if
    /// the first shows as other text (a soft line break), the second moves
    /// before it instead. None without two characters to swap, or when
    /// neither is its own source (an object, a table's gap).
    pub fn transposed(&self, src: &str, pos: usize) -> Option<Transpose> {
        let at = self.to_shown(pos);
        let i = at.paragraph;
        let text = self.paragraphs[i].text(src);
        let bounds: Vec<usize> = text
            .grapheme_indices(true)
            .map(|(b, _)| b)
            .chain([text.len()])
            .collect();
        let last = bounds.len() - 1;
        let mut k = bounds.partition_point(|&b| b <= at.offset) - 1;
        if k == last {
            k = k.checked_sub(1)?;
        }
        if k == 0 {
            return None;
        }
        let (a, b) = (bounds[k - 1]..bounds[k], bounds[k]..bounds[k + 1]);
        let source = |r: &Range<usize>| self.source_range(self.at(i, r.start), self.at(i, r.end));
        let own = |r: &Range<usize>| src[source(r)] == text[r.clone()];
        let after = self.to_source(self.at(i, b.end));
        if own(&a) && !text[b.clone()].contains(OBJECT) {
            Some(Transpose {
                moved: source(&a),
                to: after,
                cursor: after,
            })
        } else if own(&b) && !text[a.clone()].contains(OBJECT) {
            Some(Transpose {
                moved: source(&b),
                to: self.to_source(self.at(i, a.start)),
                cursor: after,
            })
        } else {
            None
        }
    }

    /// The source length the map was made for.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::md::parse;

    fn shown(src: &str, opts: &Options) -> Vec<String> {
        let doc = parse(src);
        project(src, &doc, opts)
            .paragraphs
            .iter()
            .map(|p| p.text(src))
            .collect()
    }

    fn reflow() -> Options {
        Options {
            reflow: true,
            ..Options::default()
        }
    }

    fn map(src: &str, opts: &Options) -> Display {
        project(src, &parse(src), opts)
    }

    /// The source position the cursor takes at the `|` in `shown` (one
    /// shown paragraph's text).
    fn stop(src: &str, opts: &Options, paragraph: usize, offset: usize) -> String {
        let d = map(src, opts);
        let p = d.to_source(d.position(src, paragraph, offset));
        format!("{}|{}", &src[..p], &src[p..])
    }

    #[test]
    fn hidden_syntax_does_not_show() {
        assert_eq!(
            shown("a **b** `c` d\n", &Options::default()),
            ["a b c d", ""]
        );
        assert_eq!(shown("# Title\n", &Options::default()), ["Title", ""]);
        assert_eq!(
            shown("- one\n- two\n", &Options::default()),
            ["one", "two", ""]
        );
        assert_eq!(
            shown("[a link](https://x.y) z", &Options::default()),
            ["a link z"]
        );
    }

    #[test]
    fn soft_breaks_join_lines_only_while_reflowing() {
        let src = "- **Cause:** one\n  two three\n";
        assert_eq!(
            shown(src, &Options::default()),
            ["Cause: one", "two three", ""]
        );
        assert_eq!(shown(src, &reflow()), ["Cause: one two three", ""]);
        assert_eq!(map(src, &reflow()).paragraphs[0].lines, 0..2);
        let source = Options {
            reflow: true,
            source_mode: true,
            ..Options::default()
        };
        assert_eq!(shown(src, &source), ["- **Cause:** one", "  two three", ""]);
    }

    #[test]
    fn blank_lines_and_fences_collapse_unless_revealed() {
        assert_eq!(shown("a\n\nb\n", &Options::default()), ["a", "b", ""]);
        let doc = parse("a\n\nb\n");
        let opts = Options {
            reveal: reveal_at(&doc, 2),
            ..Options::default()
        };
        assert_eq!(shown("a\n\nb\n", &opts), ["a", "", "b", ""]);
        assert_eq!(shown("```\ncode\n```\n", &Options::default()), ["code", ""]);
        let doc = parse("```\ncode\n```\n");
        let opts = Options {
            reveal: reveal_at(&doc, 5),
            ..Options::default()
        };
        assert_eq!(shown("```\ncode\n```\n", &opts), ["```", "code", "```", ""]);
    }

    #[test]
    fn a_rule_and_an_empty_item_show_as_empty_paragraphs() {
        assert_eq!(shown("a\n\n---\n\nb", &Options::default()), ["a", "", "b"]);
        assert_eq!(shown("- \n", &Options::default()), ["", ""]);
    }

    #[test]
    fn the_end_of_the_document() {
        assert_eq!(shown("", &Options::default()), [""]);
        assert_eq!(shown("a", &Options::default()), ["a"]);
        assert_eq!(shown("a\n", &Options::default()), ["a", ""]);
        let d = map("a\n", &Options::default());
        assert_eq!(
            d.to_shown(2),
            Shown {
                paragraph: 1,
                offset: 0
            }
        );
        assert_eq!(
            d.to_source(Shown {
                paragraph: 1,
                offset: 0
            }),
            2
        );
    }

    #[test]
    fn tables_show_cells_at_tab_stops() {
        let src = "| a | b |\n|---|---|\n| c | d |\n";
        assert_eq!(shown(src, &Options::default()), ["\ta\tb", "\tc\td", ""]);
    }

    #[test]
    fn images_and_diagrams_are_objects() {
        let src = "Text\n\n![alt](x.png)\n\n```mermaid\ngraph TD\nA-->B\n```\n";
        let d = map(src, &Options::default());
        let texts: Vec<String> = d.paragraphs.iter().map(|p| p.text(src)).collect();
        assert_eq!(texts, ["Text", "\u{FFFC}", "\u{FFFC}", ""]);
        assert!(
            matches!(d.paragraphs[2].pieces.last(), Some(Piece::Object(r)) if &src[r.clone()] == "```mermaid\ngraph TD\nA-->B\n```")
        );
    }

    #[test]
    fn the_cursor_stops_at_the_left_edge_of_inline_syntax() {
        let o = Options::default();
        // Start of the line, before the bold: outside it.
        assert_eq!(stop("**a**b", &o, 0, 0), "|**a**b");
        // After the bold's text: inside it.
        assert_eq!(stop("**a**b", &o, 0, 1), "**a|**b");
        assert_eq!(stop("**a**b", &o, 0, 2), "**a**b|");
        // Past a list item's marker, before its bold.
        assert_eq!(stop("- **Cause:** x", &o, 0, 0), "- |**Cause:** x");
        assert_eq!(stop("- **Cause:** x", &o, 0, 6), "- **Cause:|** x");
    }

    #[test]
    fn the_cursor_stops_past_a_joined_lines_indent() {
        let src = "- a b\n  keeps\n";
        // `a b keeps`: the `k` is at 4.
        assert_eq!(stop(src, &reflow(), 0, 4), "- a b\n  |keeps\n");
        assert_eq!(stop(src, &reflow(), 0, 3), "- a b|\n  keeps\n");
    }

    #[test]
    fn the_cursor_skips_collapsed_lines() {
        assert_eq!(stop("a\n\nb\n", &Options::default(), 1, 0), "a\n\n|b\n");
        assert_eq!(
            stop("```\ncode\n```\n", &Options::default(), 0, 0),
            "```\n|code\n```\n"
        );
    }

    #[test]
    fn positions_map_to_where_they_show() {
        let src = "- **Cause:** one\n  two\n";
        let d = map(src, &reflow());
        // Inside hidden syntax: where it starts.
        assert_eq!(
            d.to_shown(3),
            Shown {
                paragraph: 0,
                offset: 0
            }
        );
        assert_eq!(
            d.to_shown(10),
            Shown {
                paragraph: 0,
                offset: 6
            }
        );
        assert_eq!(
            d.to_shown(12),
            Shown {
                paragraph: 0,
                offset: 6
            }
        );
        // The soft break, and past the indent after it.
        assert_eq!(
            d.to_shown(16),
            Shown {
                paragraph: 0,
                offset: 10
            }
        );
        assert_eq!(
            d.to_shown(19),
            Shown {
                paragraph: 0,
                offset: 11
            }
        );
        // Every stop maps back to itself.
        for pos in 0..=src.len() {
            let at = d.to_shown(pos);
            let back = d.to_source(at);
            assert_eq!(d.to_shown(back), at, "{pos}");
        }
    }

    #[test]
    fn stepping_moves_one_shown_character() {
        let src = "a **bold** c\n";
        let d = map(src, &Options::default());
        assert_eq!(d.step(src, 7, true), 8, "into `bold|**`");
        assert_eq!(d.step(src, 8, true), 11, "over the space");
        assert_eq!(d.step(src, 10, true), 11, "from past the markers");
        assert_eq!(d.step(src, 11, false), 8);
        assert_eq!(d.step(src, 12, true), 13, "to the next line");
        let src = "a😀x";
        let d = map(src, &Options::default());
        assert_eq!(d.step(src, 1, true), 5);
        assert_eq!(d.step(src, 5, false), 1);
    }

    #[test]
    fn shown_text_maps_to_the_source_of_its_characters() {
        let src = "Some **teh** words";
        let d = map(src, &Options::default());
        // `teh` shows at 5..8: its own characters, not the markers.
        assert_eq!(
            &src[d.source_range(d.position(src, 0, 5), d.position(src, 0, 8))],
            "teh"
        );
        assert_eq!(
            &src[d.source_range(d.position(src, 0, 0), d.position(src, 0, 4))],
            "Some"
        );
    }

    #[test]
    fn a_position_from_the_editor_never_splits_a_character() {
        // A family emoji is one character of 25 bytes.
        let src = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466} next";
        let d = map(src, &Options::default());
        assert_eq!(d.position(src, 0, 4).offset(), 0);
        assert_eq!(d.position(src, 0, 25).offset(), 25);
        assert_eq!(d.position(src, 0, 99).offset(), src.len());
        assert_eq!(d.position(src, 9, 0).paragraph(), 0);
    }

    #[test]
    fn words_are_shown_words() {
        let o = Options::default();
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\u{200D}\u{1F466}";
        let src = format!("{family} next");
        let d = map(&src, &o);
        assert_eq!(
            d.word(&src, 0, true),
            src.len(),
            "past the emoji, to the word's end"
        );
        assert_eq!(d.word(&src, src.len(), false), family.len() + 1);
        let src = "say **don't** go\n";
        let d = map(src, &o);
        assert_eq!(d.word(src, 0, true), 3);
        assert_eq!(d.word(src, 3, true), 11, "to `don't|**`");
        assert_eq!(d.word(src, 11, false), 4, "to `|**don't`");
        assert_eq!(d.word(src, 16, true), 17, "on to the next paragraph");
    }

    #[test]
    fn transposing_moves_the_character_before_the_cursor_past_the_next() {
        let o = Options::default();
        let src = "one **bold** now";
        let d = map(src, &o);
        // At `bold|`: `d` moves past the space.
        assert_eq!(
            d.transposed(src, 10),
            Some(Transpose {
                moved: 9..10,
                to: 13,
                cursor: 13
            })
        );
        // At the end: the two before it.
        assert_eq!(
            d.transposed(src, src.len()),
            Some(Transpose {
                moved: 14..15,
                to: 16,
                cursor: 16
            })
        );
        assert_eq!(d.transposed(src, 0), None);
        assert_eq!(map("a", &o).transposed("a", 1), None);
        // Before a soft break shown as a space, the character after it moves.
        let src = "- ab\n  cd\n";
        let d = map(src, &reflow());
        assert_eq!(
            d.transposed(src, 7),
            Some(Transpose {
                moved: 7..8,
                to: 4,
                cursor: 8
            })
        );
    }

    #[test]
    fn show_markdown_stops_in_block_syntax() {
        let o = Options {
            source_mode: true,
            ..Options::default()
        };
        let src = "# Title\n- item\n";
        let d = map(src, &o);
        assert_eq!(d.step(src, 0, true), 1);
        assert_eq!(d.step(src, 8, true), 9);
        assert_eq!(stop(src, &o, 1, 0), "# Title\n|- item\n");
    }

    #[test]
    fn the_cursor_passes_table_padding() {
        let src = "| Name | Role |\n|:-----|:----:|\n| Grace Hopper |  |\n";
        let d = map(src, &Options::default());
        let grace = src.find("Grace").unwrap();
        let role_end = src.find("Role").unwrap() + 4;
        assert_eq!(
            d.step(src, grace, false),
            role_end,
            "to the end of the row above"
        );
        assert_eq!(d.step(src, role_end, true), grace);
        let hopper_end = src.find("Hopper").unwrap() + 6;
        assert_eq!(
            d.step(src, hopper_end, true),
            hopper_end + 3,
            "into the empty cell"
        );
    }

    #[test]
    fn stepping_takes_an_object_whole() {
        let src = "Text\n\n![alt](x.png)\n\nmore\n";
        let d = map(src, &Options::default());
        let img = src.find('!').unwrap();
        let end = src.find(")\n").unwrap() + 1;
        assert_eq!(d.step(src, img, true), end);
        assert_eq!(d.step(src, end, false), img);
    }
}
