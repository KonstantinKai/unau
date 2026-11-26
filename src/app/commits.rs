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
    /// The commit `v` marked as one end of a range, by index into `commits`;
    /// the cursor's commit is the other end.
    pub anchor: Option<usize>,
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

    /// The commit under the cursor, by index into `commits`; none on the
    /// working tree's row.
    pub fn cursor_commit(&self) -> Option<usize> {
        self.cursor
            .checked_sub(1)
            .and_then(|row| self.shown().get(row).copied())
    }

    /// The marked range as (newest, oldest) indexes into `commits`, both
    /// ends included, while `v` has marked one end and the cursor is on a
    /// commit.
    pub fn marked(&self) -> Option<(usize, usize)> {
        let (anchor, cursor) = (self.anchor?, self.cursor_commit()?);
        Some((anchor.min(cursor), anchor.max(cursor)))
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
        // The commit whose review is open, shown itself or as its fixes.
        if let Target::Range { to, .. } = Target::from_base(&self.review.base) {
            picker.cursor = picker
                .commits
                .iter()
                .position(|c| c.id == to)
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
            // Esc drops a marked range first, then closes.
            (KeyCodeKey::Esc, _) if picker.anchor.is_some() => picker.anchor = None,
            (KeyCodeKey::Esc, _) | (_, Some(Action::Commits)) => self.commits = None,
            (KeyCodeKey::Enter, _) if picker.anchor.is_some() && picker.cursor == 0 => {
                self.error("a range runs between two commits, not to the working tree");
            }
            (KeyCodeKey::Enter, _) => return self.pick_commit(),
            (_, Some(Action::Select)) => match (picker.anchor, picker.cursor_commit()) {
                (Some(_), _) => picker.anchor = None,
                (None, Some(commit)) => picker.anchor = Some(commit),
                (None, None) => self.error("mark a commit to start a range"),
            },
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

    /// Enter: review what the cursor is on, or the marked range.
    fn pick_commit(&mut self) -> Effect {
        let Some(list) = self.commits.take() else {
            return Effect::None;
        };
        if let Some((newest, oldest)) = list.marked()
            && newest != oldest
        {
            return self.pick_range(
                &list.commits[newest],
                &list.commits[oldest],
                oldest - newest + 1,
            );
        }
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
            Some(commit) if self.follow_up() => self.info(format!(
                "the fixes for {} {}, in the working tree · {}, Enter on it: the commit",
                commit.short,
                commit.subject,
                self.key_label(Action::Commits)
            )),
            Some(commit) => self.info(format!("reviewing {} {}", commit.short, commit.subject)),
        }
        Effect::None
    }

    /// Reviews `oldest` through `newest`, both included: from `oldest`'s
    /// parent to `newest`, as `unau oldest^..newest` does.
    fn pick_range(&mut self, newest: &Commit, oldest: &Commit, count: usize) -> Effect {
        let target = match self.repo.commit_range(&oldest.id) {
            Ok(Target::Range { from, .. }) => Target::Range {
                from,
                to: newest.id.clone(),
            },
            Ok(Target::WorkTree) => return Effect::None,
            Err(err) => {
                self.error(format!("{err:#}"));
                return Effect::None;
            }
        };
        if let Err(err) = self.switch_target(target) {
            self.error(format!("{err:#}"));
            return Effect::None;
        }
        self.info(format!(
            "reviewing {}..{} ({count} commits)",
            oldest.short, newest.short
        ));
        Effect::None
    }

    /// Opens the review of `base`, a commit or the working tree: the working
    /// tree's kept aside or brought back, a commit's found in the review
    /// folder or started fresh, and shown per `view_of`. Picking the commit
    /// whose review is already open switches between the commit itself and
    /// the working tree with the fixes made for it.
    pub(super) fn switch_target(&mut self, base: Target) -> Result<()> {
        let open = Target::from_base(&self.review.base);
        if base.is_range() && base == open {
            let view = if self.target.is_range() && self.review.done_items() > 0 {
                Target::WorkTree
            } else {
                base
            };
            return if view == self.target {
                Ok(())
            } else {
                self.show(view)
            };
        }
        if base == Target::WorkTree && open == Target::WorkTree {
            return Ok(());
        }
        let (review, store) = match &base {
            Target::WorkTree => self.home.take().unwrap_or_else(|| fresh(&base)),
            Target::Range { .. } => match review::for_base(&self.repo.review_dir(), &base.base()) {
                Some(path) => super::persist_open(path)?,
                None => fresh(&base),
            },
        };
        let old_review = std::mem::replace(&mut self.review, review);
        let old_store = std::mem::replace(&mut self.store, store);
        // Only the working tree's own review is kept aside; a commit's is on
        // disk and is found there again.
        if open == Target::WorkTree {
            self.home = Some((old_review, old_store));
        }
        self.show(view_of(&self.review))
    }

    /// Shows the open review against `view`.
    fn show(&mut self, view: Target) -> Result<()> {
        self.target = view;
        self.file = None;
        self.tree_cursor = 0;
        self.tree_scroll = 0;
        self.apply_instructions();
        self.refresh()?;
        self.rebuild_comments();
        self.scroll_to_cursor();
        Ok(())
    }

    /// The working tree shown with a commit's review: the fixes an agent made
    /// for comments on that commit.
    pub fn follow_up(&self) -> bool {
        self.target == Target::WorkTree && Target::from_base(&self.review.base).is_range()
    }
}

/// What a review is shown against: its own base, except a commit's review
/// with items the agent ticked, whose fixes are in the working tree.
pub fn view_of(review: &Review) -> Target {
    let base = Target::from_base(&review.base);
    if base.is_range() && review.done_items() > 0 {
        Target::WorkTree
    } else {
        base
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

    #[test]
    fn v_marks_a_range_and_enter_reviews_every_commit_in_it() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit_all("first");
        f.write("b.txt", "two\n");
        f.commit_all("second");
        f.write("c.txt", "three\n");
        f.commit_all("third");
        let mut s = state(&f);
        // Esc drops a marked end before it closes the picker.
        press(&mut s, "pjv\x1b");
        assert!(s.commits.as_ref().unwrap().anchor.is_none());
        // `third` down to `second`: both included, `first` left out.
        press(&mut s, "vj");
        assert_eq!(s.commits.as_ref().unwrap().marked(), Some((0, 1)));
        press(&mut s, "\n");
        assert!(s.commits.is_none());
        let first = f.git(&["rev-parse", "HEAD~2"]).trim().to_owned();
        let third = f.git(&["rev-parse", "HEAD"]).trim().to_owned();
        assert_eq!(
            s.target,
            Target::Range {
                from: first,
                to: third
            }
        );
        assert_eq!(s.paths, ["b.txt", "c.txt"]);
        // A range cannot end at the working tree.
        press(&mut s, "pvkk\n");
        assert!(s.commits.is_some());
        assert!(
            s.message
                .as_ref()
                .unwrap()
                .text
                .contains("between two commits")
        );
    }

    #[test]
    fn a_commit_review_the_agent_ticked_shows_its_fixes_in_the_working_tree() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit_all("first");
        f.write("a.txt", "two\n");
        f.commit_all("second");
        let mut s = state(&f);
        // Comment on the committed line.
        press(&mut s, "pj\n");
        s.focus = Pane::Diff;
        press(&mut s, "jcname it better\n");
        let path = s.review_path().unwrap().clone();
        // The agent fixes it in the working tree and ticks the item.
        f.write("a.txt", "TWO\n");
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.replace("- [ ] fix", "- [x] fix")).unwrap();
        assert!(s.poll_review());
        let message = s.message.as_ref().unwrap().text.clone();
        assert!(
            message.contains("fixes are in the working tree"),
            "{message}"
        );
        // Enter on the commit again: the working tree, with this review.
        press(&mut s, "p\n");
        assert!(s.follow_up(), "{:?}", s.target);
        assert_eq!(s.review_path(), Some(&path));
        let file = s.file.as_ref().unwrap();
        let note = file
            .view
            .rows
            .iter()
            .position(|r| *r == crate::diff::Row::Note(0))
            .unwrap();
        assert!(matches!(
            file.view.rows[note - 1],
            crate::diff::Row::Code { section, lines } if lines.iter().flatten().any(|i| {
                file.sections[section].diff.lines[*i].text == "TWO"
            })
        ));
        // `a` stages the fix, as in any re-review.
        s.file.as_mut().unwrap().cursor = note;
        press(&mut s, "a");
        assert!(s.review.items[0].accepted, "{:?}", s.message);
        assert!(f.git(&["diff", "--cached"]).contains("+TWO"));
        // And Enter on the commit once more shows the commit itself.
        press(&mut s, "p\n");
        assert!(s.target.is_range());
        assert!(!s.follow_up());
    }

    #[test]
    fn reopening_a_ticked_commit_review_opens_on_the_working_tree() {
        let mut review = crate::review::Review::new("x", "aaa..bbb");
        assert!(super::view_of(&review).is_range());
        let mut item = crate::review::Item::new(
            crate::review::Kind::Fix,
            crate::review::Scope::General,
            "x".into(),
        );
        item.done = true;
        review.items.push(item);
        assert_eq!(super::view_of(&review), Target::WorkTree);
        review.items[0].accepted = true;
        assert!(super::view_of(&review).is_range(), "nothing left to accept");
    }
}
