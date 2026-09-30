//! Editing commands with rich-text semantics on top of Markdown source.
//!
//! The view hides Markdown syntax, so a keystroke has to be interpreted the
//! way a word processor would: Backspace at the start of a heading turns it
//! into a paragraph, Enter in a list starts a new item, deleting the last
//! letter of a bold word removes its `**` too. Each command returns a
//! [`Plan`] of minimal source changes; untouched text is never rewritten.

// Deletions are lists of byte ranges, often of one range.
#![allow(clippy::single_range_in_vec_init)]

use super::doc::{Container, Doc, InlineKind, LineKind};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub range: Range<usize>,
    pub text: String,
}

impl Change {
    pub fn insert(at: usize, text: impl Into<String>) -> Self {
        Change {
            range: at..at,
            text: text.into(),
        }
    }

    pub fn delete(range: Range<usize>) -> Self {
        Change {
            range,
            text: String::new(),
        }
    }

    pub fn replace(range: Range<usize>, text: impl Into<String>) -> Self {
        Change {
            range,
            text: text.into(),
        }
    }
}

/// Source changes plus where the cursor ends up.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// Sorted, non-overlapping changes against the current source.
    pub changes: Vec<Change>,
    /// Cursor position in the resulting text.
    pub cursor: usize,
    /// Selection in the resulting text, if the command selects something.
    pub selection: Option<Range<usize>>,
}

impl Plan {
    /// A plan whose cursor is given in *old* coordinates and mapped through
    /// the changes. Insertions exactly at the cursor push it forward when
    /// `stick_after` is set.
    fn mapped(changes: Vec<Change>, old_cursor: usize, stick_after: bool) -> Plan {
        let changes = normalize(changes);
        let cursor = map_pos(&changes, old_cursor, stick_after);
        Plan {
            changes,
            cursor,
            selection: None,
        }
    }

    /// A plan whose cursor is already in new coordinates.
    fn at(changes: Vec<Change>, cursor: usize) -> Plan {
        Plan {
            changes: normalize(changes),
            cursor,
            selection: None,
        }
    }

    pub fn cursor_only(cursor: usize) -> Plan {
        Plan {
            changes: Vec::new(),
            cursor,
            selection: None,
        }
    }

    pub fn apply(&self, src: &str) -> String {
        let mut out = String::with_capacity(src.len() + 16);
        let mut pos = 0;
        for c in &self.changes {
            out.push_str(&src[pos..c.range.start]);
            out.push_str(&c.text);
            pos = c.range.end;
        }
        out.push_str(&src[pos..]);
        out
    }
}

/// Sorts changes and merges ones that touch, so they can be applied in one
/// pass. Insertions at the edge of a deletion fold into it.
fn normalize(mut changes: Vec<Change>) -> Vec<Change> {
    changes.retain(|c| !(c.range.is_empty() && c.text.is_empty()));
    changes.sort_by_key(|c| (c.range.start, c.range.end));
    let mut out: Vec<Change> = Vec::with_capacity(changes.len());
    for c in changes {
        match out.last_mut() {
            Some(last) if c.range.start <= last.range.end => {
                last.range.end = last.range.end.max(c.range.end);
                last.text.push_str(&c.text);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Maps a position in the old text through sorted changes.
pub fn map_pos(changes: &[Change], pos: usize, stick_after: bool) -> usize {
    let mut delta: isize = 0;
    for c in changes {
        let before =
            c.range.end < pos || (c.range.end == pos && (!c.range.is_empty() || stick_after));
        if before {
            delta += c.text.len() as isize - c.range.len() as isize;
        } else if c.range.start < pos {
            // Inside a replaced range: land at the end of the new text.
            return (c.range.start as isize + delta) as usize + c.text.len();
        } else {
            break;
        }
    }
    (pos as isize + delta) as usize
}

// ---------------------------------------------------------------------------
// Positions

/// The canonical position for a cursor location: never inside hidden block
/// syntax (moved to where the text starts), and never strictly inside a
/// run of hidden inline syntax (moved to its left edge). At either edge of
/// such a run the position is kept: after typing a closing `**` the cursor
/// is outside the bold text, and arrow keys, which stop at left edges, put
/// it inside, so typing there continues the bold text.
pub fn visual_pos(doc: &Doc, pos: usize) -> usize {
    let pos = pos.min(doc.len);
    let line = doc.line_at(pos);
    if line.kind == LineKind::Blank {
        return pos;
    }
    let pos = pos.max(line.content_start);
    match doc.hidden_run_at(pos) {
        Some(run) if run.start < pos && pos < run.end => run.start,
        _ => pos,
    }
}

/// Moves `p` back over hidden syntax directly before it.
fn skip_hidden_back(doc: &Doc, mut p: usize, floor: usize) -> usize {
    while p > floor {
        match doc.hidden_run_at(p) {
            Some(run) if run.end == p && run.start < p => p = run.start.max(floor),
            _ => break,
        }
    }
    p
}

/// Whether `p` is at the visual start of line `li` (only hidden syntax
/// before it).
pub fn at_visual_start(doc: &Doc, li: usize, p: usize) -> bool {
    let l = &doc.lines[li];
    p <= l.content_start || (l.content_start..p).all(|b| doc.is_hidden_byte(b))
}

/// Where typed text goes. Like [`visual_pos`], except that text typed right
/// after a link is not added to the link.
pub fn insertion_point(doc: &Doc, pos: usize) -> usize {
    let mut p = visual_pos(doc, pos);
    while let Some(e) = doc.inlines.iter().find(|e| {
        matches!(e.kind, InlineKind::Link | InlineKind::Image)
            && e.close.start == p
            && !e.close.is_empty()
    }) {
        p = e.close.end;
    }
    p
}

/// Moves `p` past closing emphasis markers that end right at `p`. Markdown
/// cannot express whitespace just inside a closing marker.
fn skip_closers(doc: &Doc, mut p: usize) -> usize {
    while let Some(e) = doc.inlines.iter().find(|e| {
        e.kind != InlineKind::Code && e.close.start == p && e.open.end < p && !e.close.is_empty()
    }) {
        p = e.close.end;
    }
    p
}

/// Moves `p` before opening emphasis markers that end at `p`. Markdown
/// cannot express whitespace just inside an opening marker.
fn skip_openers(doc: &Doc, mut p: usize) -> usize {
    while let Some(e) = doc.inlines.iter().find(|e| {
        e.kind != InlineKind::Code && e.open.end == p && p < e.close.start && !e.open.is_empty()
    }) {
        p = e.open.start;
    }
    p
}

fn prev_grapheme(src: &str, lo: usize, p: usize) -> usize {
    src[lo..p]
        .grapheme_indices(true)
        .next_back()
        .map_or(lo, |(i, _)| lo + i)
}

fn next_grapheme(src: &str, p: usize, hi: usize) -> usize {
    src[p..hi]
        .graphemes(true)
        .next()
        .map_or(hi, |g| p + g.len())
}

/// Replaces everything but `>` with spaces: the prefix a continuation line
/// needs to stay in the same containers.
fn blank_out(s: &str) -> String {
    s.chars()
        .map(|c| if c == '>' { '>' } else { ' ' })
        .collect()
}

/// The prefix a new line needs to continue the innermost container of
/// `line` (e.g. `> ` in a quote, two spaces in a `- ` list item).
pub fn continuation(src: &str, doc: &Doc, line: usize) -> String {
    match doc.lines[line].containers.last() {
        None => String::new(),
        Some(Container::Item(i)) => {
            let it = &doc.items[*i];
            let first = &doc.lines[it.line];
            blank_out(&src[first.start..it.marker.end])
        }
        Some(Container::Quote(q)) => {
            let qt = &doc.quotes[*q];
            let first = &doc.lines[qt.first_line];
            let mut s = blank_out(&src[first.start..(qt.range.start + 1).min(first.end)]);
            s.push(' ');
            s
        }
    }
}

/// Marker text for a new sibling after list item `i`, including the
/// prefix of enclosing containers.
fn next_item_marker(src: &str, doc: &Doc, i: usize) -> String {
    let it = &doc.items[i];
    let first = &doc.lines[it.line];
    let outer = blank_out(&src[first.start..it.marker.start]);
    let marker = &src[it.marker.clone()];
    let body = if doc.lists[it.list].ordered {
        let digits = marker.bytes().take_while(u8::is_ascii_digit).count();
        let n: u64 = marker[..digits].parse().unwrap_or(1);
        let same = list_repeats_number(src, doc, it.list);
        format!("{}{}", if same { n } else { n + 1 }, &marker[digits..])
    } else {
        marker.to_string()
    };
    let body = if body.ends_with(' ') {
        body
    } else {
        format!("{body} ")
    };
    let task = if it.task.is_some() { "[ ] " } else { "" };
    format!("{outer}{body}{task}")
}

/// Whether an ordered list numbers every item the same (`1.` `1.` `1.`).
fn list_repeats_number(src: &str, doc: &Doc, list: usize) -> bool {
    let items = &doc.lists[list].items;
    if items.len() < 2 {
        return false;
    }
    let number = |i: usize| -> &str {
        let m = &src[doc.items[i].marker.clone()];
        let d = m.bytes().take_while(u8::is_ascii_digit).count();
        &m[..d]
    };
    let first = number(items[0]);
    items.iter().all(|&i| number(i) == first)
}

/// End of the visible content of `line`: trailing hidden syntax excluded.
fn visible_end(doc: &Doc, line: usize) -> usize {
    let l = &doc.lines[line];
    let mut end = l.end;
    while end > l.content_start {
        match doc.hidden_run_at(end) {
            Some(run) if run.start < end && run.end == end => end = run.start.max(l.content_start),
            _ => break,
        }
    }
    end
}

fn has_visible_content(doc: &Doc, line: usize) -> bool {
    visible_end(doc, line) > doc.lines[line].content_start
}

/// Closing and reopening syntax for inline elements that span `p`, so a
/// split keeps both halves formatted.
fn split_inlines(src: &str, doc: &Doc, p: usize) -> (String, String) {
    let mut spanning: Vec<usize> = (0..doc.inlines.len())
        .filter(|&i| {
            let e = &doc.inlines[i];
            e.open.end < p && p < e.close.start
        })
        .collect();
    spanning.sort_by_key(|&i| std::cmp::Reverse(doc.inlines[i].range().len()));
    let open: String = spanning
        .iter()
        .map(|&i| &src[doc.inlines[i].open.clone()])
        .collect();
    let close: String = spanning
        .iter()
        .rev()
        .map(|&i| &src[doc.inlines[i].close.clone()])
        .collect();
    (close, open)
}

/// Moves a split point off the inner edge of an inline element: splitting
/// right after `**` or right before the closing `**` would leave an empty
/// element, i.e. literal markers, on one side.
fn split_point(doc: &Doc, mut p: usize) -> usize {
    loop {
        if let Some(e) = doc
            .inlines
            .iter()
            .find(|e| !e.close.is_empty() && e.close.start == p && e.open.end < p)
        {
            p = e.close.end;
        } else if let Some(e) = doc
            .inlines
            .iter()
            .find(|e| !e.open.is_empty() && e.open.end == p && p < e.close.start)
        {
            p = e.open.start;
        } else {
            return p;
        }
    }
}

/// Where the line break ending `line` starts: before a hard-break marker
/// (`\` or two trailing spaces) if it has one, else at the newline.
fn break_start(src: &str, line: &super::doc::Line) -> usize {
    let text = &src[line.start..line.end];
    let slashes = text.bytes().rev().take_while(|&c| c == b'\\').count();
    if slashes % 2 == 1 {
        return line.end - 1;
    }
    let spaces = text.bytes().rev().take_while(|&c| c == b' ').count();
    if spaces >= 2 && spaces < text.len() {
        return line.end - spaces;
    }
    line.end
}

// ---------------------------------------------------------------------------
// Typing

pub fn insert(_src: &str, doc: &Doc, pos: usize, text: &str) -> Plan {
    let li = doc.line_index(pos.min(doc.len));
    let line = &doc.lines[li];
    let mut p = insertion_point(doc, pos);
    if text.starts_with(char::is_whitespace) {
        p = skip_openers(doc, skip_closers(doc, p));
    }
    let mut t = String::new();
    if line.kind == LineKind::Blank && !text.starts_with('\n') && joins_previous(doc, li) {
        // Without a blank line the text would continue the block above.
        t.push('\n');
    }
    t.push_str(text);
    Plan::at(vec![Change::insert(p, t.clone())], p + t.len())
}

fn joins_previous(doc: &Doc, li: usize) -> bool {
    li > 0
        && !ends_with_hard_break(doc, li - 1)
        && matches!(
            doc.lines[li - 1].kind,
            LineKind::Paragraph | LineKind::Table | LineKind::Html | LineKind::Raw
        )
}

/// Whether `line` ends in a hard line break (Shift+Enter), so that text on
/// the next line continues the same paragraph.
fn ends_with_hard_break(doc: &Doc, line: usize) -> bool {
    let l = &doc.lines[line];
    l.kind == LineKind::Paragraph
        && doc.hidden_run_at(l.end).is_some_and(|r| r.end == l.end)
        && doc
            .hidden
            .iter()
            .any(|r| r.end == l.end && r.start + 1 == l.end)
}

/// Enter. `soft` (Shift+Enter) inserts a line break inside the paragraph.
pub fn newline(src: &str, doc: &Doc, pos: usize, soft: bool) -> Plan {
    let li = doc.line_index(pos.min(doc.len));
    let line = doc.lines[li].clone();
    let cont = continuation(src, doc, li);
    let blank = cont.trim_end().to_string();
    let ins = |at: usize, text: String, cursor_in_text: usize| {
        Plan::at(vec![Change::insert(at, text)], at + cursor_in_text)
    };

    match line.kind {
        LineKind::CodeContent => {
            let p = pos.clamp(line.content_start, line.end);
            let indent: String = src[line.content_start..p]
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            let t = format!("\n{cont}{indent}");
            let n = t.len();
            return ins(p, t, n);
        }
        LineKind::Fence => {
            let Some(ci) = doc.code_block_at_line(li) else {
                return ins(line.end, "\n".into(), 1);
            };
            let cb = &doc.code_blocks[ci];
            if cb.open_line == Some(li) {
                if cb.close_line.is_none() {
                    let text = &src[line.content_start..line.end];
                    let c = text.chars().next().unwrap_or('`');
                    let fence: String = text.chars().take_while(|&x| x == c).collect();
                    let t = format!("\n{cont}\n{cont}{fence}");
                    return ins(line.end, t, 1 + cont.len());
                }
                let t = format!("\n{cont}");
                let n = t.len();
                return ins(line.end, t, n);
            }
            let t = format!("\n{blank}\n{cont}");
            let n = t.len();
            return ins(line.end, t, n);
        }
        LineKind::Blank => return ins(pos, "\n".into(), 1),
        LineKind::Rule | LineKind::SetextUnderline => {
            return ins(line.end, "\n\n".into(), 2);
        }
        LineKind::Table | LineKind::Html | LineKind::FrontMatter | LineKind::Raw => {
            let p = pos.clamp(line.start, line.end);
            let t = format!("\n{cont}");
            let n = t.len();
            return ins(p, t, n);
        }
        LineKind::Heading(_) | LineKind::Paragraph => {}
    }

    let p = split_point(doc, insertion_point(doc, pos)).min(line.end);
    // A heading cannot hold a line break; Shift+Enter there acts as Enter.
    if soft && line.kind == LineKind::Paragraph {
        let t = format!("\\\n{cont}");
        let n = t.len();
        return ins(p, t, n);
    }
    let content = has_visible_content(doc, li);
    let vend = visible_end(doc, li);
    let at_start = at_visual_start(doc, li, p);
    let at_end = p >= vend;

    if let Some(Container::Item(i)) = line.containers.last() {
        let i = *i;
        let item = &doc.items[i];
        let item_last_line = doc.lines[..]
            .iter()
            .rposition(|l| l.containers.contains(&Container::Item(i)) && l.kind != LineKind::Blank)
            .unwrap_or(li);
        if item.line == li && !content && item_last_line == li {
            return exit_list_item(src, doc, i, li);
        }
        let marker = next_item_marker(src, doc, i);
        let renumber = renumber_after(src, doc, i, 1);
        let (range, t) = if at_end {
            (line.end..line.end, format!("\n{marker}"))
        } else {
            let (close, open) = split_inlines(src, doc, p);
            let before = &src[line.content_start..p];
            let after = &src[p..line.end];
            let ws = (
                before.len() - before.trim_end().len(),
                after.len() - after.trim_start().len(),
            );
            (p - ws.0..p + ws.1, format!("{close}\n{marker}{open}"))
        };
        let n = t.len();
        let at = range.start;
        let mut changes = vec![Change::replace(range, t)];
        changes.extend(renumber);
        return Plan::at(changes, at + n);
    }

    let heading = match line.kind {
        LineKind::Heading(level) => Some(level),
        _ => None,
    };
    if at_start && content {
        // Open an empty line above; the cursor goes there.
        let t = format!("{cont}\n{blank}\n");
        return ins(line.start, t, cont.len());
    }
    if at_end || !content {
        let t = format!("\n{blank}\n{cont}");
        let n = t.len();
        return ins(line.end, t, n);
    }
    let (close, open) = split_inlines(src, doc, p);
    let marker = heading.map_or(String::new(), |l| format!("{} ", "#".repeat(l as usize)));
    // Whitespace at the split point would end up at the edge of a line.
    let before_ws = src[line.content_start..p].len() - src[line.content_start..p].trim_end().len();
    let after_ws = src[p..line.end].len() - src[p..line.end].trim_start().len();
    let t = format!("{close}\n{blank}\n{cont}{marker}{open}");
    let n = t.len();
    Plan::at(
        vec![Change::replace(p - before_ws..p + after_ws, t)],
        p - before_ws + n,
    )
}

/// Renumbers the items after item `i` in its ordered list by `delta`, if the
/// list is numbered in sequence (not `1.` `1.` `1.`), to keep the source
/// readable when an item is added or removed.
fn renumber_after(src: &str, doc: &Doc, i: usize, delta: i64) -> Vec<Change> {
    let item = &doc.items[i];
    let list = &doc.lists[item.list];
    if !list.ordered || list_repeats_number(src, doc, item.list) {
        return Vec::new();
    }
    let number = |k: usize| -> Option<(Range<usize>, i64)> {
        let m = doc.items[k].marker.clone();
        let d = src[m.clone()]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        let n = src[m.start..m.start + d].parse().ok()?;
        Some((m.start..m.start + d, n))
    };
    let sequential = list
        .items
        .windows(2)
        .all(|w| match (number(w[0]), number(w[1])) {
            (Some((_, a)), Some((_, b))) => b == a + 1,
            _ => false,
        });
    if !sequential {
        return Vec::new();
    }
    list.items
        .iter()
        .skip_while(|&&k| k != i)
        .skip(1)
        .filter_map(|&k| number(k))
        .map(|(r, n)| Change::replace(r, (n + delta).max(0).to_string()))
        .collect()
}

fn exit_list_item(src: &str, doc: &Doc, i: usize, li: usize) -> Plan {
    let line = &doc.lines[li];
    let item = &doc.items[i];
    let parent = line
        .containers
        .iter()
        .rev()
        .skip_while(|c| **c != Container::Item(i))
        .skip(1)
        .find_map(|c| match c {
            Container::Item(p) => Some(*p),
            Container::Quote(_) => None,
        });
    if let (Some(parent), true) = (parent, item.depth > 1) {
        // Outdent: become a sibling of the parent item.
        let marker = next_item_marker(src, doc, parent);
        let n = marker.len();
        return Plan::at(
            vec![Change::replace(line.start..line.end, marker)],
            line.start + n,
        );
    }
    let outer = blank_out(&src[line.start..item.marker.start]);
    let t = format!("\n{outer}");
    let n = t.len();
    Plan::at(
        vec![Change::replace(line.start..line.end, t)],
        line.start + n,
    )
}

// ---------------------------------------------------------------------------
// Deleting

/// Deletes `del` while keeping the result well-formed: markers whose partner
/// survives are kept, elements left empty lose their markers, and
/// whitespace exposed at an element's edge moves outside it.
fn delete_ranges(src: &str, doc: &Doc, del: Vec<Range<usize>>, old_cursor: usize) -> Plan {
    let mut del = merge_ranges(del);
    let mut order: Vec<usize> = (0..doc.inlines.len()).collect();
    order.sort_by_key(|&i| doc.inlines[i].range().len());

    for &i in &order {
        let e = &doc.inlines[i];
        let open_hit = intersects(&del, &e.open);
        let close_hit = intersects(&del, &e.close);
        let open_all = covers(&del, &e.open);
        let close_all = covers(&del, &e.close);
        if open_all && close_all {
            continue;
        }
        if open_hit {
            del = subtract(del, &e.open);
        }
        if close_hit {
            del = subtract(del, &e.close);
        }
    }

    let mut inserts: Vec<Change> = Vec::new();
    // Where the cursor goes when whitespace next to it moved out of an element.
    let mut cursor_override: Option<(usize, bool)> = None;
    for &i in &order {
        let e = &doc.inlines[i];
        if covers(&del, &e.open) && covers(&del, &e.close) {
            continue;
        }
        let content = e.content();
        if !intersects(&del, &content) {
            continue;
        }
        // Remaining content, as (position, char) pairs.
        let remaining: Vec<(usize, char)> = src[content.clone()]
            .char_indices()
            .map(|(o, c)| (content.start + o, c))
            .filter(|(p, _)| !contains(&del, *p))
            .collect();
        let visible = remaining
            .iter()
            .any(|(p, c)| !c.is_whitespace() && !doc.is_hidden_byte(*p));
        if !visible && !(e.kind == InlineKind::Code && !remaining.is_empty()) {
            del.push(e.open.clone());
            del.push(e.close.clone());
            del = merge_ranges(del);
            continue;
        }
        if e.kind == InlineKind::Code {
            continue;
        }
        let lead: Vec<(usize, char)> = remaining
            .iter()
            .take_while(|(_, c)| c.is_whitespace())
            .copied()
            .collect();
        let trail: Vec<(usize, char)> = remaining
            .iter()
            .rev()
            .take_while(|(_, c)| c.is_whitespace())
            .copied()
            .collect();
        if !lead.is_empty() {
            for (p, c) in &lead {
                del.push(*p..*p + c.len_utf8());
            }
            inserts.push(Change::insert(
                e.open.start,
                lead.iter().map(|(_, c)| c).collect::<String>(),
            ));
            if old_cursor <= lead[0].0 {
                cursor_override = Some((e.open.start, false));
            }
        }
        if !trail.is_empty() {
            for (p, c) in &trail {
                del.push(*p..*p + c.len_utf8());
            }
            inserts.push(Change::insert(
                e.close.end,
                trail.iter().rev().map(|(_, c)| c).collect::<String>(),
            ));
            let (p, c) = trail[0];
            if old_cursor >= p + c.len_utf8() {
                cursor_override = Some((e.close.end, true));
            }
        }
        del = merge_ranges(del);
    }

    let mut changes: Vec<Change> = del.into_iter().map(Change::delete).collect();
    changes.extend(inserts);
    match cursor_override {
        Some((pos, after)) => Plan::mapped(changes, pos, after),
        None => Plan::mapped(changes, old_cursor, false),
    }
}

fn merge_ranges(mut v: Vec<Range<usize>>) -> Vec<Range<usize>> {
    v.retain(|r| !r.is_empty());
    v.sort_by_key(|r| r.start);
    let mut out: Vec<Range<usize>> = Vec::with_capacity(v.len());
    for r in v {
        match out.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}

fn intersects(set: &[Range<usize>], r: &Range<usize>) -> bool {
    set.iter().any(|s| s.start < r.end && r.start < s.end)
}

fn covers(set: &[Range<usize>], r: &Range<usize>) -> bool {
    r.is_empty() || set.iter().any(|s| s.start <= r.start && r.end <= s.end)
}

fn contains(set: &[Range<usize>], p: usize) -> bool {
    set.iter().any(|s| s.start <= p && p < s.end)
}

fn subtract(set: Vec<Range<usize>>, r: &Range<usize>) -> Vec<Range<usize>> {
    let mut out = Vec::with_capacity(set.len() + 1);
    for s in set {
        if s.end <= r.start || r.end <= s.start {
            out.push(s);
            continue;
        }
        if s.start < r.start {
            out.push(s.start..r.start);
        }
        if r.end < s.end {
            out.push(r.end..s.end);
        }
    }
    out
}

fn prev_visible_line(doc: &Doc, li: usize) -> Option<usize> {
    (0..li)
        .rev()
        .find(|&i| doc.lines[i].kind != LineKind::Blank)
}

fn next_visible_line(doc: &Doc, li: usize) -> Option<usize> {
    (li + 1..doc.lines.len()).find(|&i| doc.lines[i].kind != LineKind::Blank)
}

pub fn backspace(src: &str, doc: &Doc, pos: usize) -> Plan {
    let p = visual_pos(doc, pos);
    let li = doc.line_index(p);
    let line = &doc.lines[li];
    if line.kind == LineKind::Blank {
        // Back to the end of the text above, over hidden blank lines, so
        // that Enter then Backspace is a round trip.
        return match prev_visible_line(doc, li) {
            Some(pi) => {
                let end = doc.lines[pi].end;
                Plan::mapped(vec![Change::delete(end..line.end)], end, false)
            }
            None if li > 0 => {
                let prev = &doc.lines[li - 1];
                Plan::mapped(vec![Change::delete(prev.end..line.end)], prev.end, false)
            }
            None => Plan::cursor_only(p),
        };
    }
    if at_visual_start(doc, li, p) {
        return backspace_at_start(src, doc, li);
    }
    let q = skip_hidden_back(doc, p, line.content_start);
    let start = prev_grapheme(src, line.content_start, q);
    delete_ranges(src, doc, vec![start..q], start)
}

fn backspace_at_start(src: &str, doc: &Doc, li: usize) -> Plan {
    let line = doc.lines[li].clone();
    let prefix = &src[line.start..line.content_start];

    if let LineKind::Heading(_) = line.kind
        && let Some(h) = prefix.find('#')
    {
        let h = line.start + h;
        return Plan::mapped(vec![Change::delete(h..line.content_start)], h, false);
    }

    if let Some(i) = doc.item_on_line(li)
        && line.containers.last() == Some(&Container::Item(i))
    {
        let it = &doc.items[i];
        if let Some((_, task)) = &it.task {
            return Plan::mapped(
                vec![Change::delete(task.start..line.content_start)],
                task.start,
                false,
            );
        }
        if it.depth > 1 {
            let parent = line.containers.iter().rev().skip(1).find_map(|c| match c {
                Container::Item(p) => Some(*p),
                Container::Quote(_) => None,
            });
            if let Some(parent) = parent {
                let pit = &doc.items[parent];
                let pcol = pit.marker.start - doc.lines[pit.line].start;
                let col = it.marker.start - line.start;
                let remove = col.saturating_sub(pcol);
                let from = it.marker.start - remove;
                if src[from..it.marker.start].bytes().all(|b| b == b' ') && remove > 0 {
                    return Plan::mapped(
                        vec![Change::delete(from..it.marker.start)],
                        line.content_start,
                        false,
                    );
                }
            }
        }
        // A later item becomes a paragraph of the item above it (a blank
        // line and the item's indentation); without the blank line it
        // would just continue the paragraph above.
        let siblings = &doc.lists[it.list].items;
        let pos = siblings.iter().position(|&k| k == i).unwrap_or(0);
        if pos > 0 {
            let prev = &doc.items[siblings[pos - 1]];
            let prev_line = &doc.lines[prev.line];
            let cont = blank_out(&src[prev_line.start..prev.marker.end]);
            let t = format!("{}\n{cont}", cont.trim_end());
            let n = t.len();
            return Plan::at(
                vec![Change::replace(line.start..line.content_start, t)],
                line.start + n,
            );
        }
        return Plan::mapped(
            vec![Change::delete(it.marker.start..line.content_start)],
            it.marker.start,
            false,
        );
    }

    if let Some(Container::Quote(_)) = line.containers.last()
        && prefix.contains('>')
        && let Some(bi) = doc.block_at_line(li)
        && doc.blocks[bi].first_line == li
    {
        let b = &doc.blocks[bi];
        let mut changes = Vec::new();
        for l in b.first_line..=b.last_line {
            let ln = &doc.lines[l];
            let pre = &src[ln.start..ln.content_start];
            if let Some(gt) = pre.rfind('>') {
                let mut end = ln.start + gt + 1;
                if src.as_bytes().get(end) == Some(&b' ') && end < ln.content_start {
                    end += 1;
                }
                changes.push(Change::delete(ln.start + gt..end));
            }
        }
        return Plan::mapped(changes, line.content_start, false);
    }

    if line.kind == LineKind::CodeContent {
        let ci = doc.code_block_at_line(li);
        if let Some(ci) = ci {
            let cb = &doc.code_blocks[ci];
            let content = cb.content_lines();
            if content.start == li {
                let empty = content.clone().all(|l| {
                    src[doc.lines[l].content_start..doc.lines[l].end]
                        .trim()
                        .is_empty()
                });
                if empty && cb.fenced {
                    let start = doc.lines[cb.first_line].start;
                    let end = doc.line_end_incl(cb.last_line);
                    return Plan::mapped(vec![Change::delete(start..end)], start, false);
                }
                return Plan::cursor_only(line.content_start);
            }
        }
        let prev = &doc.lines[li - 1];
        return Plan::mapped(
            vec![Change::delete(prev.end..line.content_start)],
            prev.end,
            false,
        );
    }

    let Some(pi) = prev_visible_line(doc, li) else {
        return Plan::cursor_only(line.content_start);
    };
    let prev = &doc.lines[pi];
    match prev.kind {
        LineKind::Rule => Plan::mapped(
            vec![Change::delete(prev.start..line.start)],
            line.content_start,
            false,
        ),
        LineKind::Fence | LineKind::CodeContent => {
            let target = doc
                .code_block_at_line(pi)
                .map(|ci| doc.code_blocks[ci].content_lines())
                .filter(|r| !r.is_empty())
                .map_or(prev.end, |r| doc.lines[r.end - 1].end);
            Plan::cursor_only(target)
        }
        LineKind::Table | LineKind::Html | LineKind::FrontMatter | LineKind::SetextUnderline => {
            Plan::cursor_only(prev.end)
        }
        _ => {
            let from = break_start(src, prev);
            let join = visible_end(doc, pi).min(from);
            Plan::mapped(vec![Change::delete(from..line.content_start)], join, false)
        }
    }
}

pub fn delete_forward(src: &str, doc: &Doc, pos: usize) -> Plan {
    let p = visual_pos(doc, pos);
    let li = doc.line_index(p);
    let line = &doc.lines[li];
    let mut q = p;
    while let Some(run) = doc.hidden_run_at(q) {
        if run.start == q && run.end > q {
            q = run.end;
        } else {
            break;
        }
    }
    if q >= line.end {
        let Some(ni) = next_visible_line(doc, li) else {
            return Plan::cursor_only(p);
        };
        let next = &doc.lines[ni];
        return match next.kind {
            LineKind::Rule => Plan::mapped(vec![Change::delete(line.end..next.end)], p, false),
            LineKind::Paragraph | LineKind::Heading(_) if line.kind != LineKind::CodeContent => {
                let from = break_start(src, line).max(p);
                Plan::mapped(vec![Change::delete(from..next.content_start)], p, false)
            }
            LineKind::CodeContent if line.kind == LineKind::CodeContent => {
                Plan::mapped(vec![Change::delete(line.end..next.content_start)], p, false)
            }
            _ => Plan::cursor_only(p),
        };
    }
    let end = next_grapheme(src, q, line.end);
    delete_ranges(src, doc, vec![q..end], p)
}

/// Replaces a find match in place when it lies within one run of plain
/// text (soft line breaks included), so formatting around it is kept.
/// Returns `None` when the match spans hidden syntax; then it has to be
/// deleted and retyped.
pub fn replace_plain(src: &str, doc: &Doc, range: Range<usize>, with: &str) -> Option<Plan> {
    if range.clone().any(|p| doc.is_hidden_byte(p)) {
        return None;
    }
    for (off, _) in src[range.clone()].match_indices('\n') {
        let nl = range.start + off;
        let next = doc.line_index(nl + 1);
        if doc.soft_breaks.binary_search(&nl).is_err() || doc.lines[next].content_start != nl + 1 {
            return None;
        }
    }
    Some(Plan::at(
        vec![Change::replace(range.clone(), with)],
        range.start + with.len(),
    ))
}

/// Deletes a selection with word-processor semantics.
pub fn delete_range(src: &str, doc: &Doc, range: Range<usize>) -> Plan {
    if range.is_empty() {
        return Plan::cursor_only(visual_pos(doc, range.start));
    }
    let la = doc.line_index(range.start);
    let lb = doc.line_index(range.end);
    let a_line = &doc.lines[la];
    let b_line = &doc.lines[lb];
    let a_at_start = a_line.kind == LineKind::Blank || at_visual_start(doc, la, range.start);
    let b_at_start =
        lb > la && (range.end <= b_line.content_start || b_line.kind == LineKind::Blank);
    if a_at_start && b_at_start {
        // Whole lines: the following line keeps its own block type.
        return delete_ranges(src, doc, vec![a_line.start..b_line.start], a_line.start);
    }
    let a = if a_line.kind == LineKind::Blank {
        range.start
    } else {
        visual_pos(doc, range.start)
    };
    let b = if b_at_start {
        b_line.content_start
    } else {
        visual_pos(doc, range.end).max(a)
    };
    let b = extend_over_trailing_hidden(doc, b, lb);
    // From the start of a paragraph or heading, the whitespace after the
    // selection would start the block: Markdown hides it (and four spaces
    // would turn a paragraph into code), so it goes too.
    let b = if matches!(a_line.kind, LineKind::Paragraph | LineKind::Heading(_))
        && at_visual_start(doc, la, a)
    {
        b + src[b..]
            .bytes()
            .take_while(|&c| c == b' ' || c == b'\t')
            .count()
    } else {
        b
    };
    delete_ranges(src, doc, vec![a..b], a)
}

/// Typing or pasting over a selection. In place when the selection lies in
/// plain text, so the formatting and spacing around it stay (as in a word
/// processor: typing over a bold word types bold); otherwise the selection
/// is deleted and the text typed.
pub fn replace_range(src: &str, doc: &Doc, range: Range<usize>, text: &str) -> Plan {
    if range.is_empty() {
        return insert(src, doc, range.start, text);
    }
    if let Some(p) = replace_plain(src, doc, range.clone(), text) {
        return p;
    }
    let deleted = delete_range(src, doc, range);
    let after = deleted.apply(src);
    let typed = insert(&after, &super::parse(&after), deleted.cursor, text);
    let result = typed.apply(&after);
    Plan::at(crate::diff::diff_changes(src, &result), typed.cursor)
}

/// If `b` sits before hidden syntax that ends its line, include the syntax
/// so that deleting to the end of a line removes e.g. a heading's closing
/// `##` and lets empty elements lose their markers.
fn extend_over_trailing_hidden(doc: &Doc, b: usize, lb: usize) -> usize {
    let l = &doc.lines[lb];
    if b >= l.content_start && visible_end(doc, lb) <= b {
        l.end.max(b)
    } else {
        b
    }
}

// ---------------------------------------------------------------------------
// Formatting

fn marker_for(kind: InlineKind) -> Option<&'static str> {
    match kind {
        InlineKind::Strong => Some("**"),
        InlineKind::Emphasis => Some("*"),
        InlineKind::Strike => Some("~~"),
        InlineKind::Code => Some("`"),
        InlineKind::Link | InlineKind::Image => None,
    }
}

/// The word around `pos`, if the cursor is in or at the end of one.
pub fn word_at(src: &str, doc: &Doc, pos: usize) -> Option<Range<usize>> {
    let li = doc.line_index(pos);
    let l = &doc.lines[li];
    let text = &src[l.content_start..l.end];
    let rel = pos.saturating_sub(l.content_start).min(text.len());
    text.split_word_bound_indices()
        .filter(|(_, w)| w.chars().any(char::is_alphanumeric))
        .map(|(i, w)| l.content_start + i..l.content_start + i + w.len())
        .find(|r| r.start <= l.content_start + rel && l.content_start + rel <= r.end)
}

/// Shrinks `a..b` past hidden syntax and whitespace at both ends.
pub fn trim_segment(src: &str, doc: &Doc, mut a: usize, mut b: usize) -> Option<Range<usize>> {
    loop {
        let (a0, b0) = (a, b);
        if a < b {
            if let Some(run) = doc.hidden_run_at(a).filter(|r| r.start <= a && a < r.end) {
                a = run.end.min(b);
            } else if let Some(c) = src[a..b].chars().next().filter(|c| c.is_whitespace()) {
                a += c.len_utf8();
            }
        }
        if a < b {
            let last = src[a..b].chars().next_back().unwrap();
            let lp = b - last.len_utf8();
            if doc.is_hidden_byte(lp) {
                b = doc.hidden_run_at(lp).map_or(lp, |r| r.start.max(a));
            } else if last.is_whitespace() {
                b = lp;
            }
        }
        if (a, b) == (a0, b0) {
            break;
        }
    }
    (a < b).then_some(a..b)
}

/// Toggles bold/italic/strike/code on a selection (or the word at the cursor).
pub fn toggle_inline(src: &str, doc: &Doc, sel: Range<usize>, kind: InlineKind) -> Plan {
    let Some(marker) = marker_for(kind) else {
        return Plan::cursor_only(sel.end);
    };
    let sel = if sel.is_empty() {
        match word_at(src, doc, sel.start) {
            Some(w) => w,
            None => return Plan::cursor_only(sel.start),
        }
    } else {
        visual_pos(doc, sel.start)..visual_pos(doc, sel.end)
    };

    // One segment per line, trimmed to visible, non-whitespace text.
    let mut segments = Vec::new();
    for li in doc.line_index(sel.start)..=doc.line_index(sel.end) {
        let l = &doc.lines[li];
        if !matches!(l.kind, LineKind::Paragraph | LineKind::Heading(_)) {
            continue;
        }
        let a = sel.start.max(l.content_start);
        let b = sel.end.min(visible_end(doc, li));
        if let Some(seg) = trim_segment(src, doc, a, b) {
            segments.push(seg);
        }
    }
    if segments.is_empty() {
        return Plan::cursor_only(sel.end);
    }

    let enclosing = |r: &Range<usize>| {
        doc.inlines
            .iter()
            .filter(|e| e.kind == kind)
            .find(|e| e.content().start <= r.start && r.end <= e.content().end)
            .cloned()
    };
    let remove = segments.iter().all(|s| enclosing(s).is_some());
    let mut changes = Vec::new();

    if remove {
        for s in &segments {
            let e = enclosing(s).unwrap();
            let c = e.content();
            let before = &src[c.start..s.start];
            let after = &src[s.end..c.end];
            if before.trim().is_empty() {
                changes.push(Change::delete(e.open.clone()));
            } else {
                changes.push(Change::replace(e.open.clone(), marker));
                let ws = before.len() - before.trim_end().len();
                changes.push(Change::insert(s.start - ws, marker));
            }
            if after.trim().is_empty() {
                changes.push(Change::delete(e.close.clone()));
            } else {
                changes.push(Change::replace(e.close.clone(), marker));
                let ws = after.len() - after.trim_start().len();
                changes.push(Change::insert(s.end + ws, marker));
            }
        }
    } else {
        for s in &segments {
            let (mut a, mut b) = (s.start, s.end);
            // Grow over elements the selection cuts through so markers nest.
            loop {
                let cut = doc.inlines.iter().find(|e| {
                    let r = e.range();
                    let c = e.content();
                    let overlaps = r.start < b && a < r.end;
                    let inside = c.start <= a && b <= c.end;
                    let contains = a <= r.start && r.end <= b;
                    overlaps && !inside && !contains
                });
                match cut {
                    Some(e) => {
                        a = a.min(e.range().start);
                        b = b.max(e.range().end);
                    }
                    None => break,
                }
            }
            for e in doc.inlines.iter().filter(|e| e.kind == kind) {
                let r = e.range();
                if a <= r.start && r.end <= b {
                    changes.push(Change::delete(e.open.clone()));
                    changes.push(Change::delete(e.close.clone()));
                }
            }
            changes.push(Change::insert(a, marker));
            changes.push(Change::insert(b, marker));
        }
    }

    let changes = normalize(changes);
    let first = segments.first().unwrap().start;
    let last = segments.last().unwrap().end;
    let start = map_pos(&changes, first, !remove);
    let end = map_pos(&changes, last, false);
    let mut plan = Plan::at(changes, end);
    plan.selection = Some(start.min(end)..end);
    plan
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockType {
    Paragraph,
    Heading(u8),
    Bullet,
    Numbered,
    Task,
    Quote,
    Code,
}

/// The "own" block syntax of a line: a list marker (with checkbox) and a
/// heading marker, excluding enclosing quotes and outer list indentation.
struct LineSyntax {
    /// Where the item marker starts, if the line starts a list item.
    item: Option<usize>,
    /// Range of `#`s plus following whitespace, if a heading.
    heading: Option<Range<usize>>,
    /// Insertion point for new block syntax (after quotes/indentation).
    base: usize,
}

fn line_syntax(src: &str, doc: &Doc, li: usize) -> LineSyntax {
    let l = &doc.lines[li];
    let item = doc
        .item_on_line(li)
        .filter(|&i| l.containers.last() == Some(&Container::Item(i)));
    let heading = match l.kind {
        LineKind::Heading(_) => {
            let pre = &src[l.start..l.content_start];
            pre.find('#').map(|h| l.start + h..l.content_start)
        }
        _ => None,
    };
    let base = match item {
        Some(i) => doc.items[i].marker.start,
        None => heading.as_ref().map_or(l.content_start, |h| h.start),
    };
    LineSyntax {
        item,
        heading,
        base,
    }
}

pub fn set_block(src: &str, doc: &Doc, sel: Range<usize>, kind: BlockType) -> Plan {
    let first = doc.line_index(sel.start);
    let last = doc.line_index(sel.end.max(sel.start));
    let lines: Vec<usize> = (first..=last)
        .filter(|&l| {
            matches!(
                doc.lines[l].kind,
                LineKind::Paragraph | LineKind::Heading(_)
            )
        })
        .collect();
    let cursor_line = doc.line_index(sel.end);
    let cursor_off = sel.end.saturating_sub(doc.lines[cursor_line].content_start);

    if kind == BlockType::Code {
        return toggle_code_block(src, doc, first, last, sel.end);
    }
    if lines.is_empty() {
        return Plan::cursor_only(sel.end);
    }
    let mut changes = Vec::new();
    match kind {
        BlockType::Paragraph | BlockType::Heading(_) => {
            let level = match kind {
                BlockType::Heading(n) => n,
                _ => 0,
            };
            let all_same = lines
                .iter()
                .all(|&l| doc.lines[l].kind == LineKind::Heading(level));
            for &l in &lines {
                let syn = line_syntax(src, doc, l);
                let new = if level == 0 || all_same {
                    String::new()
                } else {
                    format!("{} ", "#".repeat(level as usize))
                };
                match syn.heading {
                    Some(h) => changes.push(Change::replace(h, new)),
                    None if !new.is_empty() => {
                        changes.push(Change::insert(doc.lines[l].content_start, new))
                    }
                    None => {}
                }
            }
        }
        BlockType::Bullet | BlockType::Numbered | BlockType::Task => {
            let is_kind = |l: usize| -> bool {
                let Some(i) = line_syntax(src, doc, l).item else {
                    return false;
                };
                let it = &doc.items[i];
                let ordered = doc.lists[it.list].ordered;
                match kind {
                    BlockType::Bullet => !ordered && it.task.is_none(),
                    BlockType::Numbered => ordered,
                    _ => it.task.is_some(),
                }
            };
            let toggle_off = lines.iter().all(|&l| is_kind(l));
            let mut number = 1;
            if kind == BlockType::Numbered
                && let Some(pi) = prev_visible_line(doc, lines[0])
                && let Some(i) = line_syntax(src, doc, pi).item
                && doc.lists[doc.items[i].list].ordered
            {
                number = doc.items[i].number.unwrap_or(0) + 1;
            }
            for &l in &lines {
                let syn = line_syntax(src, doc, l);
                let line = &doc.lines[l];
                let marker_end = match syn.item {
                    Some(i) => doc.items[i]
                        .task
                        .as_ref()
                        .map_or(line.content_start, |_| line.content_start),
                    None => syn.base,
                };
                let old = syn.base..marker_end.max(syn.base);
                let new = if toggle_off {
                    String::new()
                } else {
                    match kind {
                        BlockType::Bullet => "- ".to_string(),
                        BlockType::Task => "- [ ] ".to_string(),
                        _ => {
                            let s = format!("{number}. ");
                            number += 1;
                            s
                        }
                    }
                };
                // Keep a heading marker after the list marker.
                let old = match (&syn.item, &syn.heading) {
                    (Some(_), Some(h)) => syn.base..h.start,
                    _ => old,
                };
                changes.push(Change::replace(old, new));
            }
            // Separate from a following paragraph that would otherwise
            // become a lazy continuation of the last item.
            let last_line = *lines.last().unwrap();
            if !toggle_off
                && let Some(next) = doc.lines.get(last_line + 1)
                && next.kind == LineKind::Paragraph
                && line_syntax(src, doc, last_line + 1).item.is_none()
                && next.containers.is_empty()
            {
                changes.push(Change::insert(doc.lines[last_line].end, "\n"));
            }
            if toggle_off {
                // A paragraph between list items would be absorbed by them.
                let before = lines[0].checked_sub(1).map(|l| &doc.lines[l]);
                if before.is_some_and(|b| b.kind != LineKind::Blank) {
                    changes.push(Change::insert(doc.lines[lines[0]].start, "\n"));
                }
                let after = doc.lines.get(last_line + 1);
                if after.is_some_and(|a| a.kind != LineKind::Blank) {
                    changes.push(Change::insert(doc.lines[last_line].end, "\n"));
                }
            }
        }
        BlockType::Quote => {
            let quoted = |l: usize| {
                doc.lines[l]
                    .containers
                    .iter()
                    .any(|c| matches!(c, Container::Quote(_)))
            };
            let toggle_off = lines.iter().all(|&l| quoted(l));
            let (a, b) = (lines[0], *lines.last().unwrap());
            for l in a..=b {
                let line = &doc.lines[l];
                if toggle_off {
                    let pre = &src[line.start..line.content_start.max(line.start)];
                    if let Some(gt) = pre.find('>') {
                        let mut end = line.start + gt + 1;
                        if src.as_bytes().get(end) == Some(&b' ') {
                            end += 1;
                        }
                        changes.push(Change::delete(line.start + gt..end));
                    }
                } else if line.kind != LineKind::Blank {
                    changes.push(Change::insert(line.start, "> "));
                } else {
                    changes.push(Change::insert(line.start, ">"));
                }
            }
            if !toggle_off
                && let Some(next) = doc.lines.get(b + 1)
                && next.kind != LineKind::Blank
            {
                changes.push(Change::insert(doc.lines[b].end, "\n"));
            }
        }
        BlockType::Code => unreachable!(),
    }
    let changes = normalize(changes);
    let plan_src = Plan::at(changes.clone(), 0).apply(src);
    let new_doc = super::doc::parse(&plan_src);
    let new_line = map_pos(&changes, doc.lines[cursor_line].content_start, false);
    let nl = new_doc.line_index(new_line.min(new_doc.len));
    let target = (new_doc.lines[nl].content_start + cursor_off).min(new_doc.lines[nl].end);
    Plan::at(changes, target)
}

fn toggle_code_block(src: &str, doc: &Doc, first: usize, last: usize, cursor: usize) -> Plan {
    if let Some(ci) = doc.code_block_at_line(first)
        && doc.code_blocks[ci].fenced
    {
        // Unwrap: drop the fences.
        let cb = &doc.code_blocks[ci];
        let mut changes = Vec::new();
        if let Some(o) = cb.open_line {
            changes.push(Change::delete(doc.lines[o].start..doc.line_end_incl(o)));
        }
        if let Some(c) = cb.close_line {
            let start = doc.lines[c]
                .start
                .saturating_sub(1)
                .max(doc.lines[cb.first_line].start);
            changes.push(Change::delete(start..doc.lines[c].end));
        }
        return Plan::mapped(changes, cursor, false);
    }
    let cont = continuation(src, doc, first);
    let start = doc.lines[first].start;
    let end = doc.lines[last].end;
    let open = format!("{cont}```\n");
    let changes = vec![
        Change::insert(start, open.clone()),
        Change::insert(end, format!("\n{cont}```")),
    ];
    let cursor = cursor.clamp(start, end) + open.len();
    Plan::at(changes, cursor)
}

/// Tab / Shift+Tab: nests list items or indents code.
pub fn indent(src: &str, doc: &Doc, sel: Range<usize>, outdent: bool) -> Plan {
    let first = doc.line_index(sel.start);
    let last = doc.line_index(sel.end.max(sel.start));
    let mut changes = Vec::new();
    let mut handled = false;
    for li in first..=last {
        let line = &doc.lines[li];
        if line.kind == LineKind::CodeContent {
            handled = true;
            if outdent {
                let text = &src[line.content_start..line.end];
                let n = text.bytes().take(4).take_while(|&b| b == b' ').count();
                if n > 0 {
                    changes.push(Change::delete(line.content_start..line.content_start + n));
                }
            } else if first == last && sel.is_empty() {
                let at = sel.start.clamp(line.content_start, line.end);
                changes.push(Change::insert(at, "    "));
            } else {
                changes.push(Change::insert(line.content_start, "    "));
            }
            continue;
        }
        let Some(i) = line_syntax(src, doc, li).item else {
            continue;
        };
        handled = true;
        let it = &doc.items[i];
        let siblings = &doc.lists[it.list].items;
        let pos_in_list = siblings.iter().position(|&s| s == i).unwrap_or(0);
        // Every line of the item moves with it.
        let item_lines: Vec<usize> = (li..doc.lines.len())
            .take_while(|&l| l == li || doc.lines[l].containers.contains(&Container::Item(i)))
            .collect();
        if outdent {
            if it.depth <= 1 {
                continue;
            }
            let parent = line.containers.iter().rev().skip(1).find_map(|c| match c {
                Container::Item(p) => Some(*p),
                Container::Quote(_) => None,
            });
            let Some(parent) = parent else { continue };
            let pit = &doc.items[parent];
            let remove = (it.marker.start - line.start)
                .saturating_sub(pit.marker.start - doc.lines[pit.line].start);
            for &l in &item_lines {
                let ln = &doc.lines[l];
                let at = if l == li {
                    it.marker.start - remove
                } else {
                    ln.start + (it.marker.start - line.start) - remove
                };
                let n = src[at.min(ln.end)..ln.end]
                    .bytes()
                    .take(remove)
                    .take_while(|&b| b == b' ')
                    .count();
                if l == li {
                    changes.push(Change::delete(it.marker.start - remove..it.marker.start));
                } else if n > 0 && ln.kind != LineKind::Blank {
                    changes.push(Change::delete(at..at + n));
                }
            }
        } else {
            if pos_in_list == 0 {
                continue;
            }
            let prev = &doc.items[siblings[pos_in_list - 1]];
            let prev_line = &doc.lines[prev.line];
            let target_col = prev.marker.end - prev_line.start;
            let col = it.marker.start - line.start;
            let add = target_col.saturating_sub(col).max(2);
            let pad = " ".repeat(add);
            // An ordered item that starts a new sublist must be numbered 1:
            // only a list starting at 1 can interrupt a paragraph, so "2."
            // would otherwise read as text continuing the item above.
            if doc.lists[it.list].ordered {
                let digits = src[it.marker.clone()]
                    .bytes()
                    .take_while(u8::is_ascii_digit)
                    .count();
                let continues_sublist = (0..li)
                    .rev()
                    .find(|&l| doc.lines[l].kind != LineKind::Blank)
                    .and_then(|l| doc.item_on_line(l))
                    .is_some_and(|k| {
                        doc.items[k].depth > it.depth && doc.lists[doc.items[k].list].ordered
                    });
                if !continues_sublist
                    && digits > 0
                    && &src[it.marker.start..it.marker.start + digits] != "1"
                {
                    changes.push(Change::replace(
                        it.marker.start..it.marker.start + digits,
                        "1",
                    ));
                }
            }
            for &l in &item_lines {
                let ln = &doc.lines[l];
                if l == li {
                    changes.push(Change::insert(it.marker.start, pad.clone()));
                } else if ln.kind != LineKind::Blank {
                    let at = ln.start + src[ln.start..ln.end].len()
                        - src[ln.start..ln.end].trim_start_matches(['>']).len();
                    changes.push(Change::insert(at, pad.clone()));
                }
            }
        }
    }
    if !handled {
        return Plan::cursor_only(sel.end);
    }
    let changes = normalize(changes);
    let mut plan = Plan::mapped(changes.clone(), sel.end, true);
    if !sel.is_empty() {
        let a = map_pos(&changes, sel.start, false);
        plan.selection = Some(a..plan.cursor);
    }
    plan
}

/// Checks or unchecks a task list item.
pub fn toggle_task(src: &str, doc: &Doc, item: usize, cursor: usize) -> Plan {
    let Some((checked, range)) = doc.items.get(item).and_then(|it| it.task.clone()) else {
        return Plan::cursor_only(cursor);
    };
    let _ = src;
    let text = if checked { "[ ]" } else { "[x]" };
    Plan::mapped(vec![Change::replace(range, text)], cursor, false)
}

/// Wraps the selection in a link.
pub fn make_link(src: &str, doc: &Doc, sel: Range<usize>, url: &str) -> Plan {
    let sel = if sel.is_empty() {
        word_at(src, doc, sel.start).unwrap_or(sel)
    } else {
        visual_pos(doc, sel.start)..visual_pos(doc, sel.end)
    };
    // Editing an existing link's destination.
    if let Some(e) = doc.inlines.iter().find(|e| {
        e.kind == InlineKind::Link
            && e.content().start <= sel.start
            && sel.end <= e.content().end
            && e.close.len() > 1
    }) {
        let new_close = format!("]({url})");
        let changes = vec![Change::replace(e.close.clone(), new_close)];
        return Plan::mapped(changes, sel.end, false);
    }
    if sel.is_empty() {
        let t = format!("[{url}]({url})");
        let n = t.len();
        return Plan::at(vec![Change::insert(sel.start, t)], sel.start + n);
    }
    let changes = vec![
        Change::insert(sel.start, "["),
        Change::insert(sel.end, format!("]({url})")),
    ];
    Plan::mapped(changes, sel.end, true)
}

/// Removes a link, keeping its text.
pub fn remove_link(doc: &Doc, pos: usize) -> Option<Plan> {
    let e = doc.inlines.iter().find(|e| {
        e.kind == InlineKind::Link && e.content().start <= pos && pos <= e.content().end
    })?;
    let changes = vec![
        Change::delete(e.open.clone()),
        Change::delete(e.close.clone()),
    ];
    Some(Plan::mapped(changes, pos, false))
}

/// Markdown for a selection, with inline syntax balanced: selecting from
/// the middle of a bold phrase yields `**…**`, not a dangling `**`.
pub fn copy_source(src: &str, doc: &Doc, range: Range<usize>) -> String {
    let (a, b) = (
        range.start.min(range.end),
        range.end.max(range.start).min(src.len()),
    );
    let mut prefix = String::new();
    let mut suffix = String::new();
    let mut order: Vec<&super::doc::Inline> = doc.inlines.iter().collect();
    order.sort_by_key(|e| std::cmp::Reverse(e.range().len()));
    for e in &order {
        let starts_before = e.open.end <= a && a < e.close.start;
        let ends_after = e.open.end < b && b <= e.close.start;
        let open_inside = a <= e.open.start && e.open.end <= b;
        let close_inside = a <= e.close.start && e.close.end <= b;
        if starts_before && (close_inside || ends_after) {
            prefix.push_str(&src[e.open.clone()]);
        }
        if ends_after && (open_inside || starts_before) {
            suffix.insert_str(0, &src[e.close.clone()]);
        }
    }
    // Drop hidden syntax cut in half at the edges.
    let body = trim_partial_markers(doc, a, b, &src[a..b]);
    format!("{prefix}{body}{suffix}")
}

fn trim_partial_markers(doc: &Doc, a: usize, b: usize, body: &str) -> String {
    let mut start = 0;
    let mut end = body.len();
    if let Some(run) = doc.hidden_run_at(a).filter(|r| r.start < a && a < r.end) {
        start = (run.end - a).min(end);
    }
    if let Some(run) = doc.hidden_run_at(b).filter(|r| r.start < b && b < r.end) {
        end = end.saturating_sub(b - run.start).max(start);
    }
    body[start..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::super::doc::parse;
    use super::*;

    /// Runs a command on `src` with the cursor at `|` and returns the result
    /// with the new cursor marked.
    fn run(src: &str, f: impl Fn(&str, &Doc, usize) -> Plan) -> String {
        let pos = src.find('|').expect("cursor");
        let text = src.replacen('|', "", 1);
        let doc = parse(&text);
        let plan = f(&text, &doc, pos);
        let mut out = plan.apply(&text);
        out.insert(plan.cursor, '|');
        out
    }

    fn sel(src: &str, f: impl Fn(&str, &Doc, Range<usize>) -> Plan) -> String {
        let a = src.find('[').expect("selection start");
        let text = src.replacen('[', "", 1);
        let b = text.find(']').expect("selection end");
        let text = text.replacen(']', "", 1);
        let doc = parse(&text);
        let plan = f(&text, &doc, a..b);
        let mut out = plan.apply(&text);
        if let Some(s) = &plan.selection {
            out.insert(s.end, ']');
            out.insert(s.start, '[');
        } else {
            out.insert(plan.cursor, '|');
        }
        out
    }

    fn typing(s: &str, text: &str) -> String {
        run(s, |src, doc, p| insert(src, doc, p, text))
    }

    fn enter(s: &str) -> String {
        run(s, |src, doc, p| newline(src, doc, p, false))
    }

    fn bs(s: &str) -> String {
        run(s, backspace)
    }

    fn del(s: &str) -> String {
        run(s, delete_forward)
    }

    /// Replays keystrokes the way the editor does: each one is a command
    /// against a fresh analysis of the text. `|` in `start` is the cursor.
    /// Keys: plain characters are typed; `⏎` Enter, `⇧` Shift+Enter,
    /// `⌫` Backspace, `⌦` Delete, `⇥` Tab, `⇤` Shift+Tab.
    #[test]
    fn replace_plain_keeps_surrounding_formatting() {
        let rep = |src: &str, needle: &str, with: &str| {
            let doc = parse(src);
            let at = src.find(needle).unwrap();
            replace_plain(src, &doc, at..at + needle.len(), with).map(|p| p.apply(src))
        };
        assert_eq!(
            rep("Some **bold words** here\n", "words", "W").as_deref(),
            Some("Some **bold W** here\n")
        );
        assert_eq!(
            rep("two\nweeks\n", "two\nweeks", "2w").as_deref(),
            Some("2w\n")
        );
        assert_eq!(rep("**bold** text\n", "bold** text", "x"), None);
        assert_eq!(rep("- a\n  b\n", "a\n  b", "x"), None);
    }

    fn keys(start: &str, keys: &str) -> String {
        let mut cursor = start.find('|').expect("cursor");
        let mut text = start.replacen('|', "", 1);
        for k in keys.chars() {
            let doc = parse(&text);
            let plan = match k {
                '⏎' => newline(&text, &doc, cursor, false),
                '⇧' => newline(&text, &doc, cursor, true),
                '⌫' => backspace(&text, &doc, cursor),
                '⌦' => delete_forward(&text, &doc, cursor),
                '⇥' => indent(&text, &doc, cursor..cursor, false),
                '⇤' => indent(&text, &doc, cursor..cursor, true),
                c => insert(&text, &doc, cursor, &c.to_string()),
            };
            text = plan.apply(&text);
            cursor = plan.cursor;
        }
        text.insert(cursor, '|');
        text
    }

    #[test]
    fn typing_a_paragraph_with_markup() {
        assert_eq!(
            keys("|", "Some **bold** text, *italic*, and `code`."),
            "Some **bold** text, *italic*, and `code`.|"
        );
    }

    #[test]
    fn typing_spaces_at_line_end() {
        assert_eq!(keys("ab|", "  c"), "ab  c|");
        assert_eq!(keys("# Ti|", "tle two"), "# Title two|");
    }

    #[test]
    fn typing_a_heading_then_paragraph() {
        assert_eq!(keys("|", "# New section⏎Body"), "# New section\n\nBody|");
    }

    #[test]
    fn typing_a_list_and_leaving_it() {
        assert_eq!(
            keys("|", "- first⏎second⇥⏎nested⏎⏎⏎After"),
            "- first\n  - second\n  - nested\n\nAfter|"
        );
    }

    #[test]
    fn typing_a_code_block() {
        assert_eq!(keys("|", "```rust⏎let x = 1;"), "```rust\nlet x = 1;|\n```");
    }

    #[test]
    fn typing_bold_then_continuing() {
        assert_eq!(keys("|", "**b** c"), "**b** c|");
        assert_eq!(keys("|", "a **b**, c"), "a **b**, c|");
    }

    #[test]
    fn backspacing_through_bold_removes_markers() {
        assert_eq!(keys("x **bold**|", "⌫⌫⌫⌫"), "x |");
        assert_eq!(keys("**bold**| y", "⌫⌫⌫⌫"), "| y");
    }

    #[test]
    fn typing_links_tasks_and_numbers() {
        assert_eq!(
            keys("|", "see [docs](https://x) now"),
            "see [docs](https://x) now|"
        );
        assert_eq!(
            keys("|", "- [ ] buy milk⏎eggs"),
            "- [ ] buy milk\n- [ ] eggs|"
        );
        assert_eq!(keys("|", "1. one⏎two⏎⏎after"), "1. one\n2. two\n\nafter|");
        assert_eq!(keys("|", "* a⏎b"), "* a\n* b|");
    }

    #[test]
    fn typing_quotes() {
        assert_eq!(keys("|", "> q⏎r"), "> q\n>\n> r|");
    }

    #[test]
    fn heading_from_existing_paragraph() {
        assert_eq!(keys("|Title\n", "# "), "# |Title\n");
        assert_eq!(keys("|Title\n", "## ⌫"), "|Title\n");
    }

    #[test]
    fn enter_then_backspace_is_a_noop() {
        assert_eq!(keys("one| two\n", "⏎⌫"), "one|two\n");
        assert_eq!(keys("- a|\n", "⏎⌫⌫"), "- a|\n");
    }

    #[test]
    fn rule_by_typing() {
        assert_eq!(keys("a\n\n|", "---⏎b"), "a\n\n---\n\nb|");
    }

    #[test]
    fn typing_nested_emphasis_and_code() {
        assert_eq!(keys("|", "***bi*** `c` d"), "***bi*** `c` d|");
    }

    #[test]
    fn enter_at_inner_edge_of_formatting() {
        assert_eq!(keys("a **b|** c\n", "⏎"), "a **b**\n\n|c\n");
        assert_eq!(keys("a **|b** c\n", "⏎"), "a\n\n|**b** c\n");
        assert_eq!(keys("a `co|de` c\n", "⏎"), "a `co`\n\n`|de` c\n");
        assert_eq!(keys("- a **b|** c\n", "⏎"), "- a **b**\n- |c\n");
    }

    #[test]
    fn shift_enter_continues_the_paragraph() {
        assert_eq!(keys("a|\n", "⇧b"), "a\\\nb|\n");
        assert_eq!(keys("- a|\n", "⇧b"), "- a\\\n  b|\n");
        assert_eq!(keys("> a|\n", "⇧b"), "> a\\\n> b|\n");
        assert_eq!(keys("a\\\n|b\n", "⌫"), "a|b\n");
        assert_eq!(keys("a|\\\nb\n", "⌦"), "a|b\n");
    }

    #[test]
    fn shift_enter_in_heading_acts_as_enter() {
        assert_eq!(keys("# Ti|tle\n", "⇧"), keys("# Ti|tle\n", "⏎"));
    }

    #[test]
    fn enter_then_backspace_round_trips_at_block_end() {
        assert_eq!(keys("para|\n", "⏎⌫"), "para|\n");
        assert_eq!(keys("# H|\n", "⏎⌫"), "# H|\n");
        assert_eq!(keys("> q|\n", "⏎⌫"), "> q|\n");
    }

    #[test]
    fn unbulleted_item_then_merges() {
        assert_eq!(keys("- a\n- |b\n- c\n", "⌫"), "- a\n\n  |b\n- c\n");
        assert_eq!(keys("- a\n- |b\n", "⌫⌫"), "- a|b\n");
    }

    #[test]
    fn tab_on_ordered_item_starts_sublist_at_one() {
        assert_eq!(keys("1. a\n2. |b\n", "⇥"), "1. a\n   1. |b\n");
        assert_eq!(
            keys("1. a\n   1. x\n2. |b\n", "⇥"),
            "1. a\n   1. x\n   2. |b\n"
        );
    }

    #[test]
    fn enter_renumbers_following_items() {
        assert_eq!(keys("1. a|\n2. b\n3. c\n", "⏎"), "1. a\n2. |\n3. b\n4. c\n");
        assert_eq!(keys("1. a|\n1. b\n", "⏎"), "1. a\n1. |\n1. b\n");
    }

    #[test]
    fn typing_at_hidden_heading_marker_goes_after_it() {
        assert_eq!(typing("|# Title\n", "x"), "# x|Title\n");
        assert_eq!(typing("#| Title\n", "x"), "# x|Title\n");
    }

    #[test]
    fn typing_extends_bold_from_inside() {
        assert_eq!(typing("a **bold|** c\n", "x"), "a **boldx|** c\n");
        // Right after the closing marker (as after typing it) is outside.
        assert_eq!(typing("a **bold**| c\n", "x"), "a **bold**x| c\n");
    }

    #[test]
    fn typing_before_bold_is_plain() {
        assert_eq!(typing("a |**bold** c\n", "x"), "a x|**bold** c\n");
        assert_eq!(typing("a **|bold** c\n", "x"), "a **x|bold** c\n");
        assert_eq!(typing("a **|bold** c\n", " "), "a  |**bold** c\n");
    }

    #[test]
    fn space_after_bold_goes_outside() {
        assert_eq!(typing("a **bold|**\n", " "), "a **bold** |\n");
    }

    #[test]
    fn typing_after_link_is_not_linked() {
        assert_eq!(typing("[docs|](u) z\n", "s"), "[docs](u)s| z\n");
    }

    #[test]
    fn typing_below_paragraph_starts_new_paragraph() {
        assert_eq!(typing("para\n|", "x"), "para\n\nx|");
    }

    #[test]
    fn markdown_input_rule_by_parsing() {
        // "# " at a line start is just text; the parser turns it into a heading.
        assert_eq!(typing("|\n", "# "), "# |\n");
    }

    #[test]
    fn enter_in_paragraph_splits() {
        assert_eq!(enter("one two|\n"), "one two\n\n|\n");
        assert_eq!(enter("one| two\n"), "one\n\n|two\n");
    }

    #[test]
    fn enter_splitting_bold_keeps_both_halves_bold() {
        assert_eq!(enter("**ab|cd**\n"), "**ab**\n\n**|cd**\n");
    }

    #[test]
    fn enter_at_paragraph_start_opens_line_above() {
        assert_eq!(enter("a\n\n|b\n"), "a\n\n|\n\nb\n");
    }

    #[test]
    fn enter_in_list_continues_list() {
        assert_eq!(enter("- one|\n"), "- one\n- |\n");
        assert_eq!(enter("1. one|\n"), "1. one\n2. |\n");
        assert_eq!(enter("- [x] done|\n"), "- [x] done\n- [ ] |\n");
        assert_eq!(enter("> - q|\n"), "> - q\n> - |\n");
    }

    #[test]
    fn enter_on_empty_item_exits_list() {
        assert_eq!(enter("- one\n- |\n"), "- one\n\n|\n");
        assert_eq!(enter("- one\n  - |\n"), "- one\n- |\n");
    }

    #[test]
    fn enter_in_quote_continues_quote() {
        assert_eq!(enter("> a|\n"), "> a\n>\n> |\n");
    }

    #[test]
    fn enter_on_heading_end_makes_paragraph() {
        assert_eq!(enter("# Title|\n"), "# Title\n\n|\n");
    }

    #[test]
    fn enter_on_open_fence_closes_it() {
        assert_eq!(enter("```rust|\n"), "```rust\n|\n```\n");
    }

    #[test]
    fn enter_in_code_keeps_indent() {
        assert_eq!(enter("```\n    x|\n```\n"), "```\n    x\n    |\n```\n");
    }

    #[test]
    fn soft_break_is_hard_break() {
        assert_eq!(run("a|b\n", |s, d, p| newline(s, d, p, true)), "a\\\n|b\n");
    }

    #[test]
    fn backspace_deletes_visible_char() {
        assert_eq!(bs("ab|c\n"), "a|c\n");
        assert_eq!(bs("a **bold**| c\n"), "a **bol|** c\n");
    }

    #[test]
    fn backspace_removes_empty_element() {
        assert_eq!(bs("a **b**| c\n"), "a | c\n");
    }

    #[test]
    fn backspace_moves_exposed_space_outside() {
        assert_eq!(bs("**a b**|\n"), "**a** |\n");
    }

    #[test]
    fn backspace_at_heading_start_makes_paragraph() {
        assert_eq!(bs("# |Title\n"), "|Title\n");
        assert_eq!(bs("|# Title\n"), "|Title\n");
    }

    #[test]
    fn backspace_at_item_start_removes_bullet() {
        // A later item becomes a paragraph of the item above.
        assert_eq!(bs("- a\n- |b\n"), "- a\n\n  |b\n");
        assert_eq!(bs("- [ ] |t\n"), "- |t\n");
        assert_eq!(bs("- a\n  - |b\n"), "- a\n- |b\n");
    }

    #[test]
    fn backspace_merges_paragraphs() {
        assert_eq!(bs("one\n\n|two\n"), "one|two\n");
        assert_eq!(bs("# H\n\n|two\n"), "# H|two\n");
    }

    #[test]
    fn backspace_after_rule_deletes_rule() {
        assert_eq!(bs("a\n\n---\n\n|b\n"), "a\n\n|b\n");
    }

    #[test]
    fn backspace_unquotes_paragraph() {
        assert_eq!(bs("> |a\n> b\n"), "|a\nb\n");
    }

    #[test]
    fn backspace_into_code_block_moves_cursor() {
        assert_eq!(bs("```\nx\n```\n\n|p\n"), "```\nx|\n```\n\np\n");
    }

    #[test]
    fn delete_joins_next_paragraph() {
        assert_eq!(del("one|\n\ntwo\n"), "one|two\n");
        assert_eq!(del("a|b\n"), "a|\n");
        assert_eq!(del("a|**b** c\n"), "a| c\n");
    }

    #[test]
    fn delete_selection_keeps_partner_markers() {
        assert_eq!(sel("a **b[old** te]xt\n", delete_range), "a **b|**xt\n");
    }

    #[test]
    fn delete_at_block_start_takes_exposed_space() {
        assert_eq!(sel("[one] two\n", delete_range), "|two\n");
        assert_eq!(sel("- [one] two\n", delete_range), "- |two\n");
        assert_eq!(sel("# [one] two\n", delete_range), "# |two\n");
        // Five spaces would have made the rest an indented code block.
        assert_eq!(sel("[word]     rest\n", delete_range), "|rest\n");
        // Inside a paragraph, and in code, spaces are text.
        assert_eq!(sel("a [one] two\n", delete_range), "a | two\n");
        assert_eq!(sel("```\n[x] y\n```\n", delete_range), "```\n| y\n```\n");
    }

    #[test]
    fn typing_over_a_selection() {
        let over = |s: &str, t: &'static str| sel(s, move |src, d, r| replace_range(src, d, r, t));
        assert_eq!(over("[one] two three\n", "X"), "X| two three\n");
        assert_eq!(over("- [one] two\n", "X"), "- X| two\n");
        assert_eq!(over("a **[bold]** c\n", "Z"), "a **Z|** c\n");
        // Across syntax: deleted, then typed.
        assert_eq!(over("a **b[old** te]xt\n", "Z"), "a **bZ|**xt\n");
        assert_eq!(over("[one] two\n", "a\nb"), "a\nb| two\n");
    }

    #[test]
    fn delete_selection_across_blocks_merges() {
        assert_eq!(sel("# Ti[tle\n\npa]ra\n", delete_range), "# Ti|ra\n");
    }

    #[test]
    fn delete_whole_lines_keeps_next_block() {
        assert_eq!(sel("[one\n]- two\n", delete_range), "|- two\n");
    }

    #[test]
    fn toggle_bold_wraps_and_unwraps() {
        assert_eq!(
            sel("a [bold] c\n", |s, d, r| toggle_inline(
                s,
                d,
                r,
                InlineKind::Strong
            )),
            "a **[bold]** c\n"
        );
        assert_eq!(
            sel("a **[bold]** c\n", |s, d, r| toggle_inline(
                s,
                d,
                r,
                InlineKind::Strong
            )),
            "a [bold] c\n"
        );
    }

    #[test]
    fn toggle_bold_trims_whitespace_and_splits() {
        assert_eq!(
            sel("a[ bold ]c\n", |s, d, r| toggle_inline(
                s,
                d,
                r,
                InlineKind::Strong
            )),
            "a **[bold]** c\n"
        );
        assert_eq!(
            sel("**ab [cd] ef**\n", |s, d, r| toggle_inline(
                s,
                d,
                r,
                InlineKind::Strong
            )),
            "**ab** [cd] **ef**\n"
        );
    }

    #[test]
    fn toggle_bold_on_word_at_cursor() {
        let out = run("a wo|rd c\n", |s, d, p| {
            toggle_inline(s, d, p..p, InlineKind::Strong)
        });
        assert_eq!(out.replace('|', ""), "a **word** c\n");
    }

    #[test]
    fn heading_commands() {
        let h2 = |s: &str| {
            run(s, |src, d, p| {
                set_block(src, d, p..p, BlockType::Heading(2))
            })
        };
        assert_eq!(h2("ti|tle\n"), "## ti|tle\n");
        assert_eq!(h2("# ti|tle\n"), "## ti|tle\n");
        assert_eq!(h2("## ti|tle\n"), "ti|tle\n");
        assert_eq!(h2("- ti|tle\n"), "- ## ti|tle\n");
    }

    #[test]
    fn list_commands() {
        let bullet = |s: &str| run(s, |src, d, p| set_block(src, d, p..p, BlockType::Bullet));
        assert_eq!(bullet("a|b\n"), "- a|b\n");
        assert_eq!(bullet("- a|b\n"), "a|b\n");
        assert_eq!(bullet("1. a|b\n"), "- a|b\n");
        assert_eq!(bullet("# a|b\n"), "- # a|b\n");
        let numbered = |s: &str| run(s, |src, d, p| set_block(src, d, p..p, BlockType::Numbered));
        assert_eq!(numbered("a|b\n"), "1. a|b\n");
    }

    #[test]
    fn bullet_middle_of_list_off_separates() {
        let out = run("- a\n- b|\n- c\n", |src, d, p| {
            set_block(src, d, p..p, BlockType::Bullet)
        });
        assert_eq!(out, "- a\n\nb|\n\n- c\n");
    }

    #[test]
    fn quote_command() {
        let q = |s: &str| run(s, |src, d, p| set_block(src, d, p..p, BlockType::Quote));
        assert_eq!(q("a|b\n"), "> a|b\n");
        assert_eq!(q("> a|b\n"), "a|b\n");
    }

    #[test]
    fn code_block_command() {
        let c = |s: &str| run(s, |src, d, p| set_block(src, d, p..p, BlockType::Code));
        assert_eq!(c("x = |1\n"), "```\nx = |1\n```\n");
        assert_eq!(c("```\nx = |1\n```\n"), "x = |1\n");
    }

    #[test]
    fn tab_nests_list_item() {
        let tab = |s: &str| run(s, |src, d, p| indent(src, d, p..p, false));
        let untab = |s: &str| run(s, |src, d, p| indent(src, d, p..p, true));
        assert_eq!(tab("- a\n- b|\n"), "- a\n  - b|\n");
        assert_eq!(untab("- a\n  - b|\n"), "- a\n- b|\n");
        assert_eq!(tab("- a|\n"), "- a|\n");
    }

    #[test]
    fn task_toggle() {
        let src = "- [ ] t\n";
        let doc = parse(src);
        let plan = toggle_task(src, &doc, 0, 7);
        assert_eq!(plan.apply(src), "- [x] t\n");
    }

    #[test]
    fn copy_balances_markers() {
        let src = "a **bold** text\n";
        let doc = parse(src);
        assert_eq!(copy_source(src, &doc, 5..12), "**old** t");
        assert_eq!(copy_source(src, &doc, 0..6), "a **bo**");
        assert_eq!(copy_source(src, &doc, 2..10), "**bold**");
    }

    #[test]
    fn links() {
        assert_eq!(
            sel("see [docs] now\n", |s, d, r| make_link(
                s,
                d,
                r,
                "https://x"
            )),
            "see [docs](https://x)| now\n"
        );
    }
}
