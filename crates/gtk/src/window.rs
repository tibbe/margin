//! A document window: loading and autosaving the file, following changes
//! other programs (agents) make to the file and to its comments, and the
//! menus and shortcuts.

use super::buffer::{DocBuffer, Look};
use super::comments::CommentLayer;
use super::find::FindBar;
use super::view::DocView;
use margin_core::comments::activity::{self, Change, Kind};
use margin_core::comments::handoff::{AgentState, DocAgents};
use margin_core::comments::{canonical_doc_path, data_dir, read_doc, Store};
use margin_core::file_sync::{FileSync, Loaded, Reconcile, Save};
use margin_core::md::edit::{self, BlockType};
use margin_core::md::InlineKind;
use anyhow::Result;
use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::{Cell, RefCell};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::Duration;

thread_local! {
    static WINDOWS: RefCell<Vec<Rc<DocWindow>>> = const { RefCell::new(Vec::new()) };
    /// Test runs say whether the person is looking at the window.
    static SCRIPTED_LOOKING: Cell<bool> = const { Cell::new(true) };
}

/// Test runs print notifications rather than post them.
fn scripted() -> bool {
    std::env::var_os("MARGIN_SCRIPT").is_some()
}

/// In test runs: act as if the person were (not) looking at the windows.
pub fn set_scripted_looking(looking: bool) {
    SCRIPTED_LOOKING.with(|l| l.set(looking));
}

pub fn all() -> Vec<Rc<DocWindow>> {
    WINDOWS.with(|w| w.borrow().clone())
}

pub struct DocWindow {
    pub window: adw::ApplicationWindow,
    path: RefCell<PathBuf>,
    /// An untitled document, kept in the drafts folder until saved.
    draft: Cell<bool>,
    /// The user confirmed closing an unsaved draft.
    closing: Cell<bool>,
    pub buffer: DocBuffer,
    pub view: DocView,
    pub layer: Rc<CommentLayer>,
    pub find: Rc<FindBar>,
    toasts: adw::ToastOverlay,
    title: adw::WindowTitle,
    count: gtk::Label,
    send: gtk::Button,
    working: adw::Spinner,
    /// Agents waiting on the document, or working on what was sent.
    agents: RefCell<Option<DocAgents>>,
    agent_state: Cell<AgentState>,
    agent_timer: RefCell<Option<glib::SourceId>>,
    /// The text and its file: what to save, and outside edits to take in.
    sync: RefCell<FileSync>,
    save_timer: RefCell<Option<glib::SourceId>>,
    disk_timer: RefCell<Option<glib::SourceId>>,
    store_timer: RefCell<Option<glib::SourceId>>,
    monitors: RefCell<Vec<gio::FileMonitor>>,
    loading: Cell<bool>,
    /// Agent activity notified since the person last looked.
    unseen: RefCell<Vec<Change>>,
    this: RefCell<Weak<DocWindow>>,
}

fn home_relative(p: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME")
        && let Ok(rel) = p.strip_prefix(&home)
    {
        return format!("~/{}", rel.display());
    }
    p.display().to_string()
}

fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.margin-{}", std::process::id()));
    let perms = fs::metadata(path).ok().map(|m| m.permissions());
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    if let Some(p) = perms {
        let _ = fs::set_permissions(&tmp, p);
    }
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

fn drafts_dir() -> PathBuf {
    data_dir().join("drafts")
}

/// A new untitled document in its own window. It autosaves into the drafts
/// folder until it is saved somewhere with Save As.
pub fn new_draft(app: &adw::Application, look: Look) -> Result<Rc<DocWindow>> {
    let dir = drafts_dir();
    fs::create_dir_all(&dir)?;
    let stamp = chrono::Local::now().format("%Y-%m-%d %H%M%S%.3f");
    let path = dir.join(format!("Untitled {stamp}.md"));
    fs::write(&path, "")?;
    open(app, &path, look)
}

/// Reopens drafts left behind (e.g. by a crash). Returns how many.
pub fn recover_drafts(app: &adw::Application, look: Look) -> usize {
    let Ok(entries) = fs::read_dir(drafts_dir()) else { return 0 };
    let mut n = 0;
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "md") {
            if fs::metadata(&p).is_ok_and(|m| m.len() == 0) {
                let _ = fs::remove_file(&p);
                continue;
            }
            if open(app, &p, look.clone()).is_ok() {
                n += 1;
            }
        }
    }
    n
}

/// Opens `path` in its own window, or brings its window forward.
pub fn open(app: &adw::Application, path: &Path, look: Look) -> Result<Rc<DocWindow>> {
    let path = canonical_doc_path(path)?;
    if let Some(w) = all().into_iter().find(|w| w.path() == path) {
        w.window.present();
        return Ok(w);
    }
    let loaded = Loaded::new(read_doc(&path)?);
    let text = loaded.text().to_string();

    let buffer = DocBuffer::new(look);
    buffer.set_wrap_paragraphs(super::settings::get().wrap_paragraphs);
    let view = DocView::new(&buffer);
    let layer = CommentLayer::new(&view);
    let find = FindBar::new(&view);
    let draft = path.starts_with(drafts_dir());

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .default_width(1340)
        .default_height(920)
        .build();
    window.add_css_class("margin-window");

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let title = adw::WindowTitle::new(
        &file_name,
        &path.parent().map(home_relative).unwrap_or_default(),
    );
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&title));
    let menu_button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&main_menu())
        .tooltip_text("Menu (F10)")
        .primary(true)
        .build();
    if let Some(p) = menu_button.popover() {
        p.set_has_arrow(false);
    }
    header.pack_end(&menu_button);
    let add = gtk::Button::from_icon_name("chat-message-new-symbolic");
    add.set_action_name(Some("win.add-comment"));
    add.set_tooltip_text(Some("Comment on selection (Ctrl+Alt+M)"));
    header.pack_end(&add);
    let send = gtk::Button::from_icon_name("mail-send-symbolic");
    send.set_action_name(Some("win.send-to-agent"));
    header.pack_end(&send);
    let working = adw::Spinner::new();
    working.set_tooltip_text(Some("The agent is working on the comments you sent"));
    working.update_property(&[gtk::accessible::Property::Label("Agent working")]);
    working.set_visible(false);
    header.pack_end(&working);
    let count = gtk::Label::new(None);
    count.add_css_class("comment-count");
    header.pack_end(&count);

    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&view)
        .build();
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&scroller));
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.add_top_bar(&find.bar);
    toolbar.set_content(Some(&toasts));
    window.set_content(Some(&toolbar));

    let win = Rc::new(DocWindow {
        window,
        path: RefCell::new(path.clone()),
        draft: Cell::new(draft),
        closing: Cell::new(false),
        buffer: buffer.clone(),
        view: view.clone(),
        layer: layer.clone(),
        find,
        toasts,
        title,
        count,
        send,
        working,
        agents: RefCell::new(None),
        agent_state: Cell::new(AgentState::None),
        agent_timer: RefCell::new(None),
        sync: RefCell::new(FileSync::new(&text, loaded.crlf())),
        save_timer: RefCell::new(None),
        disk_timer: RefCell::new(None),
        store_timer: RefCell::new(None),
        monitors: RefCell::new(Vec::new()),
        loading: Cell::new(true),
        unseen: RefCell::new(Vec::new()),
        this: RefCell::new(Weak::new()),
    });
    win.this.replace(Rc::downgrade(&win));

    buffer.set_contents(&text);
    win.loading.set(false);

    let weak = Rc::downgrade(&win);
    layer.set_toaster(move |t| {
        if let Some(w) = weak.upgrade() {
            w.toasts.add_toast(t);
        }
    });
    let weak = Rc::downgrade(&win);
    layer.set_on_activity(move |changes| {
        if let Some(w) = weak.upgrade() {
            if let Some(a) = w.agents.borrow_mut().as_mut() {
                a.activity(now_ms());
            }
            w.notify_activity(changes);
        }
    });
    let weak = Rc::downgrade(&win);
    layer.set_before_add(move || {
        if let Some(w) = weak.upgrade() {
            w.save();
        }
    });
    let weak = Rc::downgrade(&win);
    layer.connect_changed(move || {
        if let Some(w) = weak.upgrade() {
            w.update_title();
        }
    });
    match Store::for_doc(&path) {
        Ok(store) => layer.attach(store),
        Err(e) => win.toast(&format!("Comments unavailable: {e:#}")),
    }

    win.connect_signals();
    win.install_actions();
    win.watch();
    win.follow_agents();
    win.update_title();
    WINDOWS.with(|w| w.borrow_mut().push(win.clone()));
    win.window.present();
    view.grab_focus();
    Ok(win)
}

impl DocWindow {
    pub fn path(&self) -> PathBuf {
        self.path.borrow().clone()
    }

    fn rc(&self) -> Rc<DocWindow> {
        self.this.borrow().upgrade().expect("window alive")
    }

    pub fn this(&self) -> Rc<DocWindow> {
        self.rc()
    }

    fn weak(&self) -> Weak<DocWindow> {
        self.this.borrow().clone()
    }

    pub fn toast(&self, text: &str) {
        self.toasts.add_toast(adw::Toast::new(text));
    }

    pub fn is_draft(&self) -> bool {
        self.draft.get()
    }

    fn dirty(&self) -> bool {
        !self.sync.borrow().has(&self.buffer.text_string())
    }

    pub fn display_name(&self) -> String {
        if self.draft.get() {
            return "Untitled".into();
        }
        self.path()
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn update_title(&self) {
        let name = self.display_name();
        // For the window manager: task switchers and the bar show it.
        self.window.set_title(Some(&format!("{name} – Margin")));
        self.title
            .set_title(&if self.dirty() { format!("• {name}") } else { name });
        self.title.set_subtitle(&if self.draft.get() {
            "Not saved yet".to_string()
        } else {
            self.path().parent().map(home_relative).unwrap_or_default()
        });
        let open = self.layer.open_count();
        let resolved = self.layer.resolved_count();
        let label = match (open, resolved) {
            (0, 0) => String::new(),
            (0, r) => format!("{r} resolved"),
            (1, _) => "1 open comment".into(),
            (n, _) => format!("{n} open comments"),
        };
        self.count.set_label(&label);
        self.count.set_visible(!label.is_empty());
        self.show_agent();
    }

    /// Follows the agents on the document, which drafts have none of.
    fn follow_agents(&self) {
        self.agents
            .replace((!self.draft.get()).then(|| DocAgents::new(&self.path())));
        if self.agent_timer.borrow().is_none() {
            let weak = self.weak();
            let id = glib::timeout_add_local(Duration::from_secs(1), move || match weak.upgrade() {
                Some(w) => {
                    w.update_agent();
                    glib::ControlFlow::Continue
                }
                None => glib::ControlFlow::Break,
            });
            self.agent_timer.replace(Some(id));
        }
        self.update_agent();
    }

    /// Looks at the document's agents again, and shows what they are doing.
    pub fn update_agent(&self) {
        let state = match self.agents.borrow_mut().as_mut() {
            Some(a) => a.poll(now_ms()),
            None => AgentState::None,
        };
        self.agent_state.set(state);
        self.show_agent();
    }

    pub fn send_enabled(&self) -> bool {
        self.window.lookup_action("send-to-agent").is_some_and(|a| a.is_enabled())
    }

    pub fn agent_state(&self) -> AgentState {
        self.agent_state.get()
    }

    fn can_send(&self) -> bool {
        self.agent_state.get() == AgentState::Waiting && self.layer.open_count() > 0
    }

    fn show_agent(&self) {
        let state = self.agent_state.get();
        self.working.set_visible(state == AgentState::Working);
        if let Some(a) = self.window.lookup_action("send-to-agent").and_downcast::<gio::SimpleAction>() {
            a.set_enabled(self.can_send());
        }
        self.send.set_tooltip_text(Some(match state {
            AgentState::Waiting if self.layer.open_count() > 0 => "Send open comments to the agent (Ctrl+Shift+Enter)",
            AgentState::Waiting => "An agent is waiting, but there are no open comments to send",
            AgentState::Working => "The agent is working on the comments you sent",
            AgentState::None => "No agent is waiting on this document. Ask your agent to run “margin wait” on it",
        }));
    }

    /// Sends the open comments to the agents waiting on the document.
    fn send_to_agent(&self) {
        self.update_agent();
        if !self.can_send() {
            self.toast(if self.layer.open_count() == 0 { "No open comments" } else { "No agent is waiting" });
            return;
        }
        self.save();
        let sent = self.agents.borrow_mut().as_mut().map(|a| a.send(now_ms()));
        match sent {
            Some(Ok(0)) | None => self.toast("No agent is waiting"),
            Some(Ok(n)) => {
                let open = self.layer.open_count();
                let what = if open == 1 { "1 open comment".to_string() } else { format!("{open} open comments") };
                let to = if n == 1 { "the agent".to_string() } else { format!("{n} agents") };
                self.toast(&format!("Sent {what} to {to}"));
            }
            Some(Err(e)) => self.toast(&format!("Could not send: {e:#}")),
        }
        self.update_agent();
    }

    /// The window is active, so the person sees its banners.
    fn looking(&self) -> bool {
        if scripted() {
            SCRIPTED_LOOKING.with(Cell::get)
        } else {
            self.window.is_active()
        }
    }

    fn notification_id(&self) -> String {
        format!("activity:{}", self.path().display())
    }

    /// Posts agent activity as a system notification unless the window is
    /// in front: one per document, replaced with the running totals until
    /// the person comes back. GNOME Shell shows notifications even for the
    /// focused app, so the window decides.
    fn notify_activity(&self, changes: &[Change]) {
        if self.looking() {
            return;
        }
        let mut unseen = self.unseen.borrow_mut();
        unseen.extend_from_slice(changes);
        let body = match unseen.as_slice() {
            [one] => one.line(),
            all => activity::summary(all),
        };
        // A click focuses the thread when there is just one (0: none).
        let first = unseen[0].id;
        let thread = if unseen.iter().all(|c| c.id == first && c.kind != Kind::Deleted) { first } else { 0 };
        let title = self.display_name();
        if scripted() {
            println!("notification {title} | {body} | thread {thread}");
            return;
        }
        let n = gio::Notification::new(&title);
        n.set_body(Some(&body));
        let target = (self.path().to_string_lossy().into_owned(), thread).to_variant();
        n.set_default_action_and_target_value("app.show-activity", Some(&target));
        if let Some(app) = self.window.application() {
            app.send_notification(Some(&self.notification_id()), &n);
        }
    }

    /// The person sees the document again: its notification goes.
    pub fn clear_activity(&self) {
        if self.unseen.take().is_empty() {
            return;
        }
        if scripted() {
            println!("notification withdrawn for {}", self.display_name());
        } else if let Some(app) = self.window.application() {
            app.withdraw_notification(&self.notification_id());
        }
    }

    fn connect_signals(&self) {
        let weak = self.weak();
        self.buffer.connect_changed(move |_| {
            if let Some(w) = weak.upgrade()
                && !w.loading.get()
            {
                w.sync.borrow_mut().edited();
                w.update_title();
                w.schedule_save();
            }
        });
        let weak = self.weak();
        self.window.connect_is_active_notify(move |win| {
            let Some(w) = weak.upgrade() else { return };
            if !win.is_active() {
                w.save();
            } else if w.looking() {
                w.clear_activity();
            }
        });
        let weak = self.weak();
        self.window.connect_close_request(move |_| {
            if let Some(w) = weak.upgrade() {
                // Documents save themselves, so only an untitled one with
                // text, or one whose save failed, has changes to lose.
                if !w.closing.get() {
                    if w.draft.get() {
                        if w.buffer.text_string().trim().is_empty() {
                            w.discard_draft();
                        } else {
                            w.ask_unsaved();
                            return glib::Propagation::Stop;
                        }
                    } else {
                        w.save();
                        if w.dirty() {
                            w.ask_unsaved();
                            return glib::Propagation::Stop;
                        }
                    }
                }
                for m in w.monitors.borrow().iter() {
                    m.cancel();
                }
                if let Some(id) = w.agent_timer.take() {
                    id.remove();
                }
                WINDOWS.with(|ws| ws.borrow_mut().retain(|x| !Rc::ptr_eq(x, &w)));
            }
            glib::Propagation::Proceed
        });

        let keys = gtk::EventControllerKey::new();
        let weak = self.weak();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape
                && let Some(w) = weak.upgrade()
            {
                if w.layer.escape() {
                    return glib::Propagation::Stop;
                }
                if w.find.is_open() {
                    w.find.close();
                    return glib::Propagation::Stop;
                }
            }
            glib::Propagation::Proceed
        });
        self.window.add_controller(keys);

        // Ctrl+Shift+7/8/9 (numbered, bulleted, checklist) by physical key,
        // as Google Docs does: as keyvals they are Ctrl+&, Ctrl+* and
        // Ctrl+( on a US layout, and other things elsewhere.
        let physical = gtk::EventControllerKey::new();
        physical.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = self.weak();
        physical.connect_key_pressed(move |_, _, code, state| {
            use gtk::gdk::ModifierType as M;
            let mods = state & (M::CONTROL_MASK | M::SHIFT_MASK | M::ALT_MASK | M::SUPER_MASK);
            if mods != M::CONTROL_MASK | M::SHIFT_MASK {
                return glib::Propagation::Proceed;
            }
            // Hardware keycodes of the number row's 7, 8 and 9 keys.
            let action = match code {
                16 => "numbers",
                17 => "bullets",
                18 => "checklist",
                _ => return glib::Propagation::Proceed,
            };
            if let Some(w) = weak.upgrade() {
                let _ = WidgetExt::activate_action(&w.window, &format!("win.{action}"), None);
            }
            glib::Propagation::Stop
        });
        self.window.add_controller(physical);

        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::VERTICAL);
        scroll.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = self.weak();
        scroll.connect_scroll(move |c, _, dy| {
            if !c.current_event_state().contains(gtk::gdk::ModifierType::CONTROL_MASK) {
                return glib::Propagation::Proceed;
            }
            if let Some(w) = weak.upgrade() {
                w.zoom(if dy < 0.0 { 1 } else { -1 });
            }
            glib::Propagation::Stop
        });
        self.view.add_controller(scroll);

        let weak = self.weak();
        self.view.set_link_handler(move |url| {
            if let Some(w) = weak.upgrade() {
                w.follow_link(url);
            }
        });
    }

    fn follow_link(&self, url: &str) {
        let is_web = url.contains("://") && !url.starts_with("file://");
        if !is_web && !url.starts_with('#') {
            let target = url.trim_start_matches("file://");
            let target = target.split('#').next().unwrap_or(target);
            let p = if Path::new(target).is_absolute() {
                PathBuf::from(target)
            } else {
                self.path().parent().unwrap_or(Path::new("/")).join(target)
            };
            let is_md = p
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"));
            if is_md && let Some(app) = self.window.application() {
                let app = app.downcast::<adw::Application>().expect("adw app");
                if let Err(e) = open(&app, &p, self.buffer.look()) {
                    self.toast(&format!("{e:#}"));
                }
                return;
            }
            let file = gio::File::for_path(&p);
            gtk::FileLauncher::new(Some(&file)).launch(Some(&self.window), gio::Cancellable::NONE, |_| {});
            return;
        }
        gtk::UriLauncher::new(url).launch(Some(&self.window), gio::Cancellable::NONE, |_| {});
    }

    // --- Saving -------------------------------------------------------------

    fn schedule_save(&self) {
        if let Some(id) = self.save_timer.take() {
            id.remove();
        }
        let weak = self.weak();
        let id = glib::timeout_add_local_once(Duration::from_millis(700), move || {
            if let Some(w) = weak.upgrade() {
                w.save_timer.take();
                w.save();
            }
        });
        self.save_timer.replace(Some(id));
    }

    /// Writes the document if it changed, then records comment anchors
    /// against the saved text.
    pub fn save(&self) -> bool {
        if let Some(id) = self.save_timer.take() {
            id.remove();
        }
        // The file is read first: an outside edit may still be waiting for
        // the file monitor's debounce. One retry lets taking it in finish
        // saving during this call.
        for _ in 0..2 {
            let disk = match read_doc(&self.path()) {
                Ok(raw) => Loaded::new(raw),
                Err(e) => {
                    self.toast(&format!("Could not check document before saving: {e:#}"));
                    return false;
                }
            };
            let text = self.buffer.text_string();
            let step = self.sync.borrow_mut().save(&text, disk);
            match step {
                Save::Done => return true,
                Save::Blocked => return false,
                Save::Changed(r) => {
                    if !self.take_in(r) {
                        return false;
                    }
                }
                Save::Write(out) => {
                    return match atomic_write(&self.path(), out.as_bytes()) {
                        Ok(()) => {
                            self.sync.borrow_mut().wrote(&text);
                            self.buffer.set_modified(false);
                            self.update_title();
                            self.layer.persist_anchors();
                            true
                        }
                        Err(e) => {
                            self.sync.borrow_mut().failed();
                            self.toast(&format!("Could not save: {e}"));
                            false
                        }
                    };
                }
            }
        }
        false
    }

    // --- Following outside changes ------------------------------------------

    fn watch(&self) {
        let file = gio::File::for_path(self.path());
        if let Ok(m) = file.monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) {
            let weak = self.weak();
            m.connect_changed(move |_, _, _, _| {
                if let Some(w) = weak.upgrade() {
                    w.debounce(|w| &w.disk_timer, 120, |w| w.check_disk());
                }
            });
            self.monitors.borrow_mut().push(m);
        }
        if let Some(store) = self.layer.store() {
            if let Some(dir) = store.path.parent() {
                let _ = fs::create_dir_all(dir);
            }
            let file = gio::File::for_path(&store.path);
            if let Ok(m) = file.monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) {
                let weak = self.weak();
                m.connect_changed(move |_, _, _, _| {
                    if let Some(w) = weak.upgrade() {
                        w.debounce(|w| &w.store_timer, 80, |w| w.layer.reload());
                    }
                });
                self.monitors.borrow_mut().push(m);
            }
        }
    }

    fn debounce(
        &self,
        slot: fn(&DocWindow) -> &RefCell<Option<glib::SourceId>>,
        ms: u64,
        f: fn(&DocWindow),
    ) {
        if let Some(id) = slot(self).take() {
            id.remove();
        }
        let weak = self.weak();
        let id = glib::timeout_add_local_once(Duration::from_millis(ms), move || {
            if let Some(w) = weak.upgrade() {
                slot(&w).take();
                f(&w);
            }
        });
        slot(self).replace(Some(id));
    }

    /// Picks up edits made to the file by someone else.
    fn check_disk(&self) {
        let Ok(raw) = read_doc(&self.path()) else { return };
        let ours = self.buffer.text_string();
        let change = self.sync.borrow_mut().disk_changed(&ours, Loaded::new(raw));
        if let Some(r) = change {
            self.take_in(r);
        }
    }

    /// Shows what taking in a change to the file takes; false for a
    /// conflict, which waits for the person's answer.
    fn take_in(&self, r: Reconcile) -> bool {
        match r {
            Reconcile::CaughtUp => self.update_title(),
            Reconcile::Load(text) => {
                self.buffer.apply_external(&text);
                self.after_external_change();
                self.toast("Updated from disk");
            }
            Reconcile::Merge(text) => {
                self.buffer.apply_external(&text);
                self.after_external_change();
                self.schedule_save();
                self.toast("Merged changes from disk");
            }
            Reconcile::Conflict => {
                self.ask_conflict();
                return false;
            }
        }
        true
    }

    fn after_external_change(&self) {
        self.update_title();
        self.layer.persist_anchors();
        self.layer.queue_relayout();
    }

    fn ask_conflict(&self) {
        let name = self.display_name();
        let dialog = adw::AlertDialog::new(
            Some("Document Changed on Disk"),
            Some(&format!(
                "{name} was changed by another program while you had unsaved edits, and the changes overlap."
            )),
        );
        dialog.add_response("mine", "Keep My Version");
        dialog.add_response("disk", "Load Disk Version");
        dialog.set_response_appearance("disk", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("mine"));
        let weak = self.weak();
        dialog.connect_response(None, move |_, resp| {
            let Some(w) = weak.upgrade() else { return };
            if resp == "disk" {
                let theirs = w.sync.borrow_mut().load_theirs();
                if let Some(disk) = theirs {
                    w.buffer.apply_external(&disk);
                    w.after_external_change();
                    w.check_disk();
                }
            } else {
                let kept = w.sync.borrow_mut().keep_mine();
                if kept {
                    w.save();
                }
            }
        });
        dialog.present(Some(&self.window));
    }

    // --- Actions ------------------------------------------------------------

    fn install_actions(&self) {
        let add = |name: &str, f: fn(&DocWindow)| {
            let action = gio::SimpleAction::new(name, None);
            let weak = self.weak();
            action.connect_activate(move |_, _| {
                if let Some(w) = weak.upgrade() {
                    f(&w);
                }
            });
            self.window.add_action(&action);
        };
        add("bold", |w| w.inline(InlineKind::Strong));
        add("italic", |w| w.inline(InlineKind::Emphasis));
        add("strike", |w| w.inline(InlineKind::Strike));
        add("code", |w| w.inline(InlineKind::Code));
        add("paragraph", |w| w.block(BlockType::Paragraph));
        add("heading-1", |w| w.block(BlockType::Heading(1)));
        add("heading-2", |w| w.block(BlockType::Heading(2)));
        add("heading-3", |w| w.block(BlockType::Heading(3)));
        add("heading-4", |w| w.block(BlockType::Heading(4)));
        add("heading-5", |w| w.block(BlockType::Heading(5)));
        add("heading-6", |w| w.block(BlockType::Heading(6)));
        add("bullets", |w| w.block(BlockType::Bullet));
        add("numbers", |w| w.block(BlockType::Numbered));
        add("checklist", |w| w.block(BlockType::Task));
        add("quote", |w| w.block(BlockType::Quote));
        add("codeblock", |w| w.block(BlockType::Code));
        add("link", |w| w.ask_link());
        add("add-comment", |w| {
            if !w.in_gutter() {
                w.layer.begin_draft();
            }
        });
        add("copy-comments", |w| w.copy_comments());
        add("send-to-agent", |w| w.send_to_agent());
        add("resolve-all", |w| w.layer.resolve_all());
        add("next-comment", |w| w.layer.step(true));
        add("prev-comment", |w| w.layer.step(false));
        add("reply", |w| w.layer.focus_reply());
        add("save", |w| {
            if w.draft.get() {
                w.save_as();
            } else {
                w.save();
            }
        });
        add("save-as", |w| w.save_as());
        add("new", |w| {
            if let Some(app) = w.window.application().and_downcast::<adw::Application>()
                && let Err(e) = new_draft(&app, w.buffer.look())
            {
                w.toast(&format!("Could not create a document: {e:#}"));
            }
        });
        add("print", |w| {
            if let Err(e) = super::print::print(&w.window, &w.buffer, &w.display_name()) {
                w.toast(&format!("Could not print: {e}"));
            }
        });
        add("open", |w| w.choose_file());
        add("close", |w| w.window.close());
        add("shortcuts", |w| shortcuts_dialog().present(Some(&w.window)));
        add("find", |w| w.find.open(false));
        add("find-replace", |w| w.find.open(true));
        add("find-next", |w| w.find.step(true));
        add("find-prev", |w| w.find.step(false));
        add("zoom-in", |w| w.zoom(1));
        add("zoom-out", |w| w.zoom(-1));
        add("zoom-reset", |w| w.zoom(0));
        add("fullscreen", |w| {
            if w.window.is_fullscreen() {
                w.window.unfullscreen();
            } else {
                w.window.fullscreen();
            }
        });
        add("indent", |w| w.indent(false));
        add("outdent", |w| w.indent(true));
        add("open-link", |w| {
            if !w.in_gutter() {
                w.view.open_link_at_cursor();
            }
        });

        let wrap = gio::SimpleAction::new_stateful(
            "wrap-paragraphs",
            None,
            &super::settings::get().wrap_paragraphs.to_variant(),
        );
        wrap.connect_change_state(move |_, v| {
            if let Some(v) = v {
                super::set_wrap_paragraphs(v.get::<bool>().unwrap_or(false));
            }
        });
        self.window.add_action(&wrap);

        let resolved = gio::SimpleAction::new_stateful("show-resolved", None, &false.to_variant());
        let weak = self.weak();
        resolved.connect_change_state(move |a, v| {
            if let (Some(w), Some(v)) = (weak.upgrade(), v) {
                let on = v.get::<bool>().unwrap_or(false);
                a.set_state(v);
                w.layer.set_show_resolved(on);
            }
        });
        self.window.add_action(&resolved);

        let source = gio::SimpleAction::new_stateful("show-markdown", None, &false.to_variant());
        let weak = self.weak();
        source.connect_change_state(move |a, v| {
            if let (Some(w), Some(v)) = (weak.upgrade(), v) {
                let on = v.get::<bool>().unwrap_or(false);
                a.set_state(v);
                w.view.keep_cursor_still(|| w.buffer.set_source_mode(on));
                w.view.queue_draw();
                w.layer.queue_relayout();
            }
        });
        self.window.add_action(&source);
    }

    /// Whether the keyboard focus is in a comment card rather than the
    /// document: document commands must not act then.
    fn in_gutter(&self) -> bool {
        GtkWindowExt::focus(&self.window).is_some_and(|f| self.layer.contains(&f))
    }

    pub fn sync_wrap_action(&self, on: bool) {
        if let Some(a) = self.window.lookup_action("wrap-paragraphs")
            && let Ok(a) = a.downcast::<gio::SimpleAction>()
        {
            a.set_state(&on.to_variant());
        }
    }

    fn zoom(&self, step: i32) {
        super::zoom(step);
    }

    /// Copies the open comments, numbered and with line and column, to
    /// paste into a coding agent.
    fn copy_comments(&self) {
        let threads = self.layer.open_threads();
        if threads.is_empty() {
            self.toast("No open comments");
            return;
        }
        let text = margin_core::comments::export::for_agent(&self.path(), &self.buffer.text_string(), &threads);
        self.window.clipboard().set_text(&text);
        self.toast(&match threads.len() {
            1 => "Copied 1 open comment".to_string(),
            n => format!("Copied {n} open comments"),
        });
    }

    fn indent(&self, outdent: bool) {
        if self.buffer.source_mode() || self.in_gutter() {
            return;
        }
        self.buffer.run(|s, d, c, sel| edit::indent(s, d, sel.unwrap_or(c..c), outdent));
    }

    fn inline(&self, kind: InlineKind) {
        if self.buffer.source_mode() || self.in_gutter() {
            return;
        }
        self.buffer.run(|s, d, c, sel| edit::toggle_inline(s, d, sel.unwrap_or(c..c), kind));
        self.view.grab_focus();
    }

    fn block(&self, kind: BlockType) {
        if self.buffer.source_mode() || self.in_gutter() {
            return;
        }
        self.buffer.run(|s, d, c, sel| edit::set_block(s, d, sel.unwrap_or(c..c), kind));
        self.view.grab_focus();
    }

    fn ask_link(&self) {
        if self.in_gutter() {
            return;
        }
        let current = {
            let st = self.buffer.state();
            let c = self.buffer.cursor_byte();
            st.doc
                .inlines
                .iter()
                .find(|e| e.kind == InlineKind::Link && e.content().start <= c && c <= e.content().end)
                .and_then(|e| e.url.clone())
        };
        let dialog = adw::AlertDialog::new(Some(if current.is_some() { "Edit Link" } else { "Insert Link" }), None);
        let entry = gtk::Entry::builder()
            .placeholder_text("https://…")
            .activates_default(true)
            .text(current.clone().unwrap_or_default())
            .build();
        dialog.set_extra_child(Some(&entry));
        dialog.add_response("cancel", "Cancel");
        if current.is_some() {
            dialog.add_response("remove", "Remove Link");
            dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
        }
        dialog.add_response("apply", "Apply");
        dialog.set_response_appearance("apply", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("apply"));
        let sel = self.buffer.selection_bytes();
        let cursor = self.buffer.cursor_byte();
        let weak = self.weak();
        dialog.connect_response(None, glib::clone!(
            #[weak]
            entry,
            move |_, resp| {
                let Some(w) = weak.upgrade() else { return };
                match resp {
                    "apply" => {
                        let url = entry.text().trim().to_string();
                        if !url.is_empty() {
                            let range = sel.clone().unwrap_or(cursor..cursor);
                            let plan = {
                                let st = w.buffer.state();
                                edit::make_link(&st.text, &st.doc, range, &url)
                            };
                            w.buffer.apply_plan(&plan);
                        }
                    }
                    "remove" => {
                        let plan = {
                            let st = w.buffer.state();
                            edit::remove_link(&st.doc, cursor)
                        };
                        if let Some(p) = plan {
                            w.buffer.apply_plan(&p);
                        }
                    }
                    _ => {}
                }
                w.view.grab_focus();
            }
        ));
        dialog.present(Some(&self.window));
        entry.grab_focus();
    }

    fn save_as(&self) {
        let dialog = gtk::FileDialog::builder()
            .title("Save As")
            .modal(true)
            .build();
        if self.draft.get() {
            dialog.set_initial_name(Some("Untitled.md"));
        } else {
            dialog.set_initial_file(Some(&gio::File::for_path(self.path())));
        }
        let weak = self.weak();
        dialog.save(Some(&self.window), gio::Cancellable::NONE, move |res| {
            let Some(w) = weak.upgrade() else { return };
            let Some(p) = res.ok().and_then(|f| f.path()) else {
                // Cancelled: closing waits for another answer.
                w.closing.set(false);
                return;
            };
            if let Err(e) = w.move_to(&p) {
                w.toast(&format!("Could not save: {e:#}"));
            } else if w.closing.get() {
                w.window.close();
            }
        });
    }

    /// Saves the document under a new name and continues editing it there.
    /// Comments move with it; a draft's own file is removed.
    pub fn move_to(&self, new: &Path) -> Result<()> {
        // Anchors are stored against the saved text.
        if !self.save() {
            anyhow::bail!("could not save the current document before Save As");
        }
        let new = if new.extension().is_none() { new.with_extension("md") } else { new.to_path_buf() };
        if canonical_doc_path(&new)? == self.path() {
            return Ok(());
        }
        let text = self.buffer.text_string();
        let out = self.sync.borrow().file_text(&text);
        atomic_write(&new, out.as_bytes())?;
        let new = canonical_doc_path(&new)?;
        let old = self.path();
        let was_draft = self.draft.get();
        let new_store = Store::for_doc(&new)?;
        if let Some(old_store) = self.layer.store()
            && old_store.exists()
        {
            let mut comments = old_store.load()?;
            comments.sync(&text);
            comments.doc = new_store.doc.clone();
            new_store.update(|c| {
                *c = comments;
                Ok(())
            })?;
            if was_draft {
                old_store.remove()?;
            }
        } else {
            // Threads left from a file we just replaced no longer apply.
            new_store.remove()?;
        }
        if was_draft {
            let _ = fs::remove_file(&old);
        }
        for m in self.monitors.take() {
            m.cancel();
        }
        self.path.replace(new);
        self.draft.set(false);
        self.sync.borrow_mut().wrote(&text);
        self.layer.attach(new_store);
        self.watch();
        self.follow_agents();
        self.update_title();
        Ok(())
    }

    fn discard_draft(&self) {
        if let Some(store) = self.layer.store() {
            if let Err(e) = store.remove() {
                self.toast(&format!("Could not remove draft comments: {e:#}"));
            }
        }
        for m in self.monitors.take() {
            m.cancel();
        }
        let _ = fs::remove_file(self.path());
        // Nothing left to save.
        let text = self.buffer.text_string();
        self.sync.borrow_mut().wrote(&text);
    }

    /// Asks what to do with changes that are in no file yet, as Omawrite
    /// does. Saving goes through Save As: the document is untitled or its
    /// own file could not be written.
    fn ask_unsaved(&self) {
        if self.window.visible_dialog().is_some() {
            return;
        }
        let dialog = adw::AlertDialog::new(
            Some("Unsaved Changes"),
            Some(&format!("Save changes to “{}” before closing?", self.display_name())),
        );
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("discard", "Discard");
        dialog.add_response("save", "Save…");
        dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
        dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("save"));
        dialog.set_close_response("cancel");
        let weak = self.weak();
        dialog.connect_response(None, move |_, resp| {
            let Some(w) = weak.upgrade() else { return };
            match resp {
                "discard" => {
                    if w.draft.get() {
                        w.discard_draft();
                    }
                    w.closing.set(true);
                    w.window.close();
                }
                "save" => {
                    w.closing.set(true);
                    w.save_as();
                }
                _ => {}
            }
        });
        dialog.present(Some(&self.window));
    }

    fn choose_file(&self) {
        let app = self.window.application().and_downcast::<adw::Application>();
        let look = self.buffer.look();
        choose_and_open(app.as_ref(), Some(&self.window), look, Some(self.rc()));
    }
}

fn markdown_filter() -> gtk::FileFilter {
    let f = gtk::FileFilter::new();
    f.set_name(Some("Markdown"));
    f.add_suffix("md");
    f.add_suffix("markdown");
    f.add_mime_type("text/markdown");
    f
}

pub fn choose_and_open(
    app: Option<&adw::Application>,
    parent: Option<&adw::ApplicationWindow>,
    look: Look,
    near: Option<Rc<DocWindow>>,
) {
    let Some(app) = app.cloned() else { return };
    let dialog = gtk::FileDialog::builder().title("Open Markdown Document").modal(true).build();
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&markdown_filter());
    dialog.set_filters(Some(&filters));
    if let Some(w) = &near
        && !w.draft.get()
        && let Some(dir) = w.path().parent()
    {
        dialog.set_initial_folder(Some(&gio::File::for_path(dir)));
    }
    // Launched without a file there is no window yet, and an application
    // without windows exits. The portal's file chooser is another process's
    // window, so keep the application alive until it answers.
    let hold = app.hold();
    dialog.open(parent, gio::Cancellable::NONE, move |res| {
        match res {
            Ok(file) => {
                if let Some(p) = file.path()
                    && let Err(e) = open(&app, &p, look.clone())
                {
                    eprintln!("margin: {e:#}");
                }
            }
            Err(_) => {
                if app.windows().is_empty() {
                    app.quit();
                }
            }
        }
        drop(hold);
    });
}

fn main_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let file = gio::Menu::new();
    file.append(Some("New Window"), Some("win.new"));
    file.append(Some("Open…"), Some("win.open"));
    file.append(Some("Save As…"), Some("win.save-as"));
    file.append(Some("Print…"), Some("win.print"));
    menu.append_section(None, &file);
    let find = gio::Menu::new();
    find.append(Some("Find…"), Some("win.find"));
    find.append(Some("Find and Replace…"), Some("win.find-replace"));
    menu.append_section(None, &find);

    let format = gio::Menu::new();
    let text = gio::Menu::new();
    text.append(Some("Normal Text"), Some("win.paragraph"));
    text.append(Some("Heading 1"), Some("win.heading-1"));
    text.append(Some("Heading 2"), Some("win.heading-2"));
    text.append(Some("Heading 3"), Some("win.heading-3"));
    format.append_section(None, &text);
    let blocks = gio::Menu::new();
    // Matched by physical key rather than as accelerators, so the menu is
    // told what to show.
    for (label, action, accel) in [
        ("Bulleted List", "win.bullets", "<Control><Shift>8"),
        ("Numbered List", "win.numbers", "<Control><Shift>7"),
        ("Checklist", "win.checklist", "<Control><Shift>9"),
    ] {
        let item = gio::MenuItem::new(Some(label), Some(action));
        item.set_attribute_value("accel", Some(&accel.to_variant()));
        blocks.append_item(&item);
    }
    blocks.append(Some("Quote"), Some("win.quote"));
    blocks.append(Some("Code Block"), Some("win.codeblock"));
    format.append_section(None, &blocks);
    let inline = gio::Menu::new();
    inline.append(Some("Bold"), Some("win.bold"));
    inline.append(Some("Italic"), Some("win.italic"));
    inline.append(Some("Strikethrough"), Some("win.strike"));
    inline.append(Some("Inline Code"), Some("win.code"));
    inline.append(Some("Link…"), Some("win.link"));
    format.append_section(None, &inline);
    let format_section = gio::Menu::new();
    format_section.append_submenu(Some("Format"), &format);
    menu.append_section(None, &format_section);

    let comments = gio::Menu::new();
    comments.append(Some("Copy Open"), Some("win.copy-comments"));
    comments.append(Some("Send to Agent"), Some("win.send-to-agent"));
    comments.append(Some("Resolve All"), Some("win.resolve-all"));
    comments.append(Some("Show Resolved"), Some("win.show-resolved"));
    menu.append_section(None, &comments);

    let view = gio::Menu::new();
    view.append(Some("Reflow Paragraphs"), Some("win.wrap-paragraphs"));
    view.append(Some("Show Markdown"), Some("win.show-markdown"));
    let size = gio::Menu::new();
    size.append(Some("Larger"), Some("win.zoom-in"));
    size.append(Some("Smaller"), Some("win.zoom-out"));
    size.append(Some("Reset"), Some("win.zoom-reset"));
    view.append_submenu(Some("Text Size"), &size);
    view.append(Some("Fullscreen"), Some("win.fullscreen"));
    menu.append_section(None, &view);
    let help = gio::Menu::new();
    help.append(Some("Keyboard Shortcuts"), Some("win.shortcuts"));
    help.append(Some("About Margin"), Some("app.about"));
    menu.append_section(None, &help);
    menu
}

/// Application accelerators. Ctrl+Shift+7/8/9 are matched by physical key
/// in `connect_signals` instead.
pub const ACCELS: &[(&str, &[&str])] = &[
    ("win.bold", &["<Control>b"]),
    ("win.italic", &["<Control>i"]),
    ("win.strike", &["<Control><Shift>x"]),
    ("win.code", &["<Control>e"]),
    ("win.link", &["<Control>k"]),
    ("win.paragraph", &["<Control><Alt>0"]),
    ("win.heading-1", &["<Control><Alt>1"]),
    ("win.heading-2", &["<Control><Alt>2"]),
    ("win.heading-3", &["<Control><Alt>3"]),
    ("win.heading-4", &["<Control><Alt>4"]),
    ("win.heading-5", &["<Control><Alt>5"]),
    ("win.heading-6", &["<Control><Alt>6"]),
    ("win.quote", &["<Control><Alt>q"]),
    ("win.codeblock", &["<Control><Alt>c"]),
    ("win.indent", &["<Control>bracketright"]),
    ("win.outdent", &["<Control>bracketleft"]),
    ("win.open-link", &["<Alt>Return"]),
    ("win.add-comment", &["<Control><Alt>m"]),
    ("win.copy-comments", &["<Control><Shift>c"]),
    ("win.send-to-agent", &["<Control><Shift>Return"]),
    ("win.next-comment", &["<Control><Alt>Down"]),
    ("win.prev-comment", &["<Control><Alt>Up"]),
    ("win.reply", &["<Control><Alt>r"]),
    ("win.find", &["<Control>f"]),
    ("win.find-replace", &["<Control>h"]),
    ("win.find-next", &["<Control>g"]),
    ("win.find-prev", &["<Control><Shift>g"]),
    ("win.show-markdown", &["<Control>slash"]),
    ("win.wrap-paragraphs", &["<Alt>z"]),
    ("win.zoom-in", &["<Control>plus", "<Control>equal", "<Control>KP_Add"]),
    ("win.zoom-out", &["<Control>minus", "<Control>KP_Subtract"]),
    ("win.zoom-reset", &["<Control>0", "<Control>KP_0"]),
    ("win.fullscreen", &["F11"]),
    ("win.new", &["<Control>n"]),
    ("win.save", &["<Control>s"]),
    ("win.save-as", &["<Control><Shift>s"]),
    ("win.print", &["<Control>p"]),
    ("win.open", &["<Control>o"]),
    ("win.close", &["<Control>w"]),
    ("win.shortcuts", &["<Control>question"]),
    ("app.quit", &["<Control>q"]),
];

fn shortcuts_dialog() -> adw::ShortcutsDialog {
    let dialog = adw::ShortcutsDialog::new();
    let section = |title: &str, items: &[(&str, &str)]| {
        let s = adw::ShortcutsSection::new(Some(title));
        for (t, a) in items {
            s.add(adw::ShortcutsItem::new(t, a));
        }
        dialog.add(s);
    };
    section(
        "General",
        &[
            ("Keyboard shortcuts", "<Control>question"),
            ("Main menu", "F10"),
            ("New window", "<Control>n"),
            ("Open", "<Control>o"),
            ("Save now (it autosaves anyway)", "<Control>s"),
            ("Save as", "<Control><Shift>s"),
            ("Print", "<Control>p"),
            ("Fullscreen", "F11"),
            ("Close window", "<Control>w"),
            ("Quit", "<Control>q"),
        ],
    );
    section(
        "Find",
        &[
            ("Find", "<Control>f"),
            ("Find and replace", "<Control>h"),
            ("Next match", "<Control>g"),
            ("Previous match", "<Control><Shift>g"),
        ],
    );
    section(
        "View",
        &[
            ("Larger text", "<Control>plus"),
            ("Smaller text", "<Control>minus"),
            ("Reset text size", "<Control>0"),
            ("Show Markdown source", "<Control>slash"),
            ("Reflow paragraphs (join the file's line breaks)", "<Alt>z"),
        ],
    );
    section(
        "Editing",
        &[
            ("Undo", "<Control>z"),
            ("Redo", "<Control><Shift>z"),
            ("Cut, copy, paste", "<Control>x <Control>c <Control>v"),
            ("Select all", "<Control>a"),
            ("Emoji", "<Control>period"),
            ("Line break in paragraph", "<Shift>Return"),
            ("Indent list item", "Tab <Control>bracketright"),
            ("Outdent list item", "<Shift>Tab <Control>bracketleft"),
            ("Check or uncheck task", "<Control>Return"),
            ("Open link", "<Alt>Return"),
            ("Move focus out of the document", "<Control>Tab"),
        ],
    );
    section(
        "Formatting",
        &[
            ("Bold", "<Control>b"),
            ("Italic", "<Control>i"),
            ("Strikethrough", "<Control><Shift>x"),
            ("Inline code", "<Control>e"),
            ("Link", "<Control>k"),
            ("Normal text", "<Control><Alt>0"),
            ("Heading 1–6", "<Control><Alt>1...6"),
            ("Numbered list", "<Control><Shift>7"),
            ("Bulleted list", "<Control><Shift>8"),
            ("Checklist", "<Control><Shift>9"),
            ("Quote", "<Control><Alt>q"),
            ("Code block", "<Control><Alt>c"),
        ],
    );
    section(
        "Comments",
        &[
            ("Comment on selection", "<Control><Alt>m"),
            ("Copy open comments for an agent", "<Control><Shift>c"),
            ("Send open comments to the waiting agent", "<Control><Shift>Return"),
            ("Next comment", "<Control><Alt>Down"),
            ("Previous comment", "<Control><Alt>Up"),
            ("Reply to focused comment", "<Control><Alt>r"),
            ("Post comment or reply", "<Control>Return"),
            ("Close comment", "Escape"),
        ],
    );
    dialog
}

/// Milliseconds on a clock that only goes forward.
fn now_ms() -> i64 {
    glib::monotonic_time() / 1000
}
