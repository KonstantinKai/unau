//! Stage, unstage and discard: lines or a hunk from the diff pane, whole
//! files or directories from the files pane. Staged means accepted, so
//! annotations on staged or discarded lines leave the review.

use super::{AppState, Effect, Pane, Pending};
use crate::{
    diff::Row,
    git::{Change, FileDiff, LineKind, SectionKind, apply, apply_files},
    review::{Item, Scope},
    tree::Entry,
};
use std::collections::BTreeSet;

/// One file's part of a stage, unstage or discard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub change: Change,
    pub path: String,
    /// The section the change applies to, as it was read.
    pub diff: FileDiff,
    pub untracked: bool,
    pub selected: Vec<bool>,
    /// Items that leave the review once the change is made.
    pub drop: Vec<Item>,
    /// Every change in the section, asked for as a file rather than lines.
    pub whole_file: bool,
}

/// The section a change works on: staged lines for unstage, unstaged
/// otherwise.
fn wanted(change: Change) -> SectionKind {
    if change == Change::Unstage {
        SectionKind::Staged
    } else {
        SectionKind::Unstaged
    }
}

impl AppState {
    /// `s`, `u`, `d`: whole files in the files pane, lines in the diff.
    /// `whole_file` (`gs`, `gu`, `gd`) takes the whole open file from any pane.
    pub fn stage_key(&mut self, change: Change, whole_file: bool) -> Effect {
        let whole_files = whole_file || self.focus == Pane::Tree;
        let requests = if self.target.is_range() {
            Err("range mode is read-only: stage, unstage and discard are disabled".into())
        } else if self.focus == Pane::Tree {
            self.tree_requests(change)
        } else if whole_file {
            match self.file.as_ref().and_then(|f| f.path()) {
                Some(path) => self.file_requests(change, &[path.to_owned()]),
                None => Err("no file open".into()),
            }
        } else {
            self.stage_request(change).map(|r| vec![r])
        };
        let requests = match requests {
            Ok(requests) => requests,
            Err(err) => {
                self.error(err);
                return Effect::None;
            }
        };
        let open = requests
            .iter()
            .flat_map(|r| &r.drop)
            .filter(|i| !i.done)
            .count();
        let verb = verb(change);
        if open > 0 {
            self.ask(
                format!(
                    "{open} open annotation(s) on these lines will be removed — {verb} anyway?"
                ),
                Pending::Stage(requests),
            );
        } else if change == Change::Discard {
            let question = if whole_files {
                format!(
                    "Discard every change in {} file(s) from the working tree?",
                    requests.len()
                )
            } else {
                "Discard these changes from the working tree?".to_owned()
            };
            self.ask(question, Pending::Stage(requests));
        } else {
            self.perform_stage(&requests);
        }
        Effect::None
    }

    /// Every change of the matching kind in the file under the tree cursor,
    /// or in every file under the directory there.
    fn tree_requests(&self, change: Change) -> Result<Vec<Request>, String> {
        let row = self.tree.get(self.tree_cursor).ok_or("no file selected")?;
        let paths: Vec<String> = match &row.entry {
            Entry::File(i) => vec![self.paths[*i].clone()],
            Entry::Dir { path, .. } => {
                let prefix = format!("{path}/");
                self.paths
                    .iter()
                    .filter(|p| p.starts_with(&prefix))
                    .cloned()
                    .collect()
            }
        };
        self.file_requests(change, &paths)
    }

    /// Every change of the matching kind in each of `paths`; files without
    /// one are skipped.
    fn file_requests(&self, change: Change, paths: &[String]) -> Result<Vec<Request>, String> {
        let kind = wanted(change);
        let requests: Vec<Request> = paths
            .iter()
            .filter_map(|path| {
                let entry = &self.snapshot.files[self.snapshot.find(path)?];
                let diff = entry.section(kind)?.diff.clone();
                let selected = if diff.binary {
                    vec![true; diff.lines.len().max(1)]
                } else {
                    diff.lines.iter().map(crate::git::Line::is_change).collect()
                };
                let drop = if kind == SectionKind::Unstaged {
                    touched_items(&self.review.items, path, &diff, &selected)
                } else {
                    Vec::new()
                };
                Some(Request {
                    change,
                    path: path.clone(),
                    diff,
                    untracked: entry.untracked,
                    selected,
                    drop,
                    whole_file: true,
                })
            })
            .collect();
        if requests.is_empty() {
            return Err(format!("nothing to {} here", verb(change)));
        }
        Ok(requests)
    }

    fn stage_request(&self, change: Change) -> Result<Request, String> {
        let file = self.file.as_ref().ok_or("no file open")?;
        let path = file.path().ok_or("no file open")?.to_owned();
        let section = file
            .view
            .rows
            .get(file.cursor)
            .and_then(Row::section)
            .ok_or("move the cursor onto a change")?;
        let wanted = wanted(change);
        if file.sections[section].kind != wanted {
            return Err(format!(
                "{} works on {} lines",
                verb(change),
                wanted.title()
            ));
        }
        let diff = &file.sections[section].diff;
        let mut selected = vec![false; diff.lines.len()];
        if diff.binary {
            selected = vec![true; diff.lines.len().max(1)];
        } else if file.anchor.is_some() {
            for row in &file.view.rows[file.selected_rows()] {
                if row.section() == Some(section) {
                    for i in row.lines() {
                        selected[i] = diff.lines[i].is_change();
                    }
                }
            }
        } else {
            let line = file.view.rows[file.cursor].lines().next();
            let hunk = line
                .and_then(|i| file.views[section].hunk_of(i))
                .ok_or("no change under the cursor")?;
            for i in file.views[section].hunks[hunk].clone() {
                selected[i] = diff.lines[i].is_change();
            }
        }
        if !selected.iter().any(|s| *s) {
            return Err("no change selected".into());
        }
        let drop = if wanted == SectionKind::Unstaged {
            touched_items(&self.review.items, &path, diff, &selected)
        } else {
            Vec::new()
        };
        Ok(Request {
            change,
            path,
            diff: diff.clone(),
            untracked: file.untracked,
            selected,
            drop,
            whole_file: false,
        })
    }

    /// Applies each request in turn, stopping at the first git error, then
    /// re-reads the repository once.
    pub fn perform_stage(&mut self, requests: &[Request]) {
        let Some(first) = requests.first() else {
            return;
        };
        // Where to look for the next file once whole files are staged: the
        // row under the cursor in the files pane, else the last file's row.
        let after = (first.whole_file && first.change == Change::Stage)
            .then(|| match self.focus {
                Pane::Tree => Some(self.tree_cursor),
                _ => requests.last().and_then(|r| self.tree_index(&r.path)),
            })
            .flatten();
        let mut done = Vec::new();
        let mut failed = None;
        if requests.iter().all(|r| r.whole_file) {
            // Whole files go to git in one call; patches are for lines.
            let files: Vec<(&str, bool)> = requests
                .iter()
                .map(|r| (r.path.as_str(), r.untracked))
                .collect();
            match apply_files(&self.repo, first.change, &files) {
                Ok(()) => done.extend(requests),
                Err(err) => failed = Some(format!("{err:#}")),
            }
        }
        for request in requests.iter().filter(|r| !r.whole_file) {
            match apply(
                &self.repo,
                &request.diff,
                request.untracked,
                request.change,
                &request.selected,
            ) {
                Ok(()) => done.push(request),
                Err(err) => {
                    failed = Some(format!("{}: {err:#}", request.path));
                    break;
                }
            }
        }
        if let Some(file) = &mut self.file {
            file.anchor = None;
        }
        if let Err(err) = self.refresh() {
            self.error(format!("{err:#}"));
            return;
        }
        let drop: Vec<&Item> = done.iter().flat_map(|r| &r.drop).collect();
        if !drop.is_empty() {
            self.review.items.retain(|i| !drop.contains(&i));
            self.persist();
        }
        if failed.is_none()
            && let Some(row) = after
        {
            self.select_next_unstaged(row);
        }
        if let Some(err) = failed {
            self.error(err);
        } else if self.message.is_none() {
            let verb = verb(first.change);
            if first.whole_file {
                self.info(format!("{verb}d {} file(s)", requests.len()));
            } else {
                let count = first.selected.iter().filter(|s| **s).count();
                self.info(format!("{verb}d {count} line(s)"));
            }
        }
    }
}

impl AppState {
    /// Moves the files-pane cursor, and the diff with it, to the first file
    /// row after `row` that still has unstaged changes, wrapping to the top.
    /// Stays put when no file has any left.
    fn select_next_unstaged(&mut self, row: usize) {
        let len = self.tree.len();
        let next = (1..=len)
            .map(|step| (row + step) % len)
            .find(|r| match &self.tree[*r].entry {
                Entry::File(i) => self
                    .snapshot
                    .find(&self.paths[*i])
                    .is_some_and(|f| self.snapshot.files[f].has_unstaged()),
                Entry::Dir { .. } => false,
            });
        if let Some(next) = next {
            self.tree_cursor = next;
            self.open_under_cursor();
            self.scroll_to_cursor();
        }
    }
}

fn verb(change: Change) -> &'static str {
    match change {
        Change::Stage => "stage",
        Change::Unstage => "unstage",
        Change::Discard => "discard",
    }
}

/// Items in `path` whose anchored lines overlap the selected changes. A
/// removed line counts as touching the new-side line it sits before.
fn touched_items(
    items: &[Item],
    path: &str,
    diff: &crate::git::FileDiff,
    selected: &[bool],
) -> Vec<Item> {
    let mut touched = BTreeSet::new();
    for (i, line) in diff.lines.iter().enumerate() {
        if !selected[i] {
            continue;
        }
        let at = match line.kind {
            LineKind::Removed => diff.lines[i..].iter().find_map(|l| l.new),
            _ => line.new,
        };
        touched.insert(at.unwrap_or(usize::MAX));
    }
    items
        .iter()
        .filter(|item| match &item.scope {
            Scope::Lines {
                path: p,
                start,
                end,
                ..
            } => p == path && touched.range(*start..=*end).next().is_some(),
            _ => false,
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::{
        Pane,
        tests::{press, state},
    };
    use super::*;
    use crate::git::{Target, fixture::Fixture};

    fn repo() -> Fixture {
        let f = Fixture::new();
        let body = crate::git::fixture::numbered(30);
        f.write("a.txt", &body);
        f.commit_all("init");
        let changed = body
            .replace("line 3\n", "line three\n")
            .replace("line 25\n", "line 25!\n");
        f.write("a.txt", &changed);
        f
    }

    #[test]
    fn stage_without_selection_stages_the_hunk_under_the_cursor() {
        let f = repo();
        let mut s = state(&f);
        s.focus = Pane::Diff;
        press(&mut s, "s");
        let index = f.git(&["diff", "--cached"]);
        assert!(
            index.contains("+line three") && !index.contains("line 25!"),
            "{index}"
        );
        let entry = &s.snapshot.files[0];
        assert!(entry.has_staged() && entry.has_unstaged());
    }

    #[test]
    fn unstage_works_on_the_staged_section() {
        let f = repo();
        f.git(&["add", "a.txt"]);
        let mut s = state(&f);
        s.focus = Pane::Diff;
        press(&mut s, "u");
        assert!(f.git(&["diff", "--cached"]).contains("line 25!"));
        assert!(!f.git(&["diff", "--cached"]).contains("line three"));
    }

    #[test]
    fn staging_annotated_lines_asks_and_then_drops_the_annotation() {
        let f = repo();
        let mut s = state(&f);
        s.focus = Pane::Diff;
        press(&mut s, "cbad name\n");
        let note = s
            .file
            .as_ref()
            .unwrap()
            .view
            .rows
            .iter()
            .position(|r| matches!(r, Row::Note(_)))
            .unwrap();
        s.file.as_mut().unwrap().cursor = note - 1;
        press(&mut s, "s");
        assert!(s.confirm.is_some());
        assert!(f.git(&["diff", "--cached"]).is_empty());
        press(&mut s, "y");
        assert!(s.review.items.is_empty());
        let text = std::fs::read_to_string(s.review_path().unwrap()).unwrap();
        assert!(!text.contains("bad name"), "{text}");
    }

    #[test]
    fn discard_asks_before_touching_the_working_tree() {
        let f = repo();
        let mut s = state(&f);
        s.focus = Pane::Diff;
        press(&mut s, "d");
        assert!(f.read("a.txt").contains("line three"));
        press(&mut s, "y");
        assert!(!f.read("a.txt").contains("line three"));
        assert!(f.read("a.txt").contains("line 25!"));
    }

    fn folder() -> Fixture {
        let f = Fixture::new();
        f.write("src/a.rs", "a\n");
        f.write("src/deep/b.rs", "b\n");
        f.write("top.rs", "t\n");
        f.commit_all("init");
        f.write("src/a.rs", "a\nmore\n");
        f.write("src/deep/b.rs", "b\nmore\n");
        f.write("src/new.rs", "n\n");
        f.write("top.rs", "t\nmore\n");
        f
    }

    fn staged(f: &Fixture) -> String {
        f.git(&["diff", "--cached", "--name-only"])
    }

    #[test]
    fn staging_a_directory_from_the_files_pane_stages_every_file_under_it() {
        let f = folder();
        let mut s = state(&f);
        s.focus = Pane::Tree;
        s.tree_cursor = 0;
        assert!(matches!(&s.tree[0].entry, Entry::Dir { path, .. } if path == "src"));
        press(&mut s, "s");
        assert_eq!(staged(&f), "src/a.rs\nsrc/deep/b.rs\nsrc/new.rs\n");
        assert!(s.message.as_ref().unwrap().text.contains("3 file(s)"));
        // Staging moved on to the next unstaged file; go back to the folder.
        assert_eq!(s.tree[s.tree_cursor].label, "top.rs");
        s.tree_cursor = 0;
        press(&mut s, "u");
        assert!(staged(&f).is_empty());
    }

    #[test]
    fn staging_a_file_row_stages_the_whole_file() {
        let f = repo();
        let mut s = state(&f);
        s.focus = Pane::Tree;
        press(&mut s, "s");
        let index = f.git(&["diff", "--cached"]);
        assert!(index.contains("line three") && index.contains("line 25!"));
        assert!(!s.snapshot.files[0].has_unstaged());
    }

    #[test]
    fn staging_a_whole_file_moves_on_to_the_next_file_still_unstaged() {
        let f = folder();
        let mut s = state(&f);
        let open = |s: &AppState| s.file.as_ref().and_then(|f| f.path()).map(str::to_owned);
        let files: Vec<String> = s
            .tree
            .iter()
            .filter_map(|r| match r.entry {
                Entry::File(i) => Some(s.paths[i].clone()),
                Entry::Dir { .. } => None,
            })
            .collect();
        s.focus = Pane::Tree;
        s.tree_cursor = s.tree_index(&files[0]).unwrap();
        press(&mut s, "s");
        assert_eq!(open(&s).as_deref(), Some(files[1].as_str()));
        assert_eq!(s.focus, Pane::Tree);
        // From the diff, on the last file: wraps past the staged first one.
        s.focus = Pane::Diff;
        s.tree_cursor = s.tree_index(&files[3]).unwrap();
        s.open_under_cursor();
        press(&mut s, "gs");
        assert_eq!(open(&s).as_deref(), Some(files[1].as_str()));
        assert_eq!(s.focus, Pane::Diff);
        // A hunk stage keeps the file.
        press(&mut s, "s");
        assert_eq!(open(&s).as_deref(), Some(files[1].as_str()));
    }

    #[test]
    fn whole_file_keys_work_from_the_diff_on_the_open_file() {
        let f = repo();
        let mut s = state(&f);
        s.focus = Pane::Diff;
        press(&mut s, "gs");
        let index = f.git(&["diff", "--cached"]);
        assert!(index.contains("line three") && index.contains("line 25!"));
        assert!(!s.snapshot.files[0].has_unstaged());
        assert_eq!(s.message.as_ref().unwrap().text, "staged 1 file(s)");
        press(&mut s, "gu");
        assert!(f.git(&["diff", "--cached"]).is_empty());
        press(&mut s, "gd");
        assert!(s.confirm.is_some());
        press(&mut s, "y");
        assert!(!f.read("a.txt").contains("line three"));
        assert!(!f.read("a.txt").contains("line 25!"));
    }

    #[test]
    fn discarding_a_directory_asks_and_names_the_file_count() {
        let f = folder();
        let mut s = state(&f);
        s.focus = Pane::Tree;
        s.tree_cursor = 0;
        press(&mut s, "d");
        let question = &s.confirm.as_ref().unwrap().question;
        assert!(question.contains("3 file(s)"), "{question}");
        press(&mut s, "y");
        assert_eq!(f.read("src/a.rs"), "a\n");
        assert!(!f.path().join("src/new.rs").exists());
        assert_eq!(f.read("top.rs"), "t\nmore\n");
    }

    #[test]
    fn range_mode_disables_staging() {
        let f = repo();
        f.commit_all("second");
        let mut s = state(&f);
        s.target = f.repo.commit_range("HEAD").unwrap();
        s.refresh().unwrap();
        assert!(matches!(s.target, Target::Range { .. }));
        s.focus = Pane::Diff;
        press(&mut s, "s");
        press(&mut s, "d");
        assert!(s.confirm.is_none());
        assert!(s.message.as_ref().unwrap().text.contains("read-only"));
        assert!(f.git(&["status", "--porcelain"]).is_empty());
    }
}
