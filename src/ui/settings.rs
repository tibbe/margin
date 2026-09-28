//! Preferences that persist between runs, in
//! `$XDG_CONFIG_HOME/margin/settings.json`.

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Show line breaks inside paragraphs as spaces, so paragraphs wrap to
    /// the window. The file keeps its line breaks.
    pub wrap_paragraphs: bool,
    /// Text size relative to the desktop's document font.
    pub zoom: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            wrap_paragraphs: false,
            zoom: 1.0,
        }
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Settings>> = const { RefCell::new(None) };
}

fn path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    base.join("margin").join("settings.json")
}

pub fn get() -> Settings {
    CURRENT.with(|c| {
        c.borrow_mut()
            .get_or_insert_with(|| {
                std::fs::read_to_string(path())
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default()
            })
            .clone()
    })
}

pub fn update(f: impl FnOnce(&mut Settings)) -> Settings {
    let mut s = get();
    f(&mut s);
    CURRENT.with(|c| c.replace(Some(s.clone())));
    // Tests and scripted runs must not change the user's preferences.
    if std::env::var_os("MARGIN_SCRIPT").is_none() {
        let p = path();
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(&s) {
            let _ = std::fs::write(&p, json + "\n");
        }
    }
    s
}
