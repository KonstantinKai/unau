//! The view a repository was left in: open file, cursor line, panes and
//! layout, saved on quit and applied on the next start. One small file per
//! repository in the user's state directory, never in the repository.

use crate::runline::RunLine;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Focus {
    #[default]
    Files,
    Diff,
    Comments,
    Run,
}

/// The last run, as the run pane showed it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedRun {
    pub command: String,
    /// Exit code; `-1` when it was stopped.
    pub exit: Option<i32>,
    /// Why it could not start, when it did not.
    pub error: Option<String>,
    pub elapsed_secs: u64,
    /// The pane was on screen.
    pub visible: bool,
    pub lines: Vec<RunLine>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    /// The repository root, so the file says what it is about.
    pub repo: String,
    /// The file open in the diff.
    pub file: Option<String>,
    /// New-side line under the diff cursor.
    pub line: Option<usize>,
    pub focus: Focus,
    /// The comments pane was shown.
    pub comments: bool,
    pub folded: Vec<String>,
    pub layout: Layout,
    /// Last in the file: it can be long.
    pub run: Option<SavedRun>,
}

/// How the panes were arranged.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    /// Flat file list instead of the tree.
    pub flat: bool,
    /// Files pane width in percent; `None` keeps the default.
    pub tree_percent: Option<u16>,
    pub zoom: bool,
    /// Side by side instead of unified.
    pub split: bool,
}

/// `$XDG_STATE_HOME/revmark/sessions`, else `~/.local/state/revmark/sessions`.
fn sessions_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))?;
    Some(base.join("revmark").join("sessions"))
}

/// The session file for the repository at `root`.
pub fn path(root: &Path) -> Option<PathBuf> {
    sessions_dir().map(|dir| path_in(&dir, root))
}

fn path_in(dir: &Path, root: &Path) -> PathBuf {
    dir.join(format!(
        "{:016x}.toml",
        fnv1a(root.to_string_lossy().as_bytes())
    ))
}

/// FNV-1a: stable across builds and platforms, unlike the standard hasher,
/// so a repository keeps its file after an upgrade.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The saved session, or `None` when there is none or it cannot be read.
pub fn load(path: &Path) -> Option<Session> {
    toml::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

pub fn save(path: &Path, session: &Session) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    let text = toml::to_string(session).context("encode the session")?;
    std::fs::write(path, text).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_round_trips_through_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = path_in(dir.path(), Path::new("/work/repo"));
        let session = Session {
            repo: "/work/repo".into(),
            file: Some("src/a.rs".into()),
            line: Some(12),
            focus: Focus::Comments,
            comments: true,
            folded: vec!["src/ui".into()],
            layout: Layout {
                flat: false,
                tree_percent: Some(40),
                zoom: false,
                split: true,
            },
            run: Some(SavedRun {
                command: "agent".into(),
                exit: Some(0),
                error: None,
                elapsed_secs: 185,
                visible: true,
                lines: vec![
                    RunLine::Status("· started".into()),
                    RunLine::Tool {
                        name: "edit".into(),
                        subject: "src/a.rs".into(),
                    },
                    RunLine::Prose("**Done**".into()),
                ],
            }),
        };
        save(&path, &session).unwrap();
        assert_eq!(load(&path), Some(session));
    }

    #[test]
    fn each_repository_gets_its_own_stable_file() {
        let dir = Path::new("/state");
        let a = path_in(dir, Path::new("/work/a"));
        assert_eq!(a, path_in(dir, Path::new("/work/a")));
        assert_ne!(a, path_in(dir, Path::new("/work/b")));
    }

    #[test]
    fn a_broken_file_reads_as_no_session_and_missing_keys_as_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.toml");
        std::fs::write(&path, "focus = 7\n").unwrap();
        assert_eq!(load(&path), None);
        std::fs::write(&path, "[layout]\nflat = true\n").unwrap();
        let session = load(&path).unwrap();
        assert!(session.layout.flat && session.file.is_none());
    }
}
