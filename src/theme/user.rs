//! Themes the user writes: a folder under `unau/themes/` holding a
//! `theme.toml` and, optionally, a `.tmTheme` for syntax colours.

use super::{Syntax, Theme, by_name, parse_hex};
use crate::diff::builtin_syntax_exists;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{
    borrow::Cow,
    collections::BTreeMap,
    path::{Component, Path},
    sync::Arc,
};
use syntect::highlighting::ThemeSet;

/// File inside a theme folder that describes the theme.
pub const THEME_FILE: &str = "theme.toml";

/// Theme a user theme starts from when it names none.
const DEFAULT_EXTENDS: &str = "default.dark";

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    extends: Option<String>,
    syntax: Option<String>,
    #[serde(default)]
    colors: BTreeMap<String, String>,
}

/// Names of the user themes in `themes_dir`: every folder holding a
/// `theme.toml`, sorted.
pub fn user_theme_names(themes_dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(themes_dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().join(THEME_FILE).is_file())
        .filter_map(|e| e.file_name().to_str().map(str::to_owned))
        .collect();
    names.sort();
    names
}

/// The theme called `name`: a user theme in `themes_dir` if one exists,
/// otherwise a built-in one. A user theme replaces a built-in of the same name.
pub fn resolve(name: &str, themes_dir: Option<&Path>) -> Result<Theme> {
    resolve_from(name, themes_dir, &mut Vec::new())
}

fn resolve_from(name: &str, themes_dir: Option<&Path>, chain: &mut Vec<String>) -> Result<Theme> {
    if chain.iter().any(|c| c == name) {
        // A user theme that extends its own name builds on the built-in it replaces.
        if let Ok(theme) = by_name(name) {
            return Ok(theme);
        }
        chain.push(name.to_owned());
        bail!("theme {} extends itself: {}", chain[0], chain.join(" → "));
    }
    let folder = themes_dir.map(|d| d.join(name));
    let Some(folder) = folder.filter(|f| f.join(THEME_FILE).is_file()) else {
        return by_name(name);
    };
    chain.push(name.to_owned());
    load(name, &folder, themes_dir, chain).with_context(|| format!("theme {name}"))
}

fn load(
    name: &str,
    folder: &Path,
    themes_dir: Option<&Path>,
    chain: &mut Vec<String>,
) -> Result<Theme> {
    let path = folder.join(THEME_FILE);
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let file: ThemeFile =
        toml::from_str(&text).with_context(|| format!("in {}", path.display()))?;
    let base = file.extends.as_deref().unwrap_or(DEFAULT_EXTENDS);
    let mut theme = resolve_from(base, themes_dir, chain)?;
    theme.name = Cow::Owned(name.to_owned());
    for (slot, value) in &file.colors {
        let color = parse_hex(value).with_context(|| format!("[colors] {slot}"))?;
        theme.colors.set(slot, color)?;
    }
    if let Some(syntax) = &file.syntax {
        theme.syntax = syntax_from(syntax, folder)?;
    }
    Ok(theme)
}

/// A `.tmTheme` file in the theme folder, or the name of a syntect theme.
fn syntax_from(value: &str, folder: &Path) -> Result<Syntax> {
    let relative = Path::new(value);
    let is_file = relative
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("tmTheme"));
    if !is_file {
        if builtin_syntax_exists(value) {
            return Ok(Syntax::Builtin(Cow::Owned(value.to_owned())));
        }
        bail!("syntax {value} is neither a .tmTheme file nor a built-in syntax theme");
    }
    if !relative
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
    {
        bail!("syntax {value} must be a path inside the theme folder");
    }
    let full = folder.join(relative);
    let loaded = ThemeSet::get_theme(&full).with_context(|| format!("load {}", full.display()))?;
    Ok(Syntax::Loaded(Arc::new(loaded)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;
    use std::fs;

    fn theme(dir: &Path, name: &str, toml: &str) {
        fs::create_dir_all(dir.join(name)).unwrap();
        fs::write(dir.join(name).join(THEME_FILE), toml).unwrap();
    }

    fn error(dir: &Path, name: &str) -> String {
        format!("{:#}", resolve(name, Some(dir)).unwrap_err())
    }

    #[test]
    fn user_theme_extends_a_builtin_and_changes_only_its_own_slots() {
        let dir = tempfile::tempdir().unwrap();
        theme(
            dir.path(),
            "mine",
            "extends = \"catppuccin.latte\"\n[colors]\nborder_active = \"#010203\"\n",
        );
        let mine = resolve("mine", Some(dir.path())).unwrap();
        let latte = by_name("catppuccin.latte").unwrap();
        assert_eq!(mine.name, "mine");
        assert_eq!(mine.colors.border_active, Color::Rgb(1, 2, 3));
        assert_eq!(mine.colors.text, latte.colors.text);
        assert_eq!(mine.syntax, latte.syntax);
    }

    #[test]
    fn user_themes_can_extend_each_other_and_cycles_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        theme(dir.path(), "base", "[colors]\ntext = \"#111111\"\n");
        theme(
            dir.path(),
            "leaf",
            "extends = \"base\"\n[colors]\nmuted = \"#222222\"\n",
        );
        let leaf = resolve("leaf", Some(dir.path())).unwrap();
        assert_eq!(leaf.colors.text, Color::Rgb(0x11, 0x11, 0x11));
        assert_eq!(leaf.colors.muted, Color::Rgb(0x22, 0x22, 0x22));
        theme(dir.path(), "a", "extends = \"b\"\n");
        theme(dir.path(), "b", "extends = \"a\"\n");
        let err = error(dir.path(), "a");
        assert!(err.contains("a → b → a"), "{err}");
    }

    #[test]
    fn a_user_theme_with_a_builtin_name_replaces_it_and_can_extend_it() {
        let dir = tempfile::tempdir().unwrap();
        theme(
            dir.path(),
            "catppuccin.macchiato",
            "extends = \"catppuccin.macchiato\"\n[colors]\nborder_active = \"#f5bde6\"\n",
        );
        let replaced = resolve("catppuccin.macchiato", Some(dir.path())).unwrap();
        let original = by_name("catppuccin.macchiato").unwrap();
        assert_eq!(replaced.colors.border_active, Color::Rgb(0xf5, 0xbd, 0xe6));
        assert_eq!(replaced.colors.text, original.colors.text);
        assert_eq!(
            resolve("catppuccin.latte", Some(dir.path())).unwrap(),
            by_name("catppuccin.latte").unwrap()
        );
    }

    #[test]
    fn bundled_tmtheme_files_load_from_the_theme_folder() {
        let dir = tempfile::tempdir().unwrap();
        theme(dir.path(), "file", "syntax = \"colours.tmTheme\"\n");
        fs::write(
            dir.path().join("file/colours.tmTheme"),
            super::super::LATTE_TMTHEME,
        )
        .unwrap();
        let loaded = resolve("file", Some(dir.path())).unwrap();
        let Syntax::Loaded(syntax) = loaded.syntax else {
            panic!("syntax not loaded from the file");
        };
        assert_eq!(syntax.name.as_deref(), Some("Catppuccin Latte"));
        theme(dir.path(), "named", "syntax = \"InspiredGitHub\"\n");
        assert_eq!(
            resolve("named", Some(dir.path())).unwrap().syntax,
            Syntax::Builtin("InspiredGitHub".into())
        );
    }

    #[test]
    fn bad_user_themes_fail_naming_the_theme_and_the_culprit() {
        let dir = tempfile::tempdir().unwrap();
        for (name, toml, culprit) in [
            ("slot", "[colors]\nsidebar = \"#000000\"\n", "sidebar"),
            ("hex", "[colors]\ntext = \"red\"\n", "red"),
            ("key", "colours = 1\n", "colours"),
            ("parent", "extends = \"nowhere\"\n", "nowhere"),
            ("missing", "syntax = \"gone.tmTheme\"\n", "gone.tmTheme"),
            (
                "escape",
                "syntax = \"../x.tmTheme\"\n",
                "inside the theme folder",
            ),
            (
                "absolute",
                "syntax = \"/etc/x.tmTheme\"\n",
                "inside the theme folder",
            ),
            (
                "unknown",
                "syntax = \"Solarized (blue)\"\n",
                "Solarized (blue)",
            ),
        ] {
            theme(dir.path(), name, toml);
            let err = error(dir.path(), name);
            assert!(
                err.contains(&format!("theme {name}")) && err.contains(culprit),
                "{name}: {err}"
            );
        }
    }
}
