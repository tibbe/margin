//! Printing the document as rendered, through the system print dialog, as
//! Omawrite does. Blocks are laid out with Pango and paginated at line
//! boundaries.

use super::buffer::{DocBuffer, ITEM_STEP, QUOTE_STEP};
use gtk::{pango, prelude::*};
use margin_core::md::{BlockKind, Container, Doc, LineKind, Style};
use std::cell::RefCell;
use std::rc::Rc;

/// Body text size on paper, in points.
const BODY_PT: f64 = 11.0;
/// Screen pixels (at a 12pt, 16px body) to points at `BODY_PT`.
const PX: f64 = 0.75 * BODY_PT / 12.0;
const CODE_PAD: f64 = 6.0;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Text,
    Code,
    Rule,
}

struct Block {
    kind: Kind,
    text: String,
    attrs: pango::AttrList,
    font: pango::FontDescription,
    indent: f64,
    above: f64,
    marker: Option<String>,
    /// Drawn as outlines: cairo embeds a variable font's default instance
    /// when a size needs an optical-size variation, so large bold headings
    /// would print regular.
    outline: bool,
    /// X positions of quote bars.
    bars: Vec<f64>,
    layout: RefCell<Option<pango::Layout>>,
}

struct Placement {
    block: usize,
    lines: std::ops::Range<i32>,
    /// Page y of the top of the first placed line.
    y: f64,
}

/// Visible characters of a source range, with a map from source byte to
/// output byte for attribute placement.
fn visible_text(
    src: &str,
    doc: &Doc,
    lines: std::ops::RangeInclusive<usize>,
) -> (String, Vec<(usize, usize)>) {
    let mut out = String::new();
    let mut map = Vec::new();
    let last = *lines.end();
    for li in lines {
        let l = &doc.lines[li];
        if matches!(
            l.kind,
            LineKind::Blank | LineKind::SetextUnderline | LineKind::Fence
        ) {
            continue;
        }
        for (off, c) in src[l.content_start..l.end].char_indices() {
            let p = l.content_start + off;
            if !doc.is_hidden_byte(p) {
                map.push((p, out.len()));
                out.push(c);
            }
        }
        if li < last {
            map.push((l.end, out.len()));
            out.push(if doc.soft_breaks.binary_search(&l.end).is_ok() {
                ' '
            } else {
                '\n'
            });
        }
    }
    map.push((usize::MAX, out.len()));
    (out, map)
}

fn out_pos(map: &[(usize, usize)], src_pos: usize) -> u32 {
    let i = map.partition_point(|(s, _)| *s < src_pos);
    map[i.min(map.len() - 1)].1 as u32
}

fn indent_of(containers: &[Container]) -> f64 {
    containers
        .iter()
        .map(|c| match c {
            Container::Quote(_) => QUOTE_STEP,
            Container::Item(_) => ITEM_STEP,
        })
        .sum::<f64>()
        * PX
}

fn blocks(buffer: &DocBuffer) -> Vec<Block> {
    let look = buffer.look();
    let st = buffer.state();
    let (src, doc) = (&st.text, &st.doc);
    let body = {
        let mut d = pango::FontDescription::from_string(&look.fonts.body);
        d.set_size((BODY_PT * pango::SCALE as f64) as i32);
        d
    };
    let mono = {
        let mut d = pango::FontDescription::from_string(&look.fonts.mono);
        d.set_size((BODY_PT * 0.88 * pango::SCALE as f64) as i32);
        d
    };
    let above_of = |line: usize| -> f64 {
        let start = doc.lines[line].start;
        doc.spans
            .iter()
            .filter(|s| s.range.start == start)
            .find_map(|s| match s.style {
                Style::Above(a) => Some(a as f64 * PX),
                _ => None,
            })
            .unwrap_or(0.0)
    };
    let mut out = Vec::new();
    for b in &doc.blocks {
        let first = &doc.lines[b.first_line];
        let containers = &first.containers;
        let bars: Vec<f64> = containers
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c, Container::Quote(_)))
            .map(|(k, _)| indent_of(&containers[..k]) + 2.0)
            .collect();
        let marker = b.item.and_then(|i| {
            let it = &doc.items[i];
            (it.line == b.first_line && !it.bare).then(|| match (&it.task, it.number) {
                (Some((true, _)), _) => "☑".to_string(),
                (Some((false, _)), _) => "☐".to_string(),
                (None, Some(n)) => format!("{n}."),
                (None, None) => ["•", "◦", "▪"][(it.depth.max(1) - 1) % 3].to_string(),
            })
        });
        let indent = indent_of(containers);
        let above = above_of(b.first_line);
        let attrs = pango::AttrList::new();
        let (kind, text, font) = match b.kind {
            BlockKind::Rule => (Kind::Rule, String::new(), body.clone()),
            BlockKind::FrontMatter => continue,
            BlockKind::Html if src[b.range.clone()].trim_start().starts_with("<!--") => continue,
            BlockKind::Code(ci) => {
                let lines = doc.code_blocks[ci].content_lines();
                let text = lines
                    .map(|l| &src[doc.lines[l].content_start..doc.lines[l].end])
                    .collect::<Vec<_>>()
                    .join("\n");
                (Kind::Code, text, mono.clone())
            }
            BlockKind::Table | BlockKind::Html => {
                let text = (b.first_line..=b.last_line)
                    .map(|l| &src[doc.lines[l].content_start..doc.lines[l].end])
                    .collect::<Vec<_>>()
                    .join("\n");
                (Kind::Text, text, mono.clone())
            }
            BlockKind::Paragraph | BlockKind::Heading(_) => {
                let (text, map) = visible_text(src, doc, b.first_line..=b.last_line);
                for sp in &doc.spans {
                    if sp.range.end <= b.range.start || sp.range.start >= b.range.end {
                        continue;
                    }
                    let (a, e) = (out_pos(&map, sp.range.start), out_pos(&map, sp.range.end));
                    if a >= e {
                        continue;
                    }
                    let add = |mut attr: pango::Attribute| {
                        attr.set_start_index(a);
                        attr.set_end_index(e);
                        attrs.insert(attr);
                    };
                    match sp.style {
                        Style::Strong | Style::TableHeader => {
                            add(pango::AttrInt::new_weight(pango::Weight::Bold).into())
                        }
                        Style::Emphasis | Style::Image => {
                            add(pango::AttrInt::new_style(pango::Style::Italic).into())
                        }
                        Style::Strike => add(pango::AttrInt::new_strikethrough(true).into()),
                        Style::TaskDone => {
                            add(pango::AttrInt::new_strikethrough(true).into());
                            add(pango::AttrColor::new_foreground(0x7777, 0x7777, 0x7777).into());
                        }
                        Style::Code => {
                            add(pango::AttrString::new_family(&look.fonts.mono).into());
                            add(pango::AttrFloat::new_scale(0.9).into());
                            add(pango::AttrColor::new_background(0xeeee, 0xeeee, 0xeeee).into());
                        }
                        Style::Link => {
                            add(pango::AttrColor::new_foreground(0x1a1a, 0x5f5f, 0xb4b4).into());
                            add(pango::AttrInt::new_underline(pango::Underline::Single).into());
                        }
                        Style::Quote => {
                            add(pango::AttrColor::new_foreground(0x5555, 0x5555, 0x5555).into())
                        }
                        _ => {}
                    }
                }
                if let BlockKind::Heading(n) = b.kind {
                    let scale = [1.8, 1.42, 1.2, 1.07, 1.0, 0.94][(n.clamp(1, 6) - 1) as usize];
                    let mut s: pango::Attribute = pango::AttrFloat::new_scale(scale).into();
                    s.set_start_index(0);
                    s.set_end_index(text.len() as u32);
                    attrs.insert(s);
                    let mut w: pango::Attribute =
                        pango::AttrInt::new_weight(pango::Weight::Bold).into();
                    w.set_start_index(0);
                    w.set_end_index(text.len() as u32);
                    attrs.insert(w);
                }
                (Kind::Text, text, body.clone())
            }
        };
        out.push(Block {
            kind,
            text,
            attrs,
            font,
            indent,
            above,
            marker,
            outline: matches!(b.kind, BlockKind::Heading(_)),
            bars,
            layout: RefCell::new(None),
        });
    }
    out
}

fn layout_for(ctx: &gtk::PrintContext, b: &Block, width: f64) -> pango::Layout {
    let layout = ctx.create_pango_layout();
    layout.set_font_description(Some(&b.font));
    let inner = if b.kind == Kind::Code {
        width - 2.0 * CODE_PAD
    } else {
        width
    };
    layout.set_width((inner.max(20.0) * pango::SCALE as f64) as i32);
    layout.set_wrap(pango::WrapMode::WordChar);
    layout.set_text(&b.text);
    layout.set_attributes(Some(&b.attrs));
    layout
}

/// Top and bottom (in points, from the layout top) of each line.
fn line_extents(layout: &pango::Layout) -> Vec<(f64, f64, f64)> {
    let mut out = Vec::new();
    let mut iter = layout.iter();
    loop {
        let (_, logical) = iter.line_extents();
        let top = logical.y() as f64 / pango::SCALE as f64;
        let bottom = (logical.y() + logical.height()) as f64 / pango::SCALE as f64;
        let baseline = iter.baseline() as f64 / pango::SCALE as f64;
        out.push((top, bottom, baseline));
        if !iter.next_line() {
            break;
        }
    }
    out
}

fn paginate(ctx: &gtk::PrintContext, blocks: &[Block]) -> Vec<Vec<Placement>> {
    let (width, height) = (ctx.width(), ctx.height());
    let mut pages: Vec<Vec<Placement>> = vec![Vec::new()];
    let mut y = 0.0;
    for (bi, b) in blocks.iter().enumerate() {
        if y > 0.0 {
            y += b.above;
        }
        if b.kind == Kind::Rule {
            if y + 12.0 > height {
                pages.push(Vec::new());
                y = 0.0;
            }
            pages.last_mut().unwrap().push(Placement {
                block: bi,
                lines: 0..0,
                y,
            });
            y += 12.0;
            continue;
        }
        let layout = layout_for(ctx, b, width - b.indent);
        let lines = line_extents(&layout);
        b.layout.replace(Some(layout));
        let pad = if b.kind == Kind::Code { CODE_PAD } else { 0.0 };
        let total = lines.last().map_or(0.0, |l| l.1) + 2.0 * pad;
        if y > 0.0 && y + total > height && total <= height {
            pages.push(Vec::new());
            y = 0.0;
        }
        // Place lines, breaking to a new page where they stop fitting.
        let mut start = 0usize;
        while start < lines.len() {
            let top = lines[start].0;
            let mut end = start;
            while end < lines.len() && y + pad + (lines[end].1 - top) <= height {
                end += 1;
            }
            if end == start {
                if y > 0.0 {
                    pages.push(Vec::new());
                    y = 0.0;
                    continue;
                }
                end = start + 1;
            }
            pages.last_mut().unwrap().push(Placement {
                block: bi,
                lines: start as i32..end as i32,
                y: y + pad,
            });
            y += pad + (lines[end - 1].1 - top) + if end == lines.len() { pad } else { 0.0 };
            start = end;
            if start < lines.len() {
                pages.push(Vec::new());
                y = 0.0;
            }
        }
    }
    pages
}

fn draw(ctx: &gtk::PrintContext, blocks: &[Block], page: &[Placement]) {
    let cr = ctx.cairo_context();
    let width = ctx.width();
    for p in page {
        let b = &blocks[p.block];
        if b.kind == Kind::Rule {
            cr.set_source_rgb(0.75, 0.75, 0.75);
            cr.rectangle(b.indent, p.y + 5.5, width - b.indent, 1.0);
            let _ = cr.fill();
            continue;
        }
        let Some(layout) = b.layout.borrow().clone() else {
            continue;
        };
        let lines = line_extents(&layout);
        let (first, last) = (p.lines.start as usize, p.lines.end as usize - 1);
        let top = lines[first].0;
        let bottom = p.y + lines[last].1 - top;
        if b.kind == Kind::Code {
            cr.set_source_rgb(0.95, 0.95, 0.95);
            cr.rectangle(
                b.indent,
                p.y - CODE_PAD,
                width - b.indent,
                bottom - p.y + 2.0 * CODE_PAD,
            );
            let _ = cr.fill();
        }
        for &x in &b.bars {
            cr.set_source_rgb(0.8, 0.8, 0.8);
            cr.rectangle(x, p.y, 2.0, bottom - p.y);
            let _ = cr.fill();
        }
        cr.set_source_rgb(0.0, 0.0, 0.0);
        let x = b.indent + if b.kind == Kind::Code { CODE_PAD } else { 0.0 };
        for (i, line) in layout
            .lines_readonly()
            .iter()
            .enumerate()
            .take(last + 1)
            .skip(first)
        {
            cr.move_to(x, p.y + lines[i].2 - top);
            if b.outline {
                pangocairo::functions::layout_line_path(&cr, line);
                let _ = cr.fill();
            } else {
                pangocairo::functions::show_layout_line(&cr, line);
            }
        }
        if first == 0
            && let Some(m) = &b.marker
        {
            let ml = ctx.create_pango_layout();
            ml.set_font_description(Some(&b.font));
            ml.set_text(m);
            let (w, _) = ml.pixel_size();
            cr.move_to(b.indent - w as f64 - 6.0, p.y + lines[0].2 - top);
            if let Some(line) = ml.line_readonly(0) {
                pangocairo::functions::show_layout_line(&cr, &line);
            }
        }
    }
}

fn operation(buffer: &DocBuffer, title: &str) -> gtk::PrintOperation {
    let blocks = Rc::new(blocks(buffer));
    let pages: Rc<RefCell<Vec<Vec<Placement>>>> = Rc::default();
    let op = gtk::PrintOperation::new();
    op.set_job_name(title);
    op.set_unit(gtk::Unit::Points);
    op.set_embed_page_setup(true);
    let (b, pg) = (blocks.clone(), pages.clone());
    op.connect_begin_print(move |op, ctx| {
        let p = paginate(ctx, &b);
        op.set_n_pages(p.len().max(1) as i32);
        pg.replace(p);
    });
    let (b, pg) = (blocks, pages);
    op.connect_draw_page(move |_, ctx, n| {
        if let Some(page) = pg.borrow().get(n as usize) {
            draw(ctx, &b, page);
        }
    });
    op
}

/// Shows the print dialog and prints the document as rendered.
pub fn print(
    window: &impl IsA<gtk::Window>,
    buffer: &DocBuffer,
    title: &str,
) -> Result<(), String> {
    operation(buffer, title)
        .run(gtk::PrintOperationAction::PrintDialog, Some(window))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Prints to a PDF file without a dialog (for tests).
pub fn export_pdf(buffer: &DocBuffer, path: &str) -> Result<(), String> {
    let op = operation(buffer, "export");
    op.set_export_filename(path);
    op.run(gtk::PrintOperationAction::Export, None::<&gtk::Window>)
        .map(|_| ())
        .map_err(|e| e.to_string())
}
