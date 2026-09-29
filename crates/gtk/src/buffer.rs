//! The document buffer. Its text is exactly the Markdown file; editing
//! commands go through [`margin_core::md::edit`] so that typing behaves like a
//! word processor, and styling is re-derived from the analysis after every
//! change, touching only the lines whose styling actually changed.

use super::theme::{rgba, rgba_alpha, Fonts, Palette};
use margin_core::md::edit::{self, Plan};
use margin_core::md::{self, Doc, LineKind, Style};
use gtk::{glib, pango, prelude::*, subclass::prelude::*};
use margin_core::diff::diff_changes;
use std::cell::{Cell, Ref, RefCell};
use std::collections::HashMap;
use std::ops::Range;

pub const QUOTE_STEP: f64 = 22.0;
pub const ITEM_STEP: f64 = 28.0;

/// Converts between byte offsets (the Markdown analysis) and character
/// offsets (GTK).
#[derive(Default)]
pub struct TextIndex {
    line_byte: Vec<usize>,
    line_char: Vec<usize>,
    chars: usize,
}

impl TextIndex {
    pub fn new(s: &str) -> Self {
        let mut line_byte = vec![0];
        let mut line_char = vec![0];
        let mut chars = 0;
        for (i, c) in s.char_indices() {
            chars += 1;
            if c == '\n' {
                line_byte.push(i + 1);
                line_char.push(chars);
            }
        }
        TextIndex {
            line_byte,
            line_char,
            chars,
        }
    }

    pub fn char_of(&self, s: &str, byte: usize) -> usize {
        let mut byte = byte.min(s.len());
        while !s.is_char_boundary(byte) {
            byte -= 1;
        }
        let l = self.line_byte.partition_point(|&b| b <= byte) - 1;
        self.line_char[l] + s[self.line_byte[l]..byte].chars().count()
    }

    pub fn byte_of(&self, s: &str, ch: usize) -> usize {
        let ch = ch.min(self.chars);
        let l = self.line_char.partition_point(|&c| c <= ch) - 1;
        let start = self.line_byte[l];
        let skip = ch - self.line_char[l];
        start + s[start..].chars().take(skip).map(char::len_utf8).sum::<usize>()
    }
}

#[derive(Default)]
pub struct State {
    /// The document as in the file. With paragraph wrapping on, the buffer
    /// shows some of its newlines as spaces; `text` still has the newlines.
    pub text: String,
    pub doc: Doc,
    pub index: TextIndex,
    /// Style spans currently applied, in character offsets.
    applied: HashMap<Style, Vec<Range<usize>>>,
    /// Character offsets where the buffer shows a space for a newline of
    /// the file (a line break inside a paragraph), sorted.
    soft: Vec<usize>,
}

impl State {
    pub fn char_of(&self, byte: usize) -> usize {
        self.index.char_of(&self.text, byte)
    }

    pub fn byte_of(&self, ch: usize) -> usize {
        self.index.byte_of(&self.text, ch)
    }
}

#[derive(Clone, Debug)]
pub struct Look {
    pub palette: Palette,
    pub fonts: Fonts,
}

impl Look {
    /// Spacing scale: the body text's size on screen relative to 16px
    /// (12pt at 96 dpi). It follows the font size, Margin's zoom and the
    /// desktop's text scaling, so spacing stays in proportion to the text.
    pub fn scale(&self) -> f64 {
        self.fonts.size / 12.0 * self.fonts.text_scale
    }

    /// Scale for the interface around the text (the comment cards), which
    /// follows only the desktop's text scaling.
    pub fn ui_scale(&self) -> f64 {
        self.fonts.text_scale
    }
}

/// Tags in priority order; later ones win where properties overlap.
const STATIC_STYLES: &[Style] = &[
    Style::Para,
    Style::Heading(1),
    Style::Heading(2),
    Style::Heading(3),
    Style::Heading(4),
    Style::Heading(5),
    Style::Heading(6),
    Style::CodeBlock,
    Style::Fence,
    Style::Table,
    Style::HtmlBlock,
    Style::FrontMatter,
    Style::Rule,
    Style::Raw,
    Style::Quote,
    Style::Strong,
    Style::Emphasis,
    Style::Strike,
    Style::Code,
    Style::Link,
    Style::Image,
    Style::InlineHtml,
    Style::TableHeader,
    Style::TaskDone,
];

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct DocBuffer {
        pub state: RefCell<State>,
        /// Above zero: edits are applied literally (our own, undo, loading).
        pub raw: Cell<u32>,
        /// Above zero: defer re-analysis until the batch ends.
        pub batch: Cell<u32>,
        /// Depth of user actions (GTK groups one keystroke's edits in one).
        pub user_action: Cell<u32>,
        /// Deleting the selection, held back to the end of the user action:
        /// GTK types over a selection by deleting it, then inserting at
        /// the cursor, which together is one `edit::replace_range`.
        pub pending_delete: RefCell<Option<Range<usize>>>,
        /// Above zero: we are applying our own tags.
        pub tagging: Cell<u32>,
        pub stale: Cell<bool>,
        pub source_mode: Cell<bool>,
        /// Character ranges edited since the last restyle.
        pub pending_dirty: RefCell<Vec<Range<usize>>>,
        pub tags: RefCell<HashMap<Style, gtk::TextTag>>,
        pub hidden_tag: RefCell<Option<gtk::TextTag>>,
        pub reveal_tag: RefCell<Option<gtk::TextTag>>,
        pub comment_tags: RefCell<Option<(gtk::TextTag, gtk::TextTag)>>,
        pub look: RefCell<Option<Look>>,
        pub reveal: RefCell<Vec<(gtk::TextMark, gtk::TextMark)>>,
        pub reveal_key: RefCell<Vec<Range<usize>>>,
        /// Show line breaks inside paragraphs as spaces.
        pub wrap: Cell<bool>,
        pub search_tags: RefCell<Option<(gtk::TextTag, gtk::TextTag)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DocBuffer {
        const NAME: &'static str = "MarginDocBuffer";
        type Type = super::DocBuffer;
        type ParentType = gtk::TextBuffer;
    }

    impl ObjectImpl for DocBuffer {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_enable_undo(true);
            obj.connect_changed(|b| b.imp().on_changed());
            obj.connect_mark_set(|b, _, mark| {
                if *mark == b.get_insert() {
                    b.update_reveal();
                }
            });
        }
    }

    impl TextBufferImpl for DocBuffer {
        fn insert_text(&self, iter: &mut gtk::TextIter, text: &str) {
            if self.raw.get() > 0 || self.source_mode.get() {
                self.record_insert(iter.offset() as usize, text.chars().count());
                self.parent_insert_text(iter, text);
                return;
            }
            let obj = self.obj();
            self.ensure_fresh();
            let mut pos = obj.byte_at(iter);
            if let Some(sel) = self.pending_delete.take() {
                let typed = !matches!(text, "\n" | "\r\n" | "\t");
                if typed && (pos == sel.start || pos == sel.end) {
                    let plan = {
                        let st = self.state.borrow();
                        edit::replace_range(&st.text, &st.doc, sel, &text.replace("\r\n", "\n"))
                    };
                    obj.apply_plan(&plan);
                    *iter = obj.iter_at_mark(&obj.get_insert());
                    return;
                }
                // Something else: the deletion goes first.
                let plan = {
                    let st = self.state.borrow();
                    edit::delete_range(&st.text, &st.doc, sel)
                };
                obj.apply_plan(&plan);
                pos = edit::map_pos(&plan.changes, pos, false);
                self.ensure_fresh();
            }
            let plan = {
                let st = self.state.borrow();
                match text {
                    "\n" | "\r\n" => edit::newline(&st.text, &st.doc, pos, false),
                    "\t" => edit::indent(&st.text, &st.doc, pos..pos, false),
                    _ => {
                        let t = text.replace("\r\n", "\n");
                        edit::insert(&st.text, &st.doc, pos, &t)
                    }
                }
            };
            obj.apply_plan(&plan);
            *iter = obj.iter_at_mark(&obj.get_insert());
        }

        fn delete_range(&self, start: &mut gtk::TextIter, end: &mut gtk::TextIter) {
            if self.raw.get() > 0 || self.source_mode.get() {
                let (a, b) = (start.offset().min(end.offset()), start.offset().max(end.offset()));
                self.record_delete(a as usize, (b - a) as usize);
                self.parent_delete_range(start, end);
                return;
            }
            let obj = self.obj();
            self.ensure_fresh();
            let (a, b) = (obj.byte_at(start), obj.byte_at(end));
            if self.user_action.get() > 0
                && self.pending_delete.borrow().is_none()
                && obj.selection_bytes() == Some(a.min(b)..a.max(b))
            {
                // Possibly typing over the selection: wait for the insert.
                self.pending_delete.replace(Some(a.min(b)..a.max(b)));
                return;
            }
            let plan = {
                let st = self.state.borrow();
                edit::delete_range(&st.text, &st.doc, a.min(b)..a.max(b))
            };
            obj.apply_plan(&plan);
            let it = obj.iter_at_mark(&obj.get_insert());
            *start = it;
            *end = it;
        }

        fn begin_user_action(&self) {
            self.user_action.set(self.user_action.get() + 1);
            self.parent_begin_user_action();
        }

        fn end_user_action(&self) {
            if self.user_action.get() == 1
                && let Some(sel) = self.pending_delete.take()
            {
                let obj = self.obj();
                self.ensure_fresh();
                let plan = {
                    let st = self.state.borrow();
                    edit::delete_range(&st.text, &st.doc, sel)
                };
                obj.apply_plan(&plan);
            }
            self.parent_end_user_action();
            self.user_action.set(self.user_action.get().saturating_sub(1));
        }

        fn apply_tag(&self, tag: &gtk::TextTag, start: &gtk::TextIter, end: &gtk::TextIter) {
            // Tags copied along with pasted text would leave stray styling.
            let ours = tag
                .name()
                .is_some_and(|n| n.starts_with("md-") || n.starts_with("comment"));
            if ours && self.tagging.get() == 0 {
                return;
            }
            self.parent_apply_tag(tag, start, end);
        }

        fn undo(&self) {
            self.raw.set(self.raw.get() + 1);
            self.parent_undo();
            self.raw.set(self.raw.get() - 1);
        }

        fn redo(&self) {
            self.raw.set(self.raw.get() + 1);
            self.parent_redo();
            self.raw.set(self.raw.get() - 1);
        }
    }

    impl DocBuffer {
        pub fn on_changed(&self) {
            if self.batch.get() > 0 {
                self.stale.set(true);
                return;
            }
            self.refresh();
        }

        pub fn ensure_fresh(&self) {
            if self.stale.get() && self.batch.get() == 0 {
                self.refresh();
            }
        }

        pub fn refresh(&self) {
            self.reparse();
            self.restyle();
            self.stale.set(false);
            self.obj().update_reveal();
        }

        fn reparse(&self) {
            let obj = self.obj();
            let mut text = obj
                .text(&obj.start_iter(), &obj.end_iter(), true)
                .to_string();
            {
                let mut st = self.state.borrow_mut();
                if !st.soft.is_empty() {
                    let (file, kept) = restore_newlines(&text, &st.soft);
                    text = file;
                    st.soft = kept;
                }
            }
            let doc = md::parse(&text);
            let index = TextIndex::new(&text);
            let mut st = self.state.borrow_mut();
            st.doc = doc;
            st.index = index;
            st.text = text;
        }

        fn record_insert(&self, pos: usize, n: usize) {
            let mut st = self.state.borrow_mut();
            for c in st.soft.iter_mut() {
                if *c >= pos {
                    *c += n;
                }
            }
            for v in st.applied.values_mut() {
                for r in v.iter_mut() {
                    if r.start >= pos {
                        r.start += n;
                        r.end += n;
                    } else if r.end > pos {
                        r.end += n;
                    }
                }
            }
            let mut d = self.pending_dirty.borrow_mut();
            for r in d.iter_mut() {
                if r.start >= pos {
                    r.start += n;
                    r.end += n;
                } else if r.end > pos {
                    r.end += n;
                }
            }
            d.push(pos..pos + n);
            self.stale.set(true);
        }

        fn record_delete(&self, pos: usize, n: usize) {
            let map = |x: usize| {
                if x <= pos {
                    x
                } else if x <= pos + n {
                    pos
                } else {
                    x - n
                }
            };
            let mut st = self.state.borrow_mut();
            st.soft.retain(|&c| c < pos || c >= pos + n);
            for c in st.soft.iter_mut() {
                if *c >= pos + n {
                    *c -= n;
                }
            }
            for v in st.applied.values_mut() {
                for r in v.iter_mut() {
                    *r = map(r.start)..map(r.end);
                }
                v.retain(|r| !r.is_empty());
            }
            let mut d = self.pending_dirty.borrow_mut();
            for r in d.iter_mut() {
                *r = map(r.start)..map(r.end);
            }
            d.push(pos..pos);
            self.stale.set(true);
        }

        fn restyle(&self) {
            let obj = self.obj();
            let new = char_spans(&self.state.borrow());
            let mut dirty = std::mem::take(&mut *self.pending_dirty.borrow_mut());
            {
                let st = self.state.borrow();
                for (style, ranges) in &new {
                    let old = st.applied.get(style).map_or(&[][..], Vec::as_slice);
                    if old != ranges.as_slice() {
                        sym_diff(old, ranges, &mut dirty);
                    }
                }
                for (style, old) in &st.applied {
                    if !new.contains_key(style) {
                        dirty.extend(old.iter().cloned());
                    }
                }
            }
            let dirty = expand_to_lines(dirty, &self.state.borrow());
            if !dirty.is_empty() {
                for style in new.keys() {
                    obj.tag_for(*style);
                }
                let tags = self.tags.borrow();
                self.tagging.set(self.tagging.get() + 1);
                for d in &dirty {
                    let s = obj.iter_at_offset(d.start as i32);
                    let e = obj.iter_at_offset(d.end as i32);
                    for tag in tags.values() {
                        obj.remove_tag(tag, &s, &e);
                    }
                    for (style, ranges) in &new {
                        let Some(tag) = tags.get(style) else { continue };
                        let from = ranges.partition_point(|r| r.end <= d.start);
                        for r in &ranges[from..] {
                            if r.start >= d.end {
                                break;
                            }
                            let a = r.start.max(d.start) as i32;
                            let b = r.end.min(d.end) as i32;
                            if a < b {
                                obj.apply_tag(tag, &obj.iter_at_offset(a), &obj.iter_at_offset(b));
                            }
                        }
                    }
                }
                self.tagging.set(self.tagging.get() - 1);
            }
            self.state.borrow_mut().applied = new;
        }
    }
}

glib::wrapper! {
    pub struct DocBuffer(ObjectSubclass<imp::DocBuffer>)
        @extends gtk::TextBuffer;
}

/// The file's text from the displayed text: the characters at `soft`
/// (spaces standing in for newlines) become newlines again. Returns the
/// text and the positions that still hold a space.
fn restore_newlines(display: &str, soft: &[usize]) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(display.len());
    let mut kept = Vec::with_capacity(soft.len());
    let mut next = soft.iter().peekable();
    for (i, c) in display.chars().enumerate() {
        while next.next_if(|&&s| s < i).is_some() {}
        if next.peek() == Some(&&i) {
            next.next();
            if c == ' ' {
                out.push('\n');
                kept.push(i);
                continue;
            }
        }
        out.push(c);
    }
    (out, kept)
}

fn char_spans(st: &State) -> HashMap<Style, Vec<Range<usize>>> {
    let mut map: HashMap<Style, Vec<Range<usize>>> = HashMap::new();
    for sp in &st.doc.spans {
        let a = st.char_of(sp.range.start);
        let b = st.char_of(sp.range.end);
        if a < b {
            map.entry(sp.style).or_default().push(a..b);
        }
    }
    for v in map.values_mut() {
        v.sort_by_key(|r| r.start);
        let mut out: Vec<Range<usize>> = Vec::with_capacity(v.len());
        for r in v.drain(..) {
            match out.last_mut() {
                Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
                _ => out.push(r),
            }
        }
        *v = out;
    }
    map
}

/// Appends to `out` the ranges covered by exactly one of `a` and `b`.
fn sym_diff(a: &[Range<usize>], b: &[Range<usize>], out: &mut Vec<Range<usize>>) {
    let mut pts: Vec<usize> = a.iter().chain(b).flat_map(|r| [r.start, r.end]).collect();
    pts.sort_unstable();
    pts.dedup();
    let (mut i, mut j) = (0, 0);
    for w in pts.windows(2) {
        let (s, e) = (w[0], w[1]);
        while i < a.len() && a[i].end <= s {
            i += 1;
        }
        while j < b.len() && b[j].end <= s {
            j += 1;
        }
        let in_a = i < a.len() && a[i].start <= s;
        let in_b = j < b.len() && b[j].start <= s;
        if in_a != in_b {
            match out.last_mut() {
                Some(last) if last.end == s => last.end = e,
                _ => out.push(s..e),
            }
        }
    }
}

/// Grows character ranges to whole lines (newline included) and merges them.
fn expand_to_lines(mut ranges: Vec<Range<usize>>, st: &State) -> Vec<Range<usize>> {
    let total = st.index.chars;
    let starts = &st.index.line_char;
    for r in ranges.iter_mut() {
        let a = r.start.min(total);
        let b = r.end.min(total);
        let la = starts.partition_point(|&c| c <= a) - 1;
        let lb = starts.partition_point(|&c| c <= b) - 1;
        let start = starts[la];
        let end = starts.get(lb + 1).copied().unwrap_or(total);
        *r = start..end.max(start);
    }
    ranges.sort_by_key(|r| r.start);
    let mut out: Vec<Range<usize>> = Vec::new();
    for r in ranges {
        match out.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => out.push(r),
        }
    }
    out.retain(|r| !r.is_empty());
    out
}


impl DocBuffer {
    pub fn new(look: Look) -> DocBuffer {
        let b: DocBuffer = glib::Object::new();
        b.imp().look.replace(Some(look));
        b.create_tags();
        b
    }

    fn create_tags(&self) {
        let imp = self.imp();
        let look = imp.look.borrow().clone().expect("look");
        let table = self.tag_table();
        let mut tags = imp.tags.borrow_mut();
        for &style in STATIC_STYLES {
            let tag = gtk::TextTag::new(Some(&format!("md-{style:?}")));
            configure_tag(&tag, style, &look, false);
            table.add(&tag);
            tags.insert(style, tag);
        }
        let comment = gtk::TextTag::new(Some("comment"));
        let active = gtk::TextTag::new(Some("comment-active"));
        table.add(&comment);
        table.add(&active);
        let found = gtk::TextTag::new(Some("search-match"));
        let current = gtk::TextTag::new(Some("search-current"));
        table.add(&found);
        table.add(&current);
        imp.search_tags.replace(Some((found, current)));
        let hidden = gtk::TextTag::new(Some("md-Hidden"));
        configure_tag(&hidden, Style::Hidden, &look, false);
        table.add(&hidden);
        tags.insert(Style::Hidden, hidden.clone());
        let reveal = gtk::TextTag::new(Some("md-reveal"));
        reveal.set_invisible(false);
        table.add(&reveal);
        drop(tags);
        imp.hidden_tag.replace(Some(hidden));
        imp.reveal_tag.replace(Some(reveal));
        imp.comment_tags.replace(Some((comment, active)));
        self.configure_comment_tags(&look.palette);
    }

    fn configure_comment_tags(&self, p: &Palette) {
        if let Some((found, current)) = &*self.imp().search_tags.borrow() {
            found.set_background_rgba(Some(&rgba_alpha(&p.accent, 0.25)));
            current.set_background_rgba(Some(&rgba_alpha(&p.accent, 0.55)));
        }
        if let Some((c, a)) = &*self.imp().comment_tags.borrow() {
            let (base, strong) = if p.dark { (0.22, 0.45) } else { (0.35, 0.65) };
            c.set_background_rgba(Some(&rgba_alpha(&p.highlight, base)));
            a.set_background_rgba(Some(&rgba_alpha(&p.highlight, strong)));
        }
    }

    /// The tag for a style, creating indentation tags on demand.
    pub fn tag_for(&self, style: Style) -> gtk::TextTag {
        let imp = self.imp();
        if let Some(t) = imp.tags.borrow().get(&style) {
            return t.clone();
        }
        let look = imp.look.borrow().clone().expect("look");
        let tag = gtk::TextTag::new(Some(&format!("md-{style:?}")));
        configure_tag(&tag, style, &look, imp.source_mode.get());
        self.tag_table().add(&tag);
        imp.tags.borrow_mut().insert(style, tag.clone());
        tag
    }

    pub fn comment_tag(&self, active: bool) -> gtk::TextTag {
        let tags = self.imp().comment_tags.borrow();
        let (c, a) = tags.as_ref().expect("comment tags");
        if active { a.clone() } else { c.clone() }
    }

    pub fn look(&self) -> Look {
        self.imp().look.borrow().clone().expect("look")
    }

    pub fn set_look(&self, look: Look) {
        let imp = self.imp();
        imp.look.replace(Some(look.clone()));
        let source = imp.source_mode.get();
        for (style, tag) in imp.tags.borrow().iter() {
            configure_tag(tag, *style, &look, source);
        }
        self.configure_comment_tags(&look.palette);
    }

    pub fn wrap_paragraphs(&self) -> bool {
        self.imp().wrap.get()
    }

    /// Shows line breaks inside paragraphs as spaces (or stops doing so).
    /// Not undoable: it changes the view, not the document.
    pub fn set_wrap_paragraphs(&self, on: bool) {
        let imp = self.imp();
        imp.wrap.set(on);
        self.begin_irreversible_action();
        if on {
            self.wrap_soft_breaks();
        } else {
            self.unwrap_soft_breaks();
        }
        self.end_irreversible_action();
    }

    /// Replaces the newlines of the file's soft line breaks with spaces in
    /// the buffer, so paragraphs flow. Positions are remembered so the
    /// file text keeps its newlines.
    fn wrap_soft_breaks(&self) {
        let imp = self.imp();
        if !imp.wrap.get() || imp.source_mode.get() {
            return;
        }
        let targets: Vec<usize> = {
            let st = imp.state.borrow();
            st.doc
                .soft_breaks
                .iter()
                .map(|&b| st.char_of(b))
                .filter(|c| st.soft.binary_search(c).is_err())
                .collect()
        };
        if targets.is_empty() {
            return;
        }
        self.replace_chars(&targets, " ");
        let mut st = imp.state.borrow_mut();
        st.soft.extend(targets);
        st.soft.sort_unstable();
        st.soft.dedup();
        drop(st);
        imp.refresh();
    }

    fn unwrap_soft_breaks(&self) {
        let imp = self.imp();
        let targets = std::mem::take(&mut imp.state.borrow_mut().soft);
        if targets.is_empty() {
            return;
        }
        self.replace_chars(&targets, "\n");
        imp.refresh();
    }

    /// Replaces single characters at `positions` (character offsets), as
    /// literal edits that leave the soft-break bookkeeping to the caller.
    fn replace_chars(&self, positions: &[usize], with: &str) {
        let imp = self.imp();
        imp.raw.set(imp.raw.get() + 1);
        imp.batch.set(imp.batch.get() + 1);
        for &c in positions.iter().rev() {
            let mut s = self.iter_at_offset(c as i32);
            let mut e = self.iter_at_offset(c as i32 + 1);
            // Keep the cursor where it was relative to the text.
            self.delete(&mut s, &mut e);
            let mut it = self.iter_at_offset(c as i32);
            self.insert(&mut it, with);
        }
        imp.raw.set(imp.raw.get() - 1);
        imp.batch.set(imp.batch.get() - 1);
        // The edits above shifted nothing; forget the tracked positions they
        // dropped so callers can record them afresh.
        let mut st = imp.state.borrow_mut();
        st.soft.retain(|c| positions.binary_search(c).is_err());
    }

    pub fn search_tags(&self) -> (gtk::TextTag, gtk::TextTag) {
        self.imp().search_tags.borrow().clone().expect("search tags")
    }

    pub fn source_mode(&self) -> bool {
        self.imp().source_mode.get()
    }

    pub fn set_source_mode(&self, on: bool) {
        let imp = self.imp();
        // The source shows the file's real line breaks.
        if on {
            self.begin_irreversible_action();
            self.unwrap_soft_breaks();
            self.end_irreversible_action();
        }
        imp.source_mode.set(on);
        if !on {
            self.begin_irreversible_action();
            self.wrap_soft_breaks();
            self.end_irreversible_action();
        }
        let look = self.look();
        if let Some(t) = &*imp.hidden_tag.borrow() {
            configure_tag(t, Style::Hidden, &look, on);
        }
    }

    pub fn state(&self) -> Ref<'_, State> {
        self.imp().ensure_fresh();
        self.imp().state.borrow()
    }

    pub fn text_string(&self) -> String {
        self.state().text.clone()
    }

    pub fn byte_at(&self, it: &gtk::TextIter) -> usize {
        let imp = self.imp();
        imp.state.borrow().byte_of(it.offset() as usize)
    }

    pub fn iter_at_byte(&self, b: usize) -> gtk::TextIter {
        let c = self.imp().state.borrow().char_of(b);
        self.iter_at_offset(c as i32)
    }

    pub fn cursor_byte(&self) -> usize {
        let it = self.iter_at_mark(&self.get_insert());
        self.imp().state.borrow().byte_of(it.offset() as usize)
    }

    pub fn selection_bytes(&self) -> Option<Range<usize>> {
        let (a, b) = self.selection_bounds()?;
        let st = self.imp().state.borrow();
        let (a, b) = (st.byte_of(a.offset() as usize), st.byte_of(b.offset() as usize));
        Some(a.min(b)..a.max(b))
    }

    pub fn with_tagging<T>(&self, f: impl FnOnce() -> T) -> T {
        let imp = self.imp();
        imp.tagging.set(imp.tagging.get() + 1);
        let out = f();
        imp.tagging.set(imp.tagging.get() - 1);
        out
    }

    /// Applies an editing plan as one undoable step.
    pub fn apply_plan(&self, plan: &Plan) {
        let imp = self.imp();
        if !plan.changes.is_empty() {
            let offsets: Vec<(i32, i32, String)> = {
                let st = imp.state.borrow();
                plan.changes
                    .iter()
                    .map(|c| {
                        (
                            st.char_of(c.range.start) as i32,
                            st.char_of(c.range.end) as i32,
                            c.text.clone(),
                        )
                    })
                    .collect()
            };
            self.begin_user_action();
            imp.raw.set(imp.raw.get() + 1);
            imp.batch.set(imp.batch.get() + 1);
            for (a, b, t) in offsets.iter().rev() {
                if a < b {
                    let mut s = self.iter_at_offset(*a);
                    let mut e = self.iter_at_offset(*b);
                    self.delete(&mut s, &mut e);
                }
                if !t.is_empty() {
                    let mut it = self.iter_at_offset(*a);
                    self.insert(&mut it, t);
                }
            }
            self.keep_trailing_newline();
            imp.raw.set(imp.raw.get() - 1);
            imp.batch.set(imp.batch.get() - 1);
            if imp.batch.get() == 0 {
                imp.refresh();
                self.wrap_soft_breaks();
            }
            self.end_user_action();
        }
        let (cursor, selection) = {
            let st = imp.state.borrow();
            let c = st.char_of(plan.cursor.min(st.text.len())) as i32;
            let sel = plan
                .selection
                .as_ref()
                .map(|r| (st.char_of(r.start) as i32, st.char_of(r.end) as i32));
            (c, sel)
        };
        match selection {
            Some((a, b)) => self.select_range(&self.iter_at_offset(b), &self.iter_at_offset(a)),
            None => self.place_cursor(&self.iter_at_offset(cursor)),
        }
    }

    /// Keeps the text ending in a newline, so the last line never holds
    /// hidden text: GTK 4.22 aborts on clicks below such a line.
    fn keep_trailing_newline(&self) {
        if self.char_count() == 0 {
            return;
        }
        let mut last = self.end_iter();
        last.backward_char();
        if last.char() != '\n' {
            let mut end = self.end_iter();
            self.insert(&mut end, "\n");
        }
    }

    /// Runs an editing command against the cursor and selection.
    pub fn run(&self, f: impl FnOnce(&str, &Doc, usize, Option<Range<usize>>) -> Plan) {
        self.imp().ensure_fresh();
        let cursor = self.cursor_byte();
        let sel = self.selection_bytes();
        let plan = {
            let st = self.imp().state.borrow();
            f(&st.text, &st.doc, cursor, sel)
        };
        self.apply_plan(&plan);
    }

    pub fn paste_text(&self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        self.begin_user_action();
        if self.source_mode() {
            self.delete_selection(true, true);
            self.insert_at_cursor(&text);
        } else {
            match self.selection_bytes() {
                Some(sel) => self.run(|src, doc, _, _| edit::replace_range(src, doc, sel.clone(), &text)),
                None => self.run(|src, doc, c, _| edit::insert(src, doc, c, &text)),
            }
        }
        self.end_user_action();
    }

    /// Replaces the whole text, e.g. when loading a file. Not undoable.
    pub fn set_contents(&self, text: &str) {
        let imp = self.imp();
        imp.raw.set(imp.raw.get() + 1);
        imp.batch.set(imp.batch.get() + 1);
        self.begin_irreversible_action();
        imp.state.borrow_mut().soft.clear();
        self.set_text(text);
        imp.raw.set(imp.raw.get() - 1);
        imp.batch.set(imp.batch.get() - 1);
        imp.refresh();
        self.wrap_soft_breaks();
        self.end_irreversible_action();
        self.place_cursor(&self.start_iter());
        self.set_modified(false);
    }

    /// Changes the text to `new` with minimal edits, so the cursor, marks
    /// and comment anchors stay attached to unchanged text. Not undoable.
    pub fn apply_external(&self, new: &str) {
        let old = self.text_string();
        if old == new {
            return;
        }
        let changes = diff_changes(&old, new);
        let imp = self.imp();
        let offsets: Vec<(i32, i32, String)> = {
            let st = imp.state.borrow();
            changes
                .iter()
                .map(|c| (st.char_of(c.range.start) as i32, st.char_of(c.range.end) as i32, c.text.clone()))
                .collect()
        };
        imp.raw.set(imp.raw.get() + 1);
        imp.batch.set(imp.batch.get() + 1);
        self.begin_irreversible_action();
        for (a, b, t) in offsets.iter().rev() {
            if a < b {
                let mut s = self.iter_at_offset(*a);
                let mut e = self.iter_at_offset(*b);
                self.delete(&mut s, &mut e);
            }
            if !t.is_empty() {
                let mut it = self.iter_at_offset(*a);
                self.insert(&mut it, t);
            }
        }
        imp.raw.set(imp.raw.get() - 1);
        imp.batch.set(imp.batch.get() - 1);
        imp.refresh();
        self.wrap_soft_breaks();
        self.end_irreversible_action();
    }

    /// Reveals syntax the cursor needs to see: an empty line it sits on,
    /// and the fences of a code block it is inside.
    pub fn update_reveal(&self) {
        if std::env::var_os("MARGIN_NO_REVEAL").is_some() {
            return;
        }
        let imp = self.imp();
        if imp.batch.get() > 0 || imp.stale.get() {
            return;
        }
        let want: Vec<Range<usize>> = {
            let st = imp.state.borrow();
            let doc = &st.doc;
            if doc.lines.is_empty() {
                Vec::new()
            } else {
                let cursor = self.iter_at_mark(&self.get_insert()).offset() as usize;
                let li = doc.line_index(st.byte_of(cursor));
                let mut v = Vec::new();
                let line = &doc.lines[li];
                if line.kind == LineKind::Blank {
                    v.push(st.char_of(line.start)..st.char_of(doc.line_end_incl(li)));
                }
                if let Some(ci) = doc.code_block_at_line(li) {
                    let cb = &doc.code_blocks[ci];
                    for fl in [cb.open_line, cb.close_line].into_iter().flatten() {
                        let l = &doc.lines[fl];
                        v.push(st.char_of(l.content_start)..st.char_of(doc.line_end_incl(fl)));
                    }
                }
                v.retain(|r| !r.is_empty());
                v
            }
        };
        if *imp.reveal_key.borrow() == want && !want.is_empty() {
            return;
        }
        if want.is_empty() && imp.reveal.borrow().is_empty() {
            return;
        }
        let tag = imp.reveal_tag.borrow().clone().expect("reveal tag");
        imp.tagging.set(imp.tagging.get() + 1);
        for (m1, m2) in imp.reveal.borrow_mut().drain(..) {
            let s = self.iter_at_mark(&m1);
            let e = self.iter_at_mark(&m2);
            self.remove_tag(&tag, &s, &e);
            self.delete_mark(&m1);
            self.delete_mark(&m2);
        }
        let mut marks = Vec::new();
        for r in &want {
            let s = self.iter_at_offset(r.start as i32);
            let e = self.iter_at_offset(r.end as i32);
            self.apply_tag(&tag, &s, &e);
            marks.push((self.create_mark(None, &s, true), self.create_mark(None, &e, false)));
        }
        imp.tagging.set(imp.tagging.get() - 1);
        imp.reveal.replace(marks);
        imp.reveal_key.replace(want);
    }
}

fn configure_tag(tag: &gtk::TextTag, style: Style, look: &Look, source_mode: bool) {
    let p = &look.palette;
    let s = look.scale();
    let px = |v: f64| (v * s).round() as i32;
    let mono = look.fonts.mono.as_str();
    match style {
        Style::Para => tag.set_line_height(look.fonts.body_line_factor() as f32),
        Style::Heading(n) => {
            let i = (n.clamp(1, 6) - 1) as usize;
            let scales = [1.8, 1.42, 1.2, 1.07, 1.0, 0.94];
            let weights = [700, 700, 700, 650, 650, 600];
            tag.set_scale(scales[i]);
            tag.set_weight(weights[i]);
            tag.set_line_height(1.12);
            tag.set_foreground_rgba(Some(&rgba(if n >= 6 { &p.dim } else { &p.heading })));
        }
        Style::CodeBlock => {
            tag.set_family(Some(mono));
            tag.set_scale(0.88);
            tag.set_line_height(1.2);
            tag.set_wrap_mode(gtk::WrapMode::Char);
        }
        Style::Fence => {
            tag.set_family(Some(mono));
            tag.set_scale(0.8);
            tag.set_foreground_rgba(Some(&rgba(&p.dim)));
        }
        Style::Table => {
            tag.set_family(Some(mono));
            tag.set_scale(0.88);
            tag.set_line_height(1.2);
        }
        Style::HtmlBlock => {
            tag.set_family(Some(mono));
            tag.set_scale(0.85);
            tag.set_foreground_rgba(Some(&rgba(&p.dim)));
        }
        Style::FrontMatter => {
            tag.set_family(Some(mono));
            tag.set_scale(0.82);
            tag.set_foreground_rgba(Some(&rgba(&p.dim)));
        }
        Style::Rule => {}
        Style::Raw => tag.set_foreground_rgba(Some(&rgba(&p.dim))),
        Style::Quote => tag.set_foreground_rgba(Some(&rgba_alpha(&p.fg, 0.78))),
        Style::Indent { quotes, items } => {
            tag.set_accumulative_margin(true);
            tag.set_left_margin(px(quotes as f64 * QUOTE_STEP + items as f64 * ITEM_STEP));
        }
        Style::Above(a) => tag.set_pixels_above_lines(px(a as f64)),
        Style::Strong => tag.set_weight(700),
        Style::Emphasis => tag.set_style(pango::Style::Italic),
        Style::Strike => {
            tag.set_strikethrough(true);
            tag.set_foreground_rgba(Some(&rgba_alpha(&p.fg, 0.65)));
        }
        Style::Code => {
            tag.set_family(Some(mono));
            tag.set_scale(0.9);
            tag.set_background_rgba(Some(&rgba(&p.code_bg)));
        }
        Style::Link => {
            tag.set_foreground_rgba(Some(&rgba(&p.link)));
            tag.set_underline(pango::Underline::Single);
        }
        Style::Image => {
            tag.set_foreground_rgba(Some(&rgba(&p.link)));
            tag.set_style(pango::Style::Italic);
        }
        Style::InlineHtml => tag.set_foreground_rgba(Some(&rgba(&p.dim))),
        Style::TableHeader => tag.set_weight(700),
        Style::TaskDone => {
            tag.set_strikethrough(true);
            tag.set_foreground_rgba(Some(&rgba_alpha(&p.fg, 0.5)));
        }
        Style::Hidden => {
            tag.set_invisible(!source_mode);
            if source_mode {
                tag.set_foreground_rgba(Some(&rgba(&p.dim)));
            } else {
                tag.set_foreground_rgba(None);
            }
        }
    }
}
