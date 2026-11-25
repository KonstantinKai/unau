//! The commit picker: `p` lists the working tree and the history, and picking
//! a commit reviews it. Each target keeps its own review: a commit's is found
//! again in the review folder or started fresh, and the working tree's is
//! kept aside while a commit is shown and comes back with it.

use super::{AppState, Effect, Store};
use crate::{
    config::{Action, Key, KeyCodeKey, Lookup, lookup},
    git::{Commit, Target},
    review::{self, Review},
};
use anyhow::Result;

/// Commits read at a time; more follow as the cursor nears the end.
const PAGE: usize = 200;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommitPicker {
    /// The history read so far, newest first.
    pub commits: Vec<Commit>,
    /// Commits whose id or subject contain this, ignoring case.
    pub filter: String,
    /// Keys go to the filter instead of moving the cursor.
    pub typing: bool,
    /// Row under the cursor: 0 is the working tree, then `shown()` in order.
    pub cursor: usize,
    /// `git log` may have more after the last commit read.
    pub more: bool,
}

impl CommitPicker {
    /// Indexes into `commits` that pass the filter.
    pub fn shown(&self) -> Vec<usize> {
        let needle = self.filter.to_lowercase();
        self.commits
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                needle.is_empty()
                    || c.id.starts_with(&needle)
                    || c.subject.to_lowercase().contains(&needle)
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Rows the picker lists: the working tree, then each commit shown.
    pub fn rows(&self) -> usize {
        1 + self.shown().len()
    }
}

impl AppState {
    /// `p`: the commit picker, its cursor on what is shown now.
    pub(super) fn open_commits(&mut self) {
        if matches!(self.store, Store::Stdout) {
            self.error("not with --out -: it prints one review");
            return;
        }
        let commits = self.repo.log(0, PAGE);
        let more = commits.len() == PAGE;
        let mut picker = CommitPicker {
            commits,
            more,
            ..CommitPicker::default()
        };
        if let Target::Range { to, .. } = &self.target {
            picker.cursor = picker
                .commits
                .iter()
                .position(|c| &c.id == to)
                .map_or(0, |i| i + 1);
        }
        self.commits = Some(picker);
    }

    /// A key while the picker is open.
    pub(super) fn commits_key(&mut self, key: Key) -> Effect {
        let Some(picker) = &mut self.commits else {
            return Effect::None;
        };
        if picker.typing {
            match key.code {
                KeyCodeKey::Char(c) if !key.ctrl => picker.filter.push(c),
                KeyCodeKey::Backspace => {
                    picker.filter.pop();
                }
                KeyCodeKey::Enter => picker.typing = false,
                KeyCodeKey::Esc => {
                    picker.typing = false;
                    picker.filter.clear();
                }
                _ => {}
            }
            picker.cursor = picker.cursor.min(picker.rows() - 1);
            return Effect::None;
        }
        let action = match lookup(&self.config.keys, &[key]) {
            Lookup::Action(action) => Some(action),
            Lookup::Pending | Lookup::None => None,
        };
        match (key.code, action) {
            (KeyCodeKey::Esc, _) | (_, Some(Action::Commits)) => self.commits = None,
            (KeyCodeKey::Enter, _) => return self.pick_commit(),
            (_, Some(Action::Filter)) => picker.typing = true,
            (_, Some(Action::Down)) => {
                picker.cursor = (picker.cursor + 1).min(picker.rows() - 1);
                self.load_more_commits();
            }
            (_, Some(Action::Up)) => picker.cursor = picker.cursor.saturating_sub(1),
            (_, Some(Action::Top)) => picker.cursor = 0,
            (_, Some(Action::Bottom)) => {
                picker.cursor = picker.rows() - 1;
                self.load_more_commits();
            }
            _ => {}
        }
        Effect::None
    }

    /// Reads the next page of history once the cursor is near the end.
    fn load_more_commits(&mut self) {
        let Some(picker) = &mut self.commits else {
            return;
        };
        if !picker.more || picker.cursor + PAGE / 4 < picker.rows() {
            return;
        }
        let next = self.repo.log(picker.commits.len(), PAGE);
        picker.more = next.len() == PAGE;
        picker.commits.extend(next);
    }

    /// Enter: review what the cursor is on.
    fn pick_commit(&mut self) -> Effect {
        let Some(list) = self.commits.take() else {
            return Effect::None;
        };
        let chosen = match list.cursor {
            0 => None,
            row => list.shown().get(row - 1).map(|i| list.commits[*i].clone()),
        };
        let target = match &chosen {
            None => Target::WorkTree,
            Some(commit) => match self.repo.commit_range(&commit.id) {
                Ok(target) => target,
                Err(err) => {
                    self.error(format!("{err:#}"));
                    return Effect::None;
                }
            },
        };
        if let Err(err) = self.switch_target(target) {
            self.error(format!("{err:#}"));
            return Effect::None;
        }
        match chosen {
            None => self.info("back to the working tree"),
            Some(commit) => self.info(format!("reviewing {} {}", commit.short, commit.subject)),
        }
        Effect::None
    }

    /// Shows `target` with its own review: the working tree's kept aside or
    /// brought back, a commit's found in the review folder or started fresh.
    pub(super) fn switch_target(&mut self, target: Target) -> Result<()> {
        if target == self.target {
            return Ok(());
        }
        let (review, store) = match &target {
            Target::WorkTree => self.home.take().unwrap_or_else(|| fresh(&target)),
            Target::Range { .. } => {
                match review::for_base(&self.repo.review_dir(), &target.base()) {
                    Some(path) => super::persist_open(path)?,
                    None => fresh(&target),
                }
            }
        };
        let old_review = std::mem::replace(&mut self.review, review);
        let old_store = std::mem::replace(&mut self.store, store);
        if self.target == Target::WorkTree {
            self.home = Some((old_review, old_store));
        }
        self.target = target;
        self.file = None;
        self.tree_cursor = 0;
        self.tree_scroll = 0;
        self.apply_instructions();
        self.refresh()?;
        self.rebuild_comments();
        self.scroll_to_cursor();
        Ok(())
    }
}

/// A review for `target` with no file yet; one is made on the first comment.
fn fresh(target: &Target) -> (Review, Store) {
    (
        Review::new("", &target.base()),
        Store::File {
            path: None,
            synced: String::new(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::super::{
        Pane,
        tests::{press, state},
    };
    use crate::git::{Target, fixture::Fixture};

    fn history() -> Fixture {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit_all("first");
        f.write("a.txt", "two\n");
        f.commit_all("second");
        f.write("b.txt", "draft\n");
        f
    }

    #[test]
    fn picking_a_commit_reviews_it_and_the_working_tree_review_comes_back() {
        let f = history();
        let mut s = state(&f);
        s.focus = Pane::Diff;
        press(&mut s, "cin the draft\n");
        let draft_review = s.review_path().unwrap().clone();
        // The picker lists the working tree, then the newest commit first.
        press(&mut s, "p");
        let picker = s.commits.as_ref().unwrap();
        assert_eq!(picker.commits[0].subject, "second");
        press(&mut s, "j\n");
        assert!(s.commits.is_none());
        assert!(s.target.is_range());
        assert_eq!(s.paths, ["a.txt"]);
        assert!(s.review.items.is_empty(), "a commit starts its own review");
        // A comment on the commit goes to a file of its own.
        s.focus = Pane::Diff;
        press(&mut s, "con the commit\n");
        let commit_review = s.review_path().unwrap().clone();
        assert_ne!(commit_review, draft_review);
        let text = std::fs::read_to_string(&commit_review).unwrap();
        assert!(
            text.contains(&format!("base: {}", s.target.base())),
            "{text}"
        );
        // The picker opens on what is shown now; the working tree is above.
        press(&mut s, "p");
        assert_eq!(s.commits.as_ref().unwrap().cursor, 1);
        // Back to the working tree: its review is the one it had.
        press(&mut s, "k\n");
        assert_eq!(s.target, Target::WorkTree);
        assert_eq!(s.review_path(), Some(&draft_review));
        assert_eq!(s.review.items[0].text, "in the draft");
        // The commit again: its review is found in the review folder.
        press(&mut s, "pj\n");
        assert_eq!(s.review_path(), Some(&commit_review));
        assert_eq!(s.review.items[0].text, "on the commit");
    }

    #[test]
    fn the_picker_filters_by_subject_and_closes_with_esc() {
        let f = history();
        let mut s = state(&f);
        press(&mut s, "p/first\n");
        let picker = s.commits.as_ref().unwrap();
        let shown = picker.shown();
        assert_eq!(shown.len(), 1);
        assert_eq!(picker.commits[shown[0]].subject, "first");
        press(&mut s, "\x1b");
        assert!(s.commits.is_none());
        assert_eq!(s.target, Target::WorkTree);
    }
}
