//! Opening the config and the agent instructions from inside the tool, and
//! applying changes to them without a restart.

use super::{AppState, Effect};
use crate::{
    config::{Config, INSTRUCTIONS_FILE, template},
    review::DEFAULT_INSTRUCTIONS,
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// A line that pulls in the instructions of the next level down.
pub const INHERIT_MARKER: &str = "{inherit}";

/// The text of the first level, with each `{inherit}` line replaced by the
/// resolved text of the levels below it; the built-in default is the last.
fn resolve_inherit(levels: &[&str]) -> String {
    let Some((first, rest)) = levels.split_first() else {
        return DEFAULT_INSTRUCTIONS.to_owned();
    };
    let below = || resolve_inherit(rest);
    first
        .trim_end()
        .lines()
        .map(|line| {
            if line.trim() == INHERIT_MARKER {
                below()
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Where settings come from, and what the files looked like when last read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigFiles {
    /// `config.toml` in the user's config directory, when there is one.
    pub user: Option<PathBuf>,
    /// `.unau.toml` in the repository.
    pub repo: PathBuf,
    /// `instructions.md` next to the user config: every workspace.
    pub instructions_global: Option<PathBuf>,
    /// The repository's instructions (`[review] instructions`, else
    /// `instructions.md` in the review folder): replaces the global one.
    pub instructions_repo: PathBuf,
    /// Modification times of the config files, then the instruction files.
    pub seen: [Option<SystemTime>; 4],
    /// Contents of the same files when last read. A newer time with the same
    /// text (an editor quit without saving, a save without edits) is no change.
    pub texts: [Option<String>; 4],
    /// The terminal background is dark; picks the dark or light theme.
    pub dark_background: bool,
}

impl ConfigFiles {
    pub fn new(
        user: Option<PathBuf>,
        repo_root: &Path,
        instructions_repo: PathBuf,
        dark_background: bool,
    ) -> Self {
        let mut files = Self {
            instructions_global: user
                .as_deref()
                .and_then(Path::parent)
                .map(|dir| dir.join(INSTRUCTIONS_FILE)),
            user,
            repo: repo_root.join(".unau.toml"),
            instructions_repo,
            seen: [None; 4],
            texts: Default::default(),
            dark_background,
        };
        files.mark_seen();
        files
    }

    fn paths(&self) -> [Option<&PathBuf>; 4] {
        [
            self.user.as_ref(),
            Some(&self.repo),
            self.instructions_global.as_ref(),
            Some(&self.instructions_repo),
        ]
    }

    fn current(&self) -> [Option<SystemTime>; 4] {
        self.paths()
            .map(|p| p.and_then(|p| fs::metadata(p).ok()?.modified().ok()))
    }

    fn current_texts(&self) -> [Option<String>; 4] {
        self.paths()
            .map(|p| p.and_then(|p| fs::read_to_string(p).ok()))
    }

    /// Records the files as they are now. Returns which differ in content
    /// from the last record: the config files, then the instruction files.
    pub fn mark_seen(&mut self) -> (bool, bool) {
        self.seen = self.current();
        let texts = self.current_texts();
        let changed = (texts[..2] != self.texts[..2], texts[2..] != self.texts[2..]);
        self.texts = texts;
        changed
    }

    /// The instructions file in effect: the repository's, else the global one.
    pub fn instructions_in_effect(&self) -> Option<&PathBuf> {
        if self.instructions_repo.is_file() {
            return Some(&self.instructions_repo);
        }
        self.instructions_global.as_ref().filter(|p| p.is_file())
    }

    /// The instructions in effect and the file they came from; the default
    /// text when there is no file.
    pub fn instructions(&self) -> (String, Option<PathBuf>) {
        let read = |path: Option<&PathBuf>| {
            path.filter(|p| p.is_file())
                .and_then(|p| fs::read_to_string(p).ok().map(|t| (t, p.clone())))
        };
        // Most specific first; each level may pull in the ones after it.
        let levels: Vec<(String, PathBuf)> = [
            Some(&self.instructions_repo),
            self.instructions_global.as_ref(),
        ]
        .into_iter()
        .filter_map(read)
        .collect();
        let source = levels.first().map(|(_, p)| p.clone());
        let texts: Vec<&str> = levels.iter().map(|(t, _)| t.as_str()).collect();
        (resolve_inherit(&texts), source)
    }
}

impl AppState {
    /// The edit-config key: makes sure the user config exists, then opens it.
    pub fn edit_config(&mut self) -> Effect {
        let Some(path) = self.config_files.user.clone() else {
            self.error("no config directory: set HOME or XDG_CONFIG_HOME");
            return Effect::None;
        };
        if !path.exists() {
            let created = path
                .parent()
                .map_or(Ok(()), fs::create_dir_all)
                .and_then(|()| fs::write(&path, template()));
            if let Err(err) = created {
                self.error(format!("could not create {}: {err}", path.display()));
                return Effect::None;
            }
        }
        Effect::EditFile(path)
    }

    /// The edit-instructions key: opens the instructions file in effect, or
    /// creates the global one from the default text when there is none.
    pub fn edit_instructions(&mut self) -> Effect {
        if let Some(path) = self.config_files.instructions_in_effect() {
            return Effect::EditFile(path.clone());
        }
        let Some(path) = self.config_files.instructions_global.clone() else {
            self.error("no config directory: set HOME or XDG_CONFIG_HOME");
            return Effect::None;
        };
        let created = path
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|()| fs::write(&path, format!("{DEFAULT_INSTRUCTIONS}\n")));
        if let Err(err) = created {
            self.error(format!("could not create {}: {err}", path.display()));
            return Effect::None;
        }
        Effect::EditFile(path)
    }

    /// After the editor closes: reload whichever settings file changed.
    /// Closing the editor without changing anything reloads nothing and says
    /// nothing.
    pub fn after_edit(&mut self) {
        self.reload_changed();
    }

    /// Reloads whichever of config and instructions changed in content since
    /// last read; true when either did.
    pub(super) fn reload_changed(&mut self) -> bool {
        let (config, instructions) = self.config_files.mark_seen();
        if config {
            self.reload_config();
        }
        if instructions {
            self.reload_instructions();
        }
        config || instructions
    }

    /// Puts the instructions in effect into the review, unless its file is
    /// already written: an agent may have read those, so they stay.
    /// Returns where the instructions came from.
    pub fn apply_instructions(&mut self) -> Option<PathBuf> {
        let (text, source) = self.config_files.instructions();
        if self.review_path().is_none() {
            self.review.instructions = text;
        }
        source
    }

    /// Reads the instruction files again and says what that changed.
    pub fn reload_instructions(&mut self) {
        let source = self.apply_instructions().map_or_else(
            || "the built-in default".to_owned(),
            |p| p.display().to_string(),
        );
        if self.review_path().is_none() {
            self.info(format!(
                "instructions from {source} will go into this review"
            ));
        } else {
            self.info(format!(
                "instructions from {source} apply to the next review; this one keeps its own"
            ));
        }
    }

    /// Reads the config files again. A bad file leaves the running config as
    /// it was and says why.
    pub fn reload_config(&mut self) {
        let files = &self.config_files;
        match Config::load(files.user.as_deref(), &files.repo) {
            Ok(config) => {
                self.layout.theme = config.theme(files.dark_background);
                // Cached files are highlighted in the old theme.
                self.file_cache.clear();
                let instructions = config.instructions_file(&self.repo.root);
                self.config = config;
                self.reload_file();
                self.info("config reloaded");
                if instructions != self.config_files.instructions_repo {
                    self.config_files.instructions_repo = instructions;
                    self.config_files.mark_seen();
                    self.reload_instructions();
                }
            }
            Err(err) => self.error(format!("config not applied: {err:#}")),
        }
    }

    /// Reloads the config when either file changed on disk. Called on idle
    /// ticks, so edits made in another window apply too.
    pub fn poll_config(&mut self) -> bool {
        // Times are cheap to read every tick; contents only once they move.
        if self.config_files.current() == self.config_files.seen {
            return false;
        }
        self.reload_changed()
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{press, state};
    use super::*;
    use crate::{config::Action, git::fixture::Fixture};

    fn with_user_config(f: &Fixture) -> (AppState, PathBuf) {
        let mut s = state(f);
        let dir = f.path().join("home-config");
        let user = dir.join("unau/config.toml");
        s.config_files = ConfigFiles::new(
            Some(user.clone()),
            f.path(),
            s.config.instructions_file(f.path()),
            true,
        );
        (s, user)
    }

    fn repo() -> Fixture {
        let f = Fixture::new();
        f.write("a.rs", "fn a() {}\n");
        f.commit_all("init");
        f.write("a.rs", "fn b() {}\n");
        f
    }

    #[test]
    fn edit_config_creates_a_template_once_and_opens_it() {
        let f = repo();
        let (mut s, user) = with_user_config(&f);
        assert_eq!(press(&mut s, ","), Effect::EditFile(user.clone()));
        let text = fs::read_to_string(&user).unwrap();
        assert!(text.contains("# edit_config = \",\""), "{text}");
        fs::write(&user, "# mine\n").unwrap();
        press(&mut s, ",");
        assert_eq!(fs::read_to_string(&user).unwrap(), "# mine\n");
    }

    #[test]
    fn saved_changes_apply_without_a_restart() {
        let f = repo();
        let (mut s, user) = with_user_config(&f);
        fs::create_dir_all(user.parent().unwrap()).unwrap();
        fs::write(
            &user,
            "[keys]\nquit = \"X\"\n[theme]\ndark = \"default.dark\"\n",
        )
        .unwrap();
        assert!(s.poll_config());
        assert_eq!(s.config.keys_for(Action::Quit), ["X"]);
        assert_eq!(s.layout.theme.name, "default.dark");
        assert_eq!(s.message.as_ref().unwrap().text, "config reloaded");
        assert!(!s.poll_config());
    }

    #[test]
    fn closing_the_editor_or_touching_the_file_without_changes_says_nothing() {
        let f = repo();
        let (mut s, user) = with_user_config(&f);
        fs::create_dir_all(user.parent().unwrap()).unwrap();
        fs::write(&user, "[keys]\nquit = \"X\"\n").unwrap();
        assert!(s.poll_config());
        s.message = None;
        s.after_edit();
        assert!(s.message.is_none());
        // Same text, newer time, as a save without edits leaves it.
        let later = SystemTime::now() + std::time::Duration::from_secs(5);
        fs::File::options()
            .write(true)
            .open(&user)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert!(!s.poll_config());
        assert!(s.message.is_none());
    }

    #[test]
    fn a_broken_config_keeps_the_running_one_and_says_why() {
        let f = repo();
        let (mut s, user) = with_user_config(&f);
        fs::create_dir_all(user.parent().unwrap()).unwrap();
        fs::write(&user, "[keys]\nfly = \"f\"\n").unwrap();
        s.reload_config();
        assert_eq!(s.config, Config::default());
        let message = &s.message.as_ref().unwrap().text;
        assert!(
            message.contains("not applied") && message.contains("fly"),
            "{message}"
        );
    }

    #[test]
    fn instructions_key_creates_the_global_file_from_the_default_once() {
        let f = repo();
        let (mut s, user) = with_user_config(&f);
        let global = user.parent().unwrap().join(INSTRUCTIONS_FILE);
        assert_eq!(press(&mut s, "i"), Effect::EditFile(global.clone()));
        assert_eq!(
            fs::read_to_string(&global).unwrap().trim_end(),
            DEFAULT_INSTRUCTIONS
        );
        fs::write(&global, "Global rules.\n").unwrap();
        assert_eq!(press(&mut s, "i"), Effect::EditFile(global.clone()));
        assert_eq!(fs::read_to_string(&global).unwrap(), "Global rules.\n");
    }

    #[test]
    fn inherit_pulls_in_the_level_below_and_chains_down_to_the_default() {
        assert_eq!(resolve_inherit(&[]), DEFAULT_INSTRUCTIONS);
        assert_eq!(resolve_inherit(&["Mine only."]), "Mine only.");
        assert_eq!(
            resolve_inherit(&["{inherit}\n- Run pnpm test."]),
            format!("{DEFAULT_INSTRUCTIONS}\n- Run pnpm test.")
        );
        assert_eq!(
            resolve_inherit(&[
                "Repo first.\n  {inherit}  \nRepo last.",
                "Global.\n{inherit}"
            ]),
            format!("Repo first.\nGlobal.\n{DEFAULT_INSTRUCTIONS}\nRepo last.")
        );
    }

    #[test]
    fn a_repository_file_can_extend_the_global_one() {
        let f = repo();
        let (s, user) = with_user_config(&f);
        fs::create_dir_all(user.parent().unwrap()).unwrap();
        fs::write(
            user.parent().unwrap().join(INSTRUCTIONS_FILE),
            "Global rules.\n",
        )
        .unwrap();
        f.write(".unau/instructions.md", "{inherit}\n- Run pnpm test.\n");
        let (text, source) = s.config_files.instructions();
        assert_eq!(text, "Global rules.\n- Run pnpm test.");
        assert_eq!(source, Some(f.path().join(".unau").join(INSTRUCTIONS_FILE)));
        assert!(!text.contains(INHERIT_MARKER));
    }

    #[test]
    fn the_repository_file_replaces_the_global_one() {
        let f = repo();
        let (mut s, user) = with_user_config(&f);
        fs::create_dir_all(user.parent().unwrap()).unwrap();
        fs::write(
            user.parent().unwrap().join(INSTRUCTIONS_FILE),
            "Global rules.\n",
        )
        .unwrap();
        assert_eq!(s.config_files.instructions().0, "Global rules.");
        f.write(".unau/instructions.md", "Run pnpm test.\n");
        assert_eq!(s.config_files.instructions().0, "Run pnpm test.");
        assert_eq!(
            press(&mut s, "i"),
            Effect::EditFile(f.path().join(".unau").join(INSTRUCTIONS_FILE))
        );
    }

    #[test]
    fn the_repository_file_moves_with_the_review_dir_and_the_instructions_setting() {
        let f = repo();
        let (mut s, _) = with_user_config(&f);
        f.write("ai/unau/instructions.md", "From the review dir.\n");
        f.write("AGENTS-REVIEW.md", "From the setting.\n");
        f.write(".unau.toml", "[review]\ndir = \"ai/unau\"\n");
        assert!(s.poll_config());
        assert_eq!(s.review.instructions, "From the review dir.");
        std::thread::sleep(std::time::Duration::from_millis(20));
        f.write(
            ".unau.toml",
            "[review]\ndir = \"ai/unau\"\ninstructions = \"AGENTS-REVIEW.md\"\n",
        );
        assert!(s.poll_config());
        assert_eq!(s.review.instructions, "From the setting.");
        assert_eq!(
            press(&mut s, "i"),
            Effect::EditFile(f.path().join("AGENTS-REVIEW.md"))
        );
    }

    #[test]
    fn a_written_review_keeps_its_instructions_and_a_new_one_takes_the_file() {
        let f = repo();
        let (mut s, _) = with_user_config(&f);
        f.write(".unau/instructions.md", "First rules.\n");
        assert!(s.poll_config());
        assert_eq!(s.review.instructions, "First rules.");
        assert!(
            s.message
                .as_ref()
                .unwrap()
                .text
                .contains("into this review")
        );
        press(&mut s, "nan item\n");
        let path = s.review_path().unwrap().clone();
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("> First rules.")
        );
        // Make sure the change is seen as a change even on coarse clocks.
        std::thread::sleep(std::time::Duration::from_millis(20));
        f.write(".unau/instructions.md", "Second rules.\n");
        assert!(s.poll_config());
        assert_eq!(s.review.instructions, "First rules.");
        assert!(s.message.as_ref().unwrap().text.contains("next review"));
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("> First rules.")
        );
    }

    #[test]
    fn the_template_is_a_valid_config_commented_or_not() {
        let text = template();
        Config::default().merge(&text).unwrap();
        let uncommented = text
            .lines()
            .map(|l| {
                l.strip_prefix("# ")
                    .filter(|r| r.contains(" = "))
                    .unwrap_or(l)
            })
            .collect::<Vec<_>>()
            .join("\n");
        let mut config = Config::default();
        config.merge(&uncommented).unwrap();
        config.resolve_themes(None).unwrap();
        assert_eq!(config.keys, Config::default().keys);
        assert!(config.run.is_some());
    }
}
