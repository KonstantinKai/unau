//! Command line: what to review and where the review goes.

use crate::git::{Repo, Target};
use anyhow::{Context, Result, bail};
use clap::Parser;
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Terminal code review that ends in a file an agent can act on"
)]
pub struct Args {
    /// A directory in the repository, a commit, or a range `a..b`. A path
    /// that exists wins over a revision of the same name.
    #[arg(value_name = "PATH | REF | A..B")]
    pub target: Option<String>,

    /// Re-review an existing review file.
    #[arg(long, value_name = "FILE", conflicts_with = "target")]
    pub review: Option<PathBuf>,

    /// Reopen the newest review in `.revmark/`; start a new one if there is none.
    #[arg(long = "continue", conflicts_with_all = ["review", "target", "path", "out"])]
    pub resume: bool,

    /// Print the review to stdout on exit instead of writing `.revmark/`. Only `-` is accepted.
    #[arg(long, value_name = "-")]
    pub out: Option<String>,

    /// A path, after `--`, read as a path even if a revision has the same name.
    #[arg(last = true, value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Print the licenses and third-party notices included in revmark.
    #[arg(long, exclusive = true)]
    pub licenses: bool,
}

/// Every license text revmark carries: its own, the notices for code and
/// themes bundled in it, its dependencies' notices, and those of the syntax
/// definitions and themes `two-face` embeds, which no crate manifest lists.
pub fn licenses() -> String {
    let embedded = format!(
        "# Syntax definitions and themes embedded through two-face\n\n{}",
        two_face::acknowledgement::listing().to_md()
    );
    [
        include_str!("../LICENSE"),
        include_str!("../assets/notices/ANNODIFF-LICENSE"),
        include_str!("../assets/themes/CATPPUCCIN-LICENSE"),
        include_str!("../THIRD_PARTY_NOTICES.md"),
        &embedded,
    ]
    .join("\n\n")
}

impl Args {
    pub fn stdout(&self) -> Result<bool> {
        match self.out.as_deref() {
            None => Ok(false),
            Some("-") => Ok(true),
            Some(other) => bail!("--out only accepts -, not {other}"),
        }
    }
}

/// Decides what to review. An existing path comes first, then a revision;
/// anything else is an error.
pub fn resolve(
    target: Option<&str>,
    forced_path: Option<&Path>,
    cwd: &Path,
) -> Result<(Repo, Target)> {
    if let Some(path) = forced_path {
        return Ok((repo_at(&cwd.join(path))?, Target::WorkTree));
    }
    let Some(arg) = target else {
        return Ok((Repo::discover(cwd)?, Target::WorkTree));
    };
    let as_path = cwd.join(arg);
    if as_path.exists() {
        return Ok((repo_at(&as_path)?, Target::WorkTree));
    }
    let repo = Repo::discover(cwd)?;
    let target = if let Some((from, to)) = arg.split_once("..") {
        let from = if from.is_empty() { "HEAD" } else { from };
        let to = if to.is_empty() { "HEAD" } else { to };
        Target::Range {
            from: repo.commit(from)?,
            to: repo.commit(to)?,
        }
    } else {
        repo.commit_range(arg)
            .with_context(|| format!("{arg} is neither a path nor a revision"))?
    };
    Ok((repo, target))
}

fn repo_at(path: &Path) -> Result<Repo> {
    let dir = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    if !dir.exists() {
        bail!("{} does not exist", path.display());
    }
    Repo::discover(dir)
}

/// Where the user config lives: `$XDG_CONFIG_HOME/revmark` or `~/.config/revmark`.
pub fn user_config() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("revmark").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::fixture::Fixture;

    fn repo() -> Fixture {
        let f = Fixture::new();
        f.write("src/a.rs", "a\n");
        f.commit_all("init");
        f.write("src/a.rs", "b\n");
        f.commit_all("second");
        f
    }

    #[test]
    fn no_argument_reviews_the_working_tree() {
        let f = repo();
        let (repo, target) = resolve(None, None, f.path()).unwrap();
        assert_eq!(repo.root, f.repo.root);
        assert_eq!(target, Target::WorkTree);
    }

    #[test]
    fn an_existing_path_wins_over_a_branch_of_the_same_name() {
        let f = repo();
        f.git(&["branch", "src"]);
        let (_, target) = resolve(Some("src"), None, f.path()).unwrap();
        assert_eq!(target, Target::WorkTree);
        let (repo, _) = resolve(Some("src"), None, f.path()).unwrap();
        assert_eq!(repo.root, f.repo.root);
    }

    #[test]
    fn revisions_and_ranges_resolve_to_commit_ids() {
        let f = repo();
        let (_, target) = resolve(Some("HEAD"), None, f.path()).unwrap();
        let head = f.git(&["rev-parse", "HEAD"]).trim().to_owned();
        let parent = f.git(&["rev-parse", "HEAD~1"]).trim().to_owned();
        assert_eq!(
            target,
            Target::Range {
                from: parent.clone(),
                to: head.clone()
            }
        );
        let (_, target) = resolve(Some("HEAD~1..HEAD"), None, f.path()).unwrap();
        assert_eq!(
            target,
            Target::Range {
                from: parent,
                to: head
            }
        );
    }

    #[test]
    fn unknown_names_are_an_error_and_double_dash_forces_a_path() {
        let f = repo();
        let err = resolve(Some("nope"), None, f.path()).unwrap_err();
        assert!(format!("{err:#}").contains("neither a path nor a revision"));
        let (_, target) = resolve(None, Some(Path::new("src")), f.path()).unwrap();
        assert_eq!(target, Target::WorkTree);
    }

    #[test]
    fn continue_stands_alone() {
        assert!(
            Args::try_parse_from(["revmark", "--continue"])
                .unwrap()
                .resume
        );
        for clash in [
            &["revmark", "--continue", "--review", "x.md"][..],
            &["revmark", "--continue", "HEAD"],
            &["revmark", "--continue", "--out", "-"],
            &["revmark", "--continue", "--", "src"],
        ] {
            assert!(Args::try_parse_from(clash).is_err(), "{clash:?}");
        }
    }

    #[test]
    fn licenses_carry_every_bundled_and_dependency_notice() {
        let text = licenses();
        for part in [
            "MIT License",
            "MelsRoughSketch",
            "Catppuccin",
            "# Third-party notices",
            "### MIT License",
            "embedded through two-face",
        ] {
            assert!(text.contains(part), "missing {part}");
        }
        assert!(
            Args::try_parse_from(["revmark", "--licenses"])
                .unwrap()
                .licenses
        );
        assert!(Args::try_parse_from(["revmark", "--licenses", "HEAD"]).is_err());
    }

    #[test]
    fn out_accepts_only_stdout() {
        let args = Args::try_parse_from(["revmark", "--out", "-"]).unwrap();
        assert!(args.stdout().unwrap());
        let args = Args::try_parse_from(["revmark", "--out", "x.md"]).unwrap();
        assert!(args.stdout().is_err());
        let args = Args::try_parse_from(["revmark", "--", "HEAD"]).unwrap();
        assert_eq!(args.path.as_deref(), Some(Path::new("HEAD")));
        assert!(args.target.is_none());
    }
}
