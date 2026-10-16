//! `config.toml` (user) and `.unau.toml` (repository), read in that order:
//! the repository file overrides the user's, setting by setting.

mod keys;
mod template;

pub use keys::{Action, Key, KeyCodeKey, Lookup, format_keys, lookup, parse_keys};
pub use template::template;

use crate::theme::{self, Theme};
use anyhow::{Context, Result, bail};
use ratatui::style::Color;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Name of an instructions file, globally and in a repository's review folder.
pub const INSTRUCTIONS_FILE: &str = "instructions.md";

/// `[review] dir` for the repository at `root`. Unset is `<root>/.unau`.
fn resolve_review_dir(dir: Option<&str>, root: &Path, home: Option<&Path>) -> PathBuf {
    resolve_path(dir, root, home).unwrap_or_else(|| root.join(crate::git::REVIEW_DIR))
}

/// A path from the `[review]` section, for the repository at `root`: `{repo}`
/// is its folder name, a leading `~/` is `home`, and a relative path starts at
/// `root`. `None` when unset or blank.
fn resolve_path(path: Option<&str>, root: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let dir = path.map(str::trim).filter(|d| !d.is_empty())?;
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = dir.replace("{repo}", &name);
    Some(match (dir.strip_prefix("~/"), home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => root.join(dir),
    })
}

/// Where `r` runs the command: picked from the environment, or forced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunIn {
    /// Herdr if inside Herdr, else tmux if inside tmux, else the foreground.
    #[default]
    Auto,
    Herdr,
    Tmux,
    /// In the background, output in a pane inside unau.
    Pane,
    Foreground,
}

/// How the run pane shows the command's output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunOutput {
    /// Line by line, as printed.
    #[default]
    Plain,
    /// Claude Code's `stream-json` events as short readable lines.
    ClaudeStream,
}

/// Which of the two configured themes is used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    /// Follow the terminal background.
    #[default]
    Auto,
    Light,
    Dark,
}

/// Who paints behind the text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Background {
    /// The theme's own `background`, so any theme reads right on any terminal.
    #[default]
    Theme,
    /// The terminal's, transparency included.
    Terminal,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineNumbers {
    #[default]
    Both,
    New,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// Key sequence to action. Every action has at least one binding.
    pub keys: BTreeMap<Vec<Key>, Action>,
    pub run: Option<String>,
    /// Where the run command goes: a multiplexer pane or the foreground.
    pub run_in: RunIn,
    pub run_output: RunOutput,
    pub line_numbers: LineNumbers,
    /// Quotes longer than this keep only their first and last lines; 0 = all.
    pub quote_lines: usize,
    /// `[review] dir` as written; `None` keeps reviews in `<repo>/.unau`.
    pub review_dir: Option<String>,
    /// `[review] instructions` as written; `None` reads `instructions.md` in
    /// the review folder.
    pub instructions: Option<String>,
    /// `Q` asks before quitting.
    pub confirm_quit: bool,
    pub background: Background,
    pub theme_mode: ThemeMode,
    /// Theme names as configured; a user theme may replace a built-in one.
    pub dark_name: String,
    pub light_name: String,
    /// The named themes, resolved once every config file is read.
    pub dark_theme: Theme,
    pub light_theme: Theme,
    /// `[theme.colors]` overrides, applied on top of whichever theme is active.
    pub colors: BTreeMap<String, Color>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    keys: BTreeMap<String, Binding>,
    #[serde(default)]
    run: RunSection,
    #[serde(default)]
    ui: UiSection,
    #[serde(default)]
    review: ReviewSection,
    #[serde(default)]
    theme: ThemeSection,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeSection {
    mode: Option<ThemeMode>,
    dark: Option<String>,
    light: Option<String>,
    #[serde(default)]
    colors: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Binding {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunSection {
    command: Option<String>,
    #[serde(rename = "in")]
    run_in: Option<RunIn>,
    output: Option<RunOutput>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct UiSection {
    line_numbers: Option<LineNumbers>,
    confirm_quit: Option<bool>,
    background: Option<Background>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewSection {
    quote_lines: Option<usize>,
    dir: Option<String>,
    instructions: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        let mut keys = BTreeMap::new();
        for action in Action::ALL {
            for spec in action.defaults() {
                keys.insert(parse_keys(spec).expect("default bindings parse"), *action);
            }
        }
        Self {
            keys,
            run: None,
            run_in: RunIn::Auto,
            run_output: RunOutput::Plain,
            line_numbers: LineNumbers::Both,
            quote_lines: crate::review::DEFAULT_QUOTE_LINES,
            review_dir: None,
            instructions: None,
            confirm_quit: true,
            background: Background::Theme,
            theme_mode: ThemeMode::Auto,
            dark_name: theme::CATPPUCCIN_MACCHIATO.name.into_owned(),
            light_name: theme::CATPPUCCIN_LATTE.name.into_owned(),
            dark_theme: theme::CATPPUCCIN_MACCHIATO,
            light_theme: theme::CATPPUCCIN_LATTE,
            colors: BTreeMap::new(),
        }
    }
}

impl Config {
    /// Defaults, then the user file, then the repository file. User themes are
    /// looked up in `themes/` next to the user file.
    pub fn load(user: Option<&Path>, repo: &Path) -> Result<Self> {
        let mut config = Self::default();
        for path in user.into_iter().chain([repo]) {
            if let Ok(text) = std::fs::read_to_string(path) {
                config
                    .merge(&text)
                    .with_context(|| format!("in {}", path.display()))?;
            }
        }
        let themes = user.and_then(Path::parent).map(|dir| dir.join("themes"));
        config.resolve_themes(themes.as_deref())?;
        Ok(config)
    }

    /// Turns the configured theme names into themes, user ones first.
    pub fn resolve_themes(&mut self, themes_dir: Option<&Path>) -> Result<()> {
        self.dark_theme = theme::resolve(&self.dark_name, themes_dir).context("[theme] dark")?;
        self.light_theme = theme::resolve(&self.light_name, themes_dir).context("[theme] light")?;
        Ok(())
    }

    /// Applies one config file on top of what is already loaded. A rebound
    /// action loses its previous keys.
    pub fn merge(&mut self, text: &str) -> Result<()> {
        let file: File = toml::from_str(text)?;
        let mut rebound = Vec::new();
        for name in file.keys.keys() {
            let Some(action) = Action::from_name(name) else {
                bail!("unknown action in [keys]: {name}");
            };
            rebound.push(action);
        }
        self.keys.retain(|_, a| !rebound.contains(a));
        for ((name, binding), action) in file.keys.into_iter().zip(rebound) {
            let specs = match binding {
                Binding::One(s) => vec![s],
                Binding::Many(v) => v,
            };
            for spec in specs {
                let keys = parse_keys(&spec).with_context(|| format!("binding for {name}"))?;
                if let Some(other) = self.keys.insert(keys, action)
                    && other != action
                {
                    bail!("key {spec} is bound to both {} and {name}", other.name());
                }
            }
        }
        self.validate()?;
        if let Some(command) = file.run.command {
            self.run = Some(command);
        }
        if let Some(run_in) = file.run.run_in {
            self.run_in = run_in;
        }
        if let Some(output) = file.run.output {
            self.run_output = output;
        }
        if let Some(numbers) = file.ui.line_numbers {
            self.line_numbers = numbers;
        }
        if let Some(background) = file.ui.background {
            self.background = background;
        }
        if let Some(confirm) = file.ui.confirm_quit {
            self.confirm_quit = confirm;
        }
        if let Some(dir) = file.review.dir {
            self.review_dir = Some(dir);
        }
        if let Some(path) = file.review.instructions {
            self.instructions = Some(path);
        }
        if let Some(lines) = file.review.quote_lines {
            self.quote_lines = lines;
        }
        if let Some(mode) = file.theme.mode {
            self.theme_mode = mode;
        }
        if let Some(name) = file.theme.dark {
            self.dark_name = name;
        }
        if let Some(name) = file.theme.light {
            self.light_name = name;
        }
        let mut probe = self.dark_theme.colors;
        for (slot, value) in file.theme.colors {
            let color =
                theme::parse_hex(&value).with_context(|| format!("[theme.colors] {slot}"))?;
            probe.set(&slot, color)?;
            self.colors.insert(slot, color);
        }
        Ok(())
    }

    /// The theme for a dark or light background, overrides applied.
    pub fn theme(&self, dark_background: bool) -> Theme {
        let dark = match self.theme_mode {
            ThemeMode::Auto => dark_background,
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
        };
        let mut theme = if dark {
            self.dark_theme.clone()
        } else {
            self.light_theme.clone()
        };
        for (slot, color) in &self.colors {
            // Slots were checked when the config was read.
            let _ = theme.colors.set(slot, *color);
        }
        theme
    }

    /// A key that starts a longer binding would never fire, so refuse it.
    pub fn validate(&self) -> Result<()> {
        for (short, a) in &self.keys {
            for (long, b) in &self.keys {
                if long.len() > short.len() && long.starts_with(short) {
                    bail!(
                        "key {} ({}) is a prefix of {} ({})",
                        format_keys(short),
                        a.name(),
                        format_keys(long),
                        b.name()
                    );
                }
            }
        }
        Ok(())
    }

    /// Where reviews of the repository at `root` go.
    pub fn review_dir(&self, root: &Path) -> PathBuf {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        resolve_review_dir(self.review_dir.as_deref(), root, home.as_deref())
    }

    /// The repository's instructions file: `[review] instructions`, else
    /// `instructions.md` in the review folder.
    pub fn instructions_file(&self, root: &Path) -> PathBuf {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        resolve_path(self.instructions.as_deref(), root, home.as_deref())
            .unwrap_or_else(|| self.review_dir(root).join(INSTRUCTIONS_FILE))
    }

    /// Keys bound to `action`, for help and messages.
    pub fn keys_for(&self, action: Action) -> Vec<String> {
        self.keys
            .iter()
            .filter(|(_, a)| **a == action)
            .map(|(k, _)| format_keys(k))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_keymap_has_no_prefix_clash() {
        Config::default().validate().unwrap();
    }

    #[test]
    fn unknown_action_names_fail_with_the_name() {
        let err = Config::default()
            .merge("[keys]\nfly = \"f\"\n")
            .unwrap_err();
        assert!(err.to_string().contains("fly"));
    }

    #[test]
    fn a_binding_that_prefixes_another_fails_naming_both() {
        let err = Config::default()
            .merge("[keys]\ngeneral_comment = \"g\"\n")
            .unwrap_err()
            .to_string();
        // `g` starts `gg`, `gs`, `gu` and `gd`; the first clash is named.
        assert!(
            err.contains("general_comment")
                && ["top", "stage_file", "unstage_file", "discard_file"]
                    .iter()
                    .any(|a| err.contains(a)),
            "{err}"
        );
    }

    #[test]
    fn rebinding_replaces_the_default_keys() {
        let mut config = Config::default();
        config
            .merge("[keys]\ndown = [\"n\", \"Down\"]\ngeneral_comment = \"N\"\n[run]\ncommand = \"agent {file}\"\n")
            .unwrap();
        assert_eq!(config.keys_for(Action::Down), ["n", "Down"]);
        assert_eq!(config.run.as_deref(), Some("agent {file}"));
    }

    #[test]
    fn catppuccin_is_the_default_pair_and_mode_picks_between_them() {
        let mut config = Config::default();
        assert_eq!(config.theme(true).name, "catppuccin.macchiato");
        assert_eq!(config.theme(false).name, "catppuccin.latte");
        config
            .merge("[theme]\nmode = \"light\"\nlight = \"default.light\"\n")
            .unwrap();
        config.resolve_themes(None).unwrap();
        assert_eq!(config.theme(true).name, "default.light");
    }

    #[test]
    fn load_finds_user_themes_next_to_the_user_config() {
        let dir = tempfile::tempdir().unwrap();
        let user = dir.path().join("config.toml");
        std::fs::write(&user, "[theme]\ndark = \"mine\"\n").unwrap();
        std::fs::create_dir_all(dir.path().join("themes/mine")).unwrap();
        std::fs::write(
            dir.path().join("themes/mine/theme.toml"),
            "extends = \"catppuccin.macchiato\"\n[colors]\nhunk = \"#123456\"\n",
        )
        .unwrap();
        let config = Config::load(Some(&user), &dir.path().join("none.toml")).unwrap();
        let theme = config.theme(true);
        assert_eq!(theme.name, "mine");
        assert_eq!(theme.colors.hunk, Color::Rgb(0x12, 0x34, 0x56));
    }

    #[test]
    fn colour_overrides_apply_on_top_of_the_active_theme() {
        let mut config = Config::default();
        config
            .merge("[theme.colors]\nborder_active = \"#010203\"\n")
            .unwrap();
        assert_eq!(config.theme(true).colors.border_active, Color::Rgb(1, 2, 3));
        assert_eq!(
            config.theme(false).colors.border_active,
            Color::Rgb(1, 2, 3)
        );
        assert_eq!(
            config.theme(true).colors.text,
            theme::CATPPUCCIN_MACCHIATO.colors.text
        );
    }

    #[test]
    fn the_review_dir_defaults_to_dot_unau_and_resolves_home_repo_and_relative() {
        let root = Path::new("/work/slider");
        let home = Some(Path::new("/home/me"));
        let at = |dir| resolve_review_dir(dir, root, home);
        assert_eq!(at(None), Path::new("/work/slider/.unau"));
        assert_eq!(at(Some("  ")), Path::new("/work/slider/.unau"));
        assert_eq!(
            at(Some("docs/reviews")),
            Path::new("/work/slider/docs/reviews")
        );
        assert_eq!(
            at(Some("~/reviews/{repo}")),
            Path::new("/home/me/reviews/slider")
        );
        assert_eq!(at(Some("/srv/reviews")), Path::new("/srv/reviews"));
        let mut config = Config::default();
        config.merge("[review]\ndir = \"notes\"\n").unwrap();
        assert_eq!(config.review_dir.as_deref(), Some("notes"));
    }

    #[test]
    fn the_instructions_file_follows_the_review_dir_unless_set() {
        let root = Path::new("/work/slider");
        let mut config = Config::default();
        assert_eq!(
            config.instructions_file(root),
            Path::new("/work/slider/.unau/instructions.md")
        );
        config.merge("[review]\ndir = \"ai/unau\"\n").unwrap();
        assert_eq!(
            config.instructions_file(root),
            Path::new("/work/slider/ai/unau/instructions.md")
        );
        config
            .merge("[review]\ninstructions = \"docs/{repo}-agents.md\"\n")
            .unwrap();
        assert_eq!(
            config.instructions_file(root),
            Path::new("/work/slider/docs/slider-agents.md")
        );
    }

    #[test]
    fn bad_theme_settings_fail_naming_the_culprit() {
        for (text, culprit) in [
            (
                "[theme]\ndark = \"catppuccin.espresso\"\n",
                "catppuccin.espresso",
            ),
            ("[theme.colors]\nsidebar = \"#000000\"\n", "sidebar"),
            ("[theme.colors]\ntext = \"blue\"\n", "blue"),
            ("[ui]\ntheme = \"dark\"\n", "theme"),
        ] {
            let mut config = Config::default();
            let result = config
                .merge(text)
                .and_then(|()| config.resolve_themes(None));
            let err = format!("{:#}", result.unwrap_err());
            assert!(err.contains(culprit), "{text}: {err}");
        }
    }
}
