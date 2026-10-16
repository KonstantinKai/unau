//! Everything that talks to `git`. The binary is shelled out, never linked.

mod apply;
mod parse;
mod snapshot;
mod watch;

pub use apply::{Change, apply, apply_files};
pub use parse::{FileDiff, Line, LineKind, parse};
pub use snapshot::{FileStatus, Section, SectionKind, Snapshot, snapshot};
pub use watch::fingerprint;

use anyhow::{Context, Result, bail};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

/// Default review folder, relative to the repository root; `[review] dir`
/// moves it.
pub const REVIEW_DIR: &str = ".unau";

/// The SHA-1 of git's empty tree, used as the parent of a root commit.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// What is being reviewed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// Working tree against `HEAD`: staged, unstaged and untracked changes.
    WorkTree,
    /// A commit or a range; read-only, nothing can be staged.
    Range { from: String, to: String },
}

impl Target {
    /// The `base:` value written to the review file.
    pub fn base(&self) -> String {
        match self {
            Self::WorkTree => "HEAD".to_owned(),
            Self::Range { from, to } => format!("{from}..{to}"),
        }
    }

    pub fn from_base(base: &str) -> Self {
        match base.split_once("..") {
            Some((from, to)) => Self::Range {
                from: from.to_owned(),
                to: to.to_owned(),
            },
            None => Self::WorkTree,
        }
    }

    pub fn is_range(&self) -> bool {
        matches!(self, Self::Range { .. })
    }
}

#[derive(Clone, Debug)]
pub struct Repo {
    pub root: PathBuf,
    /// Where review files go: `<root>/.unau` unless configured.
    pub reviews: PathBuf,
}

impl Repo {
    /// The repository at `root`, its reviews in the default place.
    pub fn at(root: PathBuf) -> Self {
        Self {
            reviews: root.join(REVIEW_DIR),
            root,
        }
    }

    pub fn discover(dir: &Path) -> Result<Self> {
        let output = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(dir)
            .output()
            .context("run git")?;
        if !output.status.success() {
            bail!("{} is not inside a git repository", dir.display());
        }
        let root = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        Ok(Self::at(PathBuf::from(root)))
    }

    pub fn review_dir(&self) -> PathBuf {
        self.reviews.clone()
    }

    /// The review folder as a path inside the repository, when it is inside.
    pub fn review_dir_in_tree(&self) -> Option<String> {
        let inside = self.reviews.strip_prefix(&self.root).ok()?;
        let inside = inside.to_string_lossy().replace('\\', "/");
        (!inside.is_empty()).then_some(inside)
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new("git");
        command
            .args(["-c", "core.quotepath=off", "-c", "color.ui=never"])
            .args(args)
            .current_dir(&self.root)
            .stdin(Stdio::null());
        command
    }

    pub fn output(&self, args: &[&str]) -> Result<Output> {
        self.command(args)
            .output()
            .with_context(|| format!("run git {}", args.join(" ")))
    }

    /// Runs git and returns stdout, failing on a non-zero exit.
    pub fn run(&self, args: &[&str]) -> Result<String> {
        let output = self.output(args)?;
        if !output.status.success() {
            bail!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Runs git with `input` on stdin, failing on a non-zero exit.
    pub fn run_with_input(&self, args: &[&str], input: &str) -> Result<()> {
        let mut child = self
            .command(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("run git {}", args.join(" ")))?;
        child
            .stdin
            .take()
            .context("git stdin")?
            .write_all(input.as_bytes())?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            bail!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    /// Resolves a revision to a full commit id.
    pub fn commit(&self, rev: &str) -> Result<String> {
        let output = self.output(&[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{rev}^{{commit}}"),
        ])?;
        if !output.status.success() {
            bail!("unknown revision: {rev}");
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    /// The range that shows one commit: its parent (or the empty tree) to it.
    pub fn commit_range(&self, rev: &str) -> Result<Target> {
        let to = self.commit(rev)?;
        let from = self
            .commit(&format!("{to}^"))
            .unwrap_or_else(|_| EMPTY_TREE.to_owned());
        Ok(Target::Range { from, to })
    }

    /// Blob id of `path` on the new side of `target`, without writing an object.
    pub fn blob(&self, target: &Target, path: &str) -> Result<String> {
        let id = match target {
            Target::WorkTree => {
                if !self.root.join(path).is_file() {
                    return Ok("deleted".to_owned());
                }
                self.run(&["hash-object", "--", path])?
            }
            Target::Range { to, .. } => self.run(&["rev-parse", &format!("{to}:{path}")])?,
        };
        Ok(id.trim().to_owned())
    }

    /// Contents of `path` on the new side of `target`.
    pub fn new_side(&self, target: &Target, path: &str) -> Result<String> {
        match target {
            Target::WorkTree => {
                let bytes = std::fs::read(self.root.join(path)).unwrap_or_default();
                Ok(String::from_utf8_lossy(&bytes).into_owned())
            }
            Target::Range { to, .. } => self.run(&["show", &format!("{to}:{path}")]),
        }
    }
}

#[cfg(test)]
pub mod fixture {
    //! Throwaway repositories built with real `git` commands.

    use super::Repo;
    use std::{fmt::Write as _, fs, path::Path, process::Command};
    use tempfile::TempDir;

    pub struct Fixture {
        /// Owns the directory; dropping it deletes the repository.
        _dir: TempDir,
        pub repo: Repo,
    }

    /// `line 1` to `line count`, one per line.
    pub fn numbered(count: usize) -> String {
        (1..=count).fold(String::new(), |mut text, n| {
            let _ = writeln!(text, "line {n}");
            text
        })
    }

    impl Fixture {
        pub fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let fixture = Self {
                repo: Repo::at(dir.path().canonicalize().unwrap()),
                _dir: dir,
            };
            fixture.git(&["init", "-q", "-b", "main"]);
            fixture.git(&["config", "user.email", "test@example.com"]);
            fixture.git(&["config", "user.name", "Test"]);
            fixture.git(&["config", "commit.gpgsign", "false"]);
            fixture
        }

        pub fn git(&self, args: &[&str]) -> String {
            let output = Command::new("git")
                .args(args)
                .current_dir(&self.repo.root)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).into_owned()
        }

        pub fn write(&self, path: &str, text: &str) {
            let full = self.repo.root.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, text).unwrap();
        }

        pub fn read(&self, path: &str) -> String {
            fs::read_to_string(self.repo.root.join(path)).unwrap()
        }

        pub fn commit_all(&self, message: &str) {
            self.git(&["add", "-A"]);
            self.git(&["commit", "-q", "-m", message]);
        }

        pub fn path(&self) -> &Path {
            &self.repo.root
        }
    }
}
