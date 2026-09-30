//! Margin's core for the macOS editor, through UniFFI. Positions are UTF-16
//! offsets, as `NSString` counts; the core works in UTF-8 bytes, so every
//! position crosses [`Utf16Index`].

use margin_core::comments::anchor::floor_char_boundary;
use margin_core::comments::{self, activity, export, handoff, Anchor, Author, Comments, Message, Place, Status, Store, Thread};
use margin_core::file_sync::{self, Loaded};
use margin_core::md::edit::{self, BlockType, Plan};
use margin_core::md::{self, search, Container, Doc, InlineKind, LineKind, Style};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

uniffi::setup_scaffolding!();

#[derive(Debug, uniffi::Error)]
pub enum MarginError {
    Failed { message: String },
}

impl std::fmt::Display for MarginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MarginError::Failed { message } => f.write_str(message),
        }
    }
}

impl std::error::Error for MarginError {}

impl From<anyhow::Error> for MarginError {
    fn from(e: anyhow::Error) -> Self {
        MarginError::Failed { message: format!("{e:#}") }
    }
}

type Result<T> = std::result::Result<T, MarginError>;

/// Converts between byte offsets and UTF-16 offsets of one text.
struct Utf16Index {
    line_byte: Vec<usize>,
    line_u16: Vec<usize>,
    /// Lines of only ASCII, where bytes and UTF-16 units are one to one.
    line_ascii: Vec<bool>,
    len_u16: usize,
}

impl Utf16Index {
    fn new(s: &str) -> Self {
        let mut line_byte = vec![0];
        let mut line_u16 = vec![0];
        let mut line_ascii = vec![true];
        let mut n = 0;
        for (i, c) in s.char_indices() {
            n += c.len_utf16();
            if c == '\n' {
                line_byte.push(i + 1);
                line_u16.push(n);
                line_ascii.push(true);
            } else if !c.is_ascii() {
                *line_ascii.last_mut().unwrap() = false;
            }
        }
        Utf16Index { line_byte, line_u16, line_ascii, len_u16: n }
    }

    fn u16_of(&self, s: &str, byte: usize) -> u32 {
        let byte = floor_char_boundary(s, byte.min(s.len()));
        let l = self.line_byte.partition_point(|&b| b <= byte) - 1;
        let start = self.line_byte[l];
        if self.line_ascii[l] {
            return (self.line_u16[l] + byte - start) as u32;
        }
        (self.line_u16[l] + s[start..byte].encode_utf16().count()) as u32
    }

    fn byte_of(&self, s: &str, u: u32) -> usize {
        let u = (u as usize).min(self.len_u16);
        let l = self.line_u16.partition_point(|&c| c <= u) - 1;
        let mut byte = self.line_byte[l];
        let mut left = u - self.line_u16[l];
        if self.line_ascii[l] {
            return (byte + left).min(s.len());
        }
        for c in s[byte..].chars() {
            if left == 0 {
                break;
            }
            // A position inside a surrogate pair rounds down.
            if c.len_utf16() > left {
                break;
            }
            left -= c.len_utf16();
            byte += c.len_utf8();
        }
        byte
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct TextRange {
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SpanStyle {
    Para,
    Heading { level: u8 },
    CodeBlock,
    Fence,
    Table,
    HtmlBlock,
    FrontMatter,
    Rule,
    Raw,
    Quote,
    Indent { quotes: u8, items: u8 },
    Above { px: u16 },
    Strong,
    Emphasis,
    Strike,
    Code,
    Link,
    Image,
    InlineHtml,
    TableHeader,
    TaskDone,
    Hidden,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct StyleSpan {
    pub start: u32,
    pub end: u32,
    pub style: SpanStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LineType {
    Blank,
    Paragraph,
    Heading,
    SetextUnderline,
    CodeContent,
    Fence,
    Table,
    Html,
    FrontMatter,
    Rule,
    Raw,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct LineInfo {
    pub start: u32,
    /// Excludes the newline.
    pub end: u32,
    pub content_start: u32,
    /// The first character shown (see `margin_core::md::Line`): where
    /// what is drawn beside the line goes.
    pub visible_start: u32,
    pub kind: LineType,
    /// Enclosing block quotes and list items, for indentation.
    pub quotes: u8,
    pub items: u8,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ItemInfo {
    pub line: u32,
    /// Quotes and items enclosing the item's text, itself included: the
    /// marker is drawn left of this indentation.
    pub quotes: u8,
    pub items: u8,
    pub depth: u32,
    pub number: Option<u64>,
    /// `Some(checked)` for a task item.
    pub task: Option<bool>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct QuoteInfo {
    pub first_line: u32,
    /// Trailing blank lines excluded.
    pub last_line: u32,
    /// Containers outside this quote, for where its bar goes.
    pub quotes: u8,
    pub items: u8,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct CodeBlockInfo {
    /// Lines holding code, fences excluded; may be empty.
    pub first_content_line: u32,
    pub end_content_line: u32,
    pub open_line: Option<u32>,
    pub close_line: Option<u32>,
    pub language: String,
    pub quotes: u8,
    pub items: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TableAlign {
    None,
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct TableInfo {
    pub first_line: u32,
    pub last_line: u32,
    pub delimiter_line: u32,
    pub aligns: Vec<TableAlign>,
    /// The header row first.
    pub rows: Vec<TableRowInfo>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct TableRowInfo {
    pub line: u32,
    pub cells: Vec<TableCellInfo>,
    /// Padding and the closing `|` after the last cell.
    pub trail: TextRange,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct TableCellInfo {
    /// The cell's text without its padding.
    pub content: TextRange,
    /// Padding and the `|` before the cell's text.
    pub lead: TextRange,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Replacement {
    /// UTF-16 range of the text before the change.
    pub start: u32,
    pub end: u32,
    pub text: String,
}

/// An editing command's result: changes against the current text, sorted
/// and non-overlapping, and the cursor and selection in the new text.
#[derive(Debug, Clone, uniffi::Record)]
pub struct EditPlan {
    pub changes: Vec<Replacement>,
    pub cursor: u32,
    pub selection: Option<TextRange>,
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum InlineStyle {
    Bold,
    Italic,
    Strikethrough,
    Code,
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum BlockStyle {
    Paragraph,
    Heading { level: u8 },
    Bulleted,
    Numbered,
    Checklist,
    Quote,
    CodeBlock,
}

fn counts(containers: &[Container]) -> (u8, u8) {
    let quotes = containers.iter().filter(|c| matches!(c, Container::Quote(_))).count() as u8;
    (quotes, containers.len() as u8 - quotes)
}

/// The analysis of one version of a document, and the editing commands
/// against it.
#[derive(uniffi::Object)]
pub struct Analysis {
    text: String,
    doc: Doc,
    index: Utf16Index,
}

impl Analysis {
    fn u(&self, byte: usize) -> u32 {
        self.index.u16_of(&self.text, byte)
    }

    fn b(&self, u: u32) -> usize {
        self.index.byte_of(&self.text, u)
    }

    fn range(&self, r: Range<usize>) -> TextRange {
        TextRange { start: self.u(r.start), end: self.u(r.end) }
    }

    fn bytes(&self, start: u32, end: u32) -> Range<usize> {
        let (a, b) = (self.b(start), self.b(end));
        a.min(b)..a.max(b)
    }

    fn plan(&self, p: Plan) -> EditPlan {
        let new = p.apply(&self.text);
        let new_index = Utf16Index::new(&new);
        EditPlan {
            changes: p
                .changes
                .iter()
                .map(|c| Replacement {
                    start: self.u(c.range.start),
                    end: self.u(c.range.end),
                    text: c.text.clone(),
                })
                .collect(),
            cursor: new_index.u16_of(&new, p.cursor),
            selection: p.selection.map(|r| TextRange {
                start: new_index.u16_of(&new, r.start),
                end: new_index.u16_of(&new, r.end),
            }),
        }
    }

    fn link(&self, b: usize, inclusive_end: bool) -> Option<String> {
        self.doc
            .inlines
            .iter()
            .find(|e| {
                let c = e.content();
                matches!(e.kind, InlineKind::Link | InlineKind::Image)
                    && c.start <= b
                    && if inclusive_end { b <= c.end } else { b < c.end.max(c.start + 1) }
            })
            .and_then(|e| e.url.clone())
    }
}

#[uniffi::export]
impl Analysis {
    #[uniffi::constructor]
    pub fn new(text: String) -> Arc<Self> {
        let doc = md::parse(&text);
        let index = Utf16Index::new(&text);
        Arc::new(Analysis { text, doc, index })
    }

    /// From the text as UTF-8 bytes, which Foundation produces much faster
    /// than a Swift string conversion. Invalid bytes are replaced.
    #[uniffi::constructor]
    pub fn from_utf8(bytes: Vec<u8>) -> Arc<Self> {
        let text = String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
        Analysis::new(text)
    }

    pub fn length(&self) -> u32 {
        self.index.len_u16 as u32
    }

    pub fn spans(&self) -> Vec<StyleSpan> {
        self.doc
            .spans
            .iter()
            .map(|s| StyleSpan {
                start: self.u(s.range.start),
                end: self.u(s.range.end),
                style: match s.style {
                    Style::Para => SpanStyle::Para,
                    Style::Heading(level) => SpanStyle::Heading { level },
                    Style::CodeBlock => SpanStyle::CodeBlock,
                    Style::Fence => SpanStyle::Fence,
                    Style::Table => SpanStyle::Table,
                    Style::HtmlBlock => SpanStyle::HtmlBlock,
                    Style::FrontMatter => SpanStyle::FrontMatter,
                    Style::Rule => SpanStyle::Rule,
                    Style::Raw => SpanStyle::Raw,
                    Style::Quote => SpanStyle::Quote,
                    Style::Indent { quotes, items } => SpanStyle::Indent { quotes, items },
                    Style::Above(px) => SpanStyle::Above { px },
                    Style::Strong => SpanStyle::Strong,
                    Style::Emphasis => SpanStyle::Emphasis,
                    Style::Strike => SpanStyle::Strike,
                    Style::Code => SpanStyle::Code,
                    Style::Link => SpanStyle::Link,
                    Style::Image => SpanStyle::Image,
                    Style::InlineHtml => SpanStyle::InlineHtml,
                    Style::TableHeader => SpanStyle::TableHeader,
                    Style::TaskDone => SpanStyle::TaskDone,
                    Style::Hidden => SpanStyle::Hidden,
                },
            })
            .collect()
    }

    /// The style spans packed for speed: four little-endian `u32`s each,
    /// start, end (UTF-16), style code and parameter. Codes follow
    /// [`SpanStyle`]'s order; the parameter is the heading level, the space
    /// above in pixels, or quotes << 8 | items for indentation.
    pub fn spans_packed(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.doc.spans.len() * 16);
        for s in &self.doc.spans {
            let (code, param): (u32, u32) = match s.style {
                Style::Para => (0, 0),
                Style::Heading(level) => (1, level as u32),
                Style::CodeBlock => (2, 0),
                Style::Fence => (3, 0),
                Style::Table => (4, 0),
                Style::HtmlBlock => (5, 0),
                Style::FrontMatter => (6, 0),
                Style::Rule => (7, 0),
                Style::Raw => (8, 0),
                Style::Quote => (9, 0),
                Style::Indent { quotes, items } => (10, (quotes as u32) << 8 | items as u32),
                Style::Above(px) => (11, px as u32),
                Style::Strong => (12, 0),
                Style::Emphasis => (13, 0),
                Style::Strike => (14, 0),
                Style::Code => (15, 0),
                Style::Link => (16, 0),
                Style::Image => (17, 0),
                Style::InlineHtml => (18, 0),
                Style::TableHeader => (19, 0),
                Style::TaskDone => (20, 0),
                Style::Hidden => (21, 0),
            };
            for v in [self.u(s.range.start), self.u(s.range.end), code, param] {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out
    }

    /// The lines packed for speed: seven little-endian `u32`s each: start,
    /// end, content start, visible start (UTF-16), kind (in [`LineType`]'s
    /// order), quotes and items.
    pub fn lines_packed(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.doc.lines.len() * 28);
        for l in &self.doc.lines {
            let (quotes, items) = counts(&l.containers);
            let kind: u32 = match l.kind {
                LineKind::Blank => 0,
                LineKind::Paragraph => 1,
                LineKind::Heading(_) => 2,
                LineKind::SetextUnderline => 3,
                LineKind::CodeContent => 4,
                LineKind::Fence => 5,
                LineKind::Table => 6,
                LineKind::Html => 7,
                LineKind::FrontMatter => 8,
                LineKind::Rule => 9,
                LineKind::Raw => 10,
            };
            for v in [self.u(l.start), self.u(l.end), self.u(l.content_start), self.u(l.visible_start), kind, quotes as u32, items as u32] {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out
    }

    pub fn lines(&self) -> Vec<LineInfo> {
        self.doc
            .lines
            .iter()
            .map(|l| {
                let (quotes, items) = counts(&l.containers);
                LineInfo {
                    start: self.u(l.start),
                    end: self.u(l.end),
                    content_start: self.u(l.content_start),
                    visible_start: self.u(l.visible_start),
                    kind: match l.kind {
                        LineKind::Blank => LineType::Blank,
                        LineKind::Paragraph => LineType::Paragraph,
                        LineKind::Heading(_) => LineType::Heading,
                        LineKind::SetextUnderline => LineType::SetextUnderline,
                        LineKind::CodeContent => LineType::CodeContent,
                        LineKind::Fence => LineType::Fence,
                        LineKind::Table => LineType::Table,
                        LineKind::Html => LineType::Html,
                        LineKind::FrontMatter => LineType::FrontMatter,
                        LineKind::Rule => LineType::Rule,
                        LineKind::Raw => LineType::Raw,
                    },
                    quotes,
                    items,
                }
            })
            .collect()
    }

    pub fn items(&self) -> Vec<ItemInfo> {
        self.doc
            .items
            .iter()
            .enumerate()
            .filter_map(|(ii, it)| {
                let line = &self.doc.lines[it.line];
                let k = line.containers.iter().position(|c| *c == Container::Item(ii))?;
                let (quotes, items) = counts(&line.containers[..=k]);
                Some(ItemInfo {
                    line: it.line as u32,
                    quotes,
                    items,
                    depth: it.depth as u32,
                    number: it.number,
                    task: it.task.as_ref().map(|(checked, _)| *checked),
                })
            })
            .collect()
    }

    pub fn quotes(&self) -> Vec<QuoteInfo> {
        self.doc
            .quotes
            .iter()
            .enumerate()
            .map(|(qi, q)| {
                let mut last = q.last_line;
                while last > q.first_line && self.doc.lines[last].kind == LineKind::Blank {
                    last -= 1;
                }
                let lf = &self.doc.lines[q.first_line];
                let k = lf.containers.iter().position(|c| *c == Container::Quote(qi)).unwrap_or(0);
                let (quotes, items) = counts(&lf.containers[..k]);
                QuoteInfo { first_line: q.first_line as u32, last_line: last as u32, quotes, items }
            })
            .collect()
    }

    pub fn code_blocks(&self) -> Vec<CodeBlockInfo> {
        self.doc
            .code_blocks
            .iter()
            .map(|cb| {
                let lines = cb.content_lines();
                let first = &self.doc.lines[lines.start.min(self.doc.lines.len() - 1)];
                let (quotes, items) = counts(&first.containers);
                CodeBlockInfo {
                    first_content_line: lines.start as u32,
                    end_content_line: lines.end as u32,
                    open_line: cb.open_line.map(|l| l as u32),
                    close_line: cb.close_line.map(|l| l as u32),
                    language: cb.info.split_whitespace().next().unwrap_or("").to_string(),
                    quotes,
                    items,
                }
            })
            .collect()
    }

    pub fn tables(&self) -> Vec<TableInfo> {
        self.doc
            .tables
            .iter()
            .map(|t| TableInfo {
                first_line: self.doc.line_index(t.range.start) as u32,
                last_line: t.rows.last().map_or(t.delimiter_line, |r| r.line.max(t.delimiter_line)) as u32,
                delimiter_line: t.delimiter_line as u32,
                aligns: t
                    .aligns
                    .iter()
                    .map(|a| match a {
                        md::Align::None => TableAlign::None,
                        md::Align::Left => TableAlign::Left,
                        md::Align::Center => TableAlign::Center,
                        md::Align::Right => TableAlign::Right,
                    })
                    .collect(),
                rows: t
                    .rows
                    .iter()
                    .map(|r| TableRowInfo {
                        line: r.line as u32,
                        cells: r
                            .cells
                            .iter()
                            .map(|c| TableCellInfo { content: self.range(c.content.clone()), lead: self.range(c.lead.clone()) })
                            .collect(),
                        trail: self.range(r.trail.clone()),
                    })
                    .collect(),
            })
            .collect()
    }

    /// Newlines inside paragraphs, which Reflow Paragraphs shows as spaces.
    pub fn soft_breaks(&self) -> Vec<u32> {
        self.doc.soft_breaks.iter().map(|&b| self.u(b)).collect()
    }

    pub fn line_index(&self, pos: u32) -> u32 {
        self.doc.line_index(self.b(pos).min(self.text.len())) as u32
    }

    /// Where the cursor belongs: never inside hidden syntax.
    pub fn visual_pos(&self, pos: u32) -> u32 {
        self.u(edit::visual_pos(&self.doc, self.b(pos)))
    }

    /// The run of hidden syntax containing `pos`, if any.
    pub fn hidden_run(&self, pos: u32) -> Option<TextRange> {
        self.doc.hidden_run_at(self.b(pos)).map(|r| self.range(r))
    }

    /// Where text typed at `pos` goes (e.g. after a link, not into it).
    pub fn insertion_point(&self, pos: u32) -> u32 {
        self.u(edit::insertion_point(&self.doc, self.b(pos)))
    }

    pub fn word_at(&self, pos: u32) -> Option<TextRange> {
        edit::word_at(&self.text, &self.doc, self.b(pos)).map(|r| self.range(r))
    }

    /// `start..end` without whitespace and hidden syntax at its ends.
    pub fn trim_segment(&self, start: u32, end: u32) -> Option<TextRange> {
        let r = self.bytes(start, end);
        edit::trim_segment(&self.text, &self.doc, r.start, r.end).map(|r| self.range(r))
    }

    /// The destination of the link whose text contains `pos`.
    pub fn link_at(&self, pos: u32) -> Option<String> {
        self.link(self.b(pos), false)
    }

    /// The link at the cursor, which may sit right after the link's text.
    pub fn link_at_cursor(&self, pos: u32) -> Option<String> {
        self.link(self.b(pos), true)
    }

    /// The task item on the line of `pos`, as an index for `toggle_task`.
    pub fn task_on_line_of(&self, pos: u32) -> Option<u32> {
        let li = self.doc.line_index(self.b(pos).min(self.text.len()));
        self.doc
            .item_on_line(li)
            .filter(|&i| self.doc.items[i].task.is_some())
            .map(|i| i as u32)
    }

    /// The task item drawn on `line`, as an index for `toggle_task`.
    pub fn task_on_line(&self, line: u32) -> Option<u32> {
        self.doc
            .item_on_line(line as usize)
            .filter(|&i| self.doc.items[i].task.is_some())
            .map(|i| i as u32)
    }

    pub fn insert(&self, pos: u32, text: String) -> EditPlan {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        self.plan(edit::insert(&self.text, &self.doc, self.b(pos), &text))
    }

    /// Typing or pasting over a selection.
    pub fn replace_range(&self, start: u32, end: u32, text: String) -> EditPlan {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        self.plan(edit::replace_range(&self.text, &self.doc, self.bytes(start, end), &text))
    }

    pub fn newline(&self, pos: u32, soft: bool) -> EditPlan {
        self.plan(edit::newline(&self.text, &self.doc, self.b(pos), soft))
    }

    pub fn backspace(&self, pos: u32) -> EditPlan {
        self.plan(edit::backspace(&self.text, &self.doc, self.b(pos)))
    }

    pub fn delete_forward(&self, pos: u32) -> EditPlan {
        self.plan(edit::delete_forward(&self.text, &self.doc, self.b(pos)))
    }

    pub fn delete_range(&self, start: u32, end: u32) -> EditPlan {
        self.plan(edit::delete_range(&self.text, &self.doc, self.bytes(start, end)))
    }

    pub fn toggle_inline(&self, start: u32, end: u32, style: InlineStyle) -> EditPlan {
        let kind = match style {
            InlineStyle::Bold => InlineKind::Strong,
            InlineStyle::Italic => InlineKind::Emphasis,
            InlineStyle::Strikethrough => InlineKind::Strike,
            InlineStyle::Code => InlineKind::Code,
        };
        self.plan(edit::toggle_inline(&self.text, &self.doc, self.bytes(start, end), kind))
    }

    pub fn set_block(&self, start: u32, end: u32, style: BlockStyle) -> EditPlan {
        let kind = match style {
            BlockStyle::Paragraph => BlockType::Paragraph,
            BlockStyle::Heading { level } => BlockType::Heading(level),
            BlockStyle::Bulleted => BlockType::Bullet,
            BlockStyle::Numbered => BlockType::Numbered,
            BlockStyle::Checklist => BlockType::Task,
            BlockStyle::Quote => BlockType::Quote,
            BlockStyle::CodeBlock => BlockType::Code,
        };
        self.plan(edit::set_block(&self.text, &self.doc, self.bytes(start, end), kind))
    }

    pub fn indent(&self, start: u32, end: u32, outdent: bool) -> EditPlan {
        self.plan(edit::indent(&self.text, &self.doc, self.bytes(start, end), outdent))
    }

    pub fn toggle_task(&self, item: u32, cursor: u32) -> EditPlan {
        self.plan(edit::toggle_task(&self.text, &self.doc, item as usize, self.b(cursor)))
    }

    pub fn make_link(&self, start: u32, end: u32, url: String) -> EditPlan {
        self.plan(edit::make_link(&self.text, &self.doc, self.bytes(start, end), &url))
    }

    pub fn remove_link(&self, pos: u32) -> Option<EditPlan> {
        edit::remove_link(&self.doc, self.b(pos)).map(|p| self.plan(p))
    }

    /// Replaces a find match in place, keeping formatting around it, when
    /// it lies in plain text.
    pub fn replace_plain(&self, start: u32, end: u32, with: String) -> Option<EditPlan> {
        edit::replace_plain(&self.text, &self.doc, self.bytes(start, end), &with).map(|p| self.plan(p))
    }

    /// Replace All: every match of `needle` (as shown) replaced by `with`,
    /// in place where it lies in plain text, else deleted and retyped. One
    /// plan of minimal changes against the current text.
    pub fn replace_all(&self, needle: String, match_case: bool, with: String) -> Option<EditPlan> {
        let matches = search::find_all(&self.text, &self.doc, &needle, match_case);
        let first = matches.first()?.start;
        // Last to first, so earlier matches keep their offsets.
        let mut text = self.text.clone();
        for r in matches.iter().rev() {
            let doc = md::parse(&text);
            let plan = match edit::replace_plain(&text, &doc, r.clone(), &with) {
                Some(p) => p,
                None => {
                    let deleted = edit::delete_range(&text, &doc, r.clone());
                    let after = deleted.apply(&text);
                    if with.is_empty() {
                        text = after;
                        continue;
                    }
                    let doc = md::parse(&after);
                    let inserted = edit::insert(&after, &doc, deleted.cursor, &with);
                    text = inserted.apply(&after);
                    continue;
                }
            };
            text = plan.apply(&text);
        }
        let changes = margin_core::diff::diff_changes(&self.text, &text);
        let cursor = edit::map_pos(&changes, first, false);
        let new_index = Utf16Index::new(&text);
        Some(EditPlan {
            changes: changes
                .iter()
                .map(|c| Replacement { start: self.u(c.range.start), end: self.u(c.range.end), text: c.text.clone() })
                .collect(),
            cursor: new_index.u16_of(&text, cursor),
            selection: None,
        })
    }

    /// The selection's Markdown, with inline syntax balanced.
    pub fn copy_source(&self, start: u32, end: u32) -> String {
        edit::copy_source(&self.text, &self.doc, self.bytes(start, end))
    }

    pub fn find_all(&self, needle: String, match_case: bool) -> Vec<TextRange> {
        search::find_all(&self.text, &self.doc, &needle, match_case)
            .into_iter()
            .map(|r| self.range(r))
            .collect()
    }
}

// --- Files ------------------------------------------------------------------

#[derive(Debug, Clone, uniffi::Record)]
pub struct LoadedText {
    /// With `\n` newlines and a final newline.
    pub text: String,
    /// The file used CRLF, to be written back the same way.
    pub crlf: bool,
}

fn read_loaded(path: &str) -> Result<Loaded> {
    Ok(Loaded::new(comments::read_doc(Path::new(path))?))
}

/// Reads a document as UTF-8, normalized; a missing file reads as empty.
#[uniffi::export]
pub fn read_document(path: String) -> Result<LoadedText> {
    let l = read_loaded(&path)?;
    Ok(LoadedText { crlf: l.crlf(), text: l.into_text() })
}

/// Absolute, symlink-free path of a document that may not exist yet.
#[uniffi::export]
pub fn canonical_path(path: String) -> Result<String> {
    Ok(comments::canonical_doc_path(Path::new(&path))?.display().to_string())
}

/// Minimal replacements turning `old` into `new`, as UTF-16 ranges of
/// `old`, so that applying them keeps the cursor and anchors on unchanged
/// text.
#[uniffi::export]
pub fn text_changes(old: String, new: String) -> Vec<Replacement> {
    let index = Utf16Index::new(&old);
    margin_core::diff::diff_changes(&old, &new)
        .into_iter()
        .map(|c| Replacement {
            start: index.u16_of(&old, c.range.start),
            end: index.u16_of(&old, c.range.end),
            text: c.text,
        })
        .collect()
}

/// What taking in a change to the file takes (see
/// `margin_core::file_sync::Reconcile`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum FileChange {
    /// The file has our text now: nothing to show.
    CaughtUp,
    /// We had no edits: show the file's text.
    Load { text: String },
    /// Both changed, on different lines: show the merge, and save it.
    Merge { text: String },
    /// Both changed the same lines: ask which to keep, then `keep_mine` or
    /// `load_theirs`.
    Conflict,
}

impl From<file_sync::Reconcile> for FileChange {
    fn from(r: file_sync::Reconcile) -> Self {
        match r {
            file_sync::Reconcile::CaughtUp => FileChange::CaughtUp,
            file_sync::Reconcile::Load(text) => FileChange::Load { text },
            file_sync::Reconcile::Merge(text) => FileChange::Merge { text },
            file_sync::Reconcile::Conflict => FileChange::Conflict,
        }
    }
}

/// A save's next step.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum SaveStep {
    /// The file has our text.
    Done,
    /// Write this to the file (in its newlines), then `wrote`.
    Write { text: String },
    /// The file changed first: do what this says, then (unless it is a
    /// conflict) save again.
    Changed { change: FileChange },
    /// A conflict waits for the person's answer.
    Blocked,
}

/// One document's text and its file: what to save, and outside edits to
/// take in (see `margin_core::file_sync`). It reads the file itself, so
/// what it compares is normalized; the editor writes it.
#[derive(uniffi::Object)]
pub struct FileSync {
    inner: Mutex<file_sync::FileSync>,
}

#[uniffi::export]
impl FileSync {
    /// For a document opened as `opened` (from `read_document`).
    #[uniffi::constructor]
    pub fn new(opened: LoadedText) -> Arc<Self> {
        Arc::new(FileSync { inner: Mutex::new(file_sync::FileSync::new(&opened.text, opened.crlf)) })
    }

    /// The text was edited.
    pub fn edited(&self) {
        self.inner.lock().unwrap().edited();
    }

    /// Whether the text may differ from the file: a save is due, or waits
    /// on a conflict.
    pub fn needs_save(&self) -> bool {
        self.inner.lock().unwrap().needs_save()
    }

    /// Whether the last save could not read or write the file.
    pub fn save_failed(&self) -> bool {
        self.inner.lock().unwrap().save_failed()
    }

    pub fn in_conflict(&self) -> bool {
        self.inner.lock().unwrap().in_conflict()
    }

    /// `ours` in the file's newlines, to write elsewhere (Save As).
    pub fn file_text(&self, ours: String) -> String {
        self.inner.lock().unwrap().file_text(&ours)
    }

    /// The file at `path` may have changed, with `ours` in the editor.
    /// `None` when there is nothing to take in.
    pub fn disk_changed(&self, ours: String, path: String) -> Result<Option<FileChange>> {
        let disk = read_loaded(&path)?;
        Ok(self.inner.lock().unwrap().disk_changed(&ours, disk).map(Into::into))
    }

    /// Saving `ours` to the file at `path`, which is read first.
    pub fn save(&self, ours: String, path: String) -> Result<SaveStep> {
        let disk = read_loaded(&path)?;
        Ok(match self.inner.lock().unwrap().save(&ours, disk) {
            file_sync::Save::Done => SaveStep::Done,
            file_sync::Save::Write(text) => SaveStep::Write { text },
            file_sync::Save::Changed(r) => SaveStep::Changed { change: r.into() },
            file_sync::Save::Blocked => SaveStep::Blocked,
        })
    }

    /// The file now has `ours`.
    pub fn wrote(&self, ours: String) {
        self.inner.lock().unwrap().wrote(&ours);
    }

    /// Saving could not read or write the file.
    pub fn failed(&self) {
        self.inner.lock().unwrap().failed();
    }

    /// The person's answer to a conflict: keep their text, which then
    /// needs saving. False if no conflict waits.
    pub fn keep_mine(&self) -> bool {
        self.inner.lock().unwrap().keep_mine()
    }

    /// The person's answer to a conflict: load the file's text, which this
    /// returns to show. `None` if no conflict waits.
    pub fn load_theirs(&self) -> Option<String> {
        self.inner.lock().unwrap().load_theirs()
    }
}

#[uniffi::export]
pub fn data_dir() -> String {
    comments::data_dir().display().to_string()
}

// --- Comments ---------------------------------------------------------------

/// Who wrote a message: the writer, in the editor, or an agent, through
/// the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MessageAuthor {
    User,
    Agent,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ThreadMessage {
    pub author: MessageAuthor,
    /// Milliseconds since the Unix epoch.
    pub at_ms: i64,
    pub body: String,
}

/// Where a thread's text is, in UTF-16 offsets of the text it was read
/// against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum AnchorPlace {
    /// On this range of the text; an empty one is deleted text.
    On { start: u32, end: u32 },
    /// The commented text was deleted; this is where it was.
    Detached { at: u32 },
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct CommentThread {
    pub id: u64,
    pub place: AnchorPlace,
    /// The commented text, as last seen.
    pub quote: String,
    pub messages: Vec<ThreadMessage>,
    /// When it was resolved, in milliseconds since the Unix epoch; `None`
    /// while it is open.
    pub resolved_at_ms: Option<i64>,
}

/// Where the editor has a thread's text now.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ThreadAnchor {
    pub id: u64,
    pub place: AnchorPlace,
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum CommentChange {
    /// Only record the editor's anchors.
    Anchors,
    Add { start: u32, end: u32, body: String },
    Reply { id: u64, body: String },
    SetResolved { ids: Vec<u64>, resolved: bool },
    Delete { id: u64 },
    /// Puts back a deleted thread (Undo).
    Restore { thread: CommentThread },
    /// Replaces message `index`'s body (0 is the comment).
    Edit { id: u64, index: u32, body: String },
    /// Deletes message `index`; 0 deletes the thread.
    DeleteMessage { id: u64, index: u32 },
    /// Puts back a deleted reply at `index` (Undo).
    InsertMessage { id: u64, index: u32, message: ThreadMessage },
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct CommentState {
    pub threads: Vec<CommentThread>,
    /// The thread `Add` created.
    pub added: Option<u64>,
}

fn ms(t: &chrono::DateTime<chrono::Utc>) -> i64 {
    t.timestamp_millis()
}

fn from_ms(ms: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp_millis(ms).unwrap_or_default()
}

fn message_to_ffi(m: &Message) -> ThreadMessage {
    let author = match m.author {
        Author::User => MessageAuthor::User,
        Author::Agent => MessageAuthor::Agent,
    };
    ThreadMessage { author, at_ms: ms(&m.at), body: m.body.clone() }
}

fn message_from_ffi(m: &ThreadMessage) -> Message {
    let author = match m.author {
        MessageAuthor::User => Author::User,
        MessageAuthor::Agent => Author::Agent,
    };
    Message { author, at: from_ms(m.at_ms), body: m.body.clone() }
}

fn place_to_ffi(p: &Place, text: &str, index: &Utf16Index) -> AnchorPlace {
    match p {
        Place::On(r) => AnchorPlace::On { start: index.u16_of(text, r.start), end: index.u16_of(text, r.end) },
        Place::Detached(at) => AnchorPlace::Detached { at: index.u16_of(text, *at) },
    }
}

fn place_from_ffi(p: AnchorPlace, text: &str, index: &Utf16Index) -> Place {
    match p {
        // Reversed, it is deleted text: `Anchor::follow` detaches it.
        AnchorPlace::On { start, end } => Place::On(index.byte_of(text, start)..index.byte_of(text, end)),
        AnchorPlace::Detached { at } => Place::Detached(index.byte_of(text, at)),
    }
}

fn status_from_ms(resolved_at_ms: Option<i64>) -> Status {
    resolved_at_ms.map_or(Status::Open, |t| Status::Resolved { at: from_ms(t) })
}

fn to_ffi(t: &Thread, text: &str, index: &Utf16Index) -> CommentThread {
    CommentThread {
        id: t.id,
        place: place_to_ffi(t.anchor.place(), text, index),
        quote: t.anchor.quote().to_string(),
        messages: t.messages.iter().map(message_to_ffi).collect(),
        resolved_at_ms: t.resolved_at().as_ref().map(ms),
    }
}

/// The thread against `text`, where the editor has its text now.
fn from_ffi(t: &CommentThread, text: &str, index: &Utf16Index) -> Thread {
    Thread {
        id: t.id,
        status: status_from_ms(t.resolved_at_ms),
        anchor: Anchor::at(text, place_from_ffi(t.place, text, index), t.quote.clone()),
        messages: t.messages.iter().map(message_from_ffi).collect(),
    }
}

fn threads_of(c: &Comments, text: &str, index: &Utf16Index) -> Vec<CommentThread> {
    c.threads.iter().map(|t| to_ffi(t, text, index)).collect()
}

/// One document's comment threads, shared with the CLI through the store.
#[derive(uniffi::Object)]
pub struct CommentStore {
    store: Mutex<Store>,
}

#[uniffi::export]
impl CommentStore {
    #[uniffi::constructor]
    pub fn new(document: String) -> Result<Arc<Self>> {
        Ok(Arc::new(CommentStore { store: Mutex::new(Store::for_doc(Path::new(&document))?) }))
    }

    /// The store's file, to watch for changes agents make.
    pub fn path(&self) -> String {
        self.store.lock().unwrap().path.display().to_string()
    }

    /// The threads, re-anchored against `text` (the document now).
    pub fn load(&self, text: String) -> Result<Vec<CommentThread>> {
        let store = self.store.lock().unwrap().clone();
        let mut c = store.load()?;
        c.sync(&text);
        Ok(threads_of(&c, &text, &Utf16Index::new(&text)))
    }

    /// Records the editor's anchors against `text`, then makes `change`,
    /// under the store's lock.
    pub fn update(&self, text: String, anchors: Vec<ThreadAnchor>, change: CommentChange) -> Result<CommentState> {
        let store = self.store.lock().unwrap().clone();
        if matches!(change, CommentChange::Anchors) && !store.exists() {
            return Ok(CommentState { threads: Vec::new(), added: None });
        }
        let index = Utf16Index::new(&text);
        let bytes = |u: u32| index.byte_of(&text, u);
        let (c, added) = store.update(|c| {
            c.sync(&text);
            for a in &anchors {
                if let Ok(t) = c.thread_mut(a.id) {
                    t.anchor.follow(&text, place_from_ffi(a.place, &text, &index));
                }
            }
            let mut added = None;
            match &change {
                CommentChange::Anchors => {}
                CommentChange::Add { start, end, body } => {
                    let (s, e) = (bytes(*start), bytes(*end));
                    added = Some(c.add(&text, s.min(e)..s.max(e), body, Author::User)?);
                }
                CommentChange::Reply { id, body } => c.reply(*id, body, Author::User)?,
                CommentChange::SetResolved { ids, resolved } => {
                    for id in ids {
                        c.set_resolved(*id, *resolved)?;
                    }
                }
                CommentChange::Delete { id } => c.delete(*id)?,
                CommentChange::Edit { id, index, body } => c.edit(*id, *index as usize, body)?,
                CommentChange::DeleteMessage { id, index } => c.delete_message(*id, *index as usize)?,
                CommentChange::InsertMessage { id, index, message } => {
                    c.insert_message(*id, *index as usize, message_from_ffi(message))?
                }
                CommentChange::Restore { thread } => {
                    if c.thread(thread.id).is_none() {
                        c.threads.push(from_ffi(thread, &text, &index));
                        c.threads.sort_by_key(|t| t.id);
                    }
                }
            }
            Ok((c.clone(), added))
        })?;
        Ok(CommentState { threads: threads_of(&c, &text, &index), added })
    }

    /// Moves the threads to `new_document` (Save As), re-anchored against
    /// `text`. Threads stored for the file being replaced are dropped; with
    /// `remove_old`, this store is deleted.
    pub fn move_to(&self, new_document: String, text: String, remove_old: bool) -> Result<()> {
        let mut store = self.store.lock().unwrap();
        let new_store = Store::for_doc(Path::new(&new_document))?;
        if new_store.path == store.path {
            return Ok(());
        }
        if store.exists() {
            let mut c = store.load()?;
            c.sync(&text);
            c.doc = new_store.doc.clone();
            new_store.update(|n| {
                *n = c;
                Ok(())
            })?;
            if remove_old {
                store.remove()?;
            }
        } else {
            new_store.remove()?;
        }
        *store = new_store;
        Ok(())
    }

    /// Copies the threads to `new_document` (Duplicate), re-anchored
    /// against `text`; this store stays as it is.
    pub fn copy_to(&self, new_document: String, text: String) -> Result<()> {
        let store = self.store.lock().unwrap().clone();
        let new_store = Store::for_doc(Path::new(&new_document))?;
        if new_store.path == store.path || !store.exists() {
            return Ok(());
        }
        let mut c = store.load()?;
        c.sync(&text);
        c.doc = new_store.doc.clone();
        new_store.update(|n| {
            *n = c;
            Ok(())
        })?;
        Ok(())
    }

    /// Deletes the stored threads (a discarded draft).
    pub fn remove(&self) -> Result<()> {
        self.store.lock().unwrap().remove().map_err(Into::into)
    }
}

/// What happened to a thread between two reads of the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ActivityKind {
    Added,
    Replied,
    Resolved,
    Reopened,
    Deleted,
}

/// One thing an agent did to one thread (see `margin_core::comments::activity`).
#[derive(Debug, Clone, uniffi::Record)]
pub struct ThreadActivity {
    pub id: u64,
    pub kind: ActivityKind,
    pub quote: String,
    pub message: Option<String>,
    /// For a notification's subtitle: `Resolved "quote"`.
    pub headline: String,
}

fn activity_to_core(a: &ThreadActivity) -> activity::Change {
    let kind = match a.kind {
        ActivityKind::Added => activity::Kind::Added,
        ActivityKind::Replied => activity::Kind::Replied,
        ActivityKind::Resolved => activity::Kind::Resolved,
        ActivityKind::Reopened => activity::Kind::Reopened,
        ActivityKind::Deleted => activity::Kind::Deleted,
    };
    activity::Change { id: a.id, kind, quote: a.quote.clone(), message: a.message.clone() }
}

/// The changes from `old` to `new`, the threads before and after a reload.
#[uniffi::export]
pub fn thread_activity(old: Vec<CommentThread>, new: Vec<CommentThread>) -> Vec<ThreadActivity> {
    // Only ids, status, quotes and messages matter here, not offsets.
    let core = |ts: &[CommentThread]| -> Vec<Thread> {
        ts.iter()
            .map(|t| Thread {
                id: t.id,
                status: status_from_ms(t.resolved_at_ms),
                anchor: Anchor::detached(0, t.quote.clone()),
                messages: t.messages.iter().map(message_from_ffi).collect(),
            })
            .collect()
    };
    activity::changes(&core(&old), &core(&new))
        .into_iter()
        .map(|c| ThreadActivity {
            id: c.id,
            kind: match c.kind {
                activity::Kind::Added => ActivityKind::Added,
                activity::Kind::Replied => ActivityKind::Replied,
                activity::Kind::Resolved => ActivityKind::Resolved,
                activity::Kind::Reopened => ActivityKind::Reopened,
                activity::Kind::Deleted => ActivityKind::Deleted,
            },
            headline: c.headline(),
            quote: c.quote,
            message: c.message,
        })
        .collect()
}

/// The window's announcement of `activity`: "1 new reply, 2 comments
/// resolved". Empty when there is none.
#[uniffi::export]
pub fn activity_summary(activity: Vec<ThreadActivity>) -> String {
    activity::summary(&activity.iter().map(activity_to_core).collect::<Vec<_>>())
}

/// Open comments as a list to paste into a coding agent.
#[uniffi::export]
pub fn comments_for_agent(document: String, text: String, threads: Vec<CommentThread>) -> String {
    let index = Utf16Index::new(&text);
    let threads: Vec<Thread> = threads.iter().map(|t| from_ffi(t, &text, &index)).collect();
    export::for_agent(&PathBuf::from(document), &text, &threads)
}

// --- Agents -----------------------------------------------------------------

#[derive(uniffi::Enum)]
pub enum AgentState {
    None,
    Waiting,
    Working,
}

/// The agents waiting on, or working on, one open document.
#[derive(uniffi::Object)]
pub struct DocAgents {
    inner: Mutex<handoff::DocAgents>,
}

#[uniffi::export]
impl DocAgents {
    #[uniffi::constructor]
    pub fn new(document: String) -> Arc<Self> {
        Arc::new(DocAgents { inner: Mutex::new(handoff::DocAgents::new(Path::new(&document))) })
    }

    /// The state now; `now_ms` is any clock that only goes forward.
    pub fn poll(&self, now_ms: i64) -> AgentState {
        match self.inner.lock().unwrap().poll(now_ms) {
            handoff::AgentState::None => AgentState::None,
            handoff::AgentState::Waiting => AgentState::Waiting,
            handoff::AgentState::Working => AgentState::Working,
        }
    }

    /// Sends the open comments to the waiting agents; returns how many.
    pub fn send(&self, now_ms: i64) -> Result<u32> {
        Ok(self.inner.lock().unwrap().send(now_ms)? as u32)
    }

    /// An agent changed the document's threads.
    pub fn activity(&self, now_ms: i64) {
        self.inner.lock().unwrap().activity(now_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_offsets_round_trip() {
        let s = "a\u{e9}\u{1F600}b\nc";
        let ix = Utf16Index::new(s);
        assert_eq!(ix.u16_of(s, 0), 0);
        assert_eq!(ix.u16_of(s, 3), 2); // after é (2 bytes, 1 unit)
        assert_eq!(ix.u16_of(s, 7), 4); // after 😀 (4 bytes, 2 units)
        assert_eq!(ix.byte_of(s, 4), 7);
        assert_eq!(ix.byte_of(s, 3), 3); // inside the pair rounds down
        assert_eq!(ix.byte_of(s, 6), 9); // start of "c"
        assert_eq!(ix.u16_of(s, 9), 6);
    }

    #[test]
    fn utf16_offsets_round_trip_at_every_boundary() {
        let s = "plain\ncaf\u{e9} \u{1F600} x\n\nascii again\n\u{4e2d}\u{6587}\n";
        let ix = Utf16Index::new(s);
        let mut u = 0u32;
        for (b, c) in s.char_indices().chain(std::iter::once((s.len(), ' '))) {
            assert_eq!(ix.u16_of(s, b), u, "u16_of({b})");
            assert_eq!(ix.byte_of(s, u), b, "byte_of({u})");
            u += c.len_utf16() as u32;
        }
    }

    #[test]
    fn plans_are_in_utf16() {
        let a = Analysis::new("\u{1F600} **b**\n".into());
        // Backspace after "b" deletes it and its markers.
        let p = a.backspace(7);
        assert_eq!(p.changes.len(), 1);
        assert_eq!((p.changes[0].start, p.changes[0].end), (3, 8));
        assert_eq!(p.cursor, 3);
    }
}
