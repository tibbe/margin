//! The document view: a text view that draws what the hidden Markdown
//! syntax stands for (bullets, checkboxes, quote bars, rules, code boxes),
//! centers the text in a readable column with room for the comment gutter,
//! and routes deletions and the clipboard through the editing semantics.

use super::buffer::{DocBuffer, ITEM_STEP, QUOTE_STEP};
use super::theme::{rgba, rgba_alpha};
use crate::md::{edit, Container, InlineKind, LineKind};
use gtk::{gdk, gio, glib, graphene, prelude::*, subclass::prelude::*};
use std::cell::{Cell, RefCell};

/// Comment card width at the default text size.
const CARD_WIDTH: i32 = 300;
const GUTTER_GAP: i32 = 40;
const SIDE_PAD: i32 = 28;

/// Horizontal layout of the page: the text column and the comment gutter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Geometry {
    pub width: i32,
    pub left: i32,
    pub doc_width: i32,
    pub gutter_x: i32,
    pub card_width: i32,
}

/// Layout for a view `width` wide, with `scale` the text's scale and
/// `ui_scale` the interface's (see `Look`).
pub fn geometry(width: i32, scale: f64, ui_scale: f64) -> Geometry {
    let card = (CARD_WIDTH as f64 * ui_scale).round() as i32;
    // About 100 characters of the body font. Screen studies find no speed
    // or comprehension cost up to about 100 characters per line.
    let max_doc = (760.0 * scale) as i32;
    let room = width - card - GUTTER_GAP - 2 * SIDE_PAD;
    let doc_width = room.clamp(300, max_doc).min((width - 2 * SIDE_PAD).max(120));
    let mut left = (width - doc_width) / 2;
    let needed = doc_width + GUTTER_GAP + card + SIDE_PAD;
    if left + needed > width {
        left = (width - needed).max(SIDE_PAD);
    }
    Geometry {
        width,
        left,
        doc_width,
        gutter_x: left + doc_width + GUTTER_GAP,
        card_width: card,
    }
}

fn indent_px(containers: &[Container], scale: f64) -> f32 {
    containers
        .iter()
        .map(|c| match c {
            Container::Quote(_) => QUOTE_STEP,
            Container::Item(_) => ITEM_STEP,
        })
        .sum::<f64>() as f32
        * scale as f32
}

fn bullet(depth: usize) -> &'static str {
    ["•", "◦", "▪"][(depth.max(1) - 1) % 3]
}

type Callback = Box<dyn Fn()>;
type LinkHandler = Box<dyn Fn(&str)>;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct DocView {
        pub geometry: Cell<Geometry>,
        /// Hit areas of drawn checkboxes: (rect, list item index).
        pub checkboxes: RefCell<Vec<(graphene::Rect, usize)>>,
        pub on_layout: RefCell<Vec<Callback>>,
        pub on_link: RefCell<Option<LinkHandler>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for DocView {
        const NAME: &'static str = "MarginDocView";
        type Type = super::DocView;
        type ParentType = gtk::TextView;
    }

    impl ObjectImpl for DocView {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.set_wrap_mode(gtk::WrapMode::WordChar);
            obj.add_css_class("margin-doc");
            obj.set_top_margin(32);
            obj.set_bottom_margin(240);
            obj.set_accepts_tab(true);
            obj.set_has_tooltip(true);
            obj.setup_controllers();
        }
    }

    impl WidgetImpl for DocView {
        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            let obj = self.obj();
            let (scale, ui) = obj
                .doc_buffer_opt()
                .map_or((1.0, 1.0), |b| (b.look().scale(), b.look().ui_scale()));
            let g = geometry(width, scale, ui);
            if g != self.geometry.get() {
                self.geometry.set(g);
                obj.set_left_margin(g.left);
                obj.set_right_margin((width - g.left - g.doc_width).max(0));
            }
            let top = (32.0 * scale).round() as i32;
            if obj.top_margin() != top {
                obj.set_top_margin(top);
            }
            let bottom = (height * 2 / 5).max(120);
            if obj.bottom_margin() != bottom {
                obj.set_bottom_margin(bottom);
            }
            self.parent_size_allocate(width, height, baseline);
            for cb in self.on_layout.borrow().iter() {
                cb();
            }
        }
    }

    impl TextViewImpl for DocView {
        fn snapshot_layer(&self, layer: gtk::TextViewLayer, snapshot: gtk::Snapshot) {
            self.parent_snapshot_layer(layer, snapshot.clone());
            let obj = self.obj();
            let Some(buf) = obj.doc_buffer_opt() else { return };
            match layer {
                gtk::TextViewLayer::BelowText => obj.draw_blocks(&buf, &snapshot),
                gtk::TextViewLayer::AboveText => obj.draw_markers(&buf, &snapshot),
                _ => {}
            }
        }

        fn backspace(&self) {
            let obj = self.obj();
            let buf = obj.doc_buffer();
            if buf.source_mode() {
                self.parent_backspace();
                return;
            }
            if buf.has_selection() {
                buf.run(|src, doc, _, sel| edit::delete_range(src, doc, sel.unwrap_or_default()));
            } else {
                buf.run(|src, doc, c, _| edit::backspace(src, doc, c));
            }
            obj.scroll_mark_onscreen(&buf.get_insert());
        }

        fn delete_from_cursor(&self, kind: gtk::DeleteType, count: i32) {
            let obj = self.obj();
            let buf = obj.doc_buffer();
            if buf.source_mode() || kind != gtk::DeleteType::Chars || buf.has_selection() {
                // Other deletions reach the buffer as ranges, which it
                // handles with the same semantics.
                self.parent_delete_from_cursor(kind, count);
                return;
            }
            if count < 0 {
                buf.run(|src, doc, c, _| edit::backspace(src, doc, c));
            } else {
                buf.run(|src, doc, c, _| edit::delete_forward(src, doc, c));
            }
            obj.scroll_mark_onscreen(&buf.get_insert());
        }

        fn paste_clipboard(&self) {
            let obj = self.obj();
            let clipboard = obj.clipboard();
            clipboard.read_text_async(
                None::<&gio::Cancellable>,
                glib::clone!(
                    #[weak]
                    obj,
                    move |res| {
                        if let Ok(Some(text)) = res {
                            let buf = obj.doc_buffer();
                            buf.paste_text(&text);
                            obj.scroll_mark_onscreen(&buf.get_insert());
                        }
                    }
                ),
            );
        }

        fn copy_clipboard(&self) {
            self.obj().copy_selection();
        }

        fn cut_clipboard(&self) {
            let obj = self.obj();
            if obj.copy_selection() {
                let buf = obj.doc_buffer();
                if buf.source_mode() {
                    buf.delete_selection(true, true);
                } else {
                    buf.run(|src, doc, _, sel| edit::delete_range(src, doc, sel.unwrap_or_default()));
                }
            }
        }
    }
}

glib::wrapper! {
    pub struct DocView(ObjectSubclass<imp::DocView>)
        @extends gtk::TextView, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Scrollable;
}

impl DocView {
    pub fn new(buffer: &DocBuffer) -> DocView {
        let v: DocView = glib::Object::new();
        v.set_buffer(Some(buffer));
        v
    }

    pub fn doc_buffer(&self) -> DocBuffer {
        self.doc_buffer_opt().expect("DocView without DocBuffer")
    }

    fn doc_buffer_opt(&self) -> Option<DocBuffer> {
        self.buffer().downcast::<DocBuffer>().ok()
    }

    /// Runs `f`, which changes line heights (reflowing, showing the
    /// source), keeping the cursor's line where it is in the window when
    /// it is in view. GTK would keep the top line still instead, and the
    /// text being read would move by however much the lines above it
    /// changed.
    pub fn keep_cursor_still(&self, f: impl FnOnce()) {
        let buf = self.doc_buffer();
        let visible = self.visible_rect();
        let cursor = self.iter_location(&buf.iter_at_mark(&buf.get_insert()));
        let room = (visible.height() - cursor.height()).max(1);
        let dy = cursor.y() - visible.y();
        f();
        if (0..=room).contains(&dy) {
            // A mark scroll waits for the new layout to be measured.
            self.scroll_to_mark(&buf.get_insert(), 0.0, true, 0.0, dy as f64 / room as f64);
        }
    }

    pub fn geometry(&self) -> Geometry {
        self.imp().geometry.get()
    }

    /// Called after every size allocation.
    pub fn connect_layout(&self, f: impl Fn() + 'static) {
        self.imp().on_layout.borrow_mut().push(Box::new(f));
    }

    pub fn set_link_handler(&self, f: impl Fn(&str) + 'static) {
        self.imp().on_link.replace(Some(Box::new(f)));
    }

    fn copy_selection(&self) -> bool {
        let buf = self.doc_buffer();
        let Some(sel) = buf.selection_bytes() else { return false };
        let text = {
            let st = buf.state();
            if buf.source_mode() {
                st.text[sel].to_string()
            } else {
                edit::copy_source(&st.text, &st.doc, sel)
            }
        };
        self.clipboard().set_text(&text);
        true
    }

    fn setup_controllers(&self) {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to = view)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, state| {
                let buf = view.doc_buffer();
                // Keys typed into a comment card pass through here too (the
                // cards are children of the view); they are not ours.
                if buf.source_mode() || !view.has_focus() {
                    return glib::Propagation::Proceed;
                }
                let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
                let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
                let alt = state.contains(gdk::ModifierType::ALT_MASK);
                match key {
                    gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter
                        if shift && !ctrl && !alt =>
                    {
                        view.newline(true);
                        glib::Propagation::Stop
                    }
                    gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter
                        if ctrl && !shift && !alt =>
                    {
                        view.toggle_task_at_cursor();
                        glib::Propagation::Stop
                    }
                    // Ctrl+Shift+Tab stays GTK's way out of the text view.
                    gdk::Key::ISO_Left_Tab if !ctrl && !alt => {
                        buf.run(|s, d, c, sel| edit::indent(s, d, sel.unwrap_or(c..c), true));
                        glib::Propagation::Stop
                    }
                    gdk::Key::Tab if shift && !ctrl && !alt => {
                        buf.run(|s, d, c, sel| edit::indent(s, d, sel.unwrap_or(c..c), true));
                        glib::Propagation::Stop
                    }
                    gdk::Key::Tab if !ctrl && !alt => {
                        buf.run(|s, d, c, sel| edit::indent(s, d, sel.unwrap_or(c..c), false));
                        glib::Propagation::Stop
                    }
                    _ => glib::Propagation::Proceed,
                }
            }
        ));
        self.add_controller(keys);

        let click = gtk::GestureClick::new();
        click.set_button(gdk::BUTTON_PRIMARY);
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        click.connect_pressed(glib::clone!(
            #[weak(rename_to = view)]
            self,
            move |g, _, x, y| {
                let (bx, by) =
                    view.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
                if let Some(item) = view.checkbox_at(bx as f32, by as f32) {
                    let buf = view.doc_buffer();
                    buf.run(|s, d, c, _| edit::toggle_task(s, d, item, c));
                    g.set_state(gtk::EventSequenceState::Claimed);
                    return;
                }
                if g.current_event_state().contains(gdk::ModifierType::CONTROL_MASK)
                    && let Some(url) = view.link_at(bx, by)
                {
                    g.set_state(gtk::EventSequenceState::Claimed);
                    if let Some(h) = &*view.imp().on_link.borrow() {
                        h(&url);
                    }
                }
            }
        ));
        self.add_controller(click);

        let motion = gtk::EventControllerMotion::new();
        motion.connect_motion(glib::clone!(
            #[weak(rename_to = view)]
            self,
            move |m, x, y| {
                let (bx, by) =
                    view.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
                let ctrl = m.current_event_state().contains(gdk::ModifierType::CONTROL_MASK);
                let pointer = view.checkbox_at(bx as f32, by as f32).is_some()
                    || (ctrl && view.link_at(bx, by).is_some());
                view.set_cursor_from_name(Some(if pointer { "pointer" } else { "text" }));
            }
        ));
        self.add_controller(motion);

        self.connect_query_tooltip(|view, x, y, keyboard, tooltip| {
            if keyboard {
                return false;
            }
            let (bx, by) = view.window_to_buffer_coords(gtk::TextWindowType::Widget, x, y);
            match view.link_at(bx, by) {
                Some(url) => {
                    tooltip.set_text(Some(&format!("{url}\nCtrl+click to open")));
                    true
                }
                None => false,
            }
        });
    }

    pub fn newline(&self, soft: bool) {
        let buf = self.doc_buffer();
        buf.begin_user_action();
        if let Some(sel) = buf.selection_bytes() {
            buf.run(|s, d, _, _| edit::delete_range(s, d, sel));
        }
        buf.run(|s, d, c, _| edit::newline(s, d, c, soft));
        buf.end_user_action();
        self.scroll_mark_onscreen(&buf.get_insert());
    }

    /// Center of the `n`th drawn checkbox, in buffer coordinates.
    pub fn checkbox_center(&self, n: usize) -> Option<(i32, i32)> {
        let boxes = self.imp().checkboxes.borrow();
        let (r, _) = boxes.get(n)?;
        Some(((r.x() + r.width() / 2.0) as i32, (r.y() + r.height() / 2.0) as i32))
    }

    /// Checks or unchecks the task item the cursor is on.
    pub fn toggle_task_at_cursor(&self) {
        let buf = self.doc_buffer();
        let item = {
            let st = buf.state();
            let li = st.doc.line_index(buf.cursor_byte().min(st.text.len()));
            st.doc
                .item_on_line(li)
                .filter(|&i| st.doc.items[i].task.is_some())
        };
        if let Some(item) = item {
            buf.run(|s, d, c, _| edit::toggle_task(s, d, item, c));
        }
    }

    /// Opens the link at the cursor, if any.
    pub fn open_link_at_cursor(&self) {
        let buf = self.doc_buffer();
        let url = {
            let st = buf.state();
            let c = buf.cursor_byte();
            st.doc
                .inlines
                .iter()
                .find(|e| {
                    matches!(e.kind, InlineKind::Link | InlineKind::Image)
                        && e.content().start <= c
                        && c <= e.content().end
                })
                .and_then(|e| e.url.clone())
        };
        if let Some(url) = url
            && let Some(h) = &*self.imp().on_link.borrow()
        {
            h(&url);
        }
    }

    fn checkbox_at(&self, x: f32, y: f32) -> Option<usize> {
        self.imp()
            .checkboxes
            .borrow()
            .iter()
            .find(|(r, _)| r.contains_point(&graphene::Point::new(x, y)))
            .map(|(_, i)| *i)
    }

    /// The destination of the link under a buffer position.
    pub fn link_at(&self, bx: i32, by: i32) -> Option<String> {
        let it = self.iter_at_location(bx, by)?;
        let buf = self.doc_buffer();
        let b = buf.byte_at(&it);
        let st = buf.state();
        st.doc
            .inlines
            .iter()
            .find(|e| {
                matches!(e.kind, InlineKind::Link | InlineKind::Image)
                    && e.content().start <= b
                    && b < e.content().end.max(e.content().start + 1)
            })
            .and_then(|e| e.url.clone())
    }

    fn visible_lines(&self, buf: &DocBuffer) -> (usize, usize) {
        // `line_at_y` never maps pixels to byte indices, which GTK gets
        // wrong for lines with hidden text.
        let r = self.visible_rect();
        let (top, _) = self.line_at_y(r.y());
        let (bottom, _) = self.line_at_y(r.y() + r.height());
        let st = buf.state();
        let top = st.byte_of(top.offset() as usize);
        let bottom = st.byte_of(bottom.offset() as usize);
        (st.doc.line_index(top), st.doc.line_index(bottom))
    }

    fn draw_blocks(&self, buf: &DocBuffer, snapshot: &gtk::Snapshot) {
        let (first, last) = self.visible_lines(buf);
        let look = buf.look();
        let scale = look.scale();
        let p = &look.palette;
        let g = self.geometry();
        let st = buf.state();
        let doc = &st.doc;
        let at = |b: usize| self.iter_location_of(buf, b);

        for cb in &doc.code_blocks {
            let lines = cb.content_lines();
            if lines.is_empty() || lines.end <= first || lines.start > last + 1 {
                continue;
            }
            let fl = &doc.lines[lines.start];
            let ll = &doc.lines[lines.end - 1];
            let top = at(fl.content_start.min(fl.end));
            let bottom = at(ll.end);
            let (pad_x, pad_y) = (12.0 * scale as f32, 8.0 * scale as f32);
            let x0 = g.left as f32 + indent_px(&fl.containers, scale) - pad_x;
            let x1 = (g.left + g.doc_width) as f32 + pad_x;
            let y0 = top.y() as f32 - pad_y;
            let y1 = (bottom.y() + bottom.height()) as f32 + pad_y;
            let rect = graphene::Rect::new(x0, y0, x1 - x0, y1 - y0);
            snapshot.append_color(&rgba(&p.code_bg), &rect);
            let lang = cb.info.split_whitespace().next().unwrap_or("");
            if !lang.is_empty() {
                let layout = self.create_pango_layout(Some(lang));
                let mut desc = gtk::pango::FontDescription::from_string(&look.fonts.mono);
                desc.set_size((look.fonts.size * 0.72 * gtk::pango::SCALE as f64) as i32);
                layout.set_font_description(Some(&desc));
                let (w, _) = layout.pixel_size();
                snapshot.save();
                snapshot.translate(&graphene::Point::new(x1 - w as f32 - 8.0, y0 + 3.0));
                snapshot.append_layout(&layout, &rgba_alpha(&p.dim, 0.9));
                snapshot.restore();
            }
        }

        for (qi, q) in doc.quotes.iter().enumerate() {
            let mut ll = q.last_line;
            while ll > q.first_line && doc.lines[ll].kind == LineKind::Blank {
                ll -= 1;
            }
            if ll < first || q.first_line > last {
                continue;
            }
            let lf = &doc.lines[q.first_line];
            let k = lf
                .containers
                .iter()
                .position(|c| *c == Container::Quote(qi))
                .unwrap_or(0);
            let x = g.left as f32 + indent_px(&lf.containers[..k], scale) + 2.0 * scale as f32;
            let top = at(lf.content_start.min(lf.end));
            // The line's newline can be hidden, and GTK gives hidden text an
            // empty location, so take the bottom from the display line.
            let (by, bh) = self.line_yrange(&buf.iter_at_byte(doc.lines[ll].end));
            let rect = graphene::Rect::new(
                x,
                top.y() as f32,
                3.0 * scale as f32,
                (by + bh - top.y()) as f32,
            );
            snapshot.append_color(&rgba(&p.muted), &rect);
        }

        for li in first..=last.min(doc.lines.len().saturating_sub(1)) {
            let l = &doc.lines[li];
            if l.kind != LineKind::Rule {
                continue;
            }
            let (y, h) = self.line_yrange(&buf.iter_at_byte(l.start));
            let x = g.left as f32 + indent_px(&l.containers, scale);
            let rect = graphene::Rect::new(
                x,
                (y + h / 2) as f32,
                (g.left + g.doc_width) as f32 - x,
                1.0_f32.max(scale as f32),
            );
            snapshot.append_color(&rgba(&p.muted), &rect);
        }
    }

    fn draw_markers(&self, buf: &DocBuffer, snapshot: &gtk::Snapshot) {
        let (first, last) = self.visible_lines(buf);
        let look = buf.look();
        let scale = look.scale();
        let p = &look.palette;
        let g = self.geometry();
        let st = buf.state();
        let doc = &st.doc;
        let mut boxes = Vec::new();
        for (ii, it) in doc.items.iter().enumerate() {
            if it.line < first || it.line > last {
                continue;
            }
            let line = &doc.lines[it.line];
            let Some(k) = line.containers.iter().position(|c| *c == Container::Item(ii)) else {
                continue;
            };
            let x_text = g.left as f32 + indent_px(&line.containers[..=k], scale);
            let rect = self.iter_location_of(buf, line.content_start.min(line.end));
            let (y, h) = (rect.y() as f32, rect.height() as f32);
            let gap = 8.0 * scale as f32;
            if let Some((checked, _)) = it.task {
                let size = (14.0 * scale as f32).round();
                let bx = x_text - size - gap;
                let by = y + ((h - size) / 2.0).round();
                let cr = snapshot.append_cairo(&graphene::Rect::new(bx - 2.0, by - 2.0, size + 4.0, size + 4.0));
                cr.rectangle(bx as f64 + 0.75, by as f64 + 0.75, size as f64 - 1.5, size as f64 - 1.5);
                if checked {
                    set_color(&cr, &rgba(&p.accent));
                    let _ = cr.fill();
                    set_color(&cr, &rgba(&p.bg));
                    cr.set_line_width(1.8 * scale);
                    let (x, y, s) = (bx as f64, by as f64, size as f64);
                    cr.move_to(x + 0.25 * s, y + 0.52 * s);
                    cr.line_to(x + 0.43 * s, y + 0.70 * s);
                    cr.line_to(x + 0.76 * s, y + 0.32 * s);
                    let _ = cr.stroke();
                } else {
                    set_color(&cr, &rgba_alpha(&p.fg, 0.7));
                    cr.set_line_width(1.5 * scale);
                    let _ = cr.stroke();
                }
                boxes.push((graphene::Rect::new(bx - 4.0, by - 4.0, size + 8.0, size + 8.0), ii));
            } else {
                let text = match it.number {
                    Some(n) => format!("{n}."),
                    None => bullet(it.depth).to_string(),
                };
                let layout = self.create_pango_layout(Some(&text));
                let (w, lh) = layout.pixel_size();
                let bx = x_text - w as f32 - gap;
                let by = y + ((h - lh as f32) / 2.0).round();
                snapshot.save();
                snapshot.translate(&graphene::Point::new(bx, by));
                snapshot.append_layout(&layout, &rgba_alpha(&p.fg, 0.85));
                snapshot.restore();
            }
        }
        self.imp().checkboxes.replace(boxes);
    }

    fn iter_location_of(&self, buf: &DocBuffer, b: usize) -> gdk::Rectangle {
        self.iter_location(&buf.iter_at_byte(b))
    }
}

fn set_color(cr: &gtk::cairo::Context, c: &gdk::RGBA) {
    cr.set_source_rgba(c.red() as f64, c.green() as f64, c.blue() as f64, c.alpha() as f64);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_is_about_100_characters_beside_the_gutter() {
        let g = geometry(1340, 1.0, 1.0);
        assert_eq!(g.doc_width, 760);
        assert!(g.gutter_x + g.card_width <= 1340);
        // Narrower windows give the text what is left.
        let g = geometry(1024, 1.0, 1.0);
        assert_eq!(g.doc_width, 1024 - CARD_WIDTH - GUTTER_GAP - 2 * SIDE_PAD);
        // Larger text widens the column and the cards with it.
        let g = geometry(2400, 1.25, 1.25);
        assert_eq!((g.doc_width, g.card_width), (950, 375));
        assert!(g.gutter_x + g.card_width <= 2400);
    }
}
