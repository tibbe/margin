//! Structural analysis of Markdown source for WYSIWYG display.
//!
//! The editor never converts Markdown into another representation: the text
//! buffer *is* the file. This module works out which bytes are syntax (and
//! therefore hidden in the rendered view), how each line should be styled,
//! and where list markers, quote bars and code blocks sit so the view can
//! draw them. It also exposes the structure that editing commands need.

use pulldown_cmark::{Alignment, CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd};
use std::ops::Range;

/// A styling instruction for a byte range of the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Style {
    // Line styles cover a whole line, including its newline.
    Para,
    Heading(u8),
    CodeBlock,
    Fence,
    Table,
    HtmlBlock,
    FrontMatter,
    Rule,
    Raw,
    Quote,
    /// Left indentation from enclosing block quotes and list items.
    Indent {
        quotes: u8,
        items: u8,
    },
    /// Space above a line, in pixels at a 12pt body size. All vertical
    /// spacing is expressed as space *above* lines: GTK 4.22 aborts when a
    /// click lands in the space below a line that contains hidden text.
    Above(u16),
    // Character styles.
    Strong,
    Emphasis,
    Strike,
    Code,
    Link,
    Image,
    InlineHtml,
    TableHeader,
    /// Text of a checked task item.
    TaskDone,
    /// Markdown syntax that the rendered view hides.
    Hidden,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub style: Style,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    /// Whitespace only (possibly with container markers such as `>`).
    Blank,
    Paragraph,
    Heading(u8),
    /// The `===`/`---` line under a setext heading.
    SetextUnderline,
    CodeContent,
    Fence,
    Table,
    Html,
    FrontMatter,
    Rule,
    /// Text outside any block, e.g. link reference definitions.
    Raw,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Container {
    Quote(usize),
    Item(usize),
}

#[derive(Clone, Debug)]
pub struct Line {
    pub start: usize,
    /// Exclusive; excludes the newline.
    pub end: usize,
    /// First byte after block syntax (container markers, list bullets,
    /// heading hashes). Everything in `start..content_start` is hidden.
    pub content_start: usize,
    /// First byte the rendered view shows: past the block syntax and any
    /// hidden syntax after it, such as the `**` of bold text that starts
    /// the line; `end` when it shows nothing. What a view draws beside a
    /// line (list markers, quote bars) goes by it, not by `content_start`:
    /// AppKit lays out hidden syntax that starts a line on the line before.
    pub visible_start: usize,
    pub kind: LineKind,
    /// Enclosing block quotes and list items, outermost first.
    pub containers: Vec<Container>,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub range: Range<usize>,
    pub line: usize,
    /// Bullet or number and the whitespace after it, e.g. `- ` or `12. `.
    pub marker: Range<usize>,
    /// Checkbox of a task item (`[ ]`/`[x]`) and whether it is checked.
    pub task: Option<(bool, Range<usize>)>,
    /// The number the item renders with, for ordered lists.
    pub number: Option<u64>,
    pub list: usize,
    /// 1 for a top-level list.
    pub depth: usize,
    /// Just a marker with nothing after it, not even a space (`-`, `1.`):
    /// shown as typed until the space turns it into a list item.
    pub bare: bool,
}

#[derive(Clone, Debug)]
pub struct List {
    pub range: Range<usize>,
    pub ordered: bool,
    pub start: u64,
    pub items: Vec<usize>,
    pub depth: usize,
}

#[derive(Clone, Debug)]
pub struct Quote {
    pub range: Range<usize>,
    pub first_line: usize,
    pub last_line: usize,
    pub depth: usize,
}

#[derive(Clone, Debug)]
pub struct CodeBlock {
    pub range: Range<usize>,
    pub fenced: bool,
    pub info: String,
    pub open_line: Option<usize>,
    pub close_line: Option<usize>,
    /// All lines of the block, fences included.
    pub first_line: usize,
    pub last_line: usize,
}

impl CodeBlock {
    /// Lines holding code, excluding fences. May be empty.
    pub fn content_lines(&self) -> Range<usize> {
        let first = self.open_line.map_or(self.first_line, |l| l + 1);
        let last = self.close_line.unwrap_or(self.last_line + 1);
        first..last.max(first)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph,
    Heading(u8),
    Code(usize),
    Table,
    Html,
    FrontMatter,
    Rule,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub kind: BlockKind,
    pub range: Range<usize>,
    pub first_line: usize,
    pub last_line: usize,
    /// Innermost list item containing the block.
    pub item: Option<usize>,
}

/// A GitHub pipe table, row by row, so a view can lay it out as a grid.
#[derive(Clone, Debug)]
pub struct Table {
    pub range: Range<usize>,
    /// The `|---|:--:|` line under the header.
    pub delimiter_line: usize,
    /// One per column.
    pub aligns: Vec<Align>,
    /// The header row first. A row may have fewer cells than there are
    /// columns; the parser pads such rows, but the padding has no source.
    pub rows: Vec<TableRow>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug)]
pub struct TableRow {
    pub line: usize,
    pub cells: Vec<TableCell>,
    /// After the last cell's content: its padding and the closing `|`.
    pub trail: Range<usize>,
}

#[derive(Clone, Debug)]
pub struct TableCell {
    /// The cell's text without its padding. An empty cell's is an empty
    /// range one space into the cell, where typing goes.
    pub content: Range<usize>,
    /// Between the previous cell's content (or the start of the row) and
    /// this cell's: padding and the `|` between them.
    pub lead: Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineKind {
    Strong,
    Emphasis,
    Strike,
    Code,
    Link,
    Image,
}

#[derive(Clone, Debug)]
pub struct Inline {
    pub kind: InlineKind,
    /// Opening syntax, e.g. `**` or `[`.
    pub open: Range<usize>,
    /// Closing syntax, e.g. `**` or `](https://…)`.
    pub close: Range<usize>,
    pub url: Option<String>,
}

impl Inline {
    pub fn content(&self) -> Range<usize> {
        self.open.end..self.close.start
    }

    pub fn range(&self) -> Range<usize> {
        self.open.start..self.close.end
    }
}

/// An image alone in its paragraph, in a list item or a quote too, which
/// an editor shows as the image: one object, which the cursor never rests
/// in. An image in running text is an [`Inline`] only, shown as its alt
/// text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageBlock {
    /// The whole source, `![alt](url "title")`, within one line.
    pub range: Range<usize>,
    pub line: usize,
    /// The destination, unescaped: a path or a URL.
    pub url: String,
    /// The alt text as shown, as source ranges: the text between `![` and
    /// `]`, without the syntax inside it (emphasis markers, escapes).
    pub alt: Vec<Range<usize>>,
}

/// A `mermaid` fenced code block, which an editor shows as its diagram:
/// one object, as an image block is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagramBlock {
    /// From its opening fence to the end of its last line.
    pub range: Range<usize>,
    pub first_line: usize,
    pub last_line: usize,
    /// The diagram's source: its code, without the fences or the prefixes
    /// of the lists and quotes it is in, each line ended by a newline.
    pub source: String,
    /// Where each line of `source` starts in the document.
    pub line_starts: Vec<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct Doc {
    pub len: usize,
    pub lines: Vec<Line>,
    pub blocks: Vec<Block>,
    pub items: Vec<Item>,
    pub lists: Vec<List>,
    pub quotes: Vec<Quote>,
    pub code_blocks: Vec<CodeBlock>,
    pub tables: Vec<Table>,
    /// In source order, at most one per line.
    pub images: Vec<ImageBlock>,
    /// In source order.
    pub diagrams: Vec<DiagramBlock>,
    pub inlines: Vec<Inline>,
    /// Hidden inline syntax (emphasis markers, link destinations, escapes),
    /// sorted and merged. Block prefixes are described by
    /// [`Line::content_start`] instead.
    pub hidden: Vec<Range<usize>>,
    pub spans: Vec<Span>,
    /// Newlines inside paragraphs (CommonMark soft line breaks), which
    /// render as spaces.
    pub soft_breaks: Vec<usize>,
}

// Vertical spacing at a 12pt (16px) body size, in pixels, within the range
// of common editors' and word processors' defaults. With a 24px line pitch,
// a paragraph break is 1.5 lines.
const GAP_ITEM: u16 = 5;
const GAP_BLOCK: u16 = 12;
const GAP_AFTER_HEADING: u16 = 6;
const CODE_PAD: u16 = 10;
const RULE_PAD: u16 = 4;
const HEADING_ABOVE: [u16; 6] = [26, 22, 18, 14, 14, 14];

fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
}

struct OpenInline {
    kind: InlineKind,
    range: Range<usize>,
    first_child: Option<usize>,
    last_child_end: usize,
    url: Option<String>,
    link_type: Option<LinkType>,
}

struct Builder<'a> {
    src: &'a str,
    lines: Vec<Line>,
    covered: Vec<Range<usize>>,
    hidden: Vec<Range<usize>>,
    spans: Vec<Span>,
    blocks: Vec<Block>,
    items: Vec<Item>,
    lists: Vec<List>,
    quotes: Vec<Quote>,
    code_blocks: Vec<CodeBlock>,
    inlines: Vec<Inline>,
    open_inlines: Vec<OpenInline>,
    list_stack: Vec<usize>,
    item_stack: Vec<usize>,
    quote_depth: usize,
    leaf: Option<(BlockKind, usize)>,
    /// Inline content directly inside a tight list item (no paragraph event).
    implicit: Option<Range<usize>>,
    table_heads: Vec<Range<usize>>,
    tables: Vec<Table>,
    /// Start of the table row being parsed (after any container markers).
    row_start: usize,
    code_info: Option<(bool, String)>,
    /// Plain text outside links and code, where bare URLs are looked for.
    texts: Vec<Range<usize>>,
    soft_breaks: Vec<usize>,
    /// Lines whose block marker is shown as typed (`#` without a space).
    bare_lines: Vec<usize>,
}

pub fn parse(src: &str) -> Doc {
    let mut b = Builder {
        src,
        lines: split_lines(src),
        covered: Vec::new(),
        hidden: Vec::new(),
        spans: Vec::new(),
        blocks: Vec::new(),
        items: Vec::new(),
        lists: Vec::new(),
        quotes: Vec::new(),
        code_blocks: Vec::new(),
        inlines: Vec::new(),
        open_inlines: Vec::new(),
        list_stack: Vec::new(),
        item_stack: Vec::new(),
        quote_depth: 0,
        leaf: None,
        implicit: None,
        table_heads: Vec::new(),
        tables: Vec::new(),
        row_start: 0,
        code_info: None,
        texts: Vec::new(),
        soft_breaks: Vec::new(),
        bare_lines: Vec::new(),
    };
    for (event, range) in Parser::new_ext(src, options()).into_offset_iter() {
        b.event(event, range);
    }
    b.flush_implicit();
    b.finish()
}

fn split_lines(src: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (i, c) in src.bytes().enumerate() {
        if c == b'\n' {
            lines.push(new_line(start, i));
            start = i + 1;
        }
    }
    lines.push(new_line(start, src.len()));
    lines
}

fn new_line(start: usize, end: usize) -> Line {
    Line {
        start,
        end,
        content_start: start,
        visible_start: start,
        kind: LineKind::Blank,
        containers: Vec::new(),
    }
}

fn line_index(lines: &[Line], pos: usize) -> usize {
    match lines.binary_search_by(|l| l.start.cmp(&pos)) {
        Ok(i) => i,
        Err(i) => i.saturating_sub(1),
    }
}

impl Builder<'_> {
    fn line_of(&self, pos: usize) -> usize {
        line_index(&self.lines, pos)
    }

    /// Last line touched by `range`, treating a trailing newline as part of
    /// the line it ends.
    fn last_line_of(&self, range: &Range<usize>) -> usize {
        self.line_of(range.end.saturating_sub(1).max(range.start))
    }

    fn note_child(&mut self, range: &Range<usize>) {
        if let Some(p) = self.open_inlines.last_mut() {
            p.first_child.get_or_insert(range.start);
            p.last_child_end = p.last_child_end.max(range.end);
        }
    }

    /// Tracks inline content that sits directly in a tight list item.
    fn note_inline(&mut self, range: &Range<usize>) {
        if self.leaf.is_none() && !self.item_stack.is_empty() {
            match &mut self.implicit {
                Some(r) => r.end = r.end.max(range.end),
                None => self.implicit = Some(range.clone()),
            }
        }
    }

    fn flush_implicit(&mut self) {
        if let Some(range) = self.implicit.take() {
            let first_line = self.line_of(range.start);
            let last_line = self.last_line_of(&range);
            self.blocks.push(Block {
                kind: BlockKind::Paragraph,
                range,
                first_line,
                last_line,
                item: self.item_stack.last().copied(),
            });
        }
    }

    fn event(&mut self, event: Event<'_>, range: Range<usize>) {
        match event {
            Event::Start(tag) => self.start(tag, range),
            Event::End(tag) => self.end(tag, range),
            Event::Text(_)
            | Event::Html(_)
            | Event::InlineHtml(_)
            | Event::FootnoteReference(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_) => {
                if matches!(event, Event::InlineHtml(_)) {
                    self.spans.push(Span {
                        range: range.clone(),
                        style: Style::InlineHtml,
                    });
                }
                let linkable = matches!(event, Event::Text(_))
                    && matches!(
                        self.leaf,
                        None | Some((BlockKind::Paragraph | BlockKind::Heading(_), _))
                    )
                    && !self
                        .open_inlines
                        .iter()
                        .any(|o| matches!(o.kind, InlineKind::Link | InlineKind::Image));
                if linkable {
                    self.texts.push(range.clone());
                }
                self.note_child(&range);
                self.note_inline(&range);
                self.covered.push(range);
            }
            Event::Code(_) => {
                self.note_child(&range);
                self.note_inline(&range);
                let bytes = self.src.as_bytes();
                let ticks = bytes[range.clone()]
                    .iter()
                    .take_while(|&&c| c == b'`')
                    .count();
                let open = range.start..range.start + ticks;
                let close = range.end - ticks..range.end;
                self.hidden.push(open.clone());
                self.hidden.push(close.clone());
                self.inlines.push(Inline {
                    kind: InlineKind::Code,
                    open,
                    close,
                    url: None,
                });
                self.covered.push(range);
            }
            Event::SoftBreak => {
                self.note_child(&range);
                self.note_inline(&range);
                let in_paragraph = matches!(self.leaf, None | Some((BlockKind::Paragraph, _)));
                if in_paragraph && self.src.as_bytes().get(range.start) == Some(&b'\n') {
                    self.soft_breaks.push(range.start);
                }
            }
            Event::HardBreak => {
                self.note_child(&range);
                self.note_inline(&range);
                let end = if self.src.as_bytes().get(range.end - 1) == Some(&b'\n') {
                    range.end - 1
                } else {
                    range.end
                };
                if end > range.start {
                    self.hidden.push(range.start..end);
                }
            }
            Event::Rule => {
                self.flush_implicit();
                let first_line = self.line_of(range.start);
                self.blocks.push(Block {
                    kind: BlockKind::Rule,
                    range,
                    first_line,
                    last_line: first_line,
                    item: self.item_stack.last().copied(),
                });
            }
            Event::TaskListMarker(checked) => {
                if let Some(&i) = self.item_stack.last() {
                    self.items[i].task = Some((checked, range));
                }
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>, range: Range<usize>) {
        let inline_kind = match &tag {
            Tag::Emphasis => Some(InlineKind::Emphasis),
            Tag::Strong => Some(InlineKind::Strong),
            Tag::Strikethrough => Some(InlineKind::Strike),
            Tag::Link { .. } => Some(InlineKind::Link),
            Tag::Image { .. } => Some(InlineKind::Image),
            _ => None,
        };
        if let Some(kind) = inline_kind {
            self.note_child(&range);
            self.note_inline(&range);
            self.covered.push(range.clone());
            let (url, link_type) = match tag {
                Tag::Link {
                    dest_url,
                    link_type,
                    ..
                }
                | Tag::Image {
                    dest_url,
                    link_type,
                    ..
                } => (Some(dest_url.to_string()), Some(link_type)),
                _ => (None, None),
            };
            self.open_inlines.push(OpenInline {
                kind,
                range: range.clone(),
                first_child: None,
                last_child_end: range.start,
                url,
                link_type,
            });
            return;
        }
        match tag {
            Tag::Paragraph => {
                self.flush_implicit();
                self.leaf = Some((BlockKind::Paragraph, range.start));
            }
            Tag::Heading { level, .. } => {
                self.flush_implicit();
                self.leaf = Some((BlockKind::Heading(level as u8), range.start));
            }
            Tag::CodeBlock(kind) => {
                self.flush_implicit();
                self.code_info = Some(match kind {
                    CodeBlockKind::Fenced(info) => (true, info.to_string()),
                    CodeBlockKind::Indented => (false, String::new()),
                });
                self.leaf = Some((BlockKind::Code(self.code_blocks.len()), range.start));
            }
            Tag::Table(aligns) => {
                self.flush_implicit();
                self.leaf = Some((BlockKind::Table, range.start));
                let line = self.line_of(range.start);
                self.tables.push(Table {
                    range: range.clone(),
                    delimiter_line: line + 1,
                    aligns: aligns
                        .iter()
                        .map(|a| match a {
                            Alignment::None => Align::None,
                            Alignment::Left => Align::Left,
                            Alignment::Center => Align::Center,
                            Alignment::Right => Align::Right,
                        })
                        .collect(),
                    rows: Vec::new(),
                });
            }
            Tag::TableHead | Tag::TableRow => {
                if matches!(tag, Tag::TableHead) {
                    self.table_heads.push(range.clone());
                }
                self.covered.push(range.clone());
                let line = self.line_of(range.start);
                self.row_start = range.start;
                if let Some(t) = self.tables.last_mut() {
                    t.rows.push(TableRow {
                        line,
                        cells: Vec::new(),
                        trail: range.start..range.start,
                    });
                }
            }
            Tag::TableCell => self.table_cell(range),
            Tag::HtmlBlock => {
                self.flush_implicit();
                self.leaf = Some((BlockKind::Html, range.start));
            }
            Tag::MetadataBlock(_) => {
                self.leaf = Some((BlockKind::FrontMatter, range.start));
                self.covered.push(range);
            }
            Tag::BlockQuote(_) => {
                self.flush_implicit();
                self.quote_depth += 1;
                let first_line = self.line_of(range.start);
                let last_line = self.last_line_of(&range);
                self.quotes.push(Quote {
                    range,
                    first_line,
                    last_line,
                    depth: self.quote_depth,
                });
            }
            Tag::List(start) => {
                self.flush_implicit();
                self.lists.push(List {
                    range,
                    ordered: start.is_some(),
                    start: start.unwrap_or(1),
                    items: Vec::new(),
                    depth: self.list_stack.len() + 1,
                });
                self.list_stack.push(self.lists.len() - 1);
            }
            Tag::Item => {
                self.flush_implicit();
                let Some(&list) = self.list_stack.last() else {
                    return;
                };
                let index = self.lists[list].items.len() as u64;
                let number = self.lists[list]
                    .ordered
                    .then(|| self.lists[list].start + index);
                let line = self.line_of(range.start);
                self.items.push(Item {
                    range: range.clone(),
                    line,
                    marker: range.start..range.start,
                    task: None,
                    number,
                    list,
                    depth: self.list_stack.len(),
                    bare: false,
                });
                let i = self.items.len() - 1;
                self.lists[list].items.push(i);
                self.item_stack.push(i);
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd, range: Range<usize>) {
        match tag {
            TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Link
            | TagEnd::Image => {
                let Some(o) = self.open_inlines.pop() else {
                    return;
                };
                let open_len = match o.kind {
                    InlineKind::Image => 2,
                    _ => 1,
                };
                let (open, close) = match o.first_child {
                    Some(first) => (o.range.start..first, o.last_child_end..o.range.end),
                    // Empty link text: `[](url)`.
                    None => {
                        let split = (o.range.start + open_len).min(o.range.end);
                        (o.range.start..split, split..o.range.end)
                    }
                };
                // Autolinks (`<https://…>`) keep their text visible.
                let _ = o.link_type;
                if !open.is_empty() {
                    self.hidden.push(open.clone());
                }
                if !close.is_empty() {
                    self.hidden.push(close.clone());
                }
                self.inlines.push(Inline {
                    kind: o.kind,
                    open,
                    close,
                    url: o.url,
                });
            }
            TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::CodeBlock
            | TagEnd::Table
            | TagEnd::HtmlBlock
            | TagEnd::MetadataBlock(_) => {
                let Some((kind, start)) = self.leaf.take() else {
                    return;
                };
                let range = start..range.end;
                let first_line = self.line_of(range.start);
                let last_line = self.last_line_of(&range);
                if let BlockKind::Code(_) = kind {
                    self.finish_code_block(range.clone(), first_line, last_line);
                }
                self.blocks.push(Block {
                    kind,
                    range,
                    first_line,
                    last_line,
                    item: self.item_stack.last().copied(),
                });
            }
            TagEnd::TableHead | TagEnd::TableRow => {
                let Some(row) = self.tables.last_mut().and_then(|t| t.rows.last_mut()) else {
                    return;
                };
                let from = row.cells.last().map_or(self.row_start, |c| c.content.end);
                row.trail = from..self.lines[row.line].end.max(from);
            }
            TagEnd::BlockQuote(_) => {
                self.flush_implicit();
                self.quote_depth = self.quote_depth.saturating_sub(1);
            }
            TagEnd::List(_) => {
                self.flush_implicit();
                self.list_stack.pop();
            }
            TagEnd::Item => {
                self.flush_implicit();
                self.item_stack.pop();
            }
            _ => {}
        }
    }

    fn table_cell(&mut self, raw: Range<usize>) {
        let Some(row) = self.tables.last_mut().and_then(|t| t.rows.last_mut()) else {
            return;
        };
        // Cells the parser adds to short rows sit past the line's end.
        if raw.start > self.lines[row.line].end {
            return;
        }
        let text = &self.src[raw.clone()];
        let start = raw.start + (text.len() - text.trim_start().len());
        let end = raw.end - (text.len() - text.trim_end().len());
        let content = if start < end {
            start..end
        } else {
            let p = (raw.start + 1).min(raw.end);
            p..p
        };
        let from = row.cells.last().map_or(self.row_start, |c| c.content.end);
        row.cells.push(TableCell {
            lead: from..content.start.max(from),
            content,
        });
    }

    fn finish_code_block(&mut self, range: Range<usize>, first_line: usize, last_line: usize) {
        let (fenced, info) = self.code_info.take().unwrap_or((false, String::new()));
        let (open_line, close_line) = if fenced {
            let bytes = self.src.as_bytes();
            let fence_char = bytes[range.start];
            let fence_len = bytes[range.start..]
                .iter()
                .take_while(|&&c| c == fence_char)
                .count();
            let close = (last_line > first_line
                && is_closing_fence(self.line_text(last_line), fence_char, fence_len))
            .then_some(last_line);
            (Some(first_line), close)
        } else {
            (None, None)
        };
        self.code_blocks.push(CodeBlock {
            range,
            fenced,
            info,
            open_line,
            close_line,
            first_line,
            last_line,
        });
    }

    fn line_text(&self, line: usize) -> &str {
        let l = &self.lines[line];
        &self.src[l.start..l.end]
    }

    fn finish(mut self) -> Doc {
        self.covered.sort_by_key(|r| r.start);
        let covered = merge(std::mem::take(&mut self.covered));

        self.fix_single_dash_setext();
        self.assign_line_kinds();
        self.mark_bare_markers(&covered);
        self.assign_containers();
        self.assign_content_starts(&covered);
        self.assign_item_markers();
        self.hide_uncovered(&covered);
        self.hide_pending_hard_breaks();
        self.find_bare_urls();
        self.emit_spans();
        self.assign_visible_starts();

        let mut hidden = std::mem::take(&mut self.hidden);
        hidden.retain(|r| !r.is_empty());
        hidden.sort_by_key(|r| r.start);
        let hidden = merge(hidden);
        self.inlines.sort_by_key(|i| i.open.start);
        self.blocks.sort_by_key(|b| b.range.start);
        let images = self.image_blocks(&hidden);
        let diagrams = self.diagram_blocks();

        Doc {
            len: self.src.len(),
            lines: self.lines,
            blocks: self.blocks,
            items: self.items,
            lists: self.lists,
            quotes: self.quotes,
            code_blocks: self.code_blocks,
            tables: self.tables,
            images,
            diagrams,
            inlines: self.inlines,
            hidden,
            spans: self.spans,
            soft_breaks: self.soft_breaks,
        }
    }

    /// `- one` followed by an indented `-` parses as a setext heading,
    /// because an empty list item cannot interrupt a paragraph. In an editor
    /// that line is an item the user is about to type into, so treat it as
    /// one. Real setext underlines use more than one `-`.
    fn fix_single_dash_setext(&mut self) {
        for bi in 0..self.blocks.len() {
            let b = self.blocks[bi].clone();
            if b.kind != BlockKind::Heading(2) || b.last_line == b.first_line {
                continue;
            }
            let under = self.line_text(b.last_line);
            if under.trim_matches(|c: char| c == ' ' || c == '\t' || c == '>') != "-" {
                continue;
            }
            let line = self.lines[b.last_line].clone();
            let dash = line.start + under.find('-').unwrap_or(0);
            let text_end = self.lines[b.last_line - 1].end;
            self.blocks[bi].kind = BlockKind::Paragraph;
            self.blocks[bi].range = b.range.start..text_end;
            self.blocks[bi].last_line = b.last_line - 1;
            let end = (line.end + 1).min(self.src.len());
            let depth = self
                .items
                .iter()
                .filter(|it| it.range.start <= dash && dash < it.range.end)
                .map(|it| it.depth)
                .max()
                .unwrap_or(0)
                + 1;
            self.lists.push(List {
                range: dash..end,
                ordered: false,
                start: 1,
                items: vec![self.items.len()],
                depth,
            });
            self.items.push(Item {
                range: dash..end,
                line: b.last_line,
                marker: dash..dash,
                task: None,
                number: None,
                list: self.lists.len() - 1,
                depth,
                bare: false,
            });
            self.lines[b.last_line].content_start = line.end;
        }
    }

    /// A marker alone on its line, without the space that completes it
    /// (`#`, `-`, `1.`), is shown as typed rather than as an empty heading
    /// or list item: the user is probably still typing, and every editor
    /// waits for the space.
    fn mark_bare_markers(&mut self, covered: &[Range<usize>]) {
        for bi in 0..self.blocks.len() {
            let b = &self.blocks[bi];
            if let BlockKind::Heading(_) = b.kind
                && b.first_line == b.last_line
                && first_covered(
                    covered,
                    self.lines[b.first_line].start,
                    self.lines[b.first_line].end,
                )
                .is_none()
                && !self.line_text(b.first_line).ends_with([' ', '\t'])
            {
                let l = b.first_line;
                self.blocks[bi].kind = BlockKind::Paragraph;
                self.lines[l].kind = LineKind::Paragraph;
                self.bare_lines.push(l);
            }
        }
        for i in 0..self.items.len() {
            let (line, start, has_task) = {
                let it = &self.items[i];
                (it.line, it.range.start, it.task.is_some())
            };
            let l = &self.lines[line];
            let rest = &self.src[start.min(l.end)..l.end];
            let is_marker = !rest.is_empty()
                && (rest == "-"
                    || rest == "*"
                    || rest == "+"
                    || (rest.len() >= 2
                        && rest.ends_with(['.', ')'])
                        && rest[..rest.len() - 1].bytes().all(|c| c.is_ascii_digit())));
            if is_marker && !has_task {
                self.items[i].bare = true;
                self.bare_lines.push(line);
            }
        }
    }

    fn assign_line_kinds(&mut self) {
        for bi in 0..self.blocks.len() {
            let b = self.blocks[bi].clone();
            match b.kind {
                BlockKind::Paragraph => {
                    self.set_kind(b.first_line..=b.last_line, LineKind::Paragraph)
                }
                BlockKind::Heading(level) => {
                    let setext = b.last_line > b.first_line
                        && is_setext_underline(self.line_text(b.last_line));
                    if setext {
                        self.set_kind(b.first_line..=b.last_line - 1, LineKind::Heading(level));
                        self.lines[b.last_line].kind = LineKind::SetextUnderline;
                    } else {
                        self.set_kind(b.first_line..=b.last_line, LineKind::Heading(level));
                    }
                }
                BlockKind::Code(ci) => {
                    let cb = self.code_blocks[ci].clone();
                    for l in cb.first_line..=cb.last_line {
                        self.lines[l].kind = if Some(l) == cb.open_line || Some(l) == cb.close_line
                        {
                            LineKind::Fence
                        } else {
                            LineKind::CodeContent
                        };
                    }
                }
                BlockKind::Table => self.set_kind(b.first_line..=b.last_line, LineKind::Table),
                BlockKind::Html => self.set_kind(b.first_line..=b.last_line, LineKind::Html),
                BlockKind::FrontMatter => {
                    self.set_kind(b.first_line..=b.last_line, LineKind::FrontMatter)
                }
                BlockKind::Rule => self.lines[b.first_line].kind = LineKind::Rule,
            }
        }
        for i in 0..self.lines.len() {
            if self.lines[i].kind == LineKind::Blank {
                let text = self.line_text(i);
                if !text.chars().all(|c| c.is_whitespace() || c == '>') {
                    self.lines[i].kind = LineKind::Raw;
                }
            }
        }
        // Empty list items ("- ") have no block but must stay visible.
        for item in &self.items {
            let line = &mut self.lines[item.line];
            if matches!(line.kind, LineKind::Blank | LineKind::Raw) {
                line.kind = LineKind::Paragraph;
            }
        }
    }

    fn set_kind(&mut self, lines: std::ops::RangeInclusive<usize>, kind: LineKind) {
        for l in lines {
            self.lines[l].kind = kind;
        }
    }

    fn assign_containers(&mut self) {
        let mut all: Vec<(Range<usize>, Container)> = self
            .quotes
            .iter()
            .enumerate()
            .map(|(i, q)| (q.range.clone(), Container::Quote(i)))
            .chain(
                self.items
                    .iter()
                    .enumerate()
                    .filter(|(_, it)| !it.bare)
                    .map(|(i, it)| (it.range.clone(), Container::Item(i))),
            )
            .collect();
        // Outer containers first: earlier start, then longer range.
        all.sort_by(|a, b| a.0.start.cmp(&b.0.start).then(b.0.end.cmp(&a.0.end)));
        for (range, c) in all {
            let first = self.line_of(range.start);
            let last = self.last_line_of(&range);
            for l in first..=last {
                self.lines[l].containers.push(c);
            }
        }
    }

    fn assign_content_starts(&mut self, covered: &[Range<usize>]) {
        for i in 0..self.lines.len() {
            let (start, end, kind) = {
                let l = &self.lines[i];
                (l.start, l.end, l.kind)
            };
            let in_quote = self.lines[i]
                .containers
                .iter()
                .any(|c| matches!(c, Container::Quote(_)));
            let first_covered = first_covered(covered, start, end);
            let content_start = match kind {
                LineKind::Blank | LineKind::SetextUnderline => start,
                LineKind::Paragraph | LineKind::Heading(_) => first_covered.unwrap_or(end),
                LineKind::CodeContent => first_covered.unwrap_or(start).min(end),
                LineKind::Fence => {
                    let text = &self.src[start..end];
                    start + text.find(['`', '~']).unwrap_or(0)
                }
                LineKind::Rule => {
                    let text = &self.src[start..end];
                    start + text.find(['-', '*', '_']).unwrap_or(0)
                }
                LineKind::FrontMatter => start,
                LineKind::Table | LineKind::Html | LineKind::Raw => match first_covered {
                    Some(p) if p < end => p,
                    _ => start + skip_prefix(&self.src[start..end], in_quote),
                },
            };
            let content_start = if self.bare_lines.contains(&i) {
                start + skip_prefix(&self.src[start..end], in_quote)
            } else {
                content_start
            };
            self.lines[i].content_start = content_start.min(end).max(start);
        }
    }

    fn assign_item_markers(&mut self) {
        for i in 0..self.items.len() {
            let line = &self.lines[self.items[i].line];
            let content_start = line.content_start;
            let start = self.items[i].range.start;
            let marker_end = match &self.items[i].task {
                Some((_, task)) => task.start,
                None => content_start,
            };
            self.items[i].marker = start..marker_end.max(start);
        }
    }

    /// Hides bytes inside paragraphs and headings that no content event
    /// covers: escape backslashes and closing `#`s. Uncovered whitespace
    /// (trailing spaces the parser trims) stays: it is invisible anyway,
    /// and hiding it would make a just-typed space vanish from under the
    /// cursor.
    fn hide_uncovered(&mut self, covered: &[Range<usize>]) {
        let mut out = Vec::new();
        for (li, l) in self.lines.iter().enumerate() {
            if !matches!(l.kind, LineKind::Paragraph | LineKind::Heading(_))
                || self.bare_lines.contains(&li)
            {
                continue;
            }
            let mut pos = l.content_start;
            let mut idx = covered.partition_point(|r| r.end <= pos);
            while pos < l.end {
                let gap = match covered.get(idx) {
                    Some(r) if r.start <= pos => {
                        pos = r.end.max(pos);
                        idx += 1;
                        continue;
                    }
                    Some(r) if r.start < l.end => {
                        let g = pos..r.start;
                        pos = r.end;
                        idx += 1;
                        g
                    }
                    _ => {
                        let g = pos..l.end;
                        pos = l.end;
                        g
                    }
                };
                if self.src[gap.clone()].chars().any(|c| !c.is_whitespace()) {
                    out.push(gap);
                }
            }
        }
        self.hidden.extend(out);
    }

    /// A backslash at the very end of a paragraph is a line break still
    /// waiting for its next line (Shift+Enter was just pressed); hide it
    /// rather than flash a literal `\`.
    fn hide_pending_hard_breaks(&mut self) {
        for b in &self.blocks {
            if b.kind != BlockKind::Paragraph {
                continue;
            }
            let l = &self.lines[b.last_line];
            let text = &self.src[l.content_start..l.end];
            let slashes = text.bytes().rev().take_while(|&c| c == b'\\').count();
            if slashes % 2 == 1 {
                self.hidden.push(l.end - 1..l.end);
            }
        }
    }

    /// Plain-text `http(s)://` URLs become links, as GitHub renders them.
    fn find_bare_urls(&mut self) {
        // The parser splits text at characters that might be markup (`_`,
        // `*`); rejoin the pieces so a URL is seen whole.
        let mut texts: Vec<Range<usize>> = Vec::new();
        for r in &self.texts {
            match texts.last_mut() {
                Some(last) if last.end == r.start => last.end = r.end,
                _ => texts.push(r.clone()),
            }
        }
        let mut found = Vec::new();
        for r in &texts {
            let text = &self.src[r.clone()];
            let mut from = 0;
            while let Some(off) = text[from..].find("http") {
                let start = from + off;
                let rest = &text[start..];
                let scheme = if rest.starts_with("https://") {
                    8
                } else if rest.starts_with("http://") {
                    7
                } else {
                    from = start + 4;
                    continue;
                };
                let boundary_ok = start == 0
                    || !text[..start]
                        .chars()
                        .next_back()
                        .is_some_and(|c| c.is_alphanumeric());
                let mut end = rest
                    .find(|c: char| c.is_whitespace() || c == '<' || c == '>' || c == '"')
                    .unwrap_or(rest.len());
                // Trailing punctuation belongs to the sentence, and a closing
                // parenthesis only to the URL if it opened one.
                while end > scheme {
                    let c = rest[..end].chars().next_back().unwrap();
                    let unbalanced_paren = c == ')'
                        && rest[..end].matches('(').count() < rest[..end].matches(')').count();
                    if matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | '\'' | '*' | '_')
                        || unbalanced_paren
                    {
                        end -= c.len_utf8();
                    } else {
                        break;
                    }
                }
                if boundary_ok && end > scheme {
                    let a = r.start + start;
                    found.push((a..a + end, rest[..end].to_string()));
                }
                from = start + end.max(1);
            }
        }
        for (range, url) in found {
            self.inlines.push(Inline {
                kind: InlineKind::Link,
                open: range.start..range.start,
                close: range.end..range.end,
                url: Some(url),
            });
        }
    }

    fn emit_spans(&mut self) {
        let len = self.src.len();
        let with_newline = |l: &Line| l.start..(l.end + 1).min(len);
        let mut spans = std::mem::take(&mut self.spans);

        for l in &self.lines {
            let whole = with_newline(l);
            let style = match l.kind {
                LineKind::Blank | LineKind::SetextUnderline => None,
                LineKind::Paragraph => Some(Style::Para),
                LineKind::Heading(n) => Some(Style::Heading(n)),
                LineKind::CodeContent => Some(Style::CodeBlock),
                LineKind::Fence => Some(Style::Fence),
                LineKind::Table => Some(Style::Table),
                LineKind::Html => Some(Style::HtmlBlock),
                LineKind::FrontMatter => Some(Style::FrontMatter),
                LineKind::Rule => Some(Style::Rule),
                LineKind::Raw => Some(Style::Raw),
            };
            if let Some(style) = style {
                spans.push(Span {
                    range: whole.clone(),
                    style,
                });
            }
            match l.kind {
                LineKind::Blank | LineKind::SetextUnderline | LineKind::Fence => {
                    if !whole.is_empty() {
                        spans.push(Span {
                            range: whole.clone(),
                            style: Style::Hidden,
                        });
                    }
                }
                _ => {
                    if l.content_start > l.start {
                        spans.push(Span {
                            range: l.start..l.content_start,
                            style: Style::Hidden,
                        });
                    }
                }
            }
            if l.kind == LineKind::Rule && l.end > l.content_start {
                spans.push(Span {
                    range: l.content_start..l.end,
                    style: Style::Hidden,
                });
            }
            if !l.containers.is_empty() && !matches!(l.kind, LineKind::Blank) {
                let quotes = l
                    .containers
                    .iter()
                    .filter(|c| matches!(c, Container::Quote(_)))
                    .count() as u8;
                let items = l.containers.len() as u8 - quotes;
                spans.push(Span {
                    range: whole.clone(),
                    style: Style::Indent { quotes, items },
                });
                if quotes > 0 && matches!(l.kind, LineKind::Paragraph | LineKind::Heading(_)) {
                    spans.push(Span {
                        range: whole,
                        style: Style::Quote,
                    });
                }
            }
        }

        for head in &self.table_heads {
            spans.push(Span {
                range: head.clone(),
                style: Style::TableHeader,
            });
        }

        for item in &self.items {
            if let Some((true, _)) = item.task {
                let l = &self.lines[item.line];
                if l.end > l.content_start {
                    spans.push(Span {
                        range: l.content_start..l.end,
                        style: Style::TaskDone,
                    });
                }
            }
        }

        for inline in &self.inlines {
            let style = match inline.kind {
                InlineKind::Strong => Style::Strong,
                InlineKind::Emphasis => Style::Emphasis,
                InlineKind::Strike => Style::Strike,
                InlineKind::Code => Style::Code,
                InlineKind::Link => Style::Link,
                InlineKind::Image => Style::Image,
            };
            let content = inline.content();
            if !content.is_empty() {
                spans.push(Span {
                    range: content,
                    style,
                });
            }
        }

        for r in &self.hidden {
            if !r.is_empty() {
                spans.push(Span {
                    range: r.clone(),
                    style: Style::Hidden,
                });
            }
        }

        // Vertical spacing, as space above the first visible line of each
        // block.
        let mut above: Vec<u16> = vec![0; self.lines.len()];
        struct Visual {
            first: usize,
            gap_after: u16,
            own: u16,
            heading: Option<u8>,
        }
        let mut visuals: Vec<Visual> = Vec::new();
        // End of the top-level list each list belongs to, computed once:
        // looking it up per block made this quadratic in long lists.
        let mut order: Vec<usize> = (0..self.lists.len()).collect();
        order.sort_by_key(|&i| {
            (
                self.lists[i].range.start,
                std::cmp::Reverse(self.lists[i].range.end),
            )
        });
        let mut outer_end = vec![0; self.lists.len()];
        let mut top: Option<Range<usize>> = None;
        for &i in &order {
            let r = &self.lists[i].range;
            match &top {
                Some(t) if t.start <= r.start && r.end <= t.end => outer_end[i] = t.end,
                _ => {
                    top = Some(r.clone());
                    outer_end[i] = r.end;
                }
            }
        }
        let outermost_list_end = |item: usize| -> usize { outer_end[self.items[item].list] };
        let in_list_gap = |item: Option<usize>, last_line: usize, lines: &[Line]| -> Option<u16> {
            let item = item?;
            let list_end = outermost_list_end(item);
            let next = lines[last_line + 1..]
                .iter()
                .find(|l| l.kind != LineKind::Blank)
                .map(|l| l.start);
            matches!(next, Some(p) if p < list_end).then_some(GAP_ITEM)
        };
        for b in &self.blocks {
            let (first, last) = match b.kind {
                BlockKind::Code(ci) => {
                    let lines = self.code_blocks[ci].content_lines();
                    if lines.is_empty() {
                        continue;
                    }
                    (lines.start, lines.end - 1)
                }
                BlockKind::Heading(_)
                    if self.lines[b.last_line].kind == LineKind::SetextUnderline =>
                {
                    (
                        b.first_line,
                        b.last_line.saturating_sub(1).max(b.first_line),
                    )
                }
                _ => (b.first_line, b.last_line),
            };
            let list_gap = in_list_gap(b.item, last, &self.lines);
            let (own, gap_after, heading) = match b.kind {
                BlockKind::Heading(n) => (0, list_gap.unwrap_or(GAP_AFTER_HEADING), Some(n)),
                BlockKind::Code(_) => (CODE_PAD, list_gap.unwrap_or(GAP_BLOCK) + CODE_PAD, None),
                BlockKind::Rule => (RULE_PAD, GAP_BLOCK + RULE_PAD, None),
                _ => (0, list_gap.unwrap_or(GAP_BLOCK), None),
            };
            visuals.push(Visual {
                first,
                gap_after,
                own,
                heading,
            });
        }
        for (ii, item) in self.items.iter().enumerate() {
            let l = &self.lines[item.line];
            if l.content_start >= l.end && !self.blocks.iter().any(|b| b.first_line == item.line) {
                let gap_after = in_list_gap(Some(ii), item.line, &self.lines).unwrap_or(GAP_BLOCK);
                visuals.push(Visual {
                    first: item.line,
                    gap_after,
                    own: 0,
                    heading: None,
                });
            }
        }
        visuals.sort_by_key(|v| v.first);
        let mut prev_gap: Option<u16> = None;
        for v in &visuals {
            let mut a = prev_gap.unwrap_or(0);
            if let (Some(n), Some(_)) = (v.heading, prev_gap) {
                a = a.max(HEADING_ABOVE[(n.clamp(1, 6) - 1) as usize]);
            }
            above[v.first] = above[v.first].max(a + v.own);
            prev_gap = Some(v.gap_after);
        }
        for (i, a) in above.iter().enumerate() {
            if *a > 0 {
                spans.push(Span {
                    range: with_newline(&self.lines[i]),
                    style: Style::Above(*a),
                });
            }
        }

        spans.retain(|s| !s.range.is_empty());
        self.spans = spans;
    }
}

fn merge(ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for r in ranges {
        match out.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}

impl Builder<'_> {
    /// Images alone in their paragraphs ([`ImageBlock`]). A paragraph is a
    /// tight list item's text too. `hidden` is the hidden inline syntax,
    /// merged, which the alt text as shown leaves out.
    fn image_blocks(&self, hidden: &[Range<usize>]) -> Vec<ImageBlock> {
        let mut out = Vec::new();
        for b in self
            .blocks
            .iter()
            .filter(|b| b.kind == BlockKind::Paragraph)
        {
            let text = &self.src[b.range.clone()];
            let start = b.range.start + (text.len() - text.trim_start().len());
            let end = b.range.end - (text.len() - text.trim_end().len());
            let Some(image) = self
                .inlines
                .iter()
                .find(|i| i.kind == InlineKind::Image && i.range() == (start..end))
            else {
                continue;
            };
            let line = self.line_of(start);
            if self.last_line_of(&(start..end)) != line {
                continue;
            }
            let content = image.content();
            let mut alt = Vec::new();
            let mut p = content.start;
            for h in &hidden[hidden.partition_point(|h| h.end <= p)..] {
                if h.start >= content.end {
                    break;
                }
                if h.start > p {
                    alt.push(p..h.start);
                }
                p = p.max(h.end);
            }
            if p < content.end {
                alt.push(p..content.end);
            }
            out.push(ImageBlock {
                range: start..end,
                line,
                url: image.url.clone().unwrap_or_default(),
                alt,
            });
        }
        out.sort_by_key(|i| i.range.start);
        out
    }

    /// `mermaid` fenced code blocks ([`DiagramBlock`]): those whose info
    /// string's first word is `mermaid`, in any case.
    fn diagram_blocks(&self) -> Vec<DiagramBlock> {
        let mut out = Vec::new();
        for cb in &self.code_blocks {
            let Some(open) = cb.open_line else { continue };
            let lang = cb.info.split_whitespace().next().unwrap_or("");
            if !cb.fenced || !lang.eq_ignore_ascii_case("mermaid") {
                continue;
            }
            let mut source = String::new();
            let mut line_starts = Vec::new();
            for li in cb.content_lines() {
                let l = &self.lines[li];
                line_starts.push(l.content_start);
                source.push_str(&self.src[l.content_start..l.end.max(l.content_start)]);
                source.push('\n');
            }
            out.push(DiagramBlock {
                range: self.lines[open].content_start..self.lines[cb.last_line].end,
                first_line: cb.first_line,
                last_line: cb.last_line,
                source,
                line_starts,
            });
        }
        out.sort_by_key(|d| d.range.start);
        out
    }

    /// Each line's first shown byte ([`Line::visible_start`]): from its
    /// content start, past what the hidden spans cover.
    fn assign_visible_starts(&mut self) {
        let mut hidden: Vec<Range<usize>> = self
            .spans
            .iter()
            .filter(|s| s.style == Style::Hidden && !s.range.is_empty())
            .map(|s| s.range.clone())
            .collect();
        hidden.sort_by_key(|r| r.start);
        let hidden = merge(hidden);
        for l in &mut self.lines {
            let mut p = l.content_start.min(l.end);
            while let Some(r) = hidden
                .get(hidden.partition_point(|r| r.end <= p))
                .filter(|r| r.start <= p)
            {
                p = r.end;
            }
            l.visible_start = p.min(l.end);
        }
    }
}

/// First covered byte in `start..=end` (the newline position counts).
fn first_covered(covered: &[Range<usize>], start: usize, end: usize) -> Option<usize> {
    let idx = covered.partition_point(|r| r.end <= start);
    covered
        .get(idx)
        .filter(|r| r.start <= end)
        .map(|r| r.start.max(start))
}

fn skip_prefix(text: &str, in_quote: bool) -> usize {
    text.char_indices()
        .find(|&(_, c)| !(c == ' ' || c == '\t' || (in_quote && c == '>')))
        .map_or(text.len(), |(i, _)| i)
}

fn is_setext_underline(text: &str) -> bool {
    let t = text.trim_matches(|c: char| c == ' ' || c == '\t' || c == '>');
    !t.is_empty() && (t.chars().all(|c| c == '=') || t.chars().all(|c| c == '-'))
}

fn is_closing_fence(text: &str, fence_char: u8, min_len: usize) -> bool {
    let t = text.trim_start_matches([' ', '\t', '>']);
    let run = t.bytes().take_while(|&c| c == fence_char).count();
    run >= min_len && t[run..].trim().is_empty()
}

impl Doc {
    pub fn line_index(&self, pos: usize) -> usize {
        line_index(&self.lines, pos)
    }

    pub fn line_at(&self, pos: usize) -> &Line {
        &self.lines[self.line_index(pos)]
    }

    /// End of `line` including its newline, if any.
    pub fn line_end_incl(&self, line: usize) -> usize {
        (self.lines[line].end + 1).min(self.len)
    }

    /// The list item whose marker starts on `line`.
    pub fn item_on_line(&self, line: usize) -> Option<usize> {
        self.items.iter().rposition(|it| it.line == line)
    }

    /// The innermost list item containing `line`.
    pub fn innermost_item(&self, line: usize) -> Option<usize> {
        self.lines[line]
            .containers
            .iter()
            .rev()
            .find_map(|c| match c {
                Container::Item(i) => Some(*i),
                Container::Quote(_) => None,
            })
    }

    pub fn code_block_at_line(&self, line: usize) -> Option<usize> {
        self.code_blocks
            .iter()
            .position(|cb| cb.first_line <= line && line <= cb.last_line)
    }

    pub fn block_at_line(&self, line: usize) -> Option<usize> {
        self.blocks
            .iter()
            .rposition(|b| b.first_line <= line && line <= b.last_line)
    }

    /// Inline elements whose content contains `pos` (inclusive of both
    /// content edges), innermost last.
    pub fn inlines_at(&self, pos: usize) -> Vec<usize> {
        let mut found: Vec<usize> = (0..self.inlines.len())
            .filter(|&i| {
                let c = self.inlines[i].content();
                c.start <= pos && pos <= c.end
            })
            .collect();
        found.sort_by_key(|&i| std::cmp::Reverse(self.inlines[i].range().len()));
        found
    }

    /// The hidden inline run containing `pos` (`start <= pos <= end`).
    pub fn hidden_run_at(&self, pos: usize) -> Option<Range<usize>> {
        let idx = self.hidden.partition_point(|r| r.end < pos);
        self.hidden
            .get(idx)
            .filter(|r| r.start <= pos && pos <= r.end)
            .cloned()
    }

    pub fn is_hidden_byte(&self, pos: usize) -> bool {
        let idx = self.hidden.partition_point(|r| r.end <= pos);
        self.hidden.get(idx).is_some_and(|r| r.start <= pos)
    }

    pub fn first_visible_line(&self) -> Option<usize> {
        self.lines.iter().position(|l| l.kind != LineKind::Blank)
    }

    /// The image block on `line`, if any.
    pub fn image_block_on_line(&self, line: usize) -> Option<&ImageBlock> {
        let i = self.images.partition_point(|im| im.line < line);
        self.images.get(i).filter(|im| im.line == line)
    }

    /// The diagram block that `line` is one of, if any.
    pub fn diagram_on_line(&self, line: usize) -> Option<&DiagramBlock> {
        let i = self.diagrams.partition_point(|d| d.last_line < line);
        self.diagrams.get(i).filter(|d| d.first_line <= line)
    }

    /// The source of the object, an image or a diagram, that `line` is on:
    /// what selecting the object selects.
    pub fn object_on_line(&self, line: usize) -> Option<Range<usize>> {
        self.image_block_on_line(line)
            .map(|im| im.range.clone())
            .or_else(|| self.diagram_on_line(line).map(|d| d.range.clone()))
    }

    /// The sources of the objects, images and diagrams, in source order.
    pub fn objects(&self) -> Vec<Range<usize>> {
        let mut out: Vec<Range<usize>> = self
            .images
            .iter()
            .map(|im| im.range.clone())
            .chain(self.diagrams.iter().map(|d| d.range.clone()))
            .collect();
        out.sort_by_key(|r| r.start);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_start_where_their_text_shows() {
        fn starts(src: &str) -> Vec<&str> {
            parse(src)
                .lines
                .iter()
                .map(|l| &src[l.visible_start..l.end])
                .collect()
        }
        // Bold text starting an item, a quote or a heading: past its `**`.
        assert_eq!(
            starts("- **File** menu\n- plain\n"),
            ["File** menu", "plain", ""]
        );
        assert_eq!(starts("> **Note:** read\n"), ["Note:** read", ""]);
        assert_eq!(starts("# *Big* title\n"), ["Big* title", ""]);
        assert_eq!(
            starts("1. [link](http://x) after\n"),
            ["link](http://x) after", ""]
        );
        // Nothing shown: blank lines, fences and rules show from their end.
        assert_eq!(
            starts("a\n\n```\ncode\n```\n\n---\n"),
            ["a", "", "", "code", "", "", "", ""]
        );
    }

    fn hidden_text(src: &str, doc: &Doc) -> String {
        // The text as rendered: hidden spans removed.
        let mut hide = vec![false; src.len()];
        for s in &doc.spans {
            if s.style == Style::Hidden {
                for h in &mut hide[s.range.clone()] {
                    *h = true;
                }
            }
        }
        src.char_indices()
            .filter(|(i, _)| !hide[*i])
            .map(|(_, c)| c)
            .collect()
    }

    #[test]
    fn heading_marker_is_prefix() {
        let src = "# Title\n";
        let doc = parse(src);
        assert_eq!(doc.lines[0].kind, LineKind::Heading(1));
        assert_eq!(doc.lines[0].content_start, 2);
        assert_eq!(hidden_text(src, &doc), "Title\n");
    }

    #[test]
    fn empty_heading_stays_visible() {
        let src = "# \n";
        let doc = parse(src);
        assert_eq!(doc.lines[0].kind, LineKind::Heading(1));
        assert_eq!(doc.lines[0].content_start, 2);
    }

    #[test]
    fn emphasis_markers_hidden() {
        let src = "Some **bold** and *it* and `code`.\n";
        let doc = parse(src);
        assert_eq!(hidden_text(src, &doc), "Some bold and it and code.\n");
        let strong = doc
            .inlines
            .iter()
            .find(|i| i.kind == InlineKind::Strong)
            .unwrap();
        assert_eq!(strong.open, 5..7);
        assert_eq!(strong.close, 11..13);
    }

    #[test]
    fn links_hide_destination() {
        let src = "See [the docs](https://x.y \"t\") now.\n";
        let doc = parse(src);
        assert_eq!(hidden_text(src, &doc), "See the docs now.\n");
        let link = &doc.inlines[0];
        assert_eq!(link.url.as_deref(), Some("https://x.y"));
    }

    #[test]
    fn escapes_hide_backslash() {
        let src = "a \\*b\\* c\n";
        let doc = parse(src);
        assert_eq!(hidden_text(src, &doc), "a *b* c\n");
    }

    #[test]
    fn blank_lines_hidden() {
        let src = "one\n\ntwo\n";
        let doc = parse(src);
        assert_eq!(doc.lines[1].kind, LineKind::Blank);
        assert_eq!(hidden_text(src, &doc), "one\ntwo\n");
    }

    #[test]
    fn list_items() {
        let src = "- a\n- [x] b\n  cont\n  1. n\n";
        let doc = parse(src);
        assert_eq!(doc.items.len(), 3);
        assert_eq!(doc.items[0].marker, 0..2);
        assert_eq!(doc.items[1].marker, 4..6);
        assert_eq!(doc.items[1].task, Some((true, 6..9)));
        assert_eq!(doc.lines[1].content_start, 10);
        assert_eq!(doc.lines[2].content_start, 14);
        assert_eq!(doc.items[2].number, Some(1));
        assert_eq!(doc.items[2].depth, 2);
        assert_eq!(doc.lines[3].containers.len(), 2);
        assert_eq!(hidden_text(src, &doc), "a\nb\ncont\nn\n");
    }

    #[test]
    fn empty_list_item_visible() {
        let src = "- a\n- \n";
        let doc = parse(src);
        assert_eq!(doc.lines[1].kind, LineKind::Paragraph);
        assert_eq!(doc.lines[1].content_start, doc.lines[1].end);
        assert_eq!(doc.items[1].marker, 4..6);
    }

    #[test]
    fn quotes() {
        let src = "> quote\nlazy\n> > nested\n";
        let doc = parse(src);
        assert_eq!(doc.lines[0].content_start, 2);
        assert_eq!(doc.lines[1].content_start, 8);
        assert_eq!(doc.lines[2].content_start, 17);
        assert_eq!(doc.lines[2].containers.len(), 2);
        assert_eq!(hidden_text(src, &doc), "quote\nlazy\nnested\n");
    }

    #[test]
    fn fenced_code() {
        let src = "```rust\nlet x = 1;\n\n```\nafter\n";
        let doc = parse(src);
        let cb = &doc.code_blocks[0];
        assert_eq!(cb.info, "rust");
        assert_eq!(cb.open_line, Some(0));
        assert_eq!(cb.close_line, Some(3));
        assert_eq!(cb.content_lines(), 1..3);
        assert_eq!(doc.lines[2].kind, LineKind::CodeContent);
        assert_eq!(hidden_text(src, &doc), "let x = 1;\n\nafter\n");
    }

    #[test]
    fn unclosed_fence_runs_to_end() {
        let src = "```\ncode\n";
        let doc = parse(src);
        let cb = &doc.code_blocks[0];
        assert_eq!(cb.close_line, None);
        assert_eq!(cb.content_lines(), 1..2);
    }

    #[test]
    fn setext_and_rule() {
        let src = "Title\n===\n\n---\n";
        let doc = parse(src);
        assert_eq!(doc.lines[0].kind, LineKind::Heading(1));
        assert_eq!(doc.lines[1].kind, LineKind::SetextUnderline);
        assert_eq!(doc.lines[3].kind, LineKind::Rule);
        assert_eq!(hidden_text(src, &doc), "Title\n\n");
    }

    #[test]
    fn heading_closing_hashes_hidden() {
        let src = "## Title ##\n";
        let doc = parse(src);
        assert_eq!(hidden_text(src, &doc), "Title\n");
    }

    #[test]
    fn link_reference_definitions_stay_visible() {
        let src = "[x]: https://example.com\n";
        let doc = parse(src);
        assert_eq!(doc.lines[0].kind, LineKind::Raw);
        assert_eq!(hidden_text(src, &doc), src);
    }

    #[test]
    fn tables_render_as_source() {
        let src = "| a | b |\n|---|---|\n| 1 | 2 |\n";
        let doc = parse(src);
        assert!(doc.lines[..3].iter().all(|l| l.kind == LineKind::Table));
        assert_eq!(hidden_text(src, &doc), src);
    }

    #[test]
    fn table_rows_and_cells() {
        let src = "| A | B |  C |\n|:--|--:|:-:|\n| x |  | `a\\|b` |\na | b\n";
        let doc = parse(src);
        assert_eq!(doc.tables.len(), 1);
        let t = &doc.tables[0];
        assert_eq!(t.delimiter_line, 1);
        assert_eq!(t.aligns, [Align::Left, Align::Right, Align::Center]);
        let text = |r: &Range<usize>| &src[r.clone()];
        let rows: Vec<_> = t
            .rows
            .iter()
            .map(|r| {
                (
                    r.line,
                    r.cells
                        .iter()
                        .map(|c| (text(&c.lead), text(&c.content)))
                        .collect::<Vec<_>>(),
                    text(&r.trail),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                (0, vec![("| ", "A"), (" | ", "B"), (" |  ", "C")], " |"),
                (2, vec![("| ", "x"), (" | ", ""), (" | ", "`a\\|b`")], " |"),
                // The missing third cell has no source.
                (3, vec![("", "a"), (" | ", "b")], ""),
            ]
        );
        assert_eq!(t.rows[1].cells[1].content, 35..35);
    }

    #[test]
    fn table_in_quote_rows_start_after_the_marker() {
        let src = "> | q | r |\n> |---|---|\n> | 1 | 2 |\n";
        let doc = parse(src);
        let t = &doc.tables[0];
        assert_eq!(t.rows.iter().map(|r| r.line).collect::<Vec<_>>(), [0, 2]);
        assert_eq!(&src[t.rows[1].cells[0].lead.clone()], "| ");
    }

    fn above(doc: &Doc, line: usize) -> u16 {
        doc.spans
            .iter()
            .filter(|s| s.range.start == doc.lines[line].start)
            .find_map(|s| match s.style {
                Style::Above(a) => Some(a),
                _ => None,
            })
            .unwrap_or(0)
    }

    #[test]
    fn spacing_goes_above_lines() {
        let src = "# H\n\npara\n\n- a\n- b\n\nend\n\n## Two\n";
        let doc = parse(src);
        assert_eq!(above(&doc, 0), 0);
        assert_eq!(above(&doc, 2), GAP_AFTER_HEADING);
        assert_eq!(above(&doc, 4), GAP_BLOCK);
        assert_eq!(above(&doc, 5), GAP_ITEM);
        assert_eq!(above(&doc, 7), GAP_BLOCK);
        assert_eq!(above(&doc, 9), HEADING_ABOVE[1]);
        assert!(
            !doc.spans
                .iter()
                .any(|s| matches!(s.style, Style::Above(_)) && s.range.start == doc.lines[1].start)
        );
    }

    #[test]
    fn nested_list_spacing_stays_small() {
        let src = "- a\n  - b\n- c\n\np\n";
        let doc = parse(src);
        assert_eq!(above(&doc, 1), GAP_ITEM);
        assert_eq!(above(&doc, 2), GAP_ITEM);
        assert_eq!(above(&doc, 4), GAP_BLOCK);
    }

    #[test]
    fn code_blocks_get_padding() {
        let src = "p\n\n```\nx\n```\n\nq\n";
        let doc = parse(src);
        assert_eq!(above(&doc, 3), GAP_BLOCK + CODE_PAD);
        assert_eq!(above(&doc, 6), GAP_BLOCK + CODE_PAD);
    }

    #[test]
    fn hard_break_backslash_hidden() {
        let src = "a\\\nb\n";
        let doc = parse(src);
        assert_eq!(hidden_text(src, &doc), "a\nb\n");
    }

    #[test]
    fn front_matter() {
        let src = "---\ntitle: x\n---\n\n# H\n";
        let doc = parse(src);
        assert_eq!(doc.lines[0].kind, LineKind::FrontMatter);
        assert_eq!(doc.lines[2].kind, LineKind::FrontMatter);
        assert_eq!(doc.lines[4].kind, LineKind::Heading(1));
    }

    #[test]
    fn code_in_list_hides_indent() {
        let src = "- a\n\n  ```\n  x\n  ```\n";
        let doc = parse(src);
        assert_eq!(doc.lines[3].kind, LineKind::CodeContent);
        assert_eq!(doc.lines[3].content_start, doc.lines[3].start + 2);
    }

    #[test]
    fn single_dash_after_item_is_empty_nested_item() {
        let src = "- one\n  - \n";
        let doc = parse(src);
        assert_eq!(doc.lines[0].kind, LineKind::Paragraph);
        assert_eq!(doc.lines[1].kind, LineKind::Paragraph);
        let it = doc.item_on_line(1).unwrap();
        assert_eq!(doc.items[it].depth, 2);
        assert_eq!(doc.items[it].marker, 8..10);
        assert_eq!(doc.lines[1].content_start, 10);
        // A real setext heading is untouched.
        let doc = parse("Title\n---\n");
        assert_eq!(doc.lines[0].kind, LineKind::Heading(2));
    }

    #[test]
    fn bare_markers_show_until_their_space() {
        for src in ["-\n", "*\n", "1.\n", "#\n", "##\n"] {
            let doc = parse(src);
            assert_eq!(hidden_text(src, &doc), src, "{src:?}");
        }
        assert_eq!(hidden_text("- a\n-\n", &parse("- a\n-\n")), "a\n-\n");
        let doc = parse("-\n");
        assert!(doc.items[0].bare);
        assert!(doc.lines[0].containers.is_empty());
        let doc = parse("- \n");
        assert!(!doc.items[0].bare);
        let doc = parse("# \n");
        assert_eq!(doc.lines[0].kind, LineKind::Heading(1));
        for src in ["- Open…\n", "1. café\n"] {
            assert!(!parse(src).items[0].bare, "{src:?}");
        }
    }

    #[test]
    fn soft_breaks_are_newlines_inside_paragraphs() {
        assert_eq!(parse("a\nb\n").soft_breaks, vec![1]);
        assert_eq!(parse("- a\n  b\n").soft_breaks, vec![3]);
        assert_eq!(parse("> a\n> b\n").soft_breaks, vec![3]);
        assert!(parse("a\\\nb\n").soft_breaks.is_empty());
        assert!(parse("a  \nb\n").soft_breaks.is_empty());
        assert!(parse("# a\nb\n").soft_breaks.is_empty());
        assert!(parse("```\na\nb\n```\n").soft_breaks.is_empty());
    }

    #[test]
    fn bare_urls_are_links() {
        let src = "See https://example.com/a_(b). And `http://x.y` or [t](https://z.w).\n";
        let doc = parse(src);
        let urls: Vec<_> = doc
            .inlines
            .iter()
            .filter(|i| i.kind == InlineKind::Link)
            .filter_map(|i| i.url.clone())
            .collect();
        assert_eq!(urls, vec!["https://example.com/a_(b)", "https://z.w"]);
        assert_eq!(
            hidden_text(src, &doc),
            "See https://example.com/a_(b). And http://x.y or t.\n"
        );
    }

    #[test]
    fn pending_hard_break_hidden() {
        let src = "a\\\n";
        assert_eq!(hidden_text(src, &parse(src)), "a\n");
    }

    #[test]
    fn images_alone_in_their_paragraphs_are_blocks() {
        /// Each image block's source, destination and alt text as shown.
        fn blocks(src: &str) -> Vec<[String; 3]> {
            parse(src)
                .images
                .iter()
                .map(|im| {
                    let alt = im.alt.iter().map(|r| &src[r.clone()]).collect();
                    [src[im.range.clone()].to_string(), im.url.clone(), alt]
                })
                .collect()
        }
        let one = |s: &str, url: &str, alt: &str| vec![[s, url, alt].map(String::from)];
        assert_eq!(
            blocks("Intro.\n\n![A *shot*](img/a.png \"t\")  \n\nAfter.\n"),
            one("![A *shot*](img/a.png \"t\")", "img/a.png", "A shot")
        );
        // In a list item, tight or loose, and in a quote.
        assert_eq!(
            blocks("- ![a](a.png)\n- text\n"),
            one("![a](a.png)", "a.png", "a")
        );
        assert_eq!(
            blocks("- x\n\n- ![a](a.png)\n"),
            one("![a](a.png)", "a.png", "a")
        );
        assert_eq!(
            blocks("> ![](/abs/b.png)\n"),
            one("![](/abs/b.png)", "/abs/b.png", "")
        );
        // A reference image takes its definition's destination.
        assert_eq!(
            blocks("![a][r]\n\n[r]: r.png\n"),
            one("![a][r]", "r.png", "a")
        );
        // Running text, a link around it, two images, a heading: not alone.
        for src in [
            "See ![a](a.png) here.\n",
            "[![a](a.png)](https://x.y)\n",
            "![a](a.png) ![b](b.png)\n",
            "![a](a.png)\nmore\n",
            "# ![a](a.png)\n",
            "| ![a](a.png) |\n|---|\n",
        ] {
            assert!(parse(src).images.is_empty(), "{src:?}");
        }
        let doc = parse("a\n\n![a](a.png)\n");
        assert_eq!(doc.image_block_on_line(2).map(|im| im.line), Some(2));
        assert!(doc.image_block_on_line(0).is_none());
    }

    #[test]
    fn mermaid_blocks_are_diagrams() {
        let src = "Intro.\n\n```mermaid\nflowchart TD\n  A --> B\n```\n\n- ```Mermaid\n  pie\n  ```\n\n```js\nx\n```\n";
        let doc = parse(src);
        let d: Vec<(&str, &str, usize, usize)> = doc
            .diagrams
            .iter()
            .map(|d| {
                (
                    &src[d.range.clone()],
                    d.source.as_str(),
                    d.first_line,
                    d.last_line,
                )
            })
            .collect();
        assert_eq!(
            d,
            [
                (
                    "```mermaid\nflowchart TD\n  A --> B\n```",
                    "flowchart TD\n  A --> B\n",
                    2,
                    5
                ),
                ("```Mermaid\n  pie\n  ```", "pie\n", 7, 9),
            ]
        );
        assert_eq!(&src[doc.diagrams[0].line_starts[1]..][..3], "  A");
        assert_eq!(doc.object_on_line(4), Some(doc.diagrams[0].range.clone()));
        assert_eq!(doc.object_on_line(6), None);
        assert_eq!(doc.objects().len(), 2);
    }

    #[test]
    fn utf8_content() {
        let src = "# Ångström *é*\n";
        let doc = parse(src);
        assert_eq!(hidden_text(src, &doc), "Ångström é\n");
    }
}
