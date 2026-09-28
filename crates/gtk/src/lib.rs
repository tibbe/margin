//! Margin's editor for Linux (Omarchy), with GTK 4 and libadwaita.

mod buffer;
mod card;
mod comments;
mod debug;
mod find;
mod print;
mod settings;
mod theme;
mod view;
mod window;

use buffer::Look;
use adw::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::path::PathBuf;
use std::time::Duration;
use theme::{Fonts, Palette};

pub const APP_ID: &str = "io.github.tibbe.Margin";

thread_local! {
    static LOOK: RefCell<Option<Look>> = const { RefCell::new(None) };
    static CSS: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
    static THEME_TIMER: RefCell<Option<glib::SourceId>> = const { RefCell::new(None) };
    static THEME_MONITOR: RefCell<Option<gio::FileMonitor>> = const { RefCell::new(None) };
    static INTERFACE: RefCell<Option<gio::Settings>> = const { RefCell::new(None) };
}

fn look() -> Look {
    LOOK.with(|l| l.borrow().clone()).unwrap_or_else(load_look)
}

fn load_look() -> Look {
    let mut fonts = Fonts::load();
    fonts.size *= settings::get().zoom;
    Look {
        palette: Palette::load(),
        fonts,
    }
}

const ZOOM_STEPS: &[f64] = &[0.5, 0.67, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0];

/// Makes the text of every window a step larger (`step > 0`), smaller
/// (`step < 0`) or its normal size (`0`).
pub fn zoom(step: i32) {
    let now = settings::get().zoom;
    let z = match step {
        0 => 1.0,
        s if s > 0 => ZOOM_STEPS.iter().copied().find(|&z| z > now + 0.01).unwrap_or(now),
        _ => ZOOM_STEPS.iter().rev().copied().find(|&z| z < now - 0.01).unwrap_or(now),
    };
    settings::update(|s| s.zoom = z);
    refresh_look(true);
}

/// Turns "Reflow Paragraphs" on or off in every window.
pub fn set_wrap_paragraphs(on: bool) {
    settings::update(|s| s.wrap_paragraphs = on);
    for w in window::all() {
        w.view.keep_cursor_still(|| w.buffer.set_wrap_paragraphs(on));
        w.sync_wrap_action(on);
        w.layer.queue_relayout();
    }
}

/// Loads colors and fonts and applies them everywhere, if they changed.
fn refresh_look(force: bool) {
    let style = adw::StyleManager::default();
    let omarchy = theme::omarchy_theme_dir().is_some();
    let new = load_look();
    if omarchy {
        style.set_color_scheme(if new.palette.dark {
            adw::ColorScheme::ForceDark
        } else {
            adw::ColorScheme::ForceLight
        });
    }
    let changed = LOOK.with(|l| {
        let old = l.borrow();
        old.as_ref()
            .is_none_or(|o| o.palette != new.palette || o.fonts != new.fonts)
    });
    if !changed && !force {
        return;
    }
    LOOK.with(|l| l.replace(Some(new.clone())));
    let css = theme::css(&new.palette, &new.fonts);
    CSS.with(|c| {
        let mut c = c.borrow_mut();
        let provider = c.get_or_insert_with(|| {
            let p = gtk::CssProvider::new();
            if let Some(display) = gtk::gdk::Display::default() {
                gtk::style_context_add_provider_for_display(
                    &display,
                    &p,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
            }
            p
        });
        provider.load_from_string(&css);
    });
    for w in window::all() {
        w.buffer.set_look(new.clone());
        w.view.queue_resize();
        w.view.queue_draw();
        w.layer.queue_relayout();
    }
}

fn schedule_refresh_look() {
    THEME_TIMER.with(|t| {
        if let Some(id) = t.borrow_mut().take() {
            id.remove();
        }
        let id = glib::timeout_add_local_once(Duration::from_millis(300), || {
            THEME_TIMER.with(|t| t.borrow_mut().take());
            refresh_look(false);
        });
        t.replace(Some(id));
    });
}

/// Follows Omarchy theme switches, the system dark/light preference, and
/// font and text size changes (Omarchy's text size sets GNOME's
/// text-scaling-factor, which GTK turns into its dpi).
fn watch_theme() {
    if let Some(s) = gtk::Settings::default() {
        s.connect_gtk_xft_dpi_notify(|_| schedule_refresh_look());
        s.connect_gtk_font_name_notify(|_| schedule_refresh_look());
    }
    if let Some(i) = theme::interface_settings() {
        i.connect_changed(Some("document-font-name"), |_, _| schedule_refresh_look());
        INTERFACE.with(|s| s.replace(Some(i)));
    }
    if let Some(marker) = theme::omarchy_theme_marker()
        && let Some(dir) = marker.parent()
    {
        let file = gio::File::for_path(dir);
        if let Ok(m) = file.monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE) {
            m.connect_changed(|_, _, _, _| schedule_refresh_look());
            THEME_MONITOR.with(|t| t.replace(Some(m)));
        }
    }
    adw::StyleManager::default().connect_dark_notify(|_| schedule_refresh_look());
}

fn about(app: &adw::Application) {
    let dialog = adw::AboutDialog::builder()
        .application_name("Margin")
        .application_icon(APP_ID)
        .version(env!("CARGO_PKG_VERSION"))
        .comments("Markdown documents with Google Docs-style comments that coding agents read with the margin CLI.")
        .build();
    dialog.present(app.active_window().as_ref());
}

pub fn run(files: Vec<PathBuf>) -> anyhow::Result<i32> {
    let files: Vec<PathBuf> = files
        .into_iter()
        .map(|f| std::path::absolute(&f).unwrap_or(f))
        .collect();
    let mut flags = gio::ApplicationFlags::HANDLES_OPEN;
    if std::env::var_os("MARGIN_SCRIPT").is_some() {
        // Scripted test runs must not hand their files to a running editor.
        flags |= gio::ApplicationFlags::NON_UNIQUE;
    }
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(flags)
        .build();

    app.connect_startup(|app| {
        gtk::Window::set_default_icon_name(APP_ID);
        refresh_look(true);
        for (action, accels) in window::ACCELS {
            app.set_accels_for_action(action, accels);
        }
        let quit = gio::SimpleAction::new("quit", None);
        quit.connect_activate(glib::clone!(
            #[weak]
            app,
            move |_, _| {
                // Close each window the usual way, so ones with unsaved
                // changes can ask; the app ends with its last window.
                for w in window::all() {
                    w.window.close();
                }
                if app.windows().is_empty() {
                    app.quit();
                }
            }
        ));
        app.add_action(&quit);
        let about_action = gio::SimpleAction::new("about", None);
        about_action.connect_activate(glib::clone!(
            #[weak]
            app,
            move |_, _| about(&app)
        ));
        app.add_action(&about_action);
        watch_theme();
        debug::maybe_watch_toplevels();
    });

    app.connect_activate(|app| match app.active_window() {
        Some(w) => w.present(),
        None => {
            // Untitled documents left open by a crash come back first.
            if window::recover_drafts(app, look()) == 0 {
                window::choose_and_open(Some(app), None, look(), None);
            }
        }
    });

    app.connect_open(|app, files, _| {
        for f in files {
            let Some(p) = f.path() else { continue };
            match window::open(app, &p, look()) {
                Ok(w) => debug::maybe_run_script(&w),
                Err(e) => {
                    eprintln!("margin: {e:#}");
                    if app.windows().is_empty() {
                        app.quit();
                    }
                }
            }
        }
    });

    let mut args = vec!["margin".to_string()];
    args.extend(files.iter().map(|f| f.to_string_lossy().into_owned()));
    Ok(i32::from(app.run_with_args(&args)))
}
