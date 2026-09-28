//! A scripted driver for testing the editor without a person at the
//! keyboard: `MARGIN_SCRIPT=steps.txt margin doc.md`. Each line is a step:
//!
//! ```text
//! type Hello **world**     insert text character by character
//! enter | shift-enter | backspace | delete | tab | shift-tab
//! home | end | left N | right N | up N | down N | undo N | redo N
//! start | finish           cursor to start / end of document
//! find TEXT | find-before TEXT | select TEXT
//! action win.bold          activate an action
//! compose TEXT | submit    write into the comment box and post it
//! activate ID | resolve ID | reopen ID | delete-thread ID
//! locate add|card ID|resolve ID|composer|text TEXT|checkbox N|action NAME|button LABEL
//!                          write a spot's coordinates to /tmp/margin-locate
//! focus | trace-events | trace-button WHAT   print focus, events, button signals
//!
//! Real mouse clicks come from tools/run-ui-script.sh, which connects
//! tools/broadway-click.py: `sh echo "click $(cat /tmp/margin-locate)" >
//! $MARGIN_CLICK_FIFO`. Scripted steps call handlers directly and so miss
//! bugs in how GTK routes pointer events; click through the real path when
//! testing anything a mouse does.
//! size W H | wait MS | shot PATH | shot-window N PATH | dump | probe | sh CMD | quit
//! save-as PATH | print-pdf PATH | clipboard | find-state | scroll-state | scroll-top TEXT | xft-dpi N | windows | window N (steps act on window N) | close
//! ```
//!
//! Text steps accept `\n` and `\t` escapes.

use super::window::DocWindow;
use gtk::{glib, gsk, prelude::*};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

thread_local! {
    static STARTED: Cell<bool> = const { Cell::new(false) };
    static PENDING_SHOTS: Cell<u32> = const { Cell::new(0) };
    /// The window steps act on, by index in `window::all()`; the first
    /// window if unset.
    static TARGET: Cell<Option<usize>> = const { Cell::new(None) };
}

/// With `MARGIN_TOPLEVEL_SHOTS=DIR`, screenshots every mapped top-level
/// window (dialogs included) once a second, for testing flows that happen
/// before a document window exists.
pub fn maybe_watch_toplevels() {
    let Some(dir) = std::env::var_os("MARGIN_TOPLEVEL_SHOTS") else { return };
    let dir = std::path::PathBuf::from(dir);
    let tick = Cell::new(0u32);
    glib::timeout_add_local(Duration::from_secs(1), move || {
        tick.set(tick.get() + 1);
        for (i, w) in gtk::Window::list_toplevels().iter().enumerate() {
            if w.is_mapped() {
                let path = dir.join(format!("t{}-w{i}.png", tick.get()));
                screenshot(w, &path.to_string_lossy());
            }
        }
        glib::ControlFlow::Continue
    });
}

pub fn maybe_run_script(win: &Rc<DocWindow>) {
    let Ok(path) = std::env::var("MARGIN_SCRIPT") else { return };
    if STARTED.with(|s| s.replace(true)) {
        return;
    }
    let script = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("margin: script {path}: {e}");
            std::process::exit(2);
        }
    };
    let steps: Vec<String> = script
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect();
    let win = win.clone();
    glib::timeout_add_local_once(Duration::from_millis(400), move || run(win, steps, 0));
}

fn run(win: Rc<DocWindow>, steps: Vec<String>, i: usize) {
    // Let screenshots finish before moving on.
    if PENDING_SHOTS.with(Cell::get) > 0 {
        glib::timeout_add_local_once(Duration::from_millis(50), move || run(win, steps, i));
        return;
    }
    if i >= steps.len() {
        std::process::exit(0);
    }
    if std::env::var_os("MARGIN_SCRIPT_VERBOSE").is_some() {
        eprintln!("step {i}: {}", steps[i]);
    }
    let target = TARGET
        .with(Cell::get)
        .and_then(|n| super::window::all().get(n).cloned())
        .unwrap_or_else(|| win.clone());
    let delay = step(&target, &steps[i]);
    target.view.scroll_mark_onscreen(&target.buffer.get_insert());
    glib::timeout_add_local_once(Duration::from_millis(delay), move || run(win, steps, i + 1));
}

fn unescape(s: &str) -> String {
    s.replace("\\n", "\n").replace("\\t", "\t")
}

fn step(win: &DocWindow, line: &str) -> u64 {
    let (cmd, arg) = line.split_once(' ').unwrap_or((line, ""));
    let buf = &win.buffer;
    let view = &win.view;
    let n = || arg.trim().parse::<i32>().unwrap_or(1);
    match cmd {
        "type" => {
            for c in unescape(arg).chars() {
                buf.insert_interactive_at_cursor(&c.to_string(), true);
            }
        }
        "bench-type" => {
            let n: usize = arg.trim().parse().unwrap_or(100);
            let t = std::time::Instant::now();
            for i in 0..n {
                let c = if i % 7 == 6 { " " } else { "x" };
                buf.insert_interactive_at_cursor(c, true);
            }
            let ms = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
            eprintln!("bench-type: {ms:.2} ms per keystroke over {n}");
        }
        "undo" => {
            for _ in 0..n() {
                buf.undo();
            }
        }
        "redo" => {
            for _ in 0..n() {
                buf.redo();
            }
        }
        "enter" => {
            buf.insert_interactive_at_cursor("\n", true);
        }
        "shift-enter" => view.newline(true),
        "backspace" => {
            for _ in 0..n() {
                view.emit_backspace();
            }
        }
        "delete" => {
            for _ in 0..n() {
                view.emit_delete_from_cursor(gtk::DeleteType::Chars, 1);
            }
        }
        "tab" => buf.run(|s, d, c, sel| crate::md::edit::indent(s, d, sel.unwrap_or(c..c), false)),
        "shift-tab" => buf.run(|s, d, c, sel| crate::md::edit::indent(s, d, sel.unwrap_or(c..c), true)),
        "home" => view.emit_move_cursor(gtk::MovementStep::DisplayLineEnds, -1, false),
        "end" => view.emit_move_cursor(gtk::MovementStep::DisplayLineEnds, 1, false),
        "left" => view.emit_move_cursor(gtk::MovementStep::VisualPositions, -n(), false),
        "right" => view.emit_move_cursor(gtk::MovementStep::VisualPositions, n(), false),
        "up" => view.emit_move_cursor(gtk::MovementStep::DisplayLines, -n(), false),
        "down" => view.emit_move_cursor(gtk::MovementStep::DisplayLines, n(), false),
        "start" => buf.place_cursor(&buf.start_iter()),
        "finish" => buf.place_cursor(&buf.end_iter()),
        "find" | "find-before" | "select" => {
            let needle = unescape(arg);
            let text = buf.text_string();
            match text.find(&needle) {
                Some(p) => {
                    let a = buf.iter_at_byte(p);
                    let b = buf.iter_at_byte(p + needle.len());
                    match cmd {
                        "find" => buf.place_cursor(&b),
                        "find-before" => buf.place_cursor(&a),
                        _ => buf.select_range(&b, &a),
                    }
                }
                None => eprintln!("script: {needle:?} not found"),
            }
        }
        "action" => {
            let (name, _) = arg.split_once(' ').unwrap_or((arg, ""));
            let (group, action) = name.split_once('.').unwrap_or(("win", name));
            if let Err(e) = WidgetExt::activate_action(&win.window, &format!("{group}.{action}"), None) {
                eprintln!("script: action {name}: {e}");
            }
        }
        "compose" => match win.layer.composer() {
            Some(c) => {
                c.focus();
                c.set_text(&unescape(arg));
            }
            None => eprintln!("script: no comment box"),
        },
        "submit" => match win.layer.composer() {
            Some(c) => c.submit.emit_clicked(),
            None => eprintln!("script: no comment box"),
        },
        "activate" => win.layer.activate(arg.trim().parse().ok(), true),
        "locate" => {
            // Positions are only current once the next frame is laid out.
            let what = arg.trim().to_string();
            let w = win.this();
            after_paint(win.window.upcast_ref(), move || locate(&w, &what));
            return 200;
        }
        "resolve" => win.layer.set_resolved(arg.trim().parse().unwrap_or(0), true),
        "reopen" => win.layer.set_resolved(arg.trim().parse().unwrap_or(0), false),
        "delete-thread" => win.layer.delete(arg.trim().parse().unwrap_or(0)),
        "size" => {
            let mut it = arg.split_whitespace().filter_map(|x| x.parse::<i32>().ok());
            if let (Some(w), Some(h)) = (it.next(), it.next()) {
                win.window.set_default_size(w, h);
            }
            return 500;
        }
        "wait" => return arg.trim().parse().unwrap_or(200),
        "sh" => {
            let status = std::process::Command::new("sh").arg("-c").arg(arg).status();
            if !status.is_ok_and(|s| s.success()) {
                eprintln!("script: sh {arg:?} failed");
            }
        }
        "shot" => {
            screenshot_after_paint(win.window.upcast_ref(), arg.trim().to_string());
            return 300;
        }
        "probe" => {
            // Map every few pixels of the document to a text position, the
            // way clicks do; GTK aborts if that goes wrong.
            let (_, height) = view.line_yrange(&buf.end_iter());
            let (end_y, _) = view.line_yrange(&buf.end_iter());
            let g = view.geometry();
            let mut n = 0;
            for y in (0..end_y + height + 200).step_by(3) {
                for x in [0, g.left + 5, g.left + g.doc_width / 2, g.left + g.doc_width + 20] {
                    let _ = view.iter_at_location(x, y);
                    let _ = view.iter_at_position(x, y);
                    n += 1;
                }
            }
            eprintln!("probe: {n} positions ok");
        }
        "trace-events" => {
            let legacy = gtk::EventControllerLegacy::new();
            legacy.set_propagation_phase(gtk::PropagationPhase::Capture);
            legacy.connect_event(|_, e| {
                let t = e.event_type();
                if std::env::var_os("MARGIN_TRACE_MOTION").is_some() || !matches!(t, gtk::gdk::EventType::MotionNotify) {
                    let pos = e.position().unwrap_or((-1.0, -1.0));
                    let button = e.downcast_ref::<gtk::gdk::ButtonEvent>().map(|b| b.button());
                    eprintln!("event: {t:?} at {pos:?} button {button:?} state {:?}", e.modifier_state());
                }
                glib::Propagation::Proceed
            });
            win.window.add_controller(legacy);
        }
        "trace-button" => {
            let window: &gtk::Widget = win.window.upcast_ref();
            let target = match arg.trim().strip_prefix("action ") {
                Some(a) => find_by_action(window, a),
                None => win.layer.test_widget(arg.trim()),
            };
            match target.and_then(|w| w.downcast::<gtk::Button>().ok()) {
                Some(b) => {
                    b.connect_clicked(|_| eprintln!("button: clicked"));
                    let g = gtk::GestureClick::new();
                    g.set_propagation_phase(gtk::PropagationPhase::Capture);
                    g.connect_pressed(|g, n, x, y| eprintln!("button: pressed n={n} at {x},{y} seq={:?}", g.current_sequence()));
                    g.connect_released(|_, n, x, y| eprintln!("button: released n={n} at {x},{y}"));
                    g.connect_cancel(|_, _| eprintln!("button: gesture cancelled"));
                    g.connect_stopped(|_| eprintln!("button: gesture stopped"));
                    b.add_controller(g);
                    eprintln!("tracing {} (sensitive={}, mapped={})", b.type_().name(), b.is_sensitive(), b.is_mapped());
                }
                None => eprintln!("script: no button {arg:?}"),
            }
        }
        "focus" => {
            let focus = GtkWindowExt::focus(&win.window).map(|w| w.type_().name().to_string());
            eprintln!("focus: {}", focus.unwrap_or_else(|| "none".into()));
        }
        "save-as" => {
            if let Err(e) = win.move_to(std::path::Path::new(arg.trim())) {
                eprintln!("save-as: {e:#}");
            }
        }
        "print-pdf" => {
            if let Err(e) = super::print::export_pdf(buf, arg.trim()) {
                eprintln!("print-pdf: {e}");
            }
        }
        "window" => TARGET.with(|t| t.set(arg.trim().parse().ok())),
        "close" => win.window.close(),
        "shot-popover" => {
            let open = find_widget(win.window.upcast_ref(), &|w| {
                w.is::<gtk::Popover>() && w.is_visible()
            });
            match open {
                Some(p) => screenshot_after_paint(&p, arg.trim().to_string()),
                None => eprintln!("script: no open popover"),
            }
            return 300;
        }
        "clipboard" => {
            win.window.clipboard().read_text_async(gtk::gio::Cancellable::NONE, |r| match r {
                Ok(Some(t)) => println!("----- clipboard -----\n{t}----- end -----"),
                other => eprintln!("clipboard: {other:?}"),
            });
            return 300;
        }
        "scroll-state" => {
            let r = view.visible_rect();
            let (top, _) = view.line_at_y(r.y());
            let mut end = top;
            end.forward_chars(30);
            let adj = view.vadjustment().map_or(0.0, |a| a.value());
            let cursor = view.iter_location(&buf.iter_at_mark(&buf.get_insert()));
            println!(
                "scroll: value={adj:.0} cursor at {} in window, top line {:?}",
                cursor.y() - r.y(),
                buf.text(&top, &end, false).replace('\n', "⏎")
            );
        }
        "scroll-top" => {
            // Puts the cursor before TEXT and scrolls its line to the top.
            let text = buf.text_string();
            if let Some(p) = text.find(&unescape(arg)) {
                let it = buf.iter_at_byte(p);
                buf.place_cursor(&it);
                // A mark, because scrolling to an iter fails before layout.
                let mark = buf.create_mark(None, &it, true);
                view.scroll_to_mark(&mark, 0.0, true, 0.0, 0.0);
            }
            return 300;
        }
        "xft-dpi" => {
            // Stands in for a desktop text size change: 96 * 1024 is 1.0.
            if let (Some(s), Ok(v)) = (gtk::Settings::default(), arg.trim().parse::<i32>()) {
                s.set_gtk_xft_dpi(v);
            }
            return 600;
        }
        "find-state" => println!("{}", win.find.describe()),
        "windows" => {
            for (i, w) in super::window::all().iter().enumerate() {
                println!(
                    "window {i}: {} draft={} title={:?} path={}",
                    w.display_name(),
                    w.is_draft(),
                    w.window.title(),
                    w.path().display()
                );
            }
        }
        "shot-window" => {
            let (n, path) = arg.trim().split_once(' ').unwrap_or(("0", ""));
            if let Some(w) = super::window::all().get(n.parse::<usize>().unwrap_or(0)) {
                screenshot_after_paint(w.window.upcast_ref(), path.to_string());
            }
            return 300;
        }
        "dump" => {
            println!("----- buffer -----\n{}----- end -----", buf.text_string());
        }
        "quit" => {
            win.save();
            std::process::exit(0);
        }
        other => eprintln!("script: unknown step {other:?}"),
    }
    120
}

/// Writes the surface coordinates of a widget's center (or of document
/// text) to /tmp/margin-locate, for tools/broadway-click.py to click.
fn locate(win: &DocWindow, what: &str) {
    let window: &gtk::Widget = win.window.upcast_ref();
    let (sx, sy) = win.window.surface_transform();
    let buffer_point = |x: i32, y: i32| {
        let (x, y) = win.view.buffer_to_window_coords(gtk::TextWindowType::Widget, x, y);
        win.view
            .compute_point(window, &gtk::graphene::Point::new(x as f32, y as f32))
    };
    let point = if let Some(n) = what.strip_prefix("checkbox ") {
        n.trim()
            .parse()
            .ok()
            .and_then(|n| win.view.checkbox_center(n))
            .and_then(|(x, y)| buffer_point(x, y))
    } else if let Some(action) = what.strip_prefix("action ") {
        find_by_action(window, action).and_then(|w| {
            let b = w.compute_bounds(window)?;
            Some(gtk::graphene::Point::new(b.x() + b.width() / 2.0, b.y() + b.height() / 2.0))
        })
    } else if let Some(label) = what.strip_prefix("button ") {
        find_widget(window, &|w| {
            w.downcast_ref::<gtk::Button>()
                .is_some_and(|b| b.label().as_deref() == Some(label) && b.is_mapped())
        })
        .and_then(|w| {
            let b = w.compute_bounds(window)?;
            Some(gtk::graphene::Point::new(b.x() + b.width() / 2.0, b.y() + b.height() / 2.0))
        })
    } else if let Some(text) = what.strip_prefix("text ") {
        let src = win.buffer.text_string();
        src.find(text).and_then(|p| {
            let it = win.buffer.iter_at_byte(p + text.len() / 2);
            let r = win.view.iter_location(&it);
            buffer_point(r.x(), r.y() + r.height() / 2)
        })
    } else {
        win.layer.test_widget(what).and_then(|w| {
            let b = w.compute_bounds(window)?;
            Some(gtk::graphene::Point::new(
                b.x() + b.width() / 2.0,
                b.y() + b.height() / 2.0,
            ))
        })
    };
    match point {
        Some(p) => {
            let s = format!("{} {}", p.x() as f64 + sx, p.y() as f64 + sy);
            eprintln!("locate {what}: {s}");
            let _ = std::fs::write("/tmp/margin-locate", s);
        }
        None => eprintln!("script: cannot locate {what:?}"),
    }
}

fn find_widget(root: &gtk::Widget, pred: &dyn Fn(&gtk::Widget) -> bool) -> Option<gtk::Widget> {
    if pred(root) {
        return Some(root.clone());
    }
    let mut child = root.first_child();
    while let Some(c) = child {
        if let Some(w) = find_widget(&c, pred) {
            return Some(w);
        }
        child = c.next_sibling();
    }
    None
}

fn find_by_action(root: &gtk::Widget, action: &str) -> Option<gtk::Widget> {
    if let Some(a) = root.dynamic_cast_ref::<gtk::Actionable>()
        && a.action_name().is_some_and(|n| n == action)
    {
        return Some(root.clone());
    }
    let mut child = root.first_child();
    while let Some(c) = child {
        if let Some(found) = find_by_action(&c, action) {
            return Some(found);
        }
        child = c.next_sibling();
    }
    None
}

/// Renders a widget to a PNG once the next frame has been painted.
fn screenshot_after_paint(widget: &gtk::Widget, path: String) {
    let w = widget.clone();
    after_paint(widget, move || screenshot(&w, &path));
}

/// Runs `f` after the next painted frame; the script waits for it.
fn after_paint(widget: &gtk::Widget, f: impl FnOnce() + 'static) {
    let Some(clock) = widget.frame_clock() else {
        f();
        return;
    };
    PENDING_SHOTS.with(|p| p.set(p.get() + 1));
    let handler: Rc<std::cell::RefCell<Option<glib::SignalHandlerId>>> = Rc::default();
    let h = handler.clone();
    let f = std::cell::Cell::new(Some(f));
    let id = clock.connect_after_paint(move |c| {
        if let Some(id) = h.borrow_mut().take() {
            c.disconnect(id);
            if let Some(f) = f.take() {
                f();
            }
            PENDING_SHOTS.with(|p| p.set(p.get().saturating_sub(1)));
        }
    });
    handler.replace(Some(id));
    widget.queue_draw();
}

/// Renders a widget to a PNG file.
pub fn screenshot(widget: &gtk::Widget, path: &str) {
    let paintable = gtk::WidgetPaintable::new(Some(widget));
    let (w, h) = (widget.width() as f64, widget.height() as f64);
    let snap = gtk::Snapshot::new();
    paintable.snapshot(&snap, w, h);
    let node = match snap.to_node() {
        Some(n) => n,
        None => {
            // Fall back to the window's content.
            let Some(child) = widget.first_child() else {
                eprintln!("script: nothing to render");
                return;
            };
            let p = gtk::WidgetPaintable::new(Some(&child));
            let s = gtk::Snapshot::new();
            p.snapshot(&s, w, h);
            match s.to_node() {
                Some(n) => n,
                None => {
                    eprintln!("script: nothing to render");
                    return;
                }
            }
        }
    };
    let renderer = gsk::CairoRenderer::new();
    if let Err(e) = renderer.realize_for_display(&widget.display()) {
        eprintln!("script: renderer: {e}");
        return;
    }
    let texture = renderer.render_texture(&node, None);
    if let Err(e) = texture.save_to_png(path) {
        eprintln!("script: saving {path}: {e}");
    }
    renderer.unrealize();
}
