//! Staging, unstaging and discarding. Whole files go to `git add`, `reset`
//! and `checkout` in batches; selected lines get a synthesised patch for
//! `git apply`, the way lazygit does it.

use super::{FileDiff, Line, LineKind, Repo};
use anyhow::Result;
use std::fmt::Write as _;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// Unstaged lines into the index.
    Stage,
    /// Staged lines back out of the index.
    Unstage,
    /// Unstaged lines out of the working tree.
    Discard,
}

/// Builds a single-hunk patch that carries only the selected changes.
///
/// Forward patches (`reverse == false`) apply on the old side: unselected
/// additions are dropped and unselected removals become context. Reverse
/// patches are applied with `-R`, so their new side must match: unselected
/// additions become context and unselected removals are dropped.
pub fn build_patch(
    path: &str,
    lines: &[Line],
    selected: impl Fn(usize) -> bool,
    reverse: bool,
) -> Option<String> {
    let mut body = String::new();
    let (mut old, mut new) = (0usize, 0usize);
    let mut any = false;
    for (i, line) in lines.iter().enumerate() {
        let picked = selected(i);
        let prefix = match (line.kind, picked, reverse) {
            (LineKind::Context, ..)
            | (LineKind::Removed, false, false)
            | (LineKind::Added, false, true) => ' ',
            (LineKind::Added, false, false) | (LineKind::Removed, false, true) => continue,
            (LineKind::Added, true, _) => '+',
            (LineKind::Removed, true, _) => '-',
        };
        any |= picked && line.is_change();
        if prefix != '+' {
            old += 1;
        }
        if prefix != '-' {
            new += 1;
        }
        body.push(prefix);
        body.push_str(&line.text);
        body.push('\n');
        if line.no_newline {
            body.push_str("\\ No newline at end of file\n");
        }
    }
    if !any {
        return None;
    }
    // Full-context sections always start at line 1, or at 0 when a side is empty.
    let first_line = |count: usize| usize::from(count > 0);
    let mut text = format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n");
    let _ = writeln!(
        text,
        "@@ -{},{old} +{},{new} @@",
        first_line(old),
        first_line(new)
    );
    text.push_str(&body);
    Some(text)
}

/// Paths passed to one `git` call; a folder with thousands of files is split
/// into several calls rather than hitting the command-line length limit.
const PATHS_PER_CALL: usize = 500;

/// Applies `change` to every change in each of `files` (path, untracked),
/// with one `git` call per kind rather than one patch per file.
pub fn apply_files(repo: &Repo, change: Change, files: &[(&str, bool)]) -> Result<()> {
    let run_all = |args: &[&str], paths: &[&str]| -> Result<()> {
        for chunk in paths.chunks(PATHS_PER_CALL) {
            let mut call = args.to_vec();
            call.push("--");
            call.extend(chunk);
            repo.run(&call)?;
        }
        Ok(())
    };
    let paths: Vec<&str> = files.iter().map(|(p, _)| *p).collect();
    match change {
        Change::Stage => run_all(&["add", "-A"], &paths),
        // Before the first commit there is no HEAD to reset the index to.
        Change::Unstage if repo.commit("HEAD").is_err() => {
            run_all(&["rm", "--cached", "-r", "-q", "--ignore-unmatch"], &paths)
        }
        Change::Unstage => run_all(&["reset", "-q"], &paths),
        Change::Discard => {
            let tracked: Vec<&str> = files.iter().filter(|f| !f.1).map(|f| f.0).collect();
            if !tracked.is_empty() {
                run_all(&["checkout", "-q"], &tracked)?;
            }
            for (path, _) in files.iter().filter(|f| f.1) {
                std::fs::remove_file(repo.root.join(path))?;
            }
            Ok(())
        }
    }
}

/// Applies `change` to the selected lines of one section of one file.
/// `selected[i]` says whether `diff.lines[i]` is part of the change.
pub fn apply(
    repo: &Repo,
    diff: &FileDiff,
    untracked: bool,
    change: Change,
    selected: &[bool],
) -> Result<()> {
    let path = diff.path.as_str();
    let pick = |i: usize| selected.get(i).copied().unwrap_or(false);
    let all = diff
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.is_change())
        .all(|(i, _)| pick(i));
    match change {
        Change::Stage => {
            if diff.binary || (untracked && all) {
                repo.run(&["add", "--", path])?;
            } else if diff.deleted && all {
                repo.run(&["rm", "--cached", "-q", "--", path])?;
            } else if let Some(patch) = build_patch(path, &diff.lines, pick, false) {
                if untracked {
                    repo.run(&["add", "-N", "--", path])?;
                }
                repo.run_with_input(&["apply", "--cached", "--whitespace=nowarn", "-"], &patch)?;
            }
        }
        Change::Unstage => {
            if diff.new_file && (all || diff.binary) {
                repo.run(&["rm", "--cached", "-q", "--", path])?;
            } else if diff.binary || (diff.deleted && all) {
                repo.run(&["reset", "-q", "--", path])?;
            } else if let Some(patch) = build_patch(path, &diff.lines, pick, true) {
                repo.run_with_input(
                    &["apply", "--cached", "-R", "--whitespace=nowarn", "-"],
                    &patch,
                )?;
            }
        }
        Change::Discard => {
            if untracked && (all || diff.binary) {
                std::fs::remove_file(repo.root.join(path))?;
            } else if diff.binary || (diff.deleted && all) {
                repo.run(&["checkout", "--", path])?;
            } else if let Some(patch) = build_patch(path, &diff.lines, pick, true) {
                repo.run_with_input(&["apply", "-R", "--whitespace=nowarn", "-"], &patch)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{SectionKind, Target, fixture::Fixture, snapshot};

    fn section(f: &Fixture, path: &str, kind: SectionKind) -> (FileDiff, bool) {
        let snap = snapshot(&f.repo, &Target::WorkTree).unwrap();
        let entry = &snap.files[snap.find(path).unwrap()];
        (entry.section(kind).unwrap().diff.clone(), entry.untracked)
    }

    fn select(diff: &FileDiff, texts: &[&str]) -> Vec<bool> {
        diff.lines
            .iter()
            .map(|l| l.is_change() && texts.contains(&l.text.as_str()))
            .collect()
    }

    #[test]
    fn whole_files_stage_unstage_and_discard_in_one_call_each() {
        let f = Fixture::new();
        f.write("kept.txt", "one\n");
        f.write("gone.txt", "bye\n");
        f.commit_all("init");
        f.write("kept.txt", "two\n");
        std::fs::remove_file(f.path().join("gone.txt")).unwrap();
        f.write("new.txt", "hi\n");
        let all = [("kept.txt", false), ("gone.txt", false), ("new.txt", true)];
        apply_files(&f.repo, Change::Stage, &all).unwrap();
        assert_eq!(
            f.git(&["diff", "--cached", "--name-status"]),
            "D\tgone.txt\nM\tkept.txt\nA\tnew.txt\n"
        );
        apply_files(&f.repo, Change::Unstage, &all).unwrap();
        assert!(f.git(&["diff", "--cached"]).is_empty());
        apply_files(&f.repo, Change::Discard, &all).unwrap();
        assert_eq!(f.read("kept.txt"), "one\n");
        assert_eq!(f.read("gone.txt"), "bye\n");
        assert!(!f.path().join("new.txt").exists());
    }

    #[test]
    fn unstaging_whole_files_works_before_the_first_commit() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.git(&["add", "a.txt"]);
        apply_files(&f.repo, Change::Unstage, &[("a.txt", false)]).unwrap();
        assert!(f.git(&["ls-files"]).is_empty());
        assert!(f.path().join("a.txt").exists());
    }

    #[test]
    fn staging_selected_lines_puts_exactly_those_lines_in_the_index() {
        let f = Fixture::new();
        f.write("a.txt", "one\ntwo\nthree\nfour\n");
        f.commit_all("init");
        f.write("a.txt", "one\nTWO\nthree\nFOUR\n");
        let (diff, untracked) = section(&f, "a.txt", SectionKind::Unstaged);
        apply(
            &f.repo,
            &diff,
            untracked,
            Change::Stage,
            &select(&diff, &["two", "TWO"]),
        )
        .unwrap();
        assert_eq!(f.git(&["show", ":a.txt"]), "one\nTWO\nthree\nfour\n");
        assert_eq!(f.read("a.txt"), "one\nTWO\nthree\nFOUR\n");
    }

    #[test]
    fn unstaging_selected_lines_restores_them_in_the_index_only() {
        let f = Fixture::new();
        f.write("a.txt", "one\ntwo\nthree\n");
        f.commit_all("init");
        f.write("a.txt", "ONE\ntwo\nTHREE\n");
        f.git(&["add", "a.txt"]);
        let (diff, untracked) = section(&f, "a.txt", SectionKind::Staged);
        apply(
            &f.repo,
            &diff,
            untracked,
            Change::Unstage,
            &select(&diff, &["three", "THREE"]),
        )
        .unwrap();
        assert_eq!(f.git(&["show", ":a.txt"]), "ONE\ntwo\nthree\n");
        assert_eq!(f.read("a.txt"), "ONE\ntwo\nTHREE\n");
    }

    #[test]
    fn discarding_selected_lines_reverts_them_in_the_working_tree() {
        let f = Fixture::new();
        f.write("a.txt", "one\ntwo\n");
        f.commit_all("init");
        f.write("a.txt", "ONE\nTWO\n");
        let (diff, untracked) = section(&f, "a.txt", SectionKind::Unstaged);
        apply(
            &f.repo,
            &diff,
            untracked,
            Change::Discard,
            &select(&diff, &["one", "ONE"]),
        )
        .unwrap();
        assert_eq!(f.read("a.txt"), "one\nTWO\n");
        assert_eq!(f.git(&["show", ":a.txt"]), "one\ntwo\n");
    }

    #[test]
    fn untracked_file_lines_stage_through_intent_to_add() {
        let f = Fixture::new();
        f.write("base.txt", "x\n");
        f.commit_all("init");
        f.write("new.txt", "one\ntwo\nthree\n");
        let (diff, untracked) = section(&f, "new.txt", SectionKind::Unstaged);
        assert!(untracked);
        apply(
            &f.repo,
            &diff,
            untracked,
            Change::Stage,
            &select(&diff, &["one", "three"]),
        )
        .unwrap();
        assert_eq!(f.git(&["show", ":new.txt"]), "one\nthree\n");
        assert_eq!(f.read("new.txt"), "one\ntwo\nthree\n");
    }

    #[test]
    fn missing_final_newline_survives_partial_staging() {
        let f = Fixture::new();
        f.write("a.txt", "one\ntwo");
        f.commit_all("init");
        f.write("a.txt", "ONE\ntwo");
        let (diff, untracked) = section(&f, "a.txt", SectionKind::Unstaged);
        apply(
            &f.repo,
            &diff,
            untracked,
            Change::Stage,
            &select(&diff, &["one", "ONE"]),
        )
        .unwrap();
        assert_eq!(f.git(&["show", ":a.txt"]), "ONE\ntwo");
    }

    #[test]
    fn staging_a_whole_deleted_file_removes_it_from_the_index() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.write("b.txt", "keep\n");
        f.commit_all("init");
        std::fs::remove_file(f.path().join("a.txt")).unwrap();
        let (diff, untracked) = section(&f, "a.txt", SectionKind::Unstaged);
        let all = vec![true; diff.lines.len()];
        apply(&f.repo, &diff, untracked, Change::Stage, &all).unwrap();
        assert_eq!(f.git(&["diff", "--cached", "--name-status"]), "D\ta.txt\n");
    }
}
