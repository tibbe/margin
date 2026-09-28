//! Colors and fonts. Follows the current Omarchy theme when there is one
//! (`~/.local/state/omarchy/current/theme/colors.toml`), libadwaita's
//! light/dark palette otherwise.

use gtk::{gdk, gio, pango, prelude::*};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub bg: String,
    pub fg: String,
    pub heading: String,
    pub dim: String,
    pub muted: String,
    pub accent: String,
    pub selection: String,
    pub code_bg: String,
    pub card_bg: String,
    pub highlight: String,
    pub link: String,
}

pub fn omarchy_theme_dir() -> Option<PathBuf> {
    let dir = match std::env::var_os("MARGIN_THEME_DIR") {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(std::env::var_os("HOME")?).join(".local/state/omarchy/current/theme"),
    };
    dir.join("colors.toml").exists().then_some(dir)
}

/// The file Omarchy rewrites when the theme changes.
pub fn omarchy_theme_marker() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".local/state/omarchy/current/theme.name"))
}

impl Palette {
    pub fn load() -> Palette {
        omarchy_palette().unwrap_or_else(adwaita_palette)
    }
}

fn parse_colors(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with('[') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let v = v.trim();
            let v = v.split(" #").next().unwrap_or(v).trim().trim_matches('"').trim_matches('\'');
            map.insert(k.trim().to_string(), v.to_string());
        }
    }
    map
}

fn luminance(hex: &str) -> f32 {
    match gdk::RGBA::parse(hex) {
        Ok(c) => 0.2126 * c.red() + 0.7152 * c.green() + 0.0722 * c.blue(),
        Err(_) => 0.0,
    }
}

fn omarchy_palette() -> Option<Palette> {
    let dir = omarchy_theme_dir()?;
    let map = parse_colors(&std::fs::read_to_string(dir.join("colors.toml")).ok()?);
    let get = |keys: &[&str]| keys.iter().find_map(|k| map.get(*k).cloned());
    let bg = get(&["background", "bg"])?;
    let fg = get(&["foreground", "fg"])?;
    let dark = match get(&["mode", "theme_type"]).as_deref() {
        Some("light") => false,
        Some("dark") => true,
        _ => luminance(&bg) < 0.5,
    };
    let accent = get(&["accent", "blue"]).unwrap_or_else(|| fg.clone());
    Some(Palette {
        dark,
        heading: get(&["bright_foreground", "bright_fg"]).unwrap_or_else(|| fg.clone()),
        dim: get(&["dark_foreground", "dark_fg", "muted"]).unwrap_or_else(|| fg.clone()),
        muted: get(&["muted", "dark_foreground"]).unwrap_or_else(|| fg.clone()),
        selection: get(&["selection", "selection_background", "lighter_background"])
            .unwrap_or_else(|| accent.clone()),
        code_bg: get(&["lighter_background", "dark_background"]).unwrap_or_else(|| bg.clone()),
        card_bg: get(&["lighter_background", "dark_background"]).unwrap_or_else(|| bg.clone()),
        highlight: get(&["yellow", "bright_yellow", "orange"]).unwrap_or_else(|| "#e5c07b".into()),
        link: get(&["blue", "accent"]).unwrap_or_else(|| accent.clone()),
        accent,
        bg,
        fg,
    })
}

fn adwaita_palette() -> Palette {
    let dark = adw::StyleManager::default().is_dark();
    if dark {
        Palette {
            dark,
            bg: "#1e1e1e".into(),
            fg: "#e8e8e8".into(),
            heading: "#ffffff".into(),
            dim: "#9a9a9a".into(),
            muted: "#4a4a4a".into(),
            accent: "#78aeed".into(),
            selection: "#2f4a6e".into(),
            code_bg: "#2a2a2a".into(),
            card_bg: "#2a2a2a".into(),
            highlight: "#f8e45c".into(),
            link: "#78aeed".into(),
        }
    } else {
        Palette {
            dark,
            bg: "#ffffff".into(),
            fg: "#1f1f1f".into(),
            heading: "#000000".into(),
            dim: "#6f6f6f".into(),
            muted: "#d0d0d0".into(),
            accent: "#1c71d8".into(),
            selection: "#c8ddf7".into(),
            code_bg: "#f3f3f3".into(),
            card_bg: "#fafafa".into(),
            highlight: "#f6d32d".into(),
            link: "#1c71d8".into(),
        }
    }
}

pub fn rgba(hex: &str) -> gdk::RGBA {
    gdk::RGBA::parse(hex).unwrap_or(gdk::RGBA::BLACK)
}

pub fn rgba_alpha(hex: &str, alpha: f32) -> gdk::RGBA {
    let c = rgba(hex);
    gdk::RGBA::new(c.red(), c.green(), c.blue(), alpha)
}

fn css_rgba(hex: &str, alpha: f32) -> String {
    let c = rgba(hex);
    format!(
        "rgba({},{},{},{alpha})",
        (c.red() * 255.0).round(),
        (c.green() * 255.0).round(),
        (c.blue() * 255.0).round()
    )
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fonts {
    pub body: String,
    /// Body size in points.
    pub size: f64,
    pub mono: String,
    /// The body font's natural line height (ascent plus descent), in ems.
    pub body_height: f64,
    /// The interface font family, for comment cards.
    pub ui: String,
    /// The desktop's text scaling (Omarchy's text size setting, GNOME's
    /// text-scaling-factor), which GTK applies to every font size in
    /// points. 1.0 at 96 dpi.
    pub text_scale: f64,
}

/// Line pitch of body text, in ems of the font size: the value GitHub,
/// Obsidian and Notion use for body text.
pub const BODY_LINE_PITCH: f64 = 1.5;

/// Natural line height of a font family, in ems.
fn natural_height(family: &str) -> f64 {
    let mut desc = pango::FontDescription::new();
    desc.set_family(family);
    desc.set_absolute_size(100.0 * pango::SCALE as f64);
    let ctx = pangocairo::FontMap::default().create_context();
    let m = ctx.metrics(Some(&desc), None);
    let h = (m.ascent() + m.descent()) as f64 / pango::SCALE as f64 / 100.0;
    if h > 0.5 { h } else { 1.2 }
}

impl Fonts {
    /// Pango's line-height factor that gives body text
    /// [`BODY_LINE_PITCH`]: the factor multiplies the font's natural
    /// height, not its size.
    pub fn body_line_factor(&self) -> f64 {
        BODY_LINE_PITCH / self.body_height
    }
}

impl Fonts {
    pub fn load() -> Fonts {
        let mut body = ("Adwaita Sans".to_string(), 12.0);
        // Generic monospace: fontconfig resolves it to the font `omarchy font set` picks.
        let mut mono = "monospace".to_string();
        if let Some(settings) = interface_settings()
            && let Some(f) = parse_font(&settings.string("document-font-name"))
        {
            body = f;
        }
        if let Ok(f) = std::env::var("MARGIN_FONT")
            && let Some(parsed) = parse_font(&f)
        {
            body = parsed;
        }
        if let Ok(f) = std::env::var("MARGIN_MONO_FONT")
            && let Some((family, _)) = parse_font(&f)
        {
            mono = family;
        }
        let gtk_settings = gtk::Settings::default();
        let ui = gtk_settings
            .as_ref()
            .and_then(|s| s.gtk_font_name())
            .and_then(|n| parse_font(&n))
            .map_or_else(|| body.0.clone(), |(family, _)| family);
        let dpi = gtk_settings.map_or(-1, |s| s.gtk_xft_dpi());
        Fonts {
            body_height: natural_height(&body.0),
            body: body.0,
            size: body.1,
            mono,
            ui,
            text_scale: if dpi > 0 { dpi as f64 / 1024.0 / 96.0 } else { 1.0 },
        }
    }
}

pub fn interface_settings() -> Option<gio::Settings> {
    let source = gio::SettingsSchemaSource::default()?;
    source.lookup("org.gnome.desktop.interface", true)?;
    Some(gio::Settings::new("org.gnome.desktop.interface"))
}

fn parse_font(s: &str) -> Option<(String, f64)> {
    let desc = pango::FontDescription::from_string(s);
    let family = desc.family()?.to_string();
    let size = if desc.size() > 0 {
        desc.size() as f64 / pango::SCALE as f64
    } else {
        12.0
    };
    Some((family, size))
}

/// Application CSS for a palette. Omarchy's look: square corners, no
/// shadows, 1px muted borders, the accent only on what has focus.
pub fn css(p: &Palette, f: &Fonts) -> String {
    format!(
        r#"
:root {{
  --window-bg-color: {bg};
  --window-fg-color: {fg};
  --view-bg-color: {bg};
  --view-fg-color: {fg};
  --headerbar-bg-color: {bg};
  --headerbar-fg-color: {fg};
  --headerbar-backdrop-color: {bg};
  --headerbar-shade-color: {border};
  --popover-bg-color: {card};
  --popover-fg-color: {fg};
  --dialog-bg-color: {card};
  --dialog-fg-color: {fg};
  --card-bg-color: {card};
  --card-fg-color: {fg};
  --accent-bg-color: {accent};
  --accent-fg-color: {bg};
  --accent-color: {accent};
  --window-radius: 0;
}}
window.margin-window, window.margin-window *,
popover, popover *, dialog, dialog * {{ border-radius: 0; }}
popover > contents, dialog.floating sheet {{
  box-shadow: none;
  border: 1px solid {border};
}}
popover > arrow {{ border-color: {border}; }}
window.margin-window button:not(.suggested-action):not(.destructive-action) {{ box-shadow: none; }}
window.margin-window headerbar {{ box-shadow: none; }}
window.margin-window {{ background-color: {bg}; }}
textview.margin-doc, textview.margin-doc > text {{
  background-color: {bg};
  color: {fg};
  caret-color: {accent};
  font-family: "{body}";
  font-size: {size}pt;
}}
textview.margin-doc > text > selection,
textview.margin-doc > text > selection:backdrop {{ background-color: {selection}; color: {fg}; }}
.comment-card {{
  background-color: {card};
  color: {fg};
  padding: 10px 12px 10px 12px;
  border: 1px solid {border};
  font-family: "{ui_font}";
}}
.comment-card.active {{ border-color: {accent}; }}
.comment-card.resolved {{ opacity: 0.7; }}
.comment-card.detached .quote {{ text-decoration-line: line-through; }}
.comment-card .time {{ color: {dim}; font-size: 0.85em; }}
.comment-card .quote {{ color: {dim}; font-style: italic; font-size: 0.9em; }}
.comment-card .body {{ font-size: 1em; }}
.comment-card .reply-sep {{ background-color: {border}; min-height: 1px; margin: 6px 0 6px 0; }}
.comment-card .status {{ color: {dim}; font-size: 0.85em; }}
.comment-card button.flat, .comment-card menubutton > button {{ min-height: 24px; min-width: 24px; padding: 2px; }}
.composer {{
  background-color: {bg};
  border: 1px solid {border};
  padding: 4px 6px;
}}
.composer:focus-within {{ border-color: {accent}; }}
.composer textview, .composer textview > text {{ background-color: transparent; color: {fg}; }}
.composer .placeholder {{ color: {dim}; }}
.add-comment-button {{
  background-color: {card};
  border: 1px solid {border};
  padding: 6px;
  color: {dim};
}}
.add-comment-button:hover {{ border-color: {accent}; color: {fg}; }}
.comment-count {{ color: {dim}; }}
.empty-state {{ color: {dim}; }}
searchbar > revealer > box {{ box-shadow: none; border-bottom: 1px solid {border}; }}
.find-bar entry {{ background-color: {fill}; }}
.find-count {{ color: {dim}; font-size: 0.9em; }}
"#,
        bg = p.bg,
        fg = p.fg,
        card = p.card_bg,
        accent = p.accent,
        selection = css_rgba(&p.selection, 1.0),
        border = css_rgba(&p.muted, 0.9),
        fill = css_rgba(&p.fg, 0.08),
        dim = p.dim,
        body = f.body,
        size = f.size,
        ui_font = f.ui,
    )
}
