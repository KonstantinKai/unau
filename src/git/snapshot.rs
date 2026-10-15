//! One read of the repository: every changed file with its diff sections.

use super::{FileDiff, LEGACY_REVIEW_DIR, REVIEW_DIR, Repo, Target, parse};
use anyhow::Result;
use std::collections::BTreeMap;

/// Context lines requested from git. Large enough to hold any real file, so
/// every diff carries the whole file and "expand nearby" only reveals lines
/// that are already loaded.
const FULL_CONTEXT: &str = "--unified=1000000";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionKind {
    /// Index to working tree, untracked files included. The side under review.
    Unstaged,
    /// `HEAD` to index: already accepted.
    Staged,
    /// Commit or range: read-only.
    Range,
}

impl SectionKind {
    pub fn title(self) -> &'static str {
        match self {
            Self::Unstaged => "unstaged",
            Self::Staged => "staged",
            Self::Range => "range",
        }
    }

    /// Lines in this section can carry annotations.
    pub fn annotatable(self) -> bool {
        self != Self::Staged
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub kind: SectionKind,
    pub diff: FileDiff,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    pub untracked: bool,
    pub sections: Vec<Section>,
}

impl FileEntry {
    pub fn section(&self, kind: SectionKind) -> Option<&Section> {
        self.sections.iter().find(|s| s.kind == kind)
    }

    pub fn has_unstaged(&self) -> bool {
        self.section(SectionKind::Unstaged).is_some()
    }

    pub fn has_staged(&self) -> bool {
        self.section(SectionKind::Staged).is_some()
    }

    /// One git-style letter for the whole file, across its sections:
    /// untracked, then deleted, then added, else modified.
    pub fn status(&self) -> FileStatus {
        let any = |test: fn(&FileDiff) -> bool| self.sections.iter().any(|s| test(&s.diff));
        if self.untracked {
            FileStatus::Untracked
        } else if any(|d| d.deleted) {
            FileStatus::Deleted
        } else if any(|d| d.new_file) {
            FileStatus::Added
        } else {
            FileStatus::Modified
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileStatus {
    Modified,
    Added,
    Deleted,
    Untracked,
}

impl FileStatus {
    pub fn letter(self) -> char {
        match self {
            Self::Modified => 'M',
            Self::Added => 'A',
            Self::Deleted => 'D',
            Self::Untracked => '?',
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub files: Vec<FileEntry>,
}

impl Snapshot {
    pub fn find(&self, path: &str) -> Option<usize> {
        self.files.iter().position(|f| f.path == path)
    }

    pub fn unstaged_count(&self) -> usize {
        self.files.iter().filter(|f| f.has_unstaged()).count()
    }
}

/// Review folders are never part of the change under review: the default and
/// legacy ones always, and a configured one when it is inside the repository.
fn under_review_dir(path: &str, configured: Option<&str>) -> bool {
    [Some(REVIEW_DIR), Some(LEGACY_REVIEW_DIR), configured]
        .into_iter()
        .flatten()
        .any(|dir| path == dir || path.starts_with(&format!("{dir}/")))
}

const DIFF: [&str; 5] = [
    "diff",
    "--no-ext-diff",
    "--no-renames",
    "--no-color",
    FULL_CONTEXT,
];

pub fn snapshot(repo: &Repo, target: &Target) -> Result<Snapshot> {
    let mut files: BTreeMap<String, FileEntry> = BTreeMap::new();
    let configured = repo.review_dir_in_tree();
    let configured = configured.as_deref();
    let mut add = |diff: FileDiff, kind: SectionKind, untracked: bool| {
        if under_review_dir(&diff.path, configured)
            || (diff.lines.is_empty() && !diff.binary && !diff.new_file && !diff.deleted)
        {
            return;
        }
        let entry = files.entry(diff.path.clone()).or_insert_with(|| FileEntry {
            path: diff.path.clone(),
            untracked,
            sections: Vec::new(),
        });
        entry.sections.push(Section { kind, diff });
    };
    match target {
        Target::Range { from, to } => {
            let mut args = DIFF.to_vec();
            args.extend([from.as_str(), to.as_str(), "--"]);
            for diff in parse(&repo.run(&args)?) {
                add(diff, SectionKind::Range, false);
            }
        }
        Target::WorkTree => {
            let mut args = DIFF.to_vec();
            args.push("--");
            for diff in parse(&repo.run(&args)?) {
                add(diff, SectionKind::Unstaged, false);
            }
            let untracked = repo.run(&["ls-files", "--others", "--exclude-standard", "-z"])?;
            for path in untracked.split('\0').filter(|p| !p.is_empty()) {
                if under_review_dir(path, configured) {
                    continue;
                }
                let mut args = DIFF.to_vec();
                args.extend(["--no-index", "--", "/dev/null", path]);
                // `--no-index` exits 1 when the files differ, which they always do here.
                let output = repo.output(&args)?;
                for mut diff in parse(&String::from_utf8_lossy(&output.stdout)) {
                    path.clone_into(&mut diff.path);
                    add(diff, SectionKind::Unstaged, true);
                }
            }
            let mut args = DIFF.to_vec();
            args.extend(["--cached", "--"]);
            for diff in parse(&repo.run(&args)?) {
                add(diff, SectionKind::Staged, false);
            }
        }
    }
    Ok(Snapshot {
        files: files.into_values().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::fixture::Fixture;

    #[test]
    fn working_tree_shows_staged_unstaged_and_untracked_files() {
        let f = Fixture::new();
        f.write("a.txt", "one\ntwo\n");
        f.write("b.txt", "keep\n");
        f.commit_all("init");
        f.write("a.txt", "one\nTWO\n");
        f.write("b.txt", "keep\nmore\n");
        f.git(&["add", "b.txt"]);
        f.write("c.txt", "fresh\n");
        let snap = snapshot(&f.repo, &Target::WorkTree).unwrap();
        let paths: Vec<_> = snap.files.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["a.txt", "b.txt", "c.txt"]);
        assert!(snap.files[0].has_unstaged() && !snap.files[0].has_staged());
        assert!(snap.files[1].has_staged() && !snap.files[1].has_unstaged());
        assert!(snap.files[2].untracked);
        let c = &snap.files[2].sections[0].diff;
        assert_eq!(c.lines.len(), 1);
        assert_eq!(c.lines[0].new, Some(1));
    }

    #[test]
    fn diff_carries_the_whole_file_as_context() {
        let f = Fixture::new();
        let body = crate::git::fixture::numbered(40);
        f.write("long.txt", &body);
        f.commit_all("init");
        f.write("long.txt", &body.replace("line 20\n", "line twenty\n"));
        let snap = snapshot(&f.repo, &Target::WorkTree).unwrap();
        assert_eq!(snap.files[0].sections[0].diff.lines.len(), 41);
    }

    #[test]
    fn review_directory_never_appears_in_the_snapshot() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit_all("init");
        f.write(".revmark/2026-10-14-00.md", "# Review\n");
        f.write(".revmark/instructions.md", "Be brief.\n");
        f.write(".review/2026-10-13-00.md", "# Old review\n");
        f.write("a.txt", "two\n");
        let snap = snapshot(&f.repo, &Target::WorkTree).unwrap();
        let paths: Vec<_> = snap.files.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["a.txt"]);
    }

    #[test]
    fn a_configured_review_dir_inside_the_repo_stays_out_of_the_diff() {
        let mut f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit_all("init");
        f.repo.reviews = f.path().join("docs/reviews");
        f.write("docs/reviews/2026-10-15-00.md", "# Review\n");
        f.write("docs/other.md", "kept\n");
        let snap = snapshot(&f.repo, &Target::WorkTree).unwrap();
        let paths: Vec<_> = snap.files.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["docs/other.md"]);
    }

    #[test]
    fn each_file_gets_one_status_letter() {
        let f = Fixture::new();
        f.write("kept.txt", "one\n");
        f.write("gone.txt", "bye\n");
        f.commit_all("init");
        f.write("kept.txt", "two\n");
        std::fs::remove_file(f.path().join("gone.txt")).unwrap();
        f.write("staged-new.txt", "hi\n");
        f.git(&["add", "staged-new.txt"]);
        f.write("loose.txt", "hi\n");
        let snap = snapshot(&f.repo, &Target::WorkTree).unwrap();
        let letters: Vec<(String, char)> = snap
            .files
            .iter()
            .map(|e| (e.path.clone(), e.status().letter()))
            .collect();
        assert_eq!(
            letters,
            [
                ("gone.txt".to_owned(), 'D'),
                ("kept.txt".to_owned(), 'M'),
                ("loose.txt".to_owned(), '?'),
                ("staged-new.txt".to_owned(), 'A'),
            ]
        );
    }

    #[test]
    fn commit_range_reads_one_commit() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit_all("init");
        f.write("a.txt", "two\n");
        f.commit_all("second");
        let target = f.repo.commit_range("HEAD").unwrap();
        let snap = snapshot(&f.repo, &target).unwrap();
        assert_eq!(snap.files.len(), 1);
        assert_eq!(snap.files[0].sections[0].kind, SectionKind::Range);
        let root = f.repo.commit_range("HEAD~1").unwrap();
        let snap = snapshot(&f.repo, &root).unwrap();
        assert!(snap.files[0].sections[0].diff.new_file);
    }
}
