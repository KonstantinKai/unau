//! The theme picker: every theme, previewed live as the cursor moves, and
//! the pick written to the user config so it sticks.

use super::{AppState, Effect};
use crate::{
    config::{Key, KeyCodeKey, ThemeMode, template},
    theme::{self, BUILTIN, Theme},
};
use anyhow::{Context, Result};
use std::{fs, path::Path};

#[derive(Clone, Debug, PartialEq)]
pub struct PickerEntry {
    pub name: String,
    pub dark: bool,
    pub theme: Theme,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThemePicker {
    /// Dark themes, then light ones, each group by name.
    pub entries: Vec<PickerEntry>,
    pub cursor: usize,
    /// The theme in effect when the picker opened, restored on cancel.
    pub before: Theme,
}

impl ThemePicker {
    /// Rows as the popup draws them: a group title before the first theme of
    /// each kind, then `Some(entry index)` per theme.
    pub fn rows(&self) -> Vec<(Option<&'static str>, Option<usize>)> {
        let mut rows = Vec::new();
        for (i, entry) in self.entries.iter().enumerate() {
            if i == 0 || self.entries[i - 1].dark != entry.dark {
                rows.push((Some(if entry.dark { "dark" } else { "light" }), None));
            }
            rows.push((None, Some(i)));
        }
        rows
    }
}

/// Every theme a person can pick: the built-in ones and those in
/// `themes_dir`, a user theme replacing a built-in of its name. A user theme
/// that does not load is left out and named in the returned errors.
pub fn picker_entries(themes_dir: Option<&Path>) -> (Vec<PickerEntry>, Vec<String>) {
    let mut names: Vec<String> = BUILTIN.iter().map(|t| t.name.to_string()).collect();
    if let Some(dir) = themes_dir {
        for name in theme::user_theme_names(dir) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    for name in names {
        match theme::resolve(&name, themes_dir) {
            Ok(theme) => entries.push(PickerEntry {
                dark: theme.is_dark(),
                name,
                theme,
            }),
            Err(err) => errors.push(format!("{err:#}")),
        }
    }
    entries.sort_by(|a, b| b.dark.cmp(&a.dark).then_with(|| a.name.cmp(&b.name)));
    (entries, errors)
}

impl AppState {
    fn themes_dir(&self) -> Option<std::path::PathBuf> {
        self.config_files
            .user
            .as_deref()
            .and_then(Path::parent)
            .map(|dir| dir.join("themes"))
    }

    /// `T`: lists every theme with the cursor on the one in effect.
    pub fn open_theme_picker(&mut self) {
        let (entries, errors) = picker_entries(self.themes_dir().as_deref());
        if let Some(first) = errors.first() {
            self.error(format!("theme left out of the picker: {first}"));
        }
        let before = self.layout.theme.clone();
        let cursor = entries
            .iter()
            .position(|e| e.name == before.name)
            .unwrap_or(0);
        self.theme_picker = Some(ThemePicker {
            entries,
            cursor,
            before,
        });
    }

    /// Keys while the picker is open: move and preview, save, or cancel.
    pub fn theme_picker_key(&mut self, key: Key) -> Effect {
        let Some(picker) = &mut self.theme_picker else {
            return Effect::None;
        };
        let last = picker.entries.len().saturating_sub(1);
        let to = match key.code {
            KeyCodeKey::Char('j') | KeyCodeKey::Down => picker.cursor.saturating_add(1).min(last),
            KeyCodeKey::Char('k') | KeyCodeKey::Up => picker.cursor.saturating_sub(1),
            KeyCodeKey::Char('g') => 0,
            KeyCodeKey::Char('G') => last,
            KeyCodeKey::Enter => {
                self.save_picked_theme();
                return Effect::None;
            }
            KeyCodeKey::Esc | KeyCodeKey::Char('T') => {
                let before = picker.before.clone();
                self.theme_picker = None;
                self.show_theme(before);
                return Effect::None;
            }
            _ => return Effect::None,
        };
        picker.cursor = to;
        if let Some(entry) = picker.entries.get(to) {
            let mut theme = entry.theme.clone();
            for (slot, color) in &self.config.colors {
                // Slots were checked when the config was read.
                let _ = theme.colors.set(slot, *color);
            }
            self.show_theme(theme);
        }
        Effect::None
    }

    /// Puts `theme` on screen, highlighting redone in its colours.
    fn show_theme(&mut self, theme: Theme) {
        self.layout.theme = theme;
        // Cached files are highlighted in the old theme.
        self.file_cache.clear();
        self.reload_file();
    }

    fn save_picked_theme(&mut self) {
        let Some(picker) = self.theme_picker.take() else {
            return;
        };
        let Some(entry) = picker.entries.get(picker.cursor) else {
            return;
        };
        let Some(path) = self.config_files.user.clone() else {
            self.error("no config directory: set HOME or XDG_CONFIG_HOME");
            self.show_theme(picker.before);
            return;
        };
        let shown_dark = match self.config.theme_mode {
            ThemeMode::Auto => self.config_files.dark_background,
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
        };
        let mode = (entry.dark != shown_dark).then_some(entry.dark);
        if let Err(err) = save_theme(&path, &entry.name, entry.dark, mode) {
            self.error(format!("theme not saved: {err:#}"));
            self.show_theme(picker.before);
            return;
        }
        self.reload_changed();
        if self.layout.theme.name == entry.name {
            self.info(format!("theme {} saved to {}", entry.name, path.display()));
        } else {
            self.info(format!(
                "theme {} saved to {}, but this repository's .unau.toml sets [theme] and wins here",
                entry.name,
                path.display()
            ));
        }
    }
}

/// Writes `[theme] dark` or `light` = `name` into the config at `path`, and
/// `mode` when given, keeping every other line and comment as it was. A
/// missing file starts as the commented template.
pub fn save_theme(path: &Path, name: &str, dark: bool, mode: Option<bool>) -> Result<()> {
    let text = if path.is_file() {
        fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?
    } else {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        }
        template()
    };
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .with_context(|| format!("parse {}", path.display()))?;
    let table = doc
        .entry("theme")
        .or_insert_with(toml_edit::table)
        .as_table_like_mut()
        .context("[theme] is not a table")?;
    table.insert(if dark { "dark" } else { "light" }, toml_edit::value(name));
    if let Some(dark) = mode {
        table.insert(
            "mode",
            toml_edit::value(if dark { "dark" } else { "light" }),
        );
    }
    fs::write(path, doc.to_string()).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::tests::{press, state},
        git::fixture::Fixture,
    };

    #[test]
    fn saving_changes_only_the_theme_keys_and_keeps_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            "# mine\n[keys]\nquit = \"X\" # keep\n\n[theme]\nmode = \"auto\"\ndark = \"nord\"\n",
        )
        .unwrap();
        save_theme(&path, "gruvbox.dark", true, None).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("# mine") && text.contains("quit = \"X\" # keep"),
            "{text}"
        );
        assert!(
            text.contains("dark = \"gruvbox.dark\"") && !text.contains("nord"),
            "{text}"
        );
        save_theme(&path, "rosepine.dawn", false, Some(false)).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("light = \"rosepine.dawn\""), "{text}");
        assert!(text.contains("mode = \"light\""), "{text}");
    }

    #[test]
    fn a_missing_config_starts_from_the_template() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("unau/config.toml");
        save_theme(&path, "dracula", true, None).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("# unau configuration"), "{text}");
        assert!(text.contains("dark = \"dracula\""), "{text}");
        let config: toml::Table = toml::from_str(&text).unwrap();
        assert_eq!(config["theme"]["dark"].as_str(), Some("dracula"));
    }

    #[test]
    fn the_picker_groups_dark_before_light_and_previews_and_cancels() {
        let (entries, errors) = picker_entries(None);
        assert!(errors.is_empty());
        assert_eq!(entries.len(), BUILTIN.len());
        let first_light = entries.iter().position(|e| !e.dark).unwrap();
        assert!(entries[..first_light].iter().all(|e| e.dark));
        assert!(entries[first_light..].iter().all(|e| !e.dark));

        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit_all("init");
        f.write("a.txt", "two\n");
        let mut s = state(&f);
        let before = s.layout.theme.name.clone();
        press(&mut s, "T");
        assert!(s.theme_picker.is_some());
        press(&mut s, "j");
        assert_ne!(s.layout.theme.name, before, "moving previews");
        press(&mut s, "\x1b");
        assert!(s.theme_picker.is_none());
        assert_eq!(s.layout.theme.name, before, "cancel restores");
    }

    #[test]
    fn enter_saves_to_the_user_config_and_applies() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit_all("init");
        f.write("a.txt", "two\n");
        let mut s = state(&f);
        let user = f.path().join("home/unau/config.toml");
        s.config_files = crate::app::ConfigFiles::new(
            Some(user.clone()),
            f.path(),
            s.config.instructions_file(f.path()),
            true,
        );
        press(&mut s, "T");
        let picker = s.theme_picker.as_mut().unwrap();
        picker.cursor = picker
            .entries
            .iter()
            .position(|e| e.name == "nord")
            .unwrap();
        press(&mut s, "\n");
        assert!(s.theme_picker.is_none());
        assert_eq!(s.layout.theme.name, "nord");
        assert!(
            fs::read_to_string(&user)
                .unwrap()
                .contains("dark = \"nord\"")
        );
        assert!(s.message.as_ref().unwrap().text.contains("saved"));
    }
}
