//! Find and replace, over the text as rendered (hidden Markdown syntax is
//! skipped). Replacing goes through the editing commands, so formatting
//! around a match survives.

use super::buffer::DocBuffer;
use super::view::DocView;
use margin_core::md::{edit, search};
use gtk::{gdk, glib, prelude::*};
use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::{Rc, Weak};

pub struct FindBar {
    pub bar: gtk::SearchBar,
    entry: gtk::SearchEntry,
    replace_row: gtk::Box,
    replace_entry: gtk::Entry,
    replace_toggle: gtk::ToggleButton,
    count: gtk::Label,
    match_case: gtk::ToggleButton,
    buffer: DocBuffer,
    view: DocView,
    /// Matches as byte ranges of the document text.
    matches: RefCell<Vec<Range<usize>>>,
    current: Cell<Option<usize>>,
    refresh_queued: Cell<bool>,
    this: RefCell<Weak<FindBar>>,
}

impl FindBar {
    pub fn new(view: &DocView) -> Rc<FindBar> {
        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Find")
            .hexpand(true)
            .build();
        let count = gtk::Label::new(None);
        count.add_css_class("find-count");
        count.set_width_chars(10);
        let prev = gtk::Button::from_icon_name("go-up-symbolic");
        prev.set_tooltip_text(Some("Previous match (Shift+Ctrl+G)"));
        prev.add_css_class("flat");
        let next = gtk::Button::from_icon_name("go-down-symbolic");
        next.set_tooltip_text(Some("Next match (Ctrl+G)"));
        next.add_css_class("flat");
        let match_case = gtk::ToggleButton::with_label("Aa");
        match_case.set_tooltip_text(Some("Match case"));
        match_case.add_css_class("flat");
        let replace_toggle = gtk::ToggleButton::new();
        replace_toggle.set_icon_name("edit-find-replace-symbolic");
        replace_toggle.set_tooltip_text(Some("Replace (Ctrl+H)"));
        replace_toggle.add_css_class("flat");
        let find_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        for w in [
            entry.upcast_ref::<gtk::Widget>(),
            count.upcast_ref(),
            prev.upcast_ref(),
            next.upcast_ref(),
            match_case.upcast_ref(),
            replace_toggle.upcast_ref(),
        ] {
            find_row.append(w);
        }

        let replace_entry = gtk::Entry::builder()
            .placeholder_text("Replace with")
            .hexpand(true)
            .build();
        let replace = gtk::Button::with_label("Replace");
        let replace_all = gtk::Button::with_label("Replace All");
        let replace_row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        replace_row.append(&replace_entry);
        replace_row.append(&replace);
        replace_row.append(&replace_all);
        replace_row.set_visible(false);

        let rows = gtk::Box::new(gtk::Orientation::Vertical, 4);
        rows.add_css_class("find-bar");
        rows.set_width_request(560);
        rows.append(&find_row);
        rows.append(&replace_row);

        let bar = gtk::SearchBar::new();
        bar.set_child(Some(&rows));
        bar.connect_entry(&entry);
        bar.set_show_close_button(true);

        let fb = Rc::new(FindBar {
            bar,
            entry,
            replace_row,
            replace_entry,
            replace_toggle,
            count,
            match_case,
            buffer: view.doc_buffer(),
            view: view.clone(),
            matches: RefCell::new(Vec::new()),
            current: Cell::new(None),
            refresh_queued: Cell::new(false),
            this: RefCell::new(Weak::new()),
        });
        fb.this.replace(Rc::downgrade(&fb));

        let w = Rc::downgrade(&fb);
        fb.entry.connect_search_changed(move |_| {
            if let Some(f) = w.upgrade() {
                f.refresh(true);
            }
        });
        let w = Rc::downgrade(&fb);
        fb.entry.connect_activate(move |_| {
            if let Some(f) = w.upgrade() {
                f.step(true);
            }
        });
        let w = Rc::downgrade(&fb);
        fb.entry.connect_next_match(move |_| {
            if let Some(f) = w.upgrade() {
                f.step(true);
            }
        });
        let w = Rc::downgrade(&fb);
        fb.entry.connect_previous_match(move |_| {
            if let Some(f) = w.upgrade() {
                f.step(false);
            }
        });
        let w = Rc::downgrade(&fb);
        fb.entry.connect_stop_search(move |_| {
            if let Some(f) = w.upgrade() {
                f.close();
            }
        });
        let keys = gtk::EventControllerKey::new();
        let w = Rc::downgrade(&fb);
        keys.connect_key_pressed(move |_, key, _, state| {
            let shift_enter = matches!(key, gdk::Key::Return | gdk::Key::KP_Enter)
                && state.contains(gdk::ModifierType::SHIFT_MASK);
            if shift_enter && let Some(f) = w.upgrade() {
                f.step(false);
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        fb.entry.add_controller(keys);
        let w = Rc::downgrade(&fb);
        prev.connect_clicked(move |_| {
            if let Some(f) = w.upgrade() {
                f.step(false);
            }
        });
        let w = Rc::downgrade(&fb);
        next.connect_clicked(move |_| {
            if let Some(f) = w.upgrade() {
                f.step(true);
            }
        });
        let w = Rc::downgrade(&fb);
        fb.match_case.connect_toggled(move |_| {
            if let Some(f) = w.upgrade() {
                f.refresh(true);
            }
        });
        let w = Rc::downgrade(&fb);
        fb.replace_toggle.connect_toggled(move |t| {
            if let Some(f) = w.upgrade() {
                f.replace_row.set_visible(t.is_active());
            }
        });
        let w = Rc::downgrade(&fb);
        fb.replace_entry.connect_activate(move |_| {
            if let Some(f) = w.upgrade() {
                f.replace_current();
            }
        });
        let w = Rc::downgrade(&fb);
        replace.connect_clicked(move |_| {
            if let Some(f) = w.upgrade() {
                f.replace_current();
            }
        });
        let w = Rc::downgrade(&fb);
        replace_all.connect_clicked(move |_| {
            if let Some(f) = w.upgrade() {
                f.replace_all();
            }
        });
        let w = Rc::downgrade(&fb);
        fb.bar.connect_search_mode_enabled_notify(move |bar| {
            if !bar.is_search_mode()
                && let Some(f) = w.upgrade()
            {
                f.clear_highlights();
                if let Some(r) = f.current.get().and_then(|i| f.matches.borrow().get(i).cloned()) {
                    let (s, e) = (f.buffer.iter_at_byte(r.start), f.buffer.iter_at_byte(r.end));
                    f.buffer.select_range(&s, &e);
                }
                f.view.grab_focus();
            }
        });
        let w = Rc::downgrade(&fb);
        fb.buffer.connect_changed(move |_| {
            if let Some(f) = w.upgrade()
                && f.bar.is_search_mode()
            {
                f.queue_refresh();
            }
        });
        fb
    }

    /// For test scripts: the matches, the current one and the tags on each.
    pub fn describe(&self) -> String {
        let (found, current) = self.buffer.search_tags();
        let mut out = format!("find: current={:?}", self.current.get());
        for r in self.matches.borrow().iter() {
            let it = self.buffer.iter_at_byte(r.start);
            out += &format!(
                "\n  {r:?} found={} current={}",
                it.has_tag(&found),
                it.has_tag(&current)
            );
        }
        out
    }

    pub fn is_open(&self) -> bool {
        self.bar.is_search_mode()
    }

    /// Shows the bar, starting from the selected text if there is some.
    pub fn open(&self, replace: bool) {
        if let Some(sel) = self.buffer.selection_bytes() {
            let text = self.buffer.text_string();
            if let Some(t) = text.get(sel)
                && !t.contains('\n')
                && !t.is_empty()
            {
                self.entry.set_text(t);
            }
        }
        self.replace_toggle.set_active(replace);
        self.bar.set_search_mode(true);
        self.refresh(true);
        if replace && !self.entry.text().is_empty() {
            self.replace_entry.grab_focus();
        } else {
            self.entry.grab_focus();
            self.entry.select_region(0, -1);
        }
    }

    pub fn close(&self) {
        self.bar.set_search_mode(false);
    }

    fn queue_refresh(&self) {
        if self.refresh_queued.replace(true) {
            return;
        }
        let w = self.this.borrow().clone();
        glib::idle_add_local_once(move || {
            if let Some(f) = w.upgrade() {
                f.refresh_queued.set(false);
                f.refresh(false);
            }
        });
    }

    /// Recomputes matches. With `jump`, moves to the first match at or
    /// after the cursor.
    fn refresh(&self, jump: bool) {
        let needle = self.entry.text().to_string();
        let found = {
            let st = self.buffer.state();
            search::find_all(&st.text, &st.doc, &needle, self.match_case.is_active())
        };
        let old_current = self.current.get().and_then(|i| self.matches.borrow().get(i).cloned());
        self.matches.replace(found);
        let current = if jump {
            let cursor = match self.buffer.selection_bytes() {
                Some(r) => r.start,
                None => self.buffer.cursor_byte(),
            };
            let m = self.matches.borrow();
            m.iter().position(|r| r.start >= cursor).or((!m.is_empty()).then_some(0))
        } else {
            let m = self.matches.borrow();
            old_current
                .and_then(|o| m.iter().position(|r| r.start >= o.start))
                .or((!m.is_empty()).then_some(0))
        };
        self.current.set(current);
        if jump {
            self.select_current();
        }
        self.highlight();
    }

    fn clear_highlights(&self) {
        let (found, current) = self.buffer.search_tags();
        let (s, e) = self.buffer.bounds();
        self.buffer.remove_tag(&found, &s, &e);
        self.buffer.remove_tag(&current, &s, &e);
    }

    fn highlight(&self) {
        self.clear_highlights();
        let (found, current) = self.buffer.search_tags();
        let matches = self.matches.borrow();
        for (i, r) in matches.iter().enumerate() {
            let s = self.buffer.iter_at_byte(r.start);
            let e = self.buffer.iter_at_byte(r.end);
            let tag = if Some(i) == self.current.get() { &current } else { &found };
            self.buffer.apply_tag(tag, &s, &e);
        }
        let label = match (self.current.get(), matches.len()) {
            (_, 0) if self.entry.text().is_empty() => String::new(),
            (_, 0) => "No results".to_string(),
            (Some(i), n) => format!("{} of {n}", i + 1),
            (None, n) => format!("{n} found"),
        };
        self.count.set_label(&label);
    }

    /// Moves the cursor to the current match and scrolls to it. The match
    /// is only selected when the bar closes: the selection color would hide
    /// the current-match highlight.
    fn select_current(&self) {
        let Some(r) = self.current.get().and_then(|i| self.matches.borrow().get(i).cloned()) else {
            return;
        };
        let s = self.buffer.iter_at_byte(r.start);
        self.buffer.place_cursor(&s);
        let mut it = s;
        self.view.scroll_to_iter(&mut it, 0.2, false, 0.0, 0.0);
    }

    /// Moves to the next (or previous) match, wrapping around.
    pub fn step(&self, forward: bool) {
        if !self.is_open() {
            self.open(false);
            return;
        }
        let n = self.matches.borrow().len();
        if n == 0 {
            return;
        }
        let i = match self.current.get() {
            Some(i) if forward => (i + 1) % n,
            Some(i) => (i + n - 1) % n,
            None => 0,
        };
        self.current.set(Some(i));
        self.select_current();
        self.highlight();
    }

    fn replace_range(&self, r: Range<usize>, with: &str) {
        let plain = {
            let st = self.buffer.state();
            edit::replace_plain(&st.text, &st.doc, r.clone(), with)
        };
        if let Some(plan) = plain {
            self.buffer.apply_plan(&plan);
            return;
        }
        self.buffer.run(|s, d, _, _| edit::delete_range(s, d, r.clone()));
        if !with.is_empty() {
            self.buffer.run(|s, d, c, _| edit::insert(s, d, c, with));
        }
    }

    pub fn replace_current(&self) {
        let Some(r) = self.current.get().and_then(|i| self.matches.borrow().get(i).cloned()) else {
            return;
        };
        let with = self.replace_entry.text().to_string();
        self.buffer.begin_user_action();
        self.replace_range(r, &with);
        self.buffer.end_user_action();
        self.refresh(true);
    }

    pub fn replace_all(&self) {
        let matches = self.matches.borrow().clone();
        if matches.is_empty() {
            return;
        }
        let with = self.replace_entry.text().to_string();
        self.buffer.begin_user_action();
        // Last to first, so earlier ranges stay valid.
        for r in matches.into_iter().rev() {
            self.replace_range(r, &with);
        }
        self.buffer.end_user_action();
        self.refresh(false);
    }
}
