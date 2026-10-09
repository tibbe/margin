//! Margin's core for the macOS editor, through UniFFI. Positions are UTF-16
//! offsets, as `NSString` counts; the core works in UTF-8 bytes, so every
//! position crosses `Utf16Index`.

use margin_core::comments::anchor::{OffsetMap, floor_char_boundary};
use margin_core::comments::{
    self, Anchor, Author, Comments, Message, Place, Status, Store, Thread, activity, export,
    handoff,
};
use margin_core::file_sync::{self, Loaded};
use margin_core::md::edit::{self, BlockType, Plan};
use margin_core::md::{self, Container, Doc, InlineKind, LineKind, Style, search};
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
        MarginError::Failed {
            message: format!("{e:#}"),
        }
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
        Utf16Index {
            line_byte,
            line_u16,
            line_ascii,
            len_u16: n,
        }
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

/// An image alone in its paragraph (see `margin_core::md::ImageBlock`),
/// which the editor shows as the image: one object.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ImageBlockInfo {
    /// The whole source, `![alt](url)`, within one line.
    pub range: TextRange,
    pub line: u32,
    /// The destination, unescaped: a path or a URL.
    pub url: String,
    /// The alt text as shown, as ranges of the source.
    pub alt: Vec<TextRange>,
}

/// A `mermaid` code block (see `margin_core::md::DiagramBlock`), which the
/// editor shows as its diagram: one object.
#[derive(Debug, Clone, uniffi::Record)]
pub struct DiagramBlockInfo {
    /// From its opening fence to the end of its last line.
    pub range: TextRange,
    pub first_line: u32,
    pub last_line: u32,
    /// The diagram's source, without fences or container prefixes.
    pub source: String,
    /// Where each line of `source` starts in the document.
    pub line_starts: Vec<u32>,
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
    let quotes = containers
        .iter()
        .filter(|c| matches!(c, Container::Quote(_)))
        .count() as u8;
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
        TextRange {
            start: self.u(r.start),
            end: self.u(r.end),
        }
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
                    && if inclusive_end {
                        b <= c.end
                    } else {
                        b < c.end.max(c.start + 1)
                    }
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
        let text = String::from_utf8(bytes)
            .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
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

    /// A hash per line of what styles it: its length and kind and the
    /// spans over it, relative to its start, as little-endian `u64`s. Lines
    /// whose hash is as before are styled as before.
    pub fn line_style_hashes(&self) -> Vec<u8> {
        use std::hash::{Hash, Hasher};
        let lines = &self.doc.lines;
        let mut hashers: Vec<std::collections::hash_map::DefaultHasher> = lines
            .iter()
            .map(|_| std::collections::hash_map::DefaultHasher::new())
            .collect();
        for (h, l) in hashers.iter_mut().zip(lines) {
            (self.u(l.end) - self.u(l.start)).hash(h);
            (l.kind, l.content_start - l.start).hash(h);
        }
        for sp in &self.doc.spans {
            let first = self.doc.line_index(sp.range.start);
            let last = self
                .doc
                .line_index(sp.range.end.saturating_sub(1).max(sp.range.start));
            for li in first..=last.min(lines.len().saturating_sub(1)) {
                let l = &lines[li];
                let a = self.u(sp.range.start.max(l.start)) - self.u(l.start);
                let b = self.u(sp.range.end.min(l.end + 1).max(l.start)) - self.u(l.start);
                (a, b, sp.style).hash(&mut hashers[li]);
            }
        }
        hashers
            .iter()
            .flat_map(|h| h.finish().to_le_bytes())
            .collect()
    }

    /// The style spans packed for speed: four little-endian `u32`s each,
    /// start, end (UTF-16), style code and parameter. Codes follow
    /// [`SpanStyle`]'s order; the parameter is the heading level, the space
    /// above in pixels, or quotes << 8 | items for indentation.
    pub fn spans_packed(&self) -> Vec<u8> {
        self.pack_spans(0, usize::MAX)
    }

    /// [`Analysis::spans_packed`], only the spans over UTF-16 range
    /// `start..end`.
    pub fn spans_packed_between(&self, start: u32, end: u32) -> Vec<u8> {
        let r = self.bytes(start, end);
        self.pack_spans(r.start, r.end)
    }
}

impl Analysis {
    fn pack_spans(&self, from: usize, to: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.doc.spans.len() * 16);
        for s in self
            .doc
            .spans
            .iter()
            .filter(|s| s.range.end >= from && s.range.start <= to)
        {
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
}

#[uniffi::export]
impl Analysis {
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
            for v in [
                self.u(l.start),
                self.u(l.end),
                self.u(l.content_start),
                self.u(l.visible_start),
                kind,
                quotes as u32,
                items as u32,
            ] {
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
                let k = line
                    .containers
                    .iter()
                    .position(|c| *c == Container::Item(ii))?;
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
                let k = lf
                    .containers
                    .iter()
                    .position(|c| *c == Container::Quote(qi))
                    .unwrap_or(0);
                let (quotes, items) = counts(&lf.containers[..k]);
                QuoteInfo {
                    first_line: q.first_line as u32,
                    last_line: last as u32,
                    quotes,
                    items,
                }
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
                last_line: t
                    .rows
                    .last()
                    .map_or(t.delimiter_line, |r| r.line.max(t.delimiter_line))
                    as u32,
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
                            .map(|c| TableCellInfo {
                                content: self.range(c.content.clone()),
                                lead: self.range(c.lead.clone()),
                            })
                            .collect(),
                        trail: self.range(r.trail.clone()),
                    })
                    .collect(),
            })
            .collect()
    }

    /// Images alone in their paragraphs, in source order.
    pub fn image_blocks(&self) -> Vec<ImageBlockInfo> {
        self.doc
            .images
            .iter()
            .map(|im| ImageBlockInfo {
                range: self.range(im.range.clone()),
                line: im.line as u32,
                url: im.url.clone(),
                alt: im.alt.iter().map(|r| self.range(r.clone())).collect(),
            })
            .collect()
    }

    /// `mermaid` code blocks, in source order.
    pub fn diagram_blocks(&self) -> Vec<DiagramBlockInfo> {
        self.doc
            .diagrams
            .iter()
            .map(|d| DiagramBlockInfo {
                range: self.range(d.range.clone()),
                first_line: d.first_line as u32,
                last_line: d.last_line as u32,
                source: d.source.clone(),
                line_starts: d.line_starts.iter().map(|&b| self.u(b)).collect(),
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
        self.plan(edit::replace_range(
            &self.text,
            &self.doc,
            self.bytes(start, end),
            &text,
        ))
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
        self.plan(edit::delete_range(
            &self.text,
            &self.doc,
            self.bytes(start, end),
        ))
    }

    pub fn toggle_inline(&self, start: u32, end: u32, style: InlineStyle) -> EditPlan {
        let kind = match style {
            InlineStyle::Bold => InlineKind::Strong,
            InlineStyle::Italic => InlineKind::Emphasis,
            InlineStyle::Strikethrough => InlineKind::Strike,
            InlineStyle::Code => InlineKind::Code,
        };
        self.plan(edit::toggle_inline(
            &self.text,
            &self.doc,
            self.bytes(start, end),
            kind,
        ))
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
        self.plan(edit::set_block(
            &self.text,
            &self.doc,
            self.bytes(start, end),
            kind,
        ))
    }

    pub fn indent(&self, start: u32, end: u32, outdent: bool) -> EditPlan {
        self.plan(edit::indent(
            &self.text,
            &self.doc,
            self.bytes(start, end),
            outdent,
        ))
    }

    pub fn toggle_task(&self, item: u32, cursor: u32) -> EditPlan {
        self.plan(edit::toggle_task(
            &self.text,
            &self.doc,
            item as usize,
            self.b(cursor),
        ))
    }

    pub fn make_link(&self, start: u32, end: u32, url: String) -> EditPlan {
        self.plan(edit::make_link(
            &self.text,
            &self.doc,
            self.bytes(start, end),
            &url,
        ))
    }

    pub fn remove_link(&self, pos: u32) -> Option<EditPlan> {
        edit::remove_link(&self.doc, self.b(pos)).map(|p| self.plan(p))
    }

    /// Replaces a find match in place, keeping formatting around it, when
    /// it lies in plain text.
    pub fn replace_plain(&self, start: u32, end: u32, with: String) -> Option<EditPlan> {
        edit::replace_plain(&self.text, &self.doc, self.bytes(start, end), &with)
            .map(|p| self.plan(p))
    }

    /// Replace All: every match of `needle` (as shown, as `find_all` finds
    /// them) replaced by `with`, in place where it lies in plain text, else
    /// deleted and retyped. One plan of minimal changes against the current
    /// text.
    pub fn replace_all(
        &self,
        needle: String,
        match_case: bool,
        with: String,
        alt_text_images: Vec<u32>,
        source_diagrams: Vec<u32>,
    ) -> Option<EditPlan> {
        let matches = self.find(&needle, match_case, &alt_text_images, &source_diagrams);
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
                .map(|c| Replacement {
                    start: self.u(c.range.start),
                    end: self.u(c.range.end),
                    text: c.text.clone(),
                })
                .collect(),
            cursor: new_index.u16_of(&text, cursor),
            selection: None,
        })
    }

    /// The selection's Markdown, with inline syntax balanced.
    pub fn copy_source(&self, start: u32, end: u32) -> String {
        edit::copy_source(&self.text, &self.doc, self.bytes(start, end))
    }

    /// Every match of `needle` in the text as shown. `alt_text_images` are
    /// the image blocks (indices into `image_blocks`) shown as their alt
    /// text, since they can't be loaded; the others show as images, which
    /// match nothing. `source_diagrams` are the diagrams (indices into
    /// `diagram_blocks`) shown as their source, since they can't be drawn;
    /// the others are drawn, and their source matches nothing.
    pub fn find_all(
        &self,
        needle: String,
        match_case: bool,
        alt_text_images: Vec<u32>,
        source_diagrams: Vec<u32>,
    ) -> Vec<TextRange> {
        self.find(&needle, match_case, &alt_text_images, &source_diagrams)
            .into_iter()
            .map(|r| self.range(r))
            .collect()
    }
}

impl Analysis {
    fn find(
        &self,
        needle: &str,
        match_case: bool,
        alt_text_images: &[u32],
        source_diagrams: &[u32],
    ) -> Vec<Range<usize>> {
        search::find_shown(
            &self.text,
            &self.doc,
            needle,
            match_case,
            |i| alt_text_images.contains(&(i as u32)),
            |d| source_diagrams.contains(&(d as u32)),
        )
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

// --- Diagrams ---------------------------------------------------------------

/// The page's colors, as CSS hex, which a diagram is drawn in.
#[derive(Debug, Clone, uniffi::Record)]
pub struct DiagramColors {
    pub background: String,
    pub node: String,
    pub border: String,
    pub text: String,
    pub line: String,
    /// The body font's size, in points, before zoom.
    pub font_size: u32,
}

/// Text a diagram draws, and where, in its own points.
#[derive(Debug, Clone, uniffi::Record)]
pub struct DiagramLabel {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum DiagramFailure {
    Syntax,
    UnknownType,
    Unsupported,
    TooLarge,
    TooSlow,
    Failed,
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum DiagramOutcome {
    /// Its SVG, its size in points, and the labels it draws.
    Drawn {
        svg: String,
        width: f32,
        height: f32,
        labels: Vec<DiagramLabel>,
    },
    /// Why it can't be drawn, and the line of its source at fault (from 0),
    /// when that is known.
    Failed {
        failure: DiagramFailure,
        message: String,
        line: Option<u32>,
    },
}

/// Draws a Mermaid diagram's source in `colors`. Slow for a large diagram:
/// call it off the main thread.
#[uniffi::export]
pub fn render_diagram(source: String, colors: DiagramColors) -> DiagramOutcome {
    use margin_core::diagram::{self, Failure};
    let colors = diagram::Colors {
        background: colors.background,
        node: colors.node,
        border: colors.border,
        text: colors.text,
        line: colors.line,
        font_size: colors.font_size,
    };
    match diagram::render(&source, &colors) {
        Ok(d) => DiagramOutcome::Drawn {
            svg: d.svg,
            width: d.width,
            height: d.height,
            labels: d
                .labels
                .into_iter()
                .map(|l| DiagramLabel {
                    text: l.text,
                    x: l.x,
                    y: l.y,
                    width: l.width,
                    height: l.height,
                })
                .collect(),
        },
        Err(e) => DiagramOutcome::Failed {
            failure: match e.failure {
                Failure::Syntax => DiagramFailure::Syntax,
                Failure::UnknownType => DiagramFailure::UnknownType,
                Failure::Unsupported => DiagramFailure::Unsupported,
                Failure::TooLarge => DiagramFailure::TooLarge,
                Failure::TooSlow => DiagramFailure::TooSlow,
                Failure::Failed => DiagramFailure::Failed,
            },
            message: e.message,
            line: e
                .span
                .map(|s| source[..s.start.min(source.len())].matches('\n').count() as u32),
        },
    }
}

/// A drawn diagram's SVG as `width`×`height` pixels: RGBA, premultiplied,
/// row by row.
#[uniffi::export]
pub fn rasterize_diagram(svg: String, width: u32, height: u32) -> Option<Vec<u8>> {
    margin_core::diagram::rasterize(&svg, width, height)
}

/// Reads a document as UTF-8, normalized; a missing file reads as empty.
#[uniffi::export]
pub fn read_document(path: String) -> Result<LoadedText> {
    let l = read_loaded(&path)?;
    Ok(LoadedText {
        crlf: l.crlf(),
        text: l.into_text(),
    })
}

/// Absolute, symlink-free path of a document that may not exist yet.
#[uniffi::export]
pub fn canonical_path(path: String) -> Result<String> {
    Ok(comments::canonical_doc_path(Path::new(&path))?
        .display()
        .to_string())
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

// --- Changes since the last commit -------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LineChangeKind {
    Added,
    Changed,
    Deleted,
}

/// Lines of the text that differ from the last commit (see
/// `margin_core::changes::LineChange`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LineChange {
    pub kind: LineChangeKind,
    /// Indices into `Analysis::lines`. A deletion's are equal: the line
    /// after the deleted ones.
    pub first_line: u32,
    pub end_line: u32,
}

/// A document's file as of its git repository's last commit, to mark what
/// changed since.
#[derive(uniffi::Object)]
pub struct CommittedText {
    text: String,
}

/// The file at `path` as of its repository's last commit; `None` outside a
/// repository or for a file not yet committed.
#[uniffi::export]
pub fn committed_text(path: String) -> Option<Arc<CommittedText>> {
    margin_core::changes::committed(Path::new(&path)).map(|text| Arc::new(CommittedText { text }))
}

#[uniffi::export]
impl CommittedText {
    /// The lines of `text` changed since, in order.
    pub fn changes(&self, text: Arc<Analysis>) -> Vec<LineChange> {
        use margin_core::changes::Kind;
        margin_core::changes::line_changes(&self.text, &text.text)
            .into_iter()
            .map(|c| LineChange {
                kind: match c.kind {
                    Kind::Added => LineChangeKind::Added,
                    Kind::Changed => LineChangeKind::Changed,
                    Kind::Deleted => LineChangeKind::Deleted,
                },
                first_line: c.lines.start as u32,
                end_line: c.lines.end as u32,
            })
            .collect()
    }
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
        Arc::new(FileSync {
            inner: Mutex::new(file_sync::FileSync::new(&opened.text, opened.crlf)),
        })
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
        Ok(self
            .inner
            .lock()
            .unwrap()
            .disk_changed(&ours, disk)
            .map(Into::into))
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
    /// Who resolved it; `None` while it is open.
    pub resolved_by: Option<MessageAuthor>,
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
    Add {
        start: u32,
        end: u32,
        body: String,
    },
    Reply {
        id: u64,
        body: String,
    },
    SetResolved {
        ids: Vec<u64>,
        resolved: bool,
    },
    Delete {
        id: u64,
    },
    /// Puts back a deleted thread (Undo).
    Restore {
        thread: CommentThread,
    },
    /// Replaces message `index`'s body (0 is the comment).
    Edit {
        id: u64,
        index: u32,
        body: String,
    },
    /// Deletes message `index`; 0 deletes the thread.
    DeleteMessage {
        id: u64,
        index: u32,
    },
    /// Puts back a deleted reply at `index` (Undo).
    InsertMessage {
        id: u64,
        index: u32,
        message: ThreadMessage,
    },
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

fn author_to_ffi(a: Author) -> MessageAuthor {
    match a {
        Author::User => MessageAuthor::User,
        Author::Agent => MessageAuthor::Agent,
    }
}

fn author_from_ffi(a: MessageAuthor) -> Author {
    match a {
        MessageAuthor::User => Author::User,
        MessageAuthor::Agent => Author::Agent,
    }
}

fn message_to_ffi(m: &Message) -> ThreadMessage {
    ThreadMessage {
        author: author_to_ffi(m.author),
        at_ms: ms(&m.at),
        body: m.body.clone(),
    }
}

fn message_from_ffi(m: &ThreadMessage) -> Message {
    Message {
        author: author_from_ffi(m.author),
        at: from_ms(m.at_ms),
        body: m.body.clone(),
    }
}

/// Where `places` in `old` are in `new`, by the rules anchors follow
/// (see `margin_core::comments::anchor::OffsetMap::map`), so an editor
/// taking in an outside change moves its anchors as the CLI does.
#[uniffi::export]
pub fn map_places(old: String, new: String, places: Vec<AnchorPlace>) -> Vec<AnchorPlace> {
    let (old_index, new_index) = (Utf16Index::new(&old), Utf16Index::new(&new));
    let map = OffsetMap::new(&old, &new);
    places
        .into_iter()
        .map(|p| {
            let p = map.map(&place_from_ffi(p, &old, &old_index));
            place_to_ffi(&p, &new, &new_index)
        })
        .collect()
}

fn place_to_ffi(p: &Place, text: &str, index: &Utf16Index) -> AnchorPlace {
    match p {
        Place::On(s) => AnchorPlace::On {
            start: index.u16_of(text, s.range().start),
            end: index.u16_of(text, s.range().end),
        },
        Place::Detached(at) => AnchorPlace::Detached {
            at: index.u16_of(text, *at),
        },
    }
}

fn place_from_ffi(p: AnchorPlace, text: &str, index: &Utf16Index) -> Place {
    match p {
        // Reversed, or on whitespace alone, it is deleted text.
        AnchorPlace::On { start, end } => {
            Place::of(text, index.byte_of(text, start)..index.byte_of(text, end))
        }
        AnchorPlace::Detached { at } => Place::Detached(index.byte_of(text, at)),
    }
}

fn status_from_ffi(t: &CommentThread) -> Status {
    t.resolved_at_ms
        .map_or(Status::Open, |at| Status::Resolved {
            at: from_ms(at),
            by: t.resolved_by.map_or(Author::User, author_from_ffi),
        })
}

/// The thread without where its text is, for what doesn't need it.
fn unplaced(t: &CommentThread) -> Thread {
    Thread {
        id: t.id,
        status: status_from_ffi(t),
        anchor: Anchor::detached(0, t.quote.clone()),
        messages: t.messages.iter().map(message_from_ffi).collect(),
    }
}

fn to_ffi(t: &Thread, text: &str, index: &Utf16Index) -> CommentThread {
    CommentThread {
        id: t.id,
        place: place_to_ffi(t.anchor.place(), text, index),
        quote: t.anchor.quote().to_string(),
        messages: t.messages.iter().map(message_to_ffi).collect(),
        resolved_at_ms: t.resolved_at().as_ref().map(ms),
        resolved_by: t.resolved_by().map(author_to_ffi),
    }
}

/// The thread against `text`, where the editor has its text now.
fn from_ffi(t: &CommentThread, text: &str, index: &Utf16Index) -> Thread {
    Thread {
        id: t.id,
        status: status_from_ffi(t),
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
        Ok(Arc::new(CommentStore {
            store: Mutex::new(Store::for_doc(Path::new(&document))?),
        }))
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
    pub fn update(
        &self,
        text: String,
        anchors: Vec<ThreadAnchor>,
        change: CommentChange,
    ) -> Result<CommentState> {
        let store = self.store.lock().unwrap().clone();
        if matches!(change, CommentChange::Anchors) && !store.exists() {
            return Ok(CommentState {
                threads: Vec::new(),
                added: None,
            });
        }
        let index = Utf16Index::new(&text);
        let bytes = |u: u32| index.byte_of(&text, u);
        let (c, added) = store.update(|c| {
            c.sync(&text);
            for a in &anchors {
                if let Ok(t) = c.thread_mut(a.id) {
                    t.anchor
                        .follow(&text, place_from_ffi(a.place, &text, &index));
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
                        c.set_resolved(*id, *resolved, Author::User)?;
                    }
                }
                CommentChange::Delete { id } => c.delete(*id)?,
                CommentChange::Edit { id, index, body } => c.edit(*id, *index as usize, body)?,
                CommentChange::DeleteMessage { id, index } => {
                    c.delete_message(*id, *index as usize)?
                }
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
        Ok(CommentState {
            threads: threads_of(&c, &text, &index),
            added,
        })
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
    activity::Change {
        id: a.id,
        kind,
        quote: a.quote.clone(),
        message: a.message.clone(),
    }
}

/// The changes from `old` to `new`, the threads before and after a reload.
#[uniffi::export]
pub fn thread_activity(old: Vec<CommentThread>, new: Vec<CommentThread>) -> Vec<ThreadActivity> {
    // Only ids, status, quotes and messages matter here, not offsets.
    let core = |ts: &[CommentThread]| -> Vec<Thread> { ts.iter().map(unplaced).collect() };
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

/// How many threads a send would give the agent on a document.
#[derive(Debug, Clone, Copy, uniffi::Record)]
pub struct Pending {
    pub open: u32,
    /// Resolved by the writer since the last send.
    pub resolved: u32,
}

fn pending_to_ffi(p: handoff::Pending) -> Pending {
    Pending {
        open: p.open as u32,
        resolved: p.resolved as u32,
    }
}

/// Another document a send covers, and what a send would give on it.
#[derive(uniffi::Record)]
pub struct RoundDocument {
    pub path: String,
    pub pending: Pending,
}

/// What a send gave, for its announcement: "2 open and 1 resolved comment
/// on 2 documents".
#[uniffi::export]
pub fn sent_summary(sent: Pending, docs: u32) -> String {
    handoff::sent_summary(
        handoff::Pending {
            open: sent.open as usize,
            resolved: sent.resolved as usize,
        },
        docs as usize,
    )
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
        Arc::new(DocAgents {
            inner: Mutex::new(handoff::DocAgents::new(Path::new(&document))),
        })
    }

    /// The state now; `now_ms` is any clock that only goes forward.
    pub fn poll(&self, now_ms: i64) -> AgentState {
        match self.inner.lock().unwrap().poll(now_ms) {
            handoff::AgentState::None => AgentState::None,
            handoff::AgentState::Waiting => AgentState::Waiting,
            handoff::AgentState::Working => AgentState::Working,
        }
    }

    /// The other documents a send from here covers, as of the last poll.
    pub fn others(&self) -> Vec<RoundDocument> {
        self.inner
            .lock()
            .unwrap()
            .others()
            .iter()
            .map(|(path, pending)| RoundDocument {
                path: path.display().to_string(),
                pending: pending_to_ffi(*pending),
            })
            .collect()
    }

    /// What a send from here would give on the document, with `threads` as
    /// the window has them now.
    pub fn pending(&self, threads: Vec<CommentThread>) -> Pending {
        let threads: Vec<Thread> = threads.iter().map(unplaced).collect();
        pending_to_ffi(self.inner.lock().unwrap().pending(&threads))
    }

    /// Sends the open comments on the round to the waiting agents; returns
    /// how many agents there were.
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
    fn places_map_across_a_change_in_utf16() {
        // "😀" is two UTF-16 units; "é" one, in two bytes.
        let old = "\u{1F600} is insensitive unless\n".to_string();
        let new = "\u{1F600}\u{e9} is quite insensitive unless\n".to_string();
        let places = map_places(
            old,
            new,
            vec![
                AnchorPlace::On { start: 6, end: 17 },
                AnchorPlace::On { start: 18, end: 24 },
                AnchorPlace::Detached { at: 5 },
            ],
        );
        assert_eq!(
            places,
            [
                AnchorPlace::On { start: 13, end: 24 },
                AnchorPlace::On { start: 25, end: 31 },
                AnchorPlace::Detached { at: 6 },
            ]
        );
    }

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

// MARK: - The display map

/// How a stretch of source shows (see `md::display::Piece`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PieceKind {
    Shown,
    Hidden,
    /// As other text: a soft line break as a space, a table cell's gap as
    /// a tab.
    Replaced,
    /// An image or a diagram, shown as one object character (U+FFFC).
    Object,
}

/// A stretch of a shown paragraph's source, in UTF-16 units.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ShownPiece {
    pub kind: PieceKind,
    pub source_start: u32,
    pub source_end: u32,
    /// Where it is in the paragraph's shown text, and how long it is there.
    pub shown_start: u32,
    pub shown_len: u32,
}

/// One shown paragraph: one source line, or several joined while
/// reflowing. Positions in UTF-16 units.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ShownParagraph {
    /// Its source, without the line break that ends it, with any collapsed
    /// lines before it.
    pub source_start: u32,
    pub source_end: u32,
    /// The source lines it shows; `first_line == end_line` for the empty
    /// paragraph after a final line break.
    pub first_line: u32,
    pub end_line: u32,
    pub text: String,
    pub pieces: Vec<ShownPiece>,
}

/// A paragraph and a UTF-16 offset in its shown text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct ShownPos {
    pub paragraph: u32,
    pub offset: u32,
}

/// A document as shown, and where each shown character comes from: the
/// display map. Every position the editor needs goes through it.
#[derive(uniffi::Object)]
pub struct Projection {
    analysis: Arc<Analysis>,
    display: md::display::Display,
    /// Each paragraph's shown text, and the byte offset of each of its
    /// UTF-16 units (and one past its end).
    texts: Vec<(String, Vec<usize>)>,
}

impl Projection {
    /// A paragraph and a UTF-16 offset in its shown text, as the editor
    /// gives them, as a shown position (see `Display::position`).
    fn position(&self, paragraph: u32, offset: u32) -> md::display::Shown {
        let i = (paragraph as usize).min(self.texts.len() - 1);
        let bytes = &self.texts[i].1;
        let b = bytes[(offset as usize).min(bytes.len() - 1)];
        self.display.position(&self.analysis.text, i, b)
    }

    fn shown_u16(&self, i: usize, b: usize) -> u32 {
        self.texts[i].1.partition_point(|&x| x < b) as u32
    }
}

#[uniffi::export]
impl Analysis {
    /// The display map: what shows, with `reflow` (soft breaks as spaces)
    /// and in `source_mode` (everything as it is), with `reveal`'s hidden
    /// syntax shown.
    pub fn project(
        self: Arc<Self>,
        reflow: bool,
        source_mode: bool,
        reveal: Vec<TextRange>,
    ) -> Arc<Projection> {
        let opts = md::display::Options {
            reflow,
            source_mode,
            reveal: reveal.iter().map(|r| self.bytes(r.start, r.end)).collect(),
        };
        let display = md::display::project(&self.text, &self.doc, &opts);
        let texts = display
            .paragraphs
            .iter()
            .map(|p| {
                let t = p.text(&self.text);
                let mut at = Vec::with_capacity(t.len() + 1);
                for (i, c) in t.char_indices() {
                    for _ in 0..c.len_utf16() {
                        at.push(i);
                    }
                }
                at.push(t.len());
                (t, at)
            })
            .collect();
        Arc::new(Projection {
            analysis: self,
            display,
            texts,
        })
    }

    /// The syntax the cursor at `cursor` needs to see: the blank line it is
    /// on, the fences of the code block it is in.
    pub fn reveal_at(&self, cursor: u32) -> Vec<TextRange> {
        md::display::reveal_at(&self.doc, self.b(cursor))
            .into_iter()
            .map(|r| self.range(r))
            .collect()
    }
}

#[uniffi::export]
impl Projection {
    pub fn paragraphs(&self) -> Vec<ShownParagraph> {
        let a = &self.analysis;
        self.display
            .paragraphs
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let mut x = 0;
                let pieces = p
                    .pieces
                    .iter()
                    .map(|piece| {
                        use md::display::Piece;
                        let r = piece.source();
                        let kind = match piece {
                            Piece::Shown(_) => PieceKind::Shown,
                            Piece::Hidden(_) => PieceKind::Hidden,
                            Piece::Replaced(..) => PieceKind::Replaced,
                            Piece::Object(_) => PieceKind::Object,
                        };
                        let len = match piece {
                            Piece::Shown(r) => a.text[r.clone()].encode_utf16().count(),
                            Piece::Hidden(_) => 0,
                            Piece::Replaced(_, t) => t.encode_utf16().count(),
                            Piece::Object(_) => 1,
                        } as u32;
                        let out = ShownPiece {
                            kind,
                            source_start: a.u(r.start),
                            source_end: a.u(r.end),
                            shown_start: x,
                            shown_len: len,
                        };
                        x += len;
                        out
                    })
                    .collect();
                ShownParagraph {
                    source_start: a.u(p.source.start),
                    source_end: a.u(p.source.end),
                    first_line: p.lines.start as u32,
                    end_line: p.lines.end as u32,
                    text: self.texts[i].0.clone(),
                    pieces,
                }
            })
            .collect()
    }

    /// [`Projection::paragraphs`] packed as little-endian `u32`s, which
    /// Swift reads far faster than records: the paragraph count, then per
    /// paragraph its source start and end, first and end line and piece
    /// count, then per piece its kind (shown, hidden, replaced, object),
    /// source start and end, shown start and length, and for a replaced
    /// piece the character it shows as. Without the shown text, which is
    /// the source's but for those characters.
    pub fn paragraphs_packed(&self) -> Vec<u8> {
        use md::display::{OBJECT, Piece};
        let a = &self.analysis;
        let mut out: Vec<u32> = Vec::with_capacity(1 + self.display.paragraphs.len() * 12);
        out.push(self.display.paragraphs.len() as u32);
        for p in &self.display.paragraphs {
            out.extend([
                a.u(p.source.start),
                a.u(p.source.end),
                p.lines.start as u32,
                p.lines.end as u32,
                p.pieces.len() as u32,
            ]);
            let mut x = 0u32;
            for piece in &p.pieces {
                let r = piece.source();
                let (kind, len, ch) = match piece {
                    Piece::Shown(r) => (0, (a.u(r.end) - a.u(r.start)), 0),
                    Piece::Hidden(_) => (1, 0, 0),
                    Piece::Replaced(_, t) => (
                        2,
                        t.encode_utf16().count() as u32,
                        t.chars().next().map_or(0, u32::from),
                    ),
                    Piece::Object(_) => (3, 1, u32::from(OBJECT)),
                };
                out.extend([kind, a.u(r.start), a.u(r.end), x, len, ch]);
                x += len;
            }
        }
        out.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    pub fn paragraph_count(&self) -> u32 {
        self.display.paragraphs.len() as u32
    }

    /// Where source position `pos` shows: hidden syntax where it starts.
    pub fn to_shown(&self, pos: u32) -> ShownPos {
        let s = self.display.to_shown(self.analysis.b(pos));
        ShownPos {
            paragraph: s.paragraph() as u32,
            offset: self.shown_u16(s.paragraph(), s.offset()),
        }
    }

    /// Where the cursor goes for a shown position (see
    /// `Display::to_source`).
    pub fn to_source(&self, at: ShownPos) -> u32 {
        self.analysis.u(self
            .display
            .to_source(self.position(at.paragraph, at.offset)))
    }

    /// The source of the shown text from `start` to `end` (UTF-16 offsets)
    /// of a paragraph: its characters, without hidden syntax around them.
    pub fn source_range(&self, paragraph: u32, start: u32, end: u32) -> TextRange {
        let r = self.display.source_range(
            self.position(paragraph, start),
            self.position(paragraph, end),
        );
        self.analysis.range(r)
    }

    /// One press of Right (`forward`) or Left from source position `pos`.
    pub fn step(&self, pos: u32, forward: bool) -> u32 {
        let a = &self.analysis;
        a.u(self.display.step(&a.text, a.b(pos), forward))
    }

    /// One press of Option-Right (`forward`) or Option-Left from source
    /// position `pos`: to a shown word's end or start.
    pub fn word(&self, pos: u32, forward: bool) -> u32 {
        let a = &self.analysis;
        a.u(self.display.word(&a.text, a.b(pos), forward))
    }

    /// Transpose (Ctrl+T) at source position `pos`, through the editing
    /// rules; nil where there is nothing to swap.
    pub fn transpose(&self, pos: u32) -> Option<EditPlan> {
        let a = &self.analysis;
        let t = self.display.transposed(&a.text, a.b(pos))?;
        Some(a.plan(edit::move_text(&a.text, &a.doc, t.moved, t.to, t.cursor)))
    }
}
