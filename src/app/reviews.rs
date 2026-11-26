//! The review picker: `O` lists the reviews in the review folder, newest
//! first, and opens one, as `--review` does from the command line.

use super::{AppState, Effect, Store, commits::view_of};
use crate::{
    config::{Action, Key, KeyCodeKey, Lookup, lookup},
    git::Target,
    review::{self, Kind, Status},
};
use anyhow::Result;
use std::path::PathBuf;

/// One review file, as the picker lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewEntry {
    pub path: PathBuf,
    /// The file name without `.md`: `2026-11-26-01`.
    pub name: String,
    pub status: Status,
    /// What it reviews: `HEAD` for the working tree, else the commits.
    pub target: String,
    pub open: usize,
    pub done: usize,
    pub accepted: usize,
    /// The first comment's first line, for telling reviews apart.
    pub first: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReviewPicker {
    /// Newest first.
    pub reviews: Vec<ReviewEntry>,
    /// Reviews whose name, target or first comment contain this, ignoring case.
    pub filter: String,
    /// Keys go to the filter instead of moving the cursor.
    pub typing: bool,
    /// Index into `shown()`.
    pub cursor: usize,
}

impl ReviewPicker {
    /// Indexes into `reviews` that pass the filter.
    pub fn shown(&self) -> Vec<usize> {
        let needle = self.filter.to_lowercase();
        self.reviews
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                needle.is_empty()
                    || [&r.name, &r.target, &r.first]
                        .iter()
                        .any(|text| text.to_lowercase().contains(&needle))
            })
            .map(|(i, _)| i)
            .collect()
    }
}

/// Reads every review in `dir`, newest first; files that do not parse are
/// left out.
fn entries(dir: &std::path::Path) -> Vec<ReviewEntry> {
    let mut found: Vec<ReviewEntry> = review::reviews_in(dir)
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            let parsed = review::parse(&text).ok()?;
            let name = path.file_stem()?.to_string_lossy().into_owned();
            let first = parsed
                .items
                .first()
                .map(|item| match item.kind {
                    Kind::Suggestion => "replace with …".to_owned(),
                    _ => item.text.lines().next().unwrap_or_default().to_owned(),
                })
                .unwrap_or_default();
            Some(ReviewEntry {
                name,
                status: parsed.status,
                target: Target::from_base(&parsed.base).label(),
                open: parsed.open_items(),
                done: parsed.done_items(),
                accepted: parsed.accepted_items(),
                first,
                path,
            })
        })
        .collect();
    found.reverse();
    found
}

impl AppState {
    /// `O`: the review picker, its cursor on the review open now.
    pub(super) fn open_reviews(&mut self) {
        if matches!(self.store, Store::Stdout) {
            self.error("not with --out -: it prints one review");
            return;
        }
        let reviews = entries(&self.repo.review_dir());
        if reviews.is_empty() {
            self.info(format!(
                "no reviews in {} yet",
                self.repo.review_dir().display()
            ));
            return;
        }
        let cursor = self
            .review_path()
            .and_then(|open| reviews.iter().position(|r| &r.path == open))
            .unwrap_or(0);
        self.reviews = Some(ReviewPicker {
            reviews,
            cursor,
            ..ReviewPicker::default()
        });
    }

    /// A key while the review picker is open.
    pub(super) fn reviews_key(&mut self, key: Key) -> Effect {
        let Some(picker) = &mut self.reviews else {
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
            picker.cursor = picker.cursor.min(picker.shown().len().saturating_sub(1));
            return Effect::None;
        }
        let last = picker.shown().len().saturating_sub(1);
        let action = match lookup(&self.config.keys, &[key]) {
            Lookup::Action(action) => Some(action),
            Lookup::Pending | Lookup::None => None,
        };
        match (key.code, action) {
            (KeyCodeKey::Esc, _) | (_, Some(Action::Reviews)) => self.reviews = None,
            (KeyCodeKey::Enter, _) => {
                let chosen = picker
                    .shown()
                    .get(picker.cursor)
                    .map(|i| picker.reviews[*i].clone());
                self.reviews = None;
                if let Some(entry) = chosen {
                    match self.open_review(entry.path) {
                        Ok(()) => self.info(format!("opened review {}", entry.name)),
                        Err(err) => self.error(format!("{err:#}")),
                    }
                }
            }
            (_, Some(Action::Filter)) => picker.typing = true,
            (_, Some(Action::Down)) => picker.cursor = (picker.cursor + 1).min(last),
            (_, Some(Action::Up)) => picker.cursor = picker.cursor.saturating_sub(1),
            (_, Some(Action::Top)) => picker.cursor = 0,
            (_, Some(Action::Bottom)) => picker.cursor = last,
            _ => {}
        }
        Effect::None
    }

    /// Opens the review at `path` in place of the one open now, which is
    /// already on disk. The working tree's own review is kept aside when a
    /// commit's takes its place, so `p` can bring it back.
    fn open_review(&mut self, path: PathBuf) -> Result<()> {
        if self.review_path() == Some(&path) {
            return Ok(());
        }
        let (review, store) = super::persist_open(path)?;
        let leaving_home = Target::from_base(&self.review.base) == Target::WorkTree;
        let to_commit = Target::from_base(&review.base).is_range();
        let old_review = std::mem::replace(&mut self.review, review);
        let old_store = std::mem::replace(&mut self.store, store);
        self.home = if leaving_home && to_commit {
            Some((old_review, old_store))
        } else if to_commit {
            self.home.take()
        } else {
            None
        };
        self.show(view_of(&self.review))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        Pane,
        tests::{press, state},
    };
    use crate::git::{Target, fixture::Fixture};

    #[test]
    fn o_lists_the_reviews_newest_first_and_opens_the_one_picked() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit_all("first");
        f.write("a.txt", "two\n");
        f.commit_all("second");
        f.write("b.txt", "draft\n");
        let mut s = state(&f);
        // A working-tree review, then one on the last commit.
        s.focus = Pane::Diff;
        press(&mut s, "con the draft\n");
        let draft = s.review_path().unwrap().clone();
        press(&mut s, "pj\n");
        s.focus = Pane::Diff;
        press(&mut s, "jcon the commit\n");
        let commit = s.review_path().unwrap().clone();
        // Newest first, the cursor on the one open now.
        press(&mut s, "O");
        let picker = s.reviews.as_ref().unwrap();
        let names: Vec<&str> = picker.reviews.iter().map(|r| r.first.as_str()).collect();
        assert_eq!(names, ["on the commit", "on the draft"]);
        assert_eq!(picker.reviews[0].path, commit);
        assert_eq!(picker.cursor, 0);
        assert!(picker.reviews[0].target.contains(".."));
        assert_eq!(picker.reviews[1].target, "HEAD");
        // The draft review: the working tree again.
        press(&mut s, "j\n");
        assert!(s.reviews.is_none());
        assert_eq!(s.review_path(), Some(&draft));
        assert_eq!(s.target, Target::WorkTree);
        // Filtering, then Esc.
        press(&mut s, "O/commit\n");
        let picker = s.reviews.as_ref().unwrap();
        assert_eq!(picker.shown(), [0]);
        press(&mut s, "\x1b");
        assert!(s.reviews.is_none());
    }
}
