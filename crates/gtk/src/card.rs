//! Comment cards shown in the gutter, and the text box used to write
//! comments and replies.

use margin_core::comments::{Author, Message, Status, Thread};
use chrono::{DateTime, Local, Utc};
use gtk::{gdk, glib, pango, prelude::*};
use std::cell::RefCell;
use std::rc::Rc;


/// A multi-line text box with a placeholder and submit/cancel buttons.
/// Ctrl+Enter submits; Escape cancels.
pub struct Composer {
    pub root: gtk::Box,
    pub text_view: gtk::TextView,
    pub submit: gtk::Button,
    pub cancel: gtk::Button,
    buttons: gtk::Box,
}

impl Composer {
    pub fn new(placeholder: &str, submit_label: &str, always_show_buttons: bool) -> Rc<Composer> {
        let text_view = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::WordChar)
            .accepts_tab(false)
            .top_margin(4)
            .bottom_margin(4)
            .left_margin(2)
            .right_margin(2)
            .hexpand(true)
            .build();
        let hint = gtk::Label::builder()
            .label(placeholder)
            .xalign(0.0)
            .yalign(0.0)
            .margin_top(4)
            .margin_start(2)
            .can_target(false)
            .css_classes(["placeholder"])
            .build();
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&text_view));
        overlay.add_overlay(&hint);
        let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        frame.add_css_class("composer");
        frame.append(&overlay);

        let submit = gtk::Button::with_label(submit_label);
        submit.add_css_class("suggested-action");
        submit.set_sensitive(false);
        let cancel = gtk::Button::with_label("Cancel");
        cancel.add_css_class("flat");
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        buttons.set_halign(gtk::Align::End);
        buttons.append(&cancel);
        buttons.append(&submit);
        buttons.set_visible(always_show_buttons);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.append(&frame);
        root.append(&buttons);

        text_view.buffer().connect_changed(glib::clone!(
            #[weak]
            hint,
            #[weak]
            submit,
            #[weak]
            buttons,
            move |b| {
                let empty = b.char_count() == 0;
                hint.set_visible(empty);
                submit.set_sensitive(!b.text(&b.start_iter(), &b.end_iter(), false).trim().is_empty());
                if !always_show_buttons {
                    buttons.set_visible(!empty);
                }
            }
        ));

        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(glib::clone!(
            #[weak]
            submit,
            #[weak]
            cancel,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, state| match key {
                gdk::Key::Return | gdk::Key::KP_Enter
                    if state.contains(gdk::ModifierType::CONTROL_MASK) =>
                {
                    if submit.is_sensitive() {
                        submit.emit_clicked();
                    }
                    glib::Propagation::Stop
                }
                gdk::Key::Escape => {
                    cancel.emit_clicked();
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        ));
        text_view.add_controller(keys);

        Rc::new(Composer {
            root,
            text_view,
            submit,
            cancel,
            buttons,
        })
    }

    pub fn text(&self) -> String {
        let b = self.text_view.buffer();
        b.text(&b.start_iter(), &b.end_iter(), false).trim().to_string()
    }

    pub fn set_text(&self, text: &str) {
        self.text_view.buffer().set_text(text);
    }

    pub fn clear(&self) {
        self.set_text("");
        self.buttons.set_visible(false);
    }

    pub fn focus(&self) {
        self.text_view.grab_focus();
    }

}

pub fn when(at: &DateTime<Utc>) -> String {
    let local = at.with_timezone(&Local);
    let today = Local::now().date_naive();
    let d = local.date_naive();
    if d == today {
        local.format("%H:%M").to_string()
    } else if today.signed_duration_since(d).num_days() == 1 {
        format!("Yesterday {}", local.format("%H:%M"))
    } else {
        local.format("%b %-d, %H:%M").to_string()
    }
}

fn wrap_label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::builder()
        .label(text)
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .xalign(0.0)
        .max_width_chars(1)
        .hexpand(true)
        .selectable(true)
        .build();
    for c in classes {
        l.add_css_class(c);
    }
    l
}

/// A card's first row: when it was written, then the buttons.
/// A message's header row: its author and time, then room for buttons.
fn header_row(m: Option<&Message>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let byline = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    byline.set_hexpand(true);
    if let Some(m) = m {
        let who = match m.author {
            Author::User => "You",
            Author::Agent => "Agent",
        };
        byline.append(&gtk::Label::builder().label(who).css_classes(["author"]).build());
        byline.append(
            &gtk::Label::builder()
                .label(format!(" · {}", when(&m.at)))
                .css_classes(["time"])
                .build(),
        );
    }
    row.append(&byline);
    row
}

/// What a card's buttons do.
pub struct CardActions {
    pub activate: Box<dyn Fn(u64)>,
    pub reply: Box<dyn Fn(u64, String)>,
    pub resolve: Box<dyn Fn(u64, bool)>,
    pub delete: Box<dyn Fn(u64)>,
    pub cancel_reply: Box<dyn Fn(u64)>,
}

/// A comment thread in the gutter.
pub struct Card {
    pub root: gtk::Box,
    content: gtk::Box,
    pub composer: Rc<Composer>,
    actions: Rc<CardActions>,
    active: RefCell<bool>,
    resolved: RefCell<bool>,
}

impl Card {
    pub fn new(thread: &Thread, actions: Rc<CardActions>) -> Rc<Card> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.add_css_class("comment-card");
        let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.append(&content);
        let composer = Composer::new("Reply", "Reply", false);
        composer.root.set_visible(false);
        root.append(&composer.root);

        let card = Rc::new(Card {
            root,
            content,
            composer,
            actions,
            active: RefCell::new(false),
            resolved: RefCell::new(false),
        });

        let id = thread.id;
        let click = gtk::GestureClick::new();
        click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let actions = card.actions.clone();
        click.connect_pressed(move |_, _, _, _| (actions.activate)(id));
        card.root.add_controller(click);

        let actions = card.actions.clone();
        let composer = card.composer.clone();
        card.composer.submit.connect_clicked(move |_| {
            let text = composer.text();
            if !text.is_empty() {
                composer.clear();
                (actions.reply)(id, text);
            }
        });
        let actions = card.actions.clone();
        let composer = card.composer.clone();
        card.composer.cancel.connect_clicked(move |_| {
            composer.clear();
            (actions.cancel_reply)(id);
        });

        card.update(thread);
        card
    }

    /// Rebuilds the card's contents, keeping any reply being written.
    pub fn update(&self, thread: &Thread) {
        while let Some(child) = self.content.first_child() {
            self.content.remove(&child);
        }
        let resolved = thread.status == Status::Resolved;
        self.resolved.replace(resolved);
        self.root.set_css_classes(&[]);
        self.root.add_css_class("comment-card");
        if resolved {
            self.root.add_css_class("resolved");
        }
        if thread.anchor.detached {
            self.root.add_css_class("detached");
        }
        if *self.active.borrow() {
            self.root.add_css_class("active");
        }

        let first = thread.messages.first();
        let header = header_row(first);
        let id = thread.id;
        let resolve = gtk::Button::from_icon_name(if resolved {
            "edit-undo-symbolic"
        } else {
            "object-select-symbolic"
        });
        resolve.add_css_class("flat");
        resolve.set_valign(gtk::Align::Center);
        resolve.set_tooltip_text(Some(if resolved { "Reopen" } else { "Resolve" }));
        let actions = self.actions.clone();
        resolve.connect_clicked(move |_| (actions.resolve)(id, !resolved));
        header.append(&resolve);

        let menu = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .tooltip_text("More")
            .build();
        let pop = gtk::Popover::new();
        let pop_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let delete = gtk::Button::with_label("Delete thread");
        delete.add_css_class("flat");
        let actions = self.actions.clone();
        delete.connect_clicked(glib::clone!(
            #[weak]
            pop,
            move |_| {
                pop.popdown();
                (actions.delete)(id);
            }
        ));
        pop_box.append(&delete);
        pop.set_child(Some(&pop_box));
        menu.set_popover(Some(&pop));
        header.append(&menu);
        self.content.append(&header);

        if thread.anchor.detached {
            let q = wrap_label(&format!("“{}”", thread.anchor.quote.trim()), &["quote"]);
            q.set_tooltip_text(Some("The commented text was deleted"));
            self.content.append(&q);
        }

        if let Some(m) = first {
            self.content.append(&wrap_label(&m.body, &["body"]));
        }
        for m in thread.messages.iter().skip(1) {
            let sep = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            sep.add_css_class("reply-sep");
            self.content.append(&sep);
            self.content.append(&header_row(Some(m)));
            self.content.append(&wrap_label(&m.body, &["body"]));
        }
        if resolved {
            let when_ = thread.resolved_at.as_ref().map(when).unwrap_or_default();
            self.content
                .append(&wrap_label(&format!("Resolved {when_}"), &["status"]));
        }
        self.sync_composer();
    }

    /// The resolve/reopen button in the card's header.
    pub fn resolve_button(&self) -> Option<gtk::Widget> {
        let header = self.content.first_child()?;
        let mut child = header.first_child();
        while let Some(w) = child {
            if w.is::<gtk::Button>() {
                return Some(w);
            }
            child = w.next_sibling();
        }
        None
    }

    pub fn set_active(&self, active: bool) {
        self.active.replace(active);
        if active {
            self.root.add_css_class("active");
        } else {
            self.root.remove_css_class("active");
        }
        self.sync_composer();
    }

    fn sync_composer(&self) {
        let show = *self.active.borrow() && !*self.resolved.borrow();
        self.composer.root.set_visible(show || !self.composer.text().is_empty());
    }

    pub fn has_focus(&self) -> bool {
        let Some(root) = self.root.root() else { return false };
        match root.focus() {
            Some(w) => w.is_ancestor(&self.root) || w == self.root.clone().upcast::<gtk::Widget>(),
            None => false,
        }
    }
}

/// The card for a comment being written.
pub struct DraftCard {
    pub root: gtk::Box,
    pub composer: Rc<Composer>,
}

impl DraftCard {
    pub fn new() -> DraftCard {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.add_css_class("comment-card");
        root.add_css_class("active");
        let composer = Composer::new("Comment", "Comment", true);
        root.append(&composer.root);
        DraftCard { root, composer }
    }
}
