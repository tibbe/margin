//! Comment threads in the gutter: anchoring them to text with marks,
//! laying out their cards next to the text, keeping focus in sync between
//! highlights and cards, and reading and writing the shared store.

use super::buffer::DocBuffer;
use super::card::{Card, CardActions, DraftCard};
use super::view::DocView;
use crate::comments::{Comments, Status, Store, Thread};
use crate::md::edit;
use adw::prelude::*;
use gtk::glib;
use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::{Rc, Weak};

const CARD_GAP: i32 = 10;
const ACTIVE_SHIFT: i32 = 14;

struct ThreadUi {
    thread: Thread,
    start: gtk::TextMark,
    end: gtk::TextMark,
    /// Region our highlight was applied to; grows with text typed at its
    /// edges so stale highlight never escapes it.
    hl: (gtk::TextMark, gtk::TextMark),
    card: Rc<Card>,
}

struct Draft {
    start: gtk::TextMark,
    end: gtk::TextMark,
    hl: (gtk::TextMark, gtk::TextMark),
    card: DraftCard,
}

type Notify = Box<dyn Fn()>;
type Toaster = Box<dyn Fn(adw::Toast)>;

pub struct CommentLayer {
    view: DocView,
    buffer: DocBuffer,
    store: RefCell<Option<Store>>,
    threads: RefCell<Vec<ThreadUi>>,
    draft: RefCell<Option<Draft>>,
    active: Cell<Option<u64>>,
    show_resolved: Cell<bool>,
    relayout_queued: Cell<bool>,
    /// The one overlay in the text view; cards live inside it, because
    /// GtkTextView cannot remove overlay children.
    gutter: gtk::Fixed,
    add_button: gtk::Button,
    listeners: RefCell<Vec<Notify>>,
    toaster: RefCell<Option<Toaster>>,
    /// Runs before a thread is added (the window saves the document).
    before_add: RefCell<Option<Notify>>,
    this: RefCell<Weak<CommentLayer>>,
}

impl CommentLayer {
    pub fn new(view: &DocView) -> Rc<CommentLayer> {
        let buffer = view.doc_buffer();
        let add_button = gtk::Button::from_icon_name("chat-message-new-symbolic");
        add_button.add_css_class("add-comment-button");
        add_button.set_tooltip_text(Some("Add comment (Ctrl+Alt+M)"));
        add_button.set_visible(false);
        add_button.set_focus_on_click(false);
        let gutter = gtk::Fixed::new();
        gutter.set_cursor_from_name(Some("default"));
        // Clicks in the gutter belong to the cards. GtkButton only claims a
        // click on release, so without this the press bubbles up to the text
        // view, which moves the cursor (dropping the selection the add
        // button acts on) and takes focus from the reply box.
        let clicks = gtk::EventControllerLegacy::new();
        clicks.connect_event(|_, event| {
            use gtk::gdk::EventType;
            match event.event_type() {
                EventType::ButtonPress
                | EventType::ButtonRelease
                | EventType::TouchBegin
                | EventType::TouchUpdate
                | EventType::TouchEnd
                | EventType::TouchCancel => glib::Propagation::Stop,
                _ => glib::Propagation::Proceed,
            }
        });
        gutter.add_controller(clicks);
        gutter.put(&add_button, ACTIVE_SHIFT as f64, 0.0);
        view.add_overlay(&gutter, 0, 0);

        let layer = Rc::new(CommentLayer {
            view: view.clone(),
            buffer: buffer.clone(),
            store: RefCell::new(None),
            threads: RefCell::new(Vec::new()),
            draft: RefCell::new(None),
            active: Cell::new(None),
            show_resolved: Cell::new(false),
            relayout_queued: Cell::new(false),
            gutter,
            add_button,
            listeners: RefCell::new(Vec::new()),
            toaster: RefCell::new(None),
            before_add: RefCell::new(None),
            this: RefCell::new(Weak::new()),
        });
        layer.this.replace(Rc::downgrade(&layer));

        let weak = Rc::downgrade(&layer);
        layer.add_button.connect_clicked(move |_| {
            if let Some(l) = weak.upgrade() {
                l.begin_draft();
            }
        });
        let weak = Rc::downgrade(&layer);
        buffer.connect_mark_set(move |b, _, mark| {
            if (*mark == b.get_insert() || *mark == b.selection_bound())
                && let Some(l) = weak.upgrade()
            {
                l.on_cursor_moved();
            }
        });
        let weak = Rc::downgrade(&layer);
        buffer.connect_changed(move |_| {
            if let Some(l) = weak.upgrade() {
                l.queue_relayout();
            }
        });
        let weak = Rc::downgrade(&layer);
        view.connect_layout(move || {
            if let Some(l) = weak.upgrade() {
                l.queue_relayout();
            }
        });
        if let Some(adj) = view.vadjustment() {
            let weak = Rc::downgrade(&layer);
            adj.connect_changed(move |_| {
                if let Some(l) = weak.upgrade() {
                    l.queue_relayout();
                }
            });
        }
        layer
    }

    fn rc(&self) -> Rc<CommentLayer> {
        self.this.borrow().upgrade().expect("layer alive")
    }

    pub fn connect_changed(&self, f: impl Fn() + 'static) {
        self.listeners.borrow_mut().push(Box::new(f));
    }

    pub fn set_toaster(&self, f: impl Fn(adw::Toast) + 'static) {
        self.toaster.replace(Some(Box::new(f)));
    }

    pub fn set_before_add(&self, f: impl Fn() + 'static) {
        self.before_add.replace(Some(Box::new(f)));
    }

    fn notify(&self) {
        for f in self.listeners.borrow().iter() {
            f();
        }
    }

    fn toast(&self, t: adw::Toast) {
        if let Some(f) = &*self.toaster.borrow() {
            f(t);
        }
    }

    pub fn open_count(&self) -> usize {
        self.threads.borrow().iter().filter(|t| t.thread.is_open()).count()
    }

    pub fn resolved_count(&self) -> usize {
        self.threads.borrow().iter().filter(|t| !t.thread.is_open()).count()
    }

    pub fn active(&self) -> Option<u64> {
        self.active.get()
    }

    pub fn store(&self) -> Option<Store> {
        self.store.borrow().clone()
    }

    /// The open threads, anchored where their text is in the buffer now.
    pub fn open_threads(&self) -> Vec<Thread> {
        let text = self.buffer.text_string();
        self.threads
            .borrow()
            .iter()
            .filter(|tu| tu.thread.is_open())
            .map(|tu| {
                let mut t = tu.thread.clone();
                match self.anchor_of(tu) {
                    Some(r) => {
                        t.anchor.quote = text[r.clone()].to_string();
                        t.anchor.start = r.start;
                        t.anchor.end = r.end;
                        t.anchor.detached = false;
                    }
                    None => {
                        let p = self.buffer.byte_at(&self.buffer.iter_at_mark(&tu.start));
                        t.anchor.start = p;
                        t.anchor.end = p;
                        t.anchor.detached = true;
                    }
                }
                t
            })
            .collect()
    }

    /// Whether `w` is part of the gutter (a card, the composer, a button).
    pub fn contains(&self, w: &gtk::Widget) -> bool {
        w == self.gutter.upcast_ref::<gtk::Widget>() || w.is_ancestor(&self.gutter)
    }

    // --- Store ------------------------------------------------------------

    pub fn attach(&self, store: Store) {
        self.store.replace(Some(store));
        // What is there when the document opens is not news.
        match self.store().map(|s| s.load()) {
            Some(Ok(c)) => self.merge(c, false),
            Some(Err(e)) => self.toast(adw::Toast::new(&format!("Could not read comments: {e:#}"))),
            None => {}
        }
    }

    /// Re-reads the store, e.g. after an agent replied from the CLI.
    pub fn reload(&self) {
        let Some(store) = self.store() else { return };
        match store.load() {
            Ok(c) => self.merge(c, true),
            Err(e) => self.toast(adw::Toast::new(&format!("Could not read comments: {e:#}"))),
        }
    }

    /// Current anchors as byte ranges of the buffer text; `None` when the
    /// commented text is gone.
    fn anchor_of(&self, tu: &ThreadUi) -> Option<Range<usize>> {
        if tu.thread.anchor.detached {
            return None;
        }
        let a = self.buffer.byte_at(&self.buffer.iter_at_mark(&tu.start));
        let b = self.buffer.byte_at(&self.buffer.iter_at_mark(&tu.end));
        (a < b).then_some(a..b)
    }

    /// Runs `f` on the stored threads with anchors brought up to date from
    /// the buffer, then shows the result.
    fn update_store<T>(&self, f: impl FnOnce(&mut Comments) -> anyhow::Result<T>) -> Option<T> {
        let store = self.store()?;
        let text = self.buffer.text_string();
        let anchors: Vec<(u64, Option<Range<usize>>, usize)> = self
            .threads
            .borrow()
            .iter()
            .map(|tu| {
                let pos = self.buffer.byte_at(&self.buffer.iter_at_mark(&tu.start));
                (tu.thread.id, self.anchor_of(tu), pos)
            })
            .collect();
        let result = store.update(|c| {
            c.sync(&text);
            for (id, range, pos) in &anchors {
                if let Ok(t) = c.thread_mut(*id) {
                    match range {
                        Some(r) => {
                            t.anchor.start = r.start;
                            t.anchor.end = r.end;
                            t.anchor.quote = text[r.clone()].to_string();
                            t.anchor.detached = false;
                        }
                        None => {
                            t.anchor.start = *pos;
                            t.anchor.end = *pos;
                            t.anchor.detached = true;
                        }
                    }
                }
            }
            let out = f(c)?;
            Ok((out, c.clone()))
        });
        match result {
            Ok((out, comments)) => {
                self.merge(comments, false);
                Some(out)
            }
            Err(e) => {
                self.toast(adw::Toast::new(&format!("Could not save comments: {e:#}")));
                None
            }
        }
    }

    /// Writes current anchors to the store, after the document was saved.
    pub fn persist_anchors(&self) {
        let Some(store) = self.store() else { return };
        if self.threads.borrow().is_empty() || !store.exists() {
            return;
        }
        self.update_store(|_| Ok(()));
    }

    /// Brings the gutter in line with `c`.
    fn merge(&self, mut c: Comments, announce: bool) {
        let text = self.buffer.text_string();
        c.sync(&text);
        let mut added_cards = Vec::new();
        // Our own changes reach `threads` before the store's file monitor
        // fires, so whatever is new here came from someone else (an agent).
        let (mut new_threads, mut new_replies, mut resolved) = (0, 0, 0);
        {
            let mut threads = self.threads.borrow_mut();
            for t in &c.threads {
                if let Some(tu) = threads.iter_mut().find(|x| x.thread.id == t.id) {
                    let changed = tu.thread.status != t.status
                        || tu.thread.messages != t.messages
                        || tu.thread.anchor.detached != t.anchor.detached;
                    if changed {
                        new_replies += t.messages.len().saturating_sub(tu.thread.messages.len());
                        if tu.thread.status != t.status && t.status == Status::Resolved {
                            resolved += 1;
                        }
                        let keep_anchor = !t.anchor.detached;
                        let anchor = tu.thread.anchor.clone();
                        tu.thread = t.clone();
                        if keep_anchor && !anchor.detached {
                            tu.thread.anchor = anchor;
                        }
                        tu.card.update(&tu.thread);
                    }
                } else {
                    new_threads += 1;
                    let tu = self.make_thread_ui(t.clone(), &text);
                    added_cards.push(tu.card.root.clone());
                    threads.push(tu);
                }
            }
            let buffer = &self.buffer;
            let gutter = &self.gutter;
            threads.retain(|tu| {
                let keep = c.thread(tu.thread.id).is_some();
                if !keep {
                    Self::clear_marks(buffer, gutter, tu);
                }
                keep
            });
            threads.sort_by_key(|tu| tu.thread.id);
        }
        for w in added_cards {
            self.gutter.put(&w, ACTIVE_SHIFT as f64, 0.0);
        }
        if let Some(id) = self.active.get()
            && !self.threads.borrow().iter().any(|t| t.thread.id == id && self.visible(&t.thread))
        {
            self.active.set(None);
        }
        self.sync_cards();
        self.refresh_highlights();
        self.queue_relayout();
        self.notify();
        let plural = |n: usize, one: &str, many: &str| match n {
            0 => None,
            1 => Some(format!("1 {one}")),
            n => Some(format!("{n} {many}")),
        };
        let news: Vec<String> = [
            plural(new_threads, "new comment", "new comments"),
            plural(new_replies, "new reply", "new replies"),
            plural(resolved, "comment resolved", "comments resolved"),
        ]
        .into_iter()
        .flatten()
        .collect();
        if announce && !news.is_empty() {
            self.toast(adw::Toast::new(&news.join(", ")));
        }
    }

    fn clear_marks(buffer: &DocBuffer, gutter: &gtk::Fixed, tu: &ThreadUi) {
        buffer.with_tagging(|| {
            let s = buffer.iter_at_mark(&tu.hl.0);
            let e = buffer.iter_at_mark(&tu.hl.1);
            buffer.remove_tag(&buffer.comment_tag(false), &s, &e);
            buffer.remove_tag(&buffer.comment_tag(true), &s, &e);
        });
        for m in [&tu.start, &tu.end, &tu.hl.0, &tu.hl.1] {
            buffer.delete_mark(m);
        }
        gutter.remove(&tu.card.root);
    }

    fn make_thread_ui(&self, thread: Thread, text: &str) -> ThreadUi {
        let a = thread.anchor.start.min(text.len());
        let b = thread.anchor.end.min(text.len()).max(a);
        let sa = self.buffer.iter_at_byte(a);
        let sb = self.buffer.iter_at_byte(b);
        let start = self.buffer.create_mark(None, &sa, false);
        let end = self.buffer.create_mark(None, &sb, true);
        let hl = (
            self.buffer.create_mark(None, &sa, true),
            self.buffer.create_mark(None, &sb, false),
        );
        let card = Card::new(&thread, self.card_actions());
        ThreadUi {
            thread,
            start,
            end,
            hl,
            card,
        }
    }

    fn card_actions(&self) -> Rc<CardActions> {
        let w = || self.this.borrow().clone();
        let (a, b, c, d, e) = (w(), w(), w(), w(), w());
        Rc::new(CardActions {
            activate: Box::new(move |id| {
                if let Some(l) = a.upgrade() {
                    l.activate(Some(id), true);
                }
            }),
            reply: Box::new(move |id, text| {
                if let Some(l) = b.upgrade() {
                    l.reply(id, &text);
                }
            }),
            resolve: Box::new(move |id, resolved| {
                if let Some(l) = c.upgrade() {
                    l.set_resolved(id, resolved);
                }
            }),
            delete: Box::new(move |id| {
                if let Some(l) = d.upgrade() {
                    l.delete(id);
                }
            }),
            cancel_reply: Box::new(move |_| {
                if let Some(l) = e.upgrade() {
                    l.activate(None, false);
                    l.view.grab_focus();
                }
            }),
        })
    }

    // --- Commands ---------------------------------------------------------

    fn visible(&self, t: &Thread) -> bool {
        t.is_open() || self.show_resolved.get()
    }

    pub fn set_show_resolved(&self, show: bool) {
        self.show_resolved.set(show);
        if !show
            && let Some(id) = self.active.get()
            && self.threads.borrow().iter().any(|t| t.thread.id == id && !t.thread.is_open())
        {
            self.active.set(None);
        }
        self.sync_cards();
        self.refresh_highlights();
        self.queue_relayout();
    }

    pub fn begin_draft(&self) {
        if let Some(d) = &*self.draft.borrow() {
            d.card.composer.focus();
            return;
        }
        if self.store().is_none() {
            return;
        }
        let range = {
            let st = self.buffer.state();
            let sel = self
                .buffer
                .selection_bytes()
                .or_else(|| edit::word_at(&st.text, &st.doc, self.buffer.cursor_byte()));
            sel.and_then(|r| edit::trim_segment(&st.text, &st.doc, r.start, r.end))
        };
        let Some(range) = range else {
            self.toast(adw::Toast::new("Select the text you want to comment on"));
            return;
        };
        let a = self.buffer.iter_at_byte(range.start);
        let b = self.buffer.iter_at_byte(range.end);
        let card = DraftCard::new();
        let this = self.rc();
        let composer = card.composer.clone();
        card.composer.submit.connect_clicked(glib::clone!(
            #[weak]
            this,
            move |_| {
                let text = composer.text();
                if !text.is_empty() {
                    this.post_draft(&text);
                }
            }
        ));
        card.composer.cancel.connect_clicked(glib::clone!(
            #[weak]
            this,
            move |_| this.cancel_draft()
        ));
        self.gutter.put(&card.root, 0.0, 0.0);
        let draft = Draft {
            start: self.buffer.create_mark(None, &a, false),
            end: self.buffer.create_mark(None, &b, true),
            hl: (
                self.buffer.create_mark(None, &a, true),
                self.buffer.create_mark(None, &b, false),
            ),
            card,
        };
        self.draft.replace(Some(draft));
        // The highlight marks what is being commented on; a selection
        // would hide it.
        self.buffer.place_cursor(&self.buffer.iter_at_byte(range.end));
        self.active.set(None);
        self.sync_cards();
        self.refresh_highlights();
        self.relayout();
        if let Some(d) = &*self.draft.borrow() {
            d.card.composer.focus();
        }
    }

    fn remove_draft(&self) {
        if let Some(d) = self.draft.take() {
            self.buffer.with_tagging(|| {
                let s = self.buffer.iter_at_mark(&d.hl.0);
                let e = self.buffer.iter_at_mark(&d.hl.1);
                self.buffer.remove_tag(&self.buffer.comment_tag(true), &s, &e);
            });
            for m in [&d.start, &d.end, &d.hl.0, &d.hl.1] {
                self.buffer.delete_mark(m);
            }
            self.gutter.remove(&d.card.root);
        }
    }

    pub fn cancel_draft(&self) {
        self.remove_draft();
        self.refresh_highlights();
        self.queue_relayout();
        self.view.grab_focus();
    }

    pub fn post_draft(&self, body: &str) {
        if let Some(f) = &*self.before_add.borrow() {
            f();
        }
        let range = {
            let d = self.draft.borrow();
            let Some(d) = d.as_ref() else { return };
            let a = self.buffer.byte_at(&self.buffer.iter_at_mark(&d.start));
            let b = self.buffer.byte_at(&self.buffer.iter_at_mark(&d.end));
            a..b.max(a)
        };
        let text = self.buffer.text_string();
        let body = body.to_string();
        let id = self.update_store(move |c| Ok(c.add(&text, range, &body)));
        self.remove_draft();
        if let Some(id) = id {
            self.activate(Some(id), false);
        }
        self.view.grab_focus();
    }

    pub fn reply(&self, id: u64, body: &str) {
        let body = body.to_string();
        self.update_store(move |c| c.reply(id, &body));
    }

    pub fn set_resolved(&self, id: u64, resolved: bool) {
        if self.update_store(move |c| c.set_resolved(id, resolved)).is_none() {
            return;
        }
        if resolved {
            if self.active.get() == Some(id) && !self.show_resolved.get() {
                self.activate(None, false);
            }
            let toast = adw::Toast::new("Comment resolved");
            toast.set_button_label(Some("Undo"));
            let weak = self.this.borrow().clone();
            toast.connect_button_clicked(move |_| {
                if let Some(l) = weak.upgrade() {
                    l.set_resolved(id, false);
                }
            });
            self.toast(toast);
        }
    }

    /// Resolves every open thread, with an Undo that reopens them.
    pub fn resolve_all(&self) {
        let ids: Vec<u64> = self
            .threads
            .borrow()
            .iter()
            .filter(|tu| tu.thread.is_open())
            .map(|tu| tu.thread.id)
            .collect();
        if ids.is_empty() {
            self.toast(adw::Toast::new("No open comments"));
            return;
        }
        let all = ids.clone();
        let done = self.update_store(move |c| {
            for id in &all {
                c.set_resolved(*id, true)?;
            }
            Ok(())
        });
        if done.is_none() {
            return;
        }
        if !self.show_resolved.get() {
            self.activate(None, false);
        }
        let toast = adw::Toast::new(&match ids.len() {
            1 => "Resolved 1 comment".to_string(),
            n => format!("Resolved {n} comments"),
        });
        toast.set_button_label(Some("Undo"));
        let weak = self.this.borrow().clone();
        toast.connect_button_clicked(move |_| {
            if let Some(l) = weak.upgrade() {
                let ids = ids.clone();
                l.update_store(move |c| {
                    for id in &ids {
                        c.set_resolved(*id, false)?;
                    }
                    Ok(())
                });
            }
        });
        self.toast(toast);
    }

    pub fn delete(&self, id: u64) {
        let Some(mut old) = self
            .threads
            .borrow()
            .iter()
            .find(|t| t.thread.id == id)
            .map(|t| {
                let mut th = t.thread.clone();
                if let Some(r) = self.anchor_of(t) {
                    th.anchor.start = r.start;
                    th.anchor.end = r.end;
                }
                th
            })
        else {
            return;
        };
        // Undo re-adds the thread against the text as it is now.
        let text = self.buffer.text_string();
        old.anchor.quote = text
            .get(old.anchor.start..old.anchor.end)
            .map_or(old.anchor.quote.clone(), str::to_string);
        if self.update_store(move |c| c.delete(id)).is_none() {
            return;
        }
        if self.active.get() == Some(id) {
            self.active.set(None);
        }
        let toast = adw::Toast::new("Comment deleted");
        toast.set_button_label(Some("Undo"));
        let weak = self.this.borrow().clone();
        toast.connect_button_clicked(move |_| {
            if let Some(l) = weak.upgrade() {
                let old = old.clone();
                let text = l.buffer.text_string();
                l.update_store(move |c| {
                    c.sync(&text);
                    if c.thread(old.id).is_none() {
                        c.threads.push(old);
                        c.threads.sort_by_key(|t| t.id);
                    }
                    Ok(())
                });
            }
        });
        self.toast(toast);
    }

    /// Focuses a thread: its card moves next to its text and its highlight
    /// darkens. With `scroll`, the text is scrolled into view.
    pub fn activate(&self, id: Option<u64>, scroll: bool) {
        if id.is_some() && self.draft.borrow().is_some() {
            self.remove_draft();
        }
        let changed = self.active.get() != id;
        self.active.set(id);
        self.sync_cards();
        self.refresh_highlights();
        self.relayout();
        if changed {
            self.notify();
        }
        if scroll
            && let Some(id) = id
            && let Some(tu) = self.threads.borrow().iter().find(|t| t.thread.id == id)
        {
            self.view.scroll_to_mark(&tu.start, 0.15, false, 0.0, 0.0);
        }
    }

    /// Moves focus to the next (or previous) visible thread in text order.
    pub fn step(&self, forward: bool) {
        let order: Vec<(i32, u64)> = {
            let threads = self.threads.borrow();
            let mut v: Vec<(i32, u64)> = threads
                .iter()
                .filter(|t| self.visible(&t.thread))
                .map(|t| (self.buffer.iter_at_mark(&t.start).offset(), t.thread.id))
                .collect();
            v.sort();
            v
        };
        if order.is_empty() {
            return;
        }
        let here = match self.active.get() {
            Some(id) => order.iter().position(|(_, i)| *i == id),
            None => None,
        };
        let cursor = self.buffer.iter_at_mark(&self.buffer.get_insert()).offset();
        let next = match (here, forward) {
            (Some(i), true) => (i + 1) % order.len(),
            (Some(i), false) => (i + order.len() - 1) % order.len(),
            (None, true) => order.iter().position(|(o, _)| *o > cursor).unwrap_or(0),
            (None, false) => order.iter().rposition(|(o, _)| *o < cursor).unwrap_or(order.len() - 1),
        };
        self.activate(Some(order[next].1), true);
    }

    /// The text box of the comment being written, or of the focused
    /// thread's reply.
    pub fn composer(&self) -> Option<Rc<super::card::Composer>> {
        if let Some(d) = &*self.draft.borrow() {
            return Some(d.card.composer.clone());
        }
        let id = self.active.get()?;
        self.threads
            .borrow()
            .iter()
            .find(|t| t.thread.id == id)
            .map(|t| t.card.composer.clone())
    }

    /// Widgets the test script can point the mouse at: `add` (the add
    /// comment button), `card ID`, `resolve ID`, `composer`.
    pub fn test_widget(&self, what: &str) -> Option<gtk::Widget> {
        let (kind, arg) = what.split_once(' ').unwrap_or((what, ""));
        let id: u64 = arg.trim().parse().unwrap_or(0);
        let threads = self.threads.borrow();
        let card = || threads.iter().find(|t| t.thread.id == id).map(|t| t.card.clone());
        match kind {
            "add" => Some(self.add_button.clone().upcast()),
            "card" => card().map(|c| c.root.clone().upcast()),
            "resolve" => card().and_then(|c| c.resolve_button()),
            "composer" => self.composer().map(|c| c.text_view.clone().upcast()),
            _ => None,
        }
    }

    /// Starts a reply on the focused thread.
    pub fn focus_reply(&self) {
        if let Some(id) = self.active.get()
            && let Some(tu) = self.threads.borrow().iter().find(|t| t.thread.id == id)
            && tu.thread.is_open()
        {
            tu.card.composer.focus();
        }
    }

    /// Escape: leave the focused thread or drop the draft.
    pub fn escape(&self) -> bool {
        if self.draft.borrow().is_some() {
            self.cancel_draft();
            return true;
        }
        if self.active.get().is_some() {
            self.activate(None, false);
            self.view.grab_focus();
            return true;
        }
        false
    }

    fn on_cursor_moved(&self) {
        self.queue_relayout();
        if self.buffer.has_selection() || self.draft.borrow().is_some() {
            return;
        }
        let c = self.buffer.iter_at_mark(&self.buffer.get_insert()).offset();
        let hit = self
            .threads
            .borrow()
            .iter()
            .filter(|t| self.visible(&t.thread) && !t.thread.anchor.detached)
            .filter_map(|t| {
                let s = self.buffer.iter_at_mark(&t.start).offset();
                let e = self.buffer.iter_at_mark(&t.end).offset();
                (s < e && s <= c && c <= e).then_some((e - s, t.thread.id))
            })
            .min()
            .map(|(_, id)| id);
        match hit {
            Some(id) if self.active.get() != Some(id) => self.activate(Some(id), false),
            None if self.active.get().is_some() => {
                let focused = self
                    .threads
                    .borrow()
                    .iter()
                    .any(|t| Some(t.thread.id) == self.active.get() && t.card.has_focus());
                if !focused {
                    self.activate(None, false);
                }
            }
            _ => {}
        }
    }

    // --- Presentation -----------------------------------------------------

    fn sync_cards(&self) {
        let active = self.active.get();
        for tu in self.threads.borrow().iter() {
            tu.card.root.set_visible(self.visible(&tu.thread));
            tu.card.set_active(Some(tu.thread.id) == active);
        }
    }

    fn refresh_highlights(&self) {
        let buf = &self.buffer;
        let normal = buf.comment_tag(false);
        let strong = buf.comment_tag(true);
        let active = self.active.get();
        buf.with_tagging(|| {
            let threads = self.threads.borrow();
            let draft = self.draft.borrow();
            let regions = threads
                .iter()
                .map(|t| &t.hl)
                .chain(draft.iter().map(|d| &d.hl));
            for (m1, m2) in regions {
                let s = buf.iter_at_mark(m1);
                let e = buf.iter_at_mark(m2);
                buf.remove_tag(&normal, &s, &e);
                buf.remove_tag(&strong, &s, &e);
            }
            let apply = |start: &gtk::TextMark, end: &gtk::TextMark, hl: &(gtk::TextMark, gtk::TextMark), tag: Option<&gtk::TextTag>| {
                let s = buf.iter_at_mark(start);
                let e = buf.iter_at_mark(end);
                buf.move_mark(&hl.0, &s);
                buf.move_mark(&hl.1, &e);
                if let Some(tag) = tag
                    && s.offset() < e.offset()
                {
                    buf.apply_tag(tag, &s, &e);
                }
            };
            for t in threads.iter() {
                let tag = if !self.visible(&t.thread) || t.thread.anchor.detached {
                    None
                } else if Some(t.thread.id) == active {
                    Some(&strong)
                } else {
                    Some(&normal)
                };
                apply(&t.start, &t.end, &t.hl, tag);
            }
            if let Some(d) = draft.as_ref() {
                apply(&d.start, &d.end, &d.hl, Some(&strong));
            }
        });
    }

    pub fn queue_relayout(&self) {
        if self.relayout_queued.replace(true) {
            return;
        }
        let weak = self.this.borrow().clone();
        glib::idle_add_local_once(move || {
            if let Some(l) = weak.upgrade() {
                l.relayout_queued.set(false);
                l.refresh_highlights();
                l.relayout();
            }
        });
    }

    /// Places cards next to their text. The focused card (or the draft)
    /// sits exactly beside its anchor; the others stack above and below it
    /// without overlapping.
    pub fn relayout(&self) {
        let g = self.view.geometry();
        if g.width == 0 {
            return;
        }
        struct Item {
            y: i32,
            order: i32,
            h: i32,
            widget: gtk::Widget,
            focused: bool,
        }
        let mut items: Vec<Item> = Vec::new();
        let active = self.active.get();
        let width = super::view::CARD_WIDTH;
        for tu in self.threads.borrow().iter() {
            if !self.visible(&tu.thread) {
                continue;
            }
            let it = self.buffer.iter_at_mark(&tu.start);
            let y = self.view.iter_location(&it).y();
            let (h, _, _, _) = tu.card.root.measure(gtk::Orientation::Vertical, width);
            items.push(Item {
                y,
                order: it.offset(),
                h,
                widget: tu.card.root.clone().upcast(),
                focused: Some(tu.thread.id) == active,
            });
        }
        if let Some(d) = &*self.draft.borrow() {
            let it = self.buffer.iter_at_mark(&d.start);
            let (h, _, _, _) = d.card.root.measure(gtk::Orientation::Vertical, width);
            items.push(Item {
                y: self.view.iter_location(&it).y(),
                order: it.offset(),
                h,
                widget: d.card.root.clone().upcast(),
                focused: true,
            });
        }
        items.sort_by_key(|i| (i.y, i.order));
        let n = items.len();
        let mut ys = vec![0; n];
        if let Some(a) = items.iter().position(|i| i.focused) {
            ys[a] = items[a].y;
            for i in a + 1..n {
                ys[i] = items[i].y.max(ys[i - 1] + items[i - 1].h + CARD_GAP);
            }
            for i in (0..a).rev() {
                ys[i] = items[i].y.min(ys[i + 1] - CARD_GAP - items[i].h);
            }
        }
        // Stack top to bottom without overlaps and below the page top.
        let mut bottom = -CARD_GAP;
        for i in 0..n {
            let want = if items.iter().any(|i| i.focused) { ys[i] } else { items[i].y };
            ys[i] = want.max(bottom + CARD_GAP).max(0);
            bottom = ys[i] + items[i].h;
        }
        self.view.move_overlay(&self.gutter, g.gutter_x - ACTIVE_SHIFT, 0);
        for (i, item) in items.iter().enumerate() {
            let x = if item.focused { 0 } else { ACTIVE_SHIFT };
            self.gutter.move_(&item.widget, x as f64, ys[i] as f64);
        }
        // Size the gutter explicitly: cards wrap at a fixed width, which
        // the text view's overlay allocation does not account for.
        let extent = items
            .iter()
            .zip(&ys)
            .map(|(it, y)| y + it.h)
            .max()
            .unwrap_or(0);
        let sel_y = self
            .buffer
            .selection_bounds()
            .map_or(0, |(a, _)| self.view.iter_location(&a).y() + 48);
        self.gutter
            .set_size_request(width + ACTIVE_SHIFT, extent.max(sel_y) + 24);

        let show_add = self.draft.borrow().is_none() && self.store().is_some();
        match self.buffer.selection_bounds().filter(|_| show_add) {
            Some((a, b)) if a.offset() != b.offset() => {
                let top = if a.offset() < b.offset() { a } else { b };
                let y = self.view.iter_location(&top).y().max(0);
                self.add_button.set_visible(true);
                self.gutter.move_(&self.add_button, ACTIVE_SHIFT as f64, y as f64);
            }
            _ => self.add_button.set_visible(false),
        }
    }
}
