//! The state machine. `AppState` is plain data: it holds no terminal, no
//! child process and no open handle, so every test builds one headless.

mod annotate;
mod comments;
mod files;
mod filter;
mod folding;
mod input;
mod navigate;
mod open;
mod persist;
mod picker;
mod rereview;
mod resume;
mod runlog;
mod settings;
mod stage;

pub use comments::{CommentRow, CommentsPane, label as comment_label};
pub use files::{FileState, NoteLine, NoteStyle, Source, code_widths};
pub use filter::FileFilter;
pub use persist::{Store, open as persist_open};
pub use picker::ThemePicker;
pub use runlog::{RunLog, RunStatus, follow_top, format_elapsed};
pub use settings::ConfigFiles;

use crate::{
    config::{Action, Config, Key},
    git::{Repo, Snapshot, Target, snapshot},
    review::{Kind, Review, Scope},
    theme::Theme,
    tree::{self, TreeRow},
};
use anyhow::Result;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Tree,
    /// The comments pane under the files pane, when shown.
    Comments,
    Diff,
    /// The run pane under the diff, when shown.
    Run,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub text: String,
    pub level: Level,
}

/// What a submitted prompt line turns into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptPurpose {
    /// New item on lines or a file, with the quoted lines taken at selection.
    New { scope: Scope, quote: Vec<String> },
    /// New general comment.
    General,
    /// Replace the text of item `n`.
    Edit(usize),
    /// Push back on done item `n`: it is replaced by a new open item.
    Reopen(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    pub purpose: PromptPurpose,
    pub label: String,
    /// What the comment becomes; `Tab` switches fix and question.
    pub kind: Kind,
    pub text: String,
    /// Cursor position in characters.
    pub cursor: usize,
}

/// An action waiting for `y`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pending {
    /// One request per file.
    Stage(Vec<stage::Request>),
    Quit,
    StopRun,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirm {
    pub question: String,
    pub pending: Pending,
}

/// Why the editor was opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditPurpose {
    /// A suggestion replacing `quote`, which also prefills the file.
    Suggestion { scope: Scope, quote: Vec<String> },
    /// Replacement lines of suggestion item `n`.
    Replacement(usize),
    /// A comment expanded from the prompt line.
    Prompt(Prompt),
}

/// Side effects the event loop performs for the state, which cannot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    Quit,
    /// Open `$EDITOR` on a temporary file holding `text`.
    Editor {
        text: String,
        suffix: String,
        purpose: EditPurpose,
    },
    /// Run the configured command in a pane or the foreground.
    Run {
        command: String,
        place: crate::run::Place,
    },
    /// Open `$EDITOR` on a settings file (config or instructions), then
    /// reload it.
    EditFile(std::path::PathBuf),
    /// Open `$EDITOR` on a file in the working tree, at a line when known,
    /// then re-read the diff.
    OpenFile {
        path: std::path::PathBuf,
        line: Option<usize>,
    },
    /// Stop the command running in the background.
    StopRun,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub width: u16,
    pub height: u16,
    /// Width of the files pane, in percent of the screen.
    pub tree_percent: u16,
    pub zoom: bool,
    pub split: bool,
    /// Colours and syntax theme in use.
    pub theme: Theme,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            width: 120,
            height: 40,
            tree_percent: 25,
            zoom: false,
            split: false,
            theme: Theme::default(),
        }
    }
}

pub const TREE_MIN_PERCENT: u16 = 10;
pub const TREE_MAX_PERCENT: u16 = 60;
pub const TREE_STEP_PERCENT: u16 = 5;

// Independent on/off facts about the session; an enum would not model them better.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug)]
pub struct AppState {
    pub repo: Repo,
    pub target: Target,
    pub snapshot: Snapshot,
    pub config: Config,
    pub review: Review,
    pub store: Store,
    /// Paths shown in the tree: changed files, then annotated files that no
    /// longer have changes.
    pub paths: Vec<String>,
    pub folded: HashSet<String>,
    pub flat: bool,
    /// Rows of the files pane: folders and changed or annotated files.
    pub tree: Vec<TreeRow>,
    pub tree_cursor: usize,
    pub tree_scroll: usize,
    pub focus: Pane,
    pub file: Option<FileState>,
    /// Recently shown files, most recent first, for instant switching back.
    pub file_cache: Vec<FileState>,
    pub pressed: Vec<Key>,
    pub prompt: Option<Prompt>,
    pub confirm: Option<Confirm>,
    pub message: Option<Message>,
    pub help: bool,
    pub layout: Layout,
    /// The run command has been started at least once this session.
    pub ran: bool,
    /// Herdr and tmux, when the tool runs inside them.
    pub multiplexers: crate::run::Multiplexers,
    /// Annotated files whose content differs from the blob the review recorded.
    pub changed_since_review: HashSet<String>,
    /// Config file locations, for opening and reloading them.
    pub config_files: ConfigFiles,
    /// Output and status of the latest background run.
    pub run_log: Option<RunLog>,
    /// The comments pane under the files pane.
    pub comments: CommentsPane,
    /// The theme picker, while it is open.
    pub theme_picker: Option<ThemePicker>,
    /// What narrows the files pane.
    pub filter: FileFilter,
    /// Paths before the filter, for the `shown/all` count.
    pub all_paths: usize,
}

impl AppState {
    /// Builds the state from a repository read and a review. Needs no terminal.
    pub fn new(
        repo: Repo,
        target: Target,
        snapshot: Snapshot,
        config: Config,
        review: Review,
        store: Store,
    ) -> Self {
        let mut state = Self {
            repo,
            target,
            snapshot,
            config,
            review,
            store,
            paths: Vec::new(),
            folded: HashSet::new(),
            flat: false,
            tree: Vec::new(),
            tree_cursor: 0,
            tree_scroll: 0,
            focus: Pane::Tree,
            file: None,
            file_cache: Vec::new(),
            pressed: Vec::new(),
            prompt: None,
            confirm: None,
            message: None,
            help: false,
            layout: Layout::default(),
            ran: false,
            multiplexers: crate::run::Multiplexers::default(),
            changed_since_review: HashSet::new(),
            config_files: ConfigFiles::default(),
            run_log: None,
            comments: CommentsPane::default(),
            theme_picker: None,
            filter: FileFilter::default(),
            all_paths: 0,
        };
        state.check_blobs();
        state.rebuild_tree();
        if let Some(first) = state
            .tree
            .iter()
            .position(|t| state.tree_source(t).is_some())
        {
            state.tree_cursor = first;
        }
        state.open_under_cursor();
        state
    }

    /// Reads the repository again and rebuilds everything that depends on it.
    pub fn refresh(&mut self) -> Result<()> {
        self.snapshot = snapshot(&self.repo, &self.target)?;
        // Cached files hold the old diff.
        self.file_cache.clear();
        self.check_blobs();
        self.rebuild_tree();
        let source = self.file.as_ref().map(|f| f.source.clone());
        if source.is_some_and(|s| {
            self.tree
                .iter()
                .any(|t| self.tree_source(t) == Some(s.clone()))
        }) {
            self.reload_file();
        } else {
            self.file = None;
            self.open_under_cursor();
        }
        Ok(())
    }

    /// Compares each annotated file with the blob recorded in the review.
    pub fn check_blobs(&mut self) {
        self.changed_since_review = self
            .review
            .files
            .iter()
            .filter(|(path, sha)| {
                self.repo
                    .blob(&self.target, path)
                    .is_ok_and(|now| now != **sha)
            })
            .map(|(path, _)| path.clone())
            .collect();
    }

    pub fn rebuild_tree(&mut self) {
        let current = self.tree.get(self.tree_cursor).map(|t| self.tree_key(t));
        let mut paths: Vec<String> = self.snapshot.files.iter().map(|f| f.path.clone()).collect();
        for item in &self.review.items {
            if let Some(path) = item.scope.path()
                && !paths.iter().any(|p| p == path)
            {
                paths.push(path.to_owned());
            }
        }
        self.all_paths = paths.len();
        let needle = self.filter.text.to_lowercase();
        if !needle.is_empty() {
            paths.retain(|p| p.to_lowercase().contains(&needle));
        }
        let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        self.tree = tree::build(&refs, &self.folded, self.flat);
        self.paths = paths;
        let keys: Vec<String> = self.tree.iter().map(|t| self.tree_key(t)).collect();
        // A row hidden by folding hands the cursor to the deepest directory
        // still shown above it.
        if let Some(at) = current.and_then(|key| {
            keys.iter().position(|k| *k == key).or_else(|| {
                keys.iter()
                    .enumerate()
                    .filter(|(_, k)| k.ends_with('/') && key.starts_with(k.as_str()))
                    .max_by_key(|(_, k)| k.len())
                    .map(|(i, _)| i)
            })
        }) {
            self.tree_cursor = at;
        }
        self.tree_cursor = self.tree_cursor.min(self.tree.len().saturating_sub(1));
    }

    /// Identifies a tree row across rebuilds: a file or directory path.
    fn tree_key(&self, row: &TreeRow) -> String {
        match &row.entry {
            tree::Entry::File(i) => self.paths[*i].clone(),
            tree::Entry::Dir { path, .. } => format!("{path}/"),
        }
    }

    pub fn tree_source(&self, row: &TreeRow) -> Option<Source> {
        match row.entry {
            tree::Entry::File(i) => Some(Source::File(self.paths[i].clone())),
            tree::Entry::Dir { .. } => None,
        }
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.message = Some(Message {
            text: text.into(),
            level: Level::Info,
        });
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.message = Some(Message {
            text: text.into(),
            level: Level::Error,
        });
    }

    /// Keys bound to `action`, joined for messages and help.
    pub fn key_label(&self, action: Action) -> String {
        self.config.keys_for(action).join("/")
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        self.layout.width = width;
        self.layout.height = height;
        self.relayout();
    }

    /// Size of the diff pane's text area, without borders and gutter.
    pub fn diff_size(&self) -> (usize, usize) {
        let layout = &self.layout;
        let width = if layout.zoom {
            layout.width
        } else {
            layout.width - layout.width * layout.tree_percent / 100
        };
        let height = usize::from(
            layout
                .height
                .saturating_sub(3)
                .saturating_sub(self.run_pane_height()),
        );
        (usize::from(width.saturating_sub(2)), height.max(1))
    }

    pub fn tree_height(&self) -> usize {
        usize::from(
            self.layout
                .height
                .saturating_sub(3)
                .saturating_sub(self.comments_pane_height()),
        )
        .max(1)
    }

    /// The review is finished: nothing open and, in the working tree, nothing
    /// left unstaged.
    pub fn complete(&self) -> bool {
        (self.review_path().is_some() || !self.review.is_empty())
            && self.review.open_items() == 0
            && (self.target.is_range() || self.snapshot.unstaged_count() == 0)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::git::fixture::Fixture;

    /// A state over a fixture repository, with a review written to `.unau/`.
    pub fn state(f: &Fixture) -> AppState {
        let target = Target::WorkTree;
        let snap = snapshot(&f.repo, &target).unwrap();
        let review = Review::new("", &target.base());
        let mut state = AppState::new(
            f.repo.clone(),
            target,
            snap,
            Config::default(),
            review,
            Store::File {
                path: None,
                synced: String::new(),
            },
        );
        state.resize(120, 40);
        state
    }

    pub fn press(state: &mut AppState, keys: &str) -> Effect {
        let mut effect = Effect::None;
        for c in keys.chars() {
            let key = match c {
                '\n' => crate::config::Key {
                    code: crate::config::KeyCodeKey::Enter,
                    ctrl: false,
                },
                '\x1b' => crate::config::Key {
                    code: crate::config::KeyCodeKey::Esc,
                    ctrl: false,
                },
                c => Key::char(c),
            };
            effect = state.handle_key(key);
        }
        effect
    }

    #[test]
    fn state_builds_headless_from_snapshot_and_review() {
        let f = Fixture::new();
        f.write("src/a.rs", "fn a() {}\n");
        f.commit_all("init");
        f.write("src/a.rs", "fn a() { b() }\n");
        let state = state(&f);
        assert_eq!(state.paths, ["src/a.rs"]);
        assert!(state.file.is_some());
        assert!(!state.complete());
    }
}
