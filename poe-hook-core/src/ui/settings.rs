//! The overlay's own user settings, kept in `logs/poe-hook/overlay.toml`. The overlay writes
//! the file itself (atomically) whenever a setting changes; `poe-hook.toml` stays the
//! deploy-written config, and a deploy never touches this file.

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Whether a slow hook shows a popup (it is logged either way).
    pub show_slow_hook_popups: bool,
    /// The page shown last (`Page`'s debug name), opened again at the next start.
    pub last_page: String,
    /// Names of the sidebar groups the user folded.
    pub sidebar_folded: Vec<String>,
    /// Replaces `overlay.scale` of `poe-hook.toml` when set (applies at the next start).
    pub scale_override: Option<f32>,
    /// Left-pane widths of the list/detail pages (draggable splitters), by page id.
    pub split_widths: std::collections::BTreeMap<String, f32>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            show_slow_hook_popups: true,
            last_page: String::new(),
            sidebar_folded: vec!["RESEARCH".to_string()],
            scale_override: None,
            split_widths: Default::default(),
        }
    }
}

impl Settings {
    /// Reads the file; defaults when it is missing or unreadable. `None` when it does not
    /// exist (the caller may migrate an older file).
    fn read(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        match toml::from_str(&text) {
            Ok(s) => Some(s),
            Err(e) => {
                log::warn!("{}: {e}; using defaults", path.display());
                Some(Self::default())
            }
        }
    }

    /// Writes the file atomically (a temporary file, then a rename).
    fn write(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }
}

fn dir() -> PathBuf {
    poe_hook_shared::exe_dir().join("logs").join("poe-hook")
}

fn path() -> PathBuf {
    dir().join("overlay.toml")
}

/// Loads `overlay.toml`; when it is missing, takes the page from the old `overlay_page.txt`
/// once (and removes that file).
fn load(path: &Path, old_page: &Path) -> Settings {
    if let Some(s) = Settings::read(path) {
        return s;
    }
    let mut s = Settings::default();
    if let Ok(old) = std::fs::read_to_string(old_page) {
        s.last_page = old.trim().to_string();
        if s.write(path).is_ok() {
            let _ = std::fs::remove_file(old_page);
        }
    }
    s
}

static SETTINGS: Mutex<Option<Settings>> = Mutex::new(None);

/// A copy of the current settings (loaded from disk on first use).
pub fn get() -> Settings {
    with(Settings::clone)
}

/// Reads the settings without copying them (per-frame callers).
fn with<T>(f: impl FnOnce(&Settings) -> T) -> T {
    let mut g = SETTINGS.lock().unwrap();
    f(g.get_or_insert_with(|| load(&path(), &dir().join("overlay_page.txt"))))
}

/// A splitter's saved left width.
pub fn split_width(id: &str) -> Option<f32> {
    with(|s| s.split_widths.get(id).copied())
}

/// Whether a sidebar group is folded.
pub fn folded(group: &str) -> bool {
    with(|s| s.sidebar_folded.iter().any(|n| n == group))
}

/// Whether a slow hook may show a popup.
pub fn show_slow_hook_popups() -> bool {
    get().show_slow_hook_popups
}

/// Changes the settings; saves the file when something changed.
pub fn update(f: impl FnOnce(&mut Settings)) {
    let mut g = SETTINGS.lock().unwrap();
    let cur = g.get_or_insert_with(|| load(&path(), &dir().join("overlay_page.txt")));
    let before = cur.clone();
    f(cur);
    if *cur != before {
        if let Err(e) = cur.write(&path()) {
            log::warn!("overlay.toml: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("poe-hook-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn defaults_show_popups_and_fold_research() {
        let s = Settings::default();
        assert!(s.show_slow_hook_popups);
        assert_eq!(s.sidebar_folded, ["RESEARCH"]);
        assert_eq!(s.scale_override, None);
    }

    #[test]
    fn settings_survive_a_write_and_read() {
        let d = temp("roundtrip");
        let p = d.join("overlay.toml");
        let s = Settings {
            show_slow_hook_popups: false,
            last_page: "Packets".into(),
            sidebar_folded: vec![],
            scale_override: Some(1.5),
            split_widths: [("packets".to_string(), 420.0)].into(),
        };
        s.write(&p).unwrap();
        assert!(!d.join("overlay.toml.tmp").exists());
        assert_eq!(Settings::read(&p), Some(s));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_partial_or_broken_file_falls_back_to_defaults() {
        let d = temp("partial");
        let p = d.join("overlay.toml");
        std::fs::write(&p, "show_slow_hook_popups = false\n").unwrap();
        let s = Settings::read(&p).unwrap();
        assert!(!s.show_slow_hook_popups);
        assert_eq!(s.sidebar_folded, ["RESEARCH"]);
        std::fs::write(&p, "this is = not [ toml").unwrap();
        assert_eq!(Settings::read(&p), Some(Settings::default()));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_old_page_file_is_taken_over_once() {
        let d = temp("migrate");
        let (p, old) = (d.join("overlay.toml"), d.join("overlay_page.txt"));
        std::fs::write(&old, "Timing\n").unwrap();
        let s = load(&p, &old);
        assert_eq!(s.last_page, "Timing");
        assert!(p.exists() && !old.exists());
        assert_eq!(load(&p, &old).last_page, "Timing");
        let _ = std::fs::remove_dir_all(&d);
    }
}
