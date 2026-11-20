//! Re-review: finding anchors again after the agent changed the file,
//! showing what changed, and accepting a fix by staging it.

use super::{NoteLine, NoteStyle, reviewed::Reviewed};
use crate::{
    git::{FileDiff, LineKind},
    review::{Item, Scope},
};
use std::collections::HashMap;

/// New-side lines `start..=end`, one-based.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub start: usize,
    pub end: usize,
}

/// Whether `item`'s quote sits in `text` from zero-based line `p`. A capped
/// quote is its head at `p` and its tail at the end of the lines it stood for.
pub fn quote_at(item: &Item, text: &[String], p: usize) -> bool {
    let q = item.quote_len();
    let (head, tail) = item.elided.map_or((item.quote.as_slice(), &[][..]), |e| {
        item.quote.split_at(e.at.min(item.quote.len()))
    });
    text.get(p..p + head.len()).is_some_and(|w| w == head)
        && (p + q)
            .checked_sub(tail.len())
            .and_then(|t| text.get(t..p + q))
            .is_some_and(|w| w == tail)
}

/// Where an item's lines are in `new_text` now. Tries the anchor, then the
/// file as it was reviewed (`reviewed`), mapped line by line to now, then the
/// quoted lines anywhere (nearest wins), then the first or last quoted line
/// alone, then the changed lines that read most like the quote, then the
/// anchor itself if a change in `changed` (new-side lines the changes touch,
/// staged ones included) lands on it. `None` means the anchor is lost.
pub fn find_region(
    item: &Item,
    new_text: &[String],
    reviewed: Option<&Reviewed>,
    changed: &[usize],
) -> Option<Region> {
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
    let q = item.quote_len();
    let at = |p: usize| quote_at(item, new_text, p);
    let nearest =
        |candidates: Vec<usize>| candidates.into_iter().min_by_key(|p| p.abs_diff(start - 1));
    if at(start - 1) {
        return Some(Region { start, end });
    }
    if let Some(region) = reviewed.and_then(|r| r.map(item)) {
        return Some(region);
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
    // Edited, and maybe moved too: the changed stretch most like the quote.
    if let Some(region) = super::likeness::closest(item, new_text, changed) {
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

/// New-side position each line touches: its own number, or for a removed
/// line the new line it sits before (one past the last when none follows).
pub(super) fn touched_new(diff: &FileDiff) -> Vec<usize> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{git::fixture::Fixture, review::Kind};

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
            find_region(&item, &now, None, &[]),
            Some(Region { start: 3, end: 4 })
        );
    }

    #[test]
    fn region_survives_a_changed_middle_and_is_lost_when_all_lines_changed() {
        let now = texts(&["a", "B", "c"]);
        assert_eq!(
            find_region(&lines_item(1, 3, &["a", "b", "c"]), &now, None, &[]),
            Some(Region { start: 1, end: 3 })
        );
        assert_eq!(
            find_region(&lines_item(1, 1, &["z"]), &now, None, &[]),
            None
        );
    }

    #[test]
    fn a_capped_quote_is_found_by_its_ends_after_it_moved() {
        let mut item = lines_item(1, 20, &[]);
        let original: Vec<String> = (1..=20).map(|n| format!("l{n}")).collect();
        item.set_quote(original.clone(), 12);
        let mut now = texts(&["new a", "new b"]);
        now.extend(original);
        assert_eq!(
            find_region(&item, &now, None, &[]),
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
            find_region(&lines_item(2, 2, &["b"]), &now, None, &[2]),
            Some(Region { start: 2, end: 2 })
        );
        assert_eq!(
            find_region(&lines_item(2, 2, &["b"]), &now, None, &[3]),
            None
        );
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
}
