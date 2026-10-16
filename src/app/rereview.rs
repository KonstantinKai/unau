//! Re-review: finding anchors again after the agent changed the file,
//! showing what changed, and accepting a fix by staging it.

use super::{AppState, NoteLine, NoteStyle};
use crate::{
    git::{Change, FileDiff, LineKind, SectionKind, apply},
    review::{Item, Scope},
};
use std::collections::HashMap;

/// New-side lines `start..=end`, one-based.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub start: usize,
    pub end: usize,
}

/// Where an item's lines are in `new_text` now. Tries the anchor, then the
/// quoted lines anywhere (nearest wins), then the first or last quoted line
/// alone, then the anchor itself if a change in `changed` (new-side lines
/// touched by the unstaged diff) lands on it. `None` means the anchor is lost.
pub fn find_region(item: &Item, new_text: &[String], changed: &[usize]) -> Option<Region> {
    let Scope::Lines {
        start,
        end,
        removed,
        ..
    } = &item.scope
    else {
        return None;
    };
    let (start, end) = (*start, *end);
    if *removed || item.quote.is_empty() {
        return (start >= 1 && start <= new_text.len() + 1).then_some(Region { start, end });
    }
    // A capped quote is its head at the region's start and its tail at its end.
    let q = item.quote_len();
    let (head, tail) = item.elided.map_or((item.quote.as_slice(), &[][..]), |e| {
        item.quote.split_at(e.at.min(item.quote.len()))
    });
    let at = |p: usize| {
        new_text.get(p..p + head.len()).is_some_and(|w| w == head)
            && (p + q)
                .checked_sub(tail.len())
                .and_then(|t| new_text.get(t..p + q))
                .is_some_and(|w| w == tail)
    };
    let nearest =
        |candidates: Vec<usize>| candidates.into_iter().min_by_key(|p| p.abs_diff(start - 1));
    if at(start - 1) {
        return Some(Region { start, end });
    }
    if let Some(p) = nearest((0..new_text.len()).filter(|p| at(*p)).collect()) {
        return Some(Region {
            start: p + 1,
            end: p + q,
        });
    }
    let first = &item.quote[0];
    let last = &item.quote[item.quote.len() - 1];
    let mut candidates: Vec<(usize, Region)> = Vec::new();
    for (p, line) in new_text.iter().enumerate() {
        if line == first && q > 1 {
            let end = (p + q).min(new_text.len());
            candidates.push((p.abs_diff(start - 1), Region { start: p + 1, end }));
        }
        if line == last && q > 1 && p + 1 >= q {
            let s = p + 2 - q;
            candidates.push((
                s.abs_diff(start),
                Region {
                    start: s,
                    end: p + 1,
                },
            ));
        }
    }
    if let Some((_, region)) = candidates.into_iter().min_by_key(|(d, _)| *d) {
        return Some(region);
    }
    // The quoted lines were all rewritten in place: the change sits on the anchor.
    changed
        .iter()
        .any(|n| (start..=end).contains(n))
        .then(|| Region {
            start,
            end: end.min(new_text.len()).max(start),
        })
}

/// New-side lines touched by the changes of `diff`, sorted.
pub fn changed_lines(diff: &FileDiff) -> Vec<usize> {
    let touched = touched_new(diff);
    let mut lines: Vec<usize> = (0..diff.lines.len())
        .filter(|i| diff.lines[*i].is_change())
        .map(|i| touched[i])
        .collect();
    lines.dedup();
    lines
}

/// Maps index lines to the working-tree lines they became, given the
/// unstaged diff (index → working tree, whole file as context). A line the
/// working tree removed maps to the line that now sits in its place.
pub fn index_to_worktree(unstaged: &FileDiff) -> impl Fn(usize) -> Option<usize> {
    let touched = touched_new(unstaged);
    let mut by_old = HashMap::new();
    for (i, line) in unstaged.lines.iter().enumerate() {
        if let Some(old) = line.old {
            by_old.entry(old).or_insert(touched[i]);
        }
    }
    move |line| by_old.get(&line).copied()
}

/// What changed at an item's lines: its quote against the region now. A
/// capped quote compares its head and tail with the region's first and last
/// lines, with the marker between.
pub fn quote_diff(item: &Item, now: &[String]) -> Vec<NoteLine> {
    let Some(elision) = item.elided else {
        return line_diff(&item.quote, now);
    };
    let (head, tail) = item.quote.split_at(elision.at.min(item.quote.len()));
    let now_head = &now[..head.len().min(now.len())];
    let now_tail = &now[now.len().saturating_sub(tail.len()).max(now_head.len())..];
    let mut lines = line_diff(head, now_head);
    lines.push(NoteLine {
        text: format!("  {}", elision.marker()),
        style: NoteStyle::Same,
    });
    lines.extend(line_diff(tail, now_tail));
    lines
}

/// Line diff of `then` against `now`, for showing what the agent changed.
pub fn line_diff(then: &[String], now: &[String]) -> Vec<NoteLine> {
    let (n, m) = (then.len(), now.len());
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if then[i] == now[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    let line = |prefix: char, text: &str, style| NoteLine {
        text: format!("{prefix} {text}"),
        style,
    };
    while i < n || j < m {
        if i < n && j < m && then[i] == now[j] {
            out.push(line(' ', &then[i], NoteStyle::Same));
            i += 1;
            j += 1;
        } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            out.push(line('+', &now[j], NoteStyle::Added));
            j += 1;
        } else {
            out.push(line('-', &then[i], NoteStyle::Removed));
            i += 1;
        }
    }
    out
}

/// New-side position each change line touches: its own number, or for a
/// removed line the new line it sits before.
fn touched_new(diff: &FileDiff) -> Vec<usize> {
    // One pass from the end, so a long run of removals costs its length
    // rather than its length squared.
    let past_end = diff.lines.iter().filter_map(|l| l.new).max().unwrap_or(0) + 1;
    let mut next_new = past_end;
    let mut touched: Vec<usize> = diff
        .lines
        .iter()
        .rev()
        .map(|line| {
            if let Some(new) = line.new {
                next_new = new;
            }
            match line.kind {
                LineKind::Removed => next_new,
                _ => line.new.unwrap_or(0),
            }
        })
        .collect();
    touched.reverse();
    touched
}

impl AppState {
    /// Accepts the done item under the cursor: stages the hunks that touch its
    /// lines and removes it from the review.
    pub fn accept(&mut self) {
        let Some(file) = &self.file else {
            return;
        };
        let Some(n) = file.note_under_cursor() else {
            self.error("put the cursor on a done annotation");
            return;
        };
        let item = self.review.items[n].clone();
        if !item.done {
            self.error("not done yet: the agent has not ticked this item");
            return;
        }
        let region = match &item.scope {
            Scope::Lines { .. } => {
                let Some(region) = find_region(&item, &file.new_text, &file.changed_lines()) else {
                    self.error("anchor lost: stage the fix with s, then delete the item with x");
                    return;
                };
                Some(region)
            }
            Scope::File(_) => Some(Region {
                start: 1,
                end: usize::MAX,
            }),
            Scope::General => None,
        };
        let section = file
            .sections
            .iter()
            .position(|s| s.kind == SectionKind::Unstaged);
        let mut staged = 0;
        if let (Some(region), Some(s), false) = (region, section, self.target.is_range()) {
            let diff = &file.sections[s].diff;
            let mut selected = vec![false; diff.lines.len()];
            let new_lines = touched_new(diff);
            for hunk in &file.views[s].hunks {
                let touches = hunk.clone().any(|i| {
                    diff.lines[i].is_change() && (region.start..=region.end).contains(&new_lines[i])
                });
                if touches {
                    for i in hunk.clone() {
                        selected[i] = diff.lines[i].is_change();
                    }
                }
            }
            if diff.binary && region.end == usize::MAX {
                selected = vec![true; diff.lines.len().max(1)];
            }
            staged = selected.iter().filter(|s| **s).count();
            if staged > 0
                && let Err(err) = apply(&self.repo, diff, file.untracked, Change::Stage, &selected)
            {
                self.error(format!("{err:#}"));
                return;
            }
        }
        self.review.items.retain(|i| *i != item);
        if let Err(err) = self.refresh() {
            self.error(format!("{err:#}"));
        }
        self.persist();
        if self.message.is_none() {
            if staged > 0 {
                self.info(format!("accepted: staged {staged} line(s)"));
            } else {
                self.info("accepted: its fix was already staged");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        Pane, Store,
        tests::{press, state},
    };
    use super::*;
    use crate::{
        diff::Row,
        git::fixture::Fixture,
        review::{Kind, write},
    };

    fn texts(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|l| (*l).to_owned()).collect()
    }

    fn lines_item(start: usize, end: usize, quote: &[&str]) -> Item {
        let mut item = Item::new(
            Kind::Fix,
            Scope::Lines {
                path: "a.txt".into(),
                start,
                end,
                removed: false,
            },
            "x".into(),
        );
        item.quote = texts(quote);
        item
    }

    #[test]
    fn region_follows_quoted_lines_that_moved() {
        let now = texts(&["new", "new", "a", "b", "c"]);
        let item = lines_item(1, 2, &["a", "b"]);
        assert_eq!(
            find_region(&item, &now, &[]),
            Some(Region { start: 3, end: 4 })
        );
    }

    #[test]
    fn region_survives_a_changed_middle_and_is_lost_when_all_lines_changed() {
        let now = texts(&["a", "B", "c"]);
        assert_eq!(
            find_region(&lines_item(1, 3, &["a", "b", "c"]), &now, &[]),
            Some(Region { start: 1, end: 3 })
        );
        assert_eq!(find_region(&lines_item(1, 1, &["z"]), &now, &[]), None);
    }

    #[test]
    fn a_capped_quote_is_found_by_its_ends_after_it_moved() {
        let mut item = lines_item(1, 20, &[]);
        let original: Vec<String> = (1..=20).map(|n| format!("l{n}")).collect();
        item.set_quote(original.clone(), 12);
        let mut now = texts(&["new a", "new b"]);
        now.extend(original);
        assert_eq!(
            find_region(&item, &now, &[]),
            Some(Region { start: 3, end: 22 })
        );
        let diff = quote_diff(&item, &now[2..]);
        assert!(diff.iter().any(|l| l.text.contains("⋯ 14 more lines ⋯")));
        assert!(diff.iter().all(|l| matches!(l.style, NoteStyle::Same)));
    }

    #[test]
    fn a_rewritten_line_is_found_where_the_change_sits() {
        let now = texts(&["a", "B", "c"]);
        assert_eq!(
            find_region(&lines_item(2, 2, &["b"]), &now, &[2]),
            Some(Region { start: 2, end: 2 })
        );
        assert_eq!(find_region(&lines_item(2, 2, &["b"]), &now, &[3]), None);
    }

    #[test]
    fn line_diff_marks_kept_removed_and_added_lines() {
        let diff = line_diff(&texts(&["a", "b"]), &texts(&["a", "c"]));
        let styles: Vec<_> = diff.iter().map(|l| l.style).collect();
        assert_eq!(
            styles,
            [NoteStyle::Same, NoteStyle::Added, NoteStyle::Removed]
        );
    }

    /// A review written at one point, then the agent edits the file and ticks
    /// the item; the tool reopens it with `--review`.
    fn reviewed(f: &Fixture) -> super::super::AppState {
        let body = crate::git::fixture::numbered(20);
        f.write("a.txt", &body);
        f.commit_all("init");
        f.write("a.txt", &body.replace("line 5\n", "line 5 bad\n"));
        let mut s = state(f);
        s.focus = Pane::Diff;
        let file = s.file.as_mut().unwrap();
        let line = file.sections[0]
            .diff
            .lines
            .iter()
            .position(|l| l.text == "line 5 bad")
            .unwrap();
        file.cursor = file.view.row_of(0, line).unwrap();
        press(&mut s, "cname it better\n");
        let path = s.review_path().unwrap().clone();
        f.write("a.txt", &body.replace("line 5\n", "line 5 good\nline 5b\n"));
        let mut review = s.review.clone();
        review.items[0].done = true;
        std::fs::write(&path, write(&review)).unwrap();
        let (review, store) = super::super::persist::open(path).unwrap();
        let mut s = super::super::AppState::new(
            f.repo.clone(),
            crate::git::Target::WorkTree,
            crate::git::snapshot(&f.repo, &crate::git::Target::WorkTree).unwrap(),
            crate::config::Config::default(),
            review,
            store,
        );
        assert!(matches!(s.store, Store::File { path: Some(_), .. }));
        s.resize(120, 40);
        s.focus = Pane::Diff;
        s
    }

    #[test]
    fn reopened_review_shows_done_items_with_what_changed() {
        let f = Fixture::new();
        let s = reviewed(&f);
        assert!(s.changed_since_review.contains("a.txt"));
        let file = s.file.as_ref().unwrap();
        let notes = &file.notes[&0];
        assert!(matches!(
            notes[0].style,
            NoteStyle::Header { done: true, .. }
        ));
        assert!(
            notes
                .iter()
                .any(|l| l.style == NoteStyle::Added && l.text.contains("line 5 good"))
        );
        assert!(
            notes
                .iter()
                .any(|l| l.style == NoteStyle::Removed && l.text.contains("line 5 bad"))
        );
    }

    #[test]
    fn accepting_a_done_item_stages_its_hunk_and_removes_it() {
        let f = Fixture::new();
        let mut s = reviewed(&f);
        let note = s
            .file
            .as_ref()
            .unwrap()
            .view
            .rows
            .iter()
            .position(|r| matches!(r, Row::Note(_)))
            .unwrap();
        s.file.as_mut().unwrap().cursor = note;
        press(&mut s, "a");
        assert!(s.review.items.is_empty(), "{:?}", s.message);
        assert!(f.git(&["diff", "--cached"]).contains("+line 5 good"));
        assert!(s.complete());
    }

    #[test]
    fn an_item_whose_rewritten_fix_is_already_staged_is_found_and_closes() {
        let f = Fixture::new();
        let mut s = reviewed(&f);
        // Accepting a neighbour stages the whole hunk, this fix with it.
        f.git(&["add", "a.txt"]);
        s.refresh().unwrap();
        let file = s.file.as_ref().unwrap();
        let note = file
            .view
            .rows
            .iter()
            .position(|r| matches!(r, Row::Note(_)))
            .unwrap();
        let shown = &file.notes[&0];
        assert!(
            !shown.iter().any(|l| l.text.contains("anchor lost")),
            "{shown:?}"
        );
        s.file.as_mut().unwrap().cursor = note;
        press(&mut s, "a");
        assert!(s.review.items.is_empty(), "{:?}", s.message);
        // The last item closing completes the review, which says so instead.
        assert!(s.complete());
    }

    #[test]
    fn index_lines_map_to_where_the_working_tree_has_them() {
        let f = Fixture::new();
        f.write("a.txt", "one\ntwo\nthree\n");
        f.commit_all("init");
        f.write("a.txt", "new\none\nthree\n");
        let snap = crate::git::snapshot(&f.repo, &crate::git::Target::WorkTree).unwrap();
        let diff = &snap.files[0].sections[0].diff;
        let worktree = index_to_worktree(diff);
        assert_eq!(worktree(1), Some(2));
        // `two` is gone; `three` now stands where it was.
        assert_eq!(worktree(2), Some(3));
        assert_eq!(worktree(3), Some(3));
    }

    #[test]
    fn removed_lines_touch_the_next_new_line_or_the_end_of_the_file() {
        let f = Fixture::new();
        f.write("a.txt", "one\ntwo\nthree\nfour\nfive\n");
        f.commit_all("init");
        f.write("a.txt", "one\nTHREE\nfour\n");
        let snap = crate::git::snapshot(&f.repo, &crate::git::Target::WorkTree).unwrap();
        let diff = &snap.files[0].sections[0].diff;
        // `two` and `three` give way to `THREE` on line 2; `five` was last.
        assert_eq!(changed_lines(diff), [2, 4]);
    }

    #[test]
    fn a_long_run_of_removed_lines_is_mapped_in_linear_time() {
        let f = Fixture::new();
        f.write("a.txt", &crate::git::fixture::numbered(50_000));
        f.commit_all("init");
        f.write("a.txt", "kept\n");
        let snap = crate::git::snapshot(&f.repo, &crate::git::Target::WorkTree).unwrap();
        let diff = &snap.files[0].sections[0].diff;
        let started = std::time::Instant::now();
        assert_eq!(changed_lines(diff), [1]);
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }

    #[test]
    fn pushing_back_replaces_the_done_item_with_an_open_one() {
        let f = Fixture::new();
        let mut s = reviewed(&f);
        let note = s
            .file
            .as_ref()
            .unwrap()
            .view
            .rows
            .iter()
            .position(|r| matches!(r, Row::Note(_)))
            .unwrap();
        s.file.as_mut().unwrap().cursor = note;
        press(&mut s, "cstill unclear\n");
        assert_eq!(s.review.items.len(), 1);
        let item = &s.review.items[0];
        assert!(!item.done);
        assert_eq!(item.text, "still unclear");
        assert_eq!(s.review.status, crate::review::Status::Open);
        let text = std::fs::read_to_string(s.review_path().unwrap()).unwrap();
        assert!(text.contains("status: open"), "{text}");
    }
}
