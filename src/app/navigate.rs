//! Cursor movement, selection, expanding context, pane sizes.

use super::{AppState, Pane, TREE_MAX_PERCENT, TREE_MIN_PERCENT, TREE_STEP_PERCENT};
use crate::diff::Row;
use std::ops::Range;

/// How far a scroll key moves, the way nvim's do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scroll {
    /// `C-e` / `C-y`: the view moves, the cursor stays unless pushed off-screen.
    Line,
    /// `C-d` / `C-u`: view and cursor move half a screen.
    HalfPage,
    /// `C-f` / `C-b`: view and cursor move a screen less two lines of overlap.
    Page,
}

impl Scroll {
    fn lines(self, height: usize) -> usize {
        match self {
            Self::Line => 1,
            Self::HalfPage => (height / 2).max(1),
            Self::Page => height.saturating_sub(2).max(1),
        }
    }

    fn moves_cursor(self) -> bool {
        self != Self::Line
    }
}

/// New viewport top and cursor position after scrolling, in visual lines.
/// `starts[r]` is where row `r` begins and `starts[rows]` is the total; rows
/// for which `selectable` is false never hold the cursor.
fn scrolled(
    starts: &[usize],
    selectable: impl Fn(usize) -> bool,
    (top, cursor): (usize, usize),
    height: usize,
    amount: usize,
    down: bool,
    move_cursor: bool,
) -> (usize, usize) {
    let rows = starts.len().saturating_sub(1);
    if rows == 0 {
        return (0, 0);
    }
    let total = starts[rows];
    let shift = |at: usize, limit: usize| {
        if down {
            (at + amount).min(limit)
        } else {
            at.saturating_sub(amount)
        }
    };
    let top = shift(top, total.saturating_sub(height));
    let row_of = |visual: usize| {
        starts
            .partition_point(|s| *s <= visual)
            .saturating_sub(1)
            .min(rows - 1)
    };
    let mut cursor = if move_cursor {
        row_of(shift(starts[cursor], total.saturating_sub(1)))
    } else {
        cursor
    };
    // Keep the cursor on a fully visible selectable row.
    let fits = |r: usize| starts[r] >= top && starts[r + 1] <= top + height;
    if !(fits(cursor) && selectable(cursor)) {
        let visible: Vec<usize> = (0..rows).filter(|r| fits(*r) && selectable(*r)).collect();
        let choice = if starts[cursor] < top {
            visible.first()
        } else if !fits(cursor) {
            visible.last()
        } else {
            visible.iter().min_by_key(|r| r.abs_diff(cursor))
        };
        if let Some(r) = choice {
            cursor = *r;
        }
    }
    (top, cursor)
}

impl AppState {
    /// `C-e`, `C-y`, `C-d`, `C-u`, `C-f`, `C-b`. The scroll keys move the diff from the files and comments panes too,
    /// so a file can be read without leaving the list it was picked from.
    /// The run pane keeps them: they are how its output is paged.
    pub fn scroll(&mut self, by: Scroll, down: bool) {
        if self.focus == Pane::Run {
            let lines = by.lines(self.run_view_size().1).cast_signed();
            self.run_scroll_by(if down { lines } else { -lines });
            return;
        }
        let (_, height) = self.diff_size();
        if let Some(file) = &mut self.file {
            let (top, cursor) = scrolled(
                &file.view.starts,
                |r| file.selectable(r),
                (file.scroll, file.cursor),
                height,
                by.lines(height),
                down,
                by.moves_cursor(),
            );
            file.scroll = top;
            file.cursor = cursor;
        }
        self.scroll_to_cursor();
    }

    pub fn move_by(&mut self, delta: isize) {
        match self.focus {
            Pane::Tree => {
                let last = self.tree.len().saturating_sub(1);
                self.tree_cursor = self.tree_cursor.saturating_add_signed(delta).min(last);
                self.open_under_cursor();
            }
            Pane::Comments => {
                let row = self.comments.cursor.saturating_add_signed(delta);
                self.select_comment_row(row, delta > 0);
            }
            Pane::Run => self.run_scroll_by(delta),
            Pane::Diff => {
                if let Some(file) = &mut self.file {
                    let last = file.view.rows.len().saturating_sub(1);
                    let mut row = file.cursor;
                    loop {
                        let next = row.saturating_add_signed(delta).min(last);
                        if next == row {
                            break;
                        }
                        row = next;
                        if file.selectable(row) {
                            file.cursor = row;
                            break;
                        }
                    }
                }
            }
        }
        self.scroll_to_cursor();
    }

    pub fn move_to(&mut self, end: bool) {
        match self.focus {
            Pane::Tree => {
                self.tree_cursor = if end {
                    self.tree.len().saturating_sub(1)
                } else {
                    0
                };
                self.open_under_cursor();
            }
            Pane::Comments => {
                let row = if end { self.comments.rows.len() } else { 0 };
                self.select_comment_row(row, !end);
            }
            Pane::Run => self.run_scroll_to(end),
            Pane::Diff => {
                if let Some(file) = &mut self.file {
                    file.cursor = if end {
                        file.view.rows.len().saturating_sub(1)
                    } else {
                        0
                    };
                    file.settle_cursor(!end);
                }
            }
        }
        self.scroll_to_cursor();
    }

    /// Moves to the first row of the next or previous block of changed
    /// lines. Staging hunks join blocks a few lines apart; jumping does not,
    /// so a deletion just below an edit is still a stop.
    pub fn jump_hunk(&mut self, forward: bool) {
        if self.focus == Pane::Tree {
            self.move_by(if forward { 1 } else { -1 });
            return;
        }
        let Some(file) = &mut self.file else {
            return;
        };
        let starts: Vec<usize> = file
            .view
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| match row {
                Row::Code { section, lines } => {
                    let diff = &file.sections[*section].diff.lines;
                    lines
                        .iter()
                        .flatten()
                        .any(|i| diff[*i].is_change() && (*i == 0 || !diff[*i - 1].is_change()))
                }
                _ => false,
            })
            .map(|(r, _)| r)
            .collect();
        let target = if forward {
            starts.into_iter().find(|r| *r > file.cursor)
        } else {
            starts.into_iter().rev().find(|r| *r < file.cursor)
        };
        if let Some(row) = target {
            file.cursor = row;
        }
        self.scroll_to_cursor();
    }

    pub fn toggle_select(&mut self) {
        self.focus_diff();
        if let Some(file) = &mut self.file {
            file.anchor = match file.anchor {
                Some(_) => None,
                None => Some(file.cursor),
            };
        }
    }

    /// Reveals hidden lines near the cursor. On a separator both its edges
    /// grow; anywhere else the nearest separator grows from the edge facing
    /// the cursor, so the code next to what you are reading appears.
    pub fn expand(&mut self) {
        let Some(file) = &mut self.file else {
            return;
        };
        let gaps: Vec<(usize, usize, Range<usize>)> = file
            .view
            .rows
            .iter()
            .enumerate()
            .filter_map(|(r, row)| match row {
                Row::Gap {
                    section,
                    start,
                    end,
                } => Some((r, *section, *start..*end)),
                _ => None,
            })
            .collect();
        let cursor = file.cursor;
        if let Some((_, section, gap)) = gaps.iter().find(|(r, ..)| *r == cursor) {
            let (section, gap) = (*section, gap.clone());
            let keep = file.views[section].expand(&gap);
            self.rebuild_rows();
            if let Some(file) = &mut self.file
                && let Some(row) = file.view.row_of(section, keep)
            {
                file.cursor = row;
            }
        } else if let Some((row, section, gap)) = gaps
            .iter()
            .min_by_key(|(r, ..)| (r.abs_diff(cursor), *r < cursor))
        {
            file.views[*section].expand_edge(gap, *row < cursor);
            // Rebuilding keeps the cursor on the row it was on.
            self.rebuild_rows();
        } else {
            self.info("no omitted lines in this file");
            return;
        }
        self.scroll_to_cursor();
    }

    pub fn resize_tree(&mut self, grow_tree: bool) {
        let p = &mut self.layout.tree_percent;
        *p = if grow_tree {
            (*p + TREE_STEP_PERCENT).min(TREE_MAX_PERCENT)
        } else {
            p.saturating_sub(TREE_STEP_PERCENT).max(TREE_MIN_PERCENT)
        };
        self.relayout();
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{press, state};
    use super::*;
    use crate::{
        config::{Key, KeyCodeKey},
        git::fixture::Fixture,
    };

    /// One-line rows `0..100` in a ten-line view.
    fn scroll(
        at: (usize, usize),
        by: Scroll,
        down: bool,
        selectable: fn(usize) -> bool,
    ) -> (usize, usize) {
        let starts: Vec<usize> = (0..=100).collect();
        scrolled(
            &starts,
            selectable,
            at,
            10,
            by.lines(10),
            down,
            by.moves_cursor(),
        )
    }

    fn any(_: usize) -> bool {
        true
    }

    #[test]
    fn line_scroll_moves_the_view_and_only_pushes_the_cursor() {
        assert_eq!(scroll((0, 0), Scroll::Line, true, any), (1, 1));
        assert_eq!(scroll((0, 5), Scroll::Line, true, any), (1, 5));
        assert_eq!(scroll((5, 14), Scroll::Line, false, any), (4, 13));
        assert_eq!(scroll((0, 3), Scroll::Line, false, any), (0, 3));
        assert_eq!(scroll((90, 95), Scroll::Line, true, any), (90, 95));
    }

    #[test]
    fn half_and_full_pages_move_view_and_cursor_together() {
        assert_eq!(scroll((0, 0), Scroll::HalfPage, true, any), (5, 5));
        assert_eq!(scroll((50, 55), Scroll::HalfPage, false, any), (45, 50));
        assert_eq!(scroll((0, 3), Scroll::Page, true, any), (8, 11));
        assert_eq!(scroll((20, 25), Scroll::Page, false, any), (12, 17));
    }

    #[test]
    fn at_the_ends_the_view_stops_but_the_cursor_still_moves() {
        assert_eq!(scroll((90, 92), Scroll::HalfPage, true, any), (90, 97));
        assert_eq!(scroll((90, 97), Scroll::Page, true, any), (90, 99));
        assert_eq!(scroll((0, 6), Scroll::HalfPage, false, any), (0, 1));
    }

    #[test]
    fn the_cursor_never_lands_on_a_row_that_cannot_hold_it() {
        let even = |r: usize| r.is_multiple_of(2);
        assert_eq!(scroll((0, 0), Scroll::Line, true, even), (1, 2));
        assert_eq!(scroll((0, 0), Scroll::HalfPage, true, even), (5, 6));
    }

    fn ctrl(s: &mut AppState, c: char) {
        s.handle_key(Key {
            code: KeyCodeKey::Char(c),
            ctrl: true,
        });
    }

    #[test]
    fn scroll_keys_move_the_diff_from_any_pane_but_the_run_pane() {
        let f = Fixture::new();
        for i in 0..60 {
            f.write(&format!("f{i:02}.txt"), "a\n");
        }
        f.write("a-long.txt", &crate::git::fixture::numbered(400));
        f.commit_all("init");
        for i in 0..60 {
            f.write(&format!("f{i:02}.txt"), "b\n");
        }
        f.write(
            "a-long.txt",
            &crate::git::fixture::numbered(400).replace("line ", "row "),
        );
        let mut s = state(&f);
        s.focus = Pane::Tree;
        let (_, height) = s.diff_size();
        let (tree_cursor, tree_scroll) = (s.tree_cursor, s.tree_scroll);
        ctrl(&mut s, 'f');
        assert_eq!(s.file.as_ref().unwrap().scroll, height - 2);
        assert_eq!((s.tree_cursor, s.tree_scroll), (tree_cursor, tree_scroll));
        assert_eq!(s.focus, Pane::Tree);
        ctrl(&mut s, 'e');
        assert_eq!(s.file.as_ref().unwrap().scroll, height - 1);
        s.focus = Pane::Diff;
        ctrl(&mut s, 'b');
        assert_eq!(s.file.as_ref().unwrap().scroll, 1);
        s.handle_key(Key {
            code: KeyCodeKey::Char('G'),
            ctrl: false,
        });
        s.focus = Pane::Tree;
        s.handle_key(Key {
            code: KeyCodeKey::Char('G'),
            ctrl: false,
        });
        assert_eq!(s.tree_cursor, s.tree.len() - 1);
        s.run_started("agent");
        s.focus = Pane::Run;
        let scroll = s.file.as_ref().unwrap().scroll;
        ctrl(&mut s, 'u');
        assert_eq!(s.file.as_ref().unwrap().scroll, scroll);
    }

    fn long_change() -> Fixture {
        let f = Fixture::new();
        let body = crate::git::fixture::numbered(60);
        f.write("a.txt", &body);
        f.commit_all("init");
        let changed = body
            .replace("line 10\n", "line ten\n")
            .replace("line 50\n", "line fifty\n");
        f.write("a.txt", &changed);
        f
    }

    #[test]
    fn z_on_a_code_line_grows_the_nearest_separator_toward_the_cursor() {
        let f = long_change();
        let mut s = state(&f);
        s.focus = Pane::Diff;
        let text_at = |s: &AppState| {
            let file = s.file.as_ref().unwrap();
            let Row::Code { section, lines } = file.view.rows[file.cursor] else {
                panic!("cursor not on code");
            };
            file.sections[section].diff.lines[lines[0].unwrap()]
                .text
                .clone()
        };
        // The cursor starts on the first change, line 10: the nearest
        // separator is above it, so its last lines (just above line 7) appear.
        let before = text_at(&s);
        let hidden_before = s.file.as_ref().unwrap().views[0]
            .visible
            .iter()
            .filter(|v| !**v)
            .count();
        press(&mut s, "z");
        let file = s.file.as_ref().unwrap();
        let visible = &file.views[0].visible;
        assert_eq!(visible.iter().filter(|v| !**v).count(), hidden_before - 6);
        assert!(visible[0..6].iter().all(|v| *v), "top of the file revealed");
        assert_eq!(text_at(&s), before, "cursor stays on its line");
        // Further down, below the middle separator, z grows that separator's bottom edge.
        press(&mut s, "J");
        let below = text_at(&s);
        press(&mut s, "z");
        let visible = &s.file.as_ref().unwrap().views[0].visible;
        assert!(visible[37..47].iter().all(|v| *v), "{visible:?}");
        assert!(!visible[36], "only ten lines grow: {visible:?}");
        assert_eq!(text_at(&s), below);
    }

    #[test]
    fn expanding_on_the_separator_reveals_real_file_lines() {
        let f = long_change();
        let mut s = state(&f);
        s.focus = Pane::Diff;
        let file = s.file.as_ref().unwrap();
        let gap = file
            .view
            .rows
            .iter()
            .position(|r| matches!(r, Row::Gap { start, .. } if *start > 0))
            .unwrap();
        let shown = file.views[0].visible.iter().filter(|v| **v).count();
        s.file.as_mut().unwrap().cursor = gap;
        press(&mut s, "z");
        let file = s.file.as_ref().unwrap();
        assert_eq!(
            file.views[0].visible.iter().filter(|v| **v).count(),
            shown + 20
        );
        let Row::Code { lines, .. } = file.view.rows[file.cursor] else {
            panic!("cursor not on code after expand");
        };
        let text = &file.sections[0].diff.lines[lines[0].unwrap()].text;
        assert!(text.starts_with("line "), "{text}");
    }

    #[test]
    fn hunk_keys_jump_between_change_groups() {
        let f = long_change();
        let mut s = state(&f);
        s.focus = Pane::Diff;
        let first = s.file.as_ref().unwrap().cursor;
        press(&mut s, "J");
        let second = s.file.as_ref().unwrap().cursor;
        assert!(second > first);
        press(&mut s, "K");
        assert_eq!(s.file.as_ref().unwrap().cursor, first);
    }

    #[test]
    fn hunk_keys_stop_at_each_change_block_even_close_together() {
        let f = Fixture::new();
        let body = crate::git::fixture::numbered(20);
        f.write("a.txt", &body);
        f.commit_all("init");
        // A changed line, four unchanged ones, then a deleted block: one
        // staging hunk, two places to stop.
        let changed = body
            .replace("line 3\n", "line three\n")
            .replace("line 8\nline 9\n", "");
        f.write("a.txt", &changed);
        let mut s = state(&f);
        s.focus = Pane::Diff;
        let text = |s: &AppState| {
            let file = s.file.as_ref().unwrap();
            let line = file.view.rows[file.cursor].lines().next().unwrap();
            file.sections[0].diff.lines[line].text.clone()
        };
        assert_eq!(text(&s), "line 3");
        press(&mut s, "J");
        assert_eq!(text(&s), "line 8");
        press(&mut s, "K");
        assert_eq!(text(&s), "line 3");
    }

    #[test]
    fn grow_and_shrink_keep_the_files_pane_within_bounds() {
        let f = long_change();
        let mut s = state(&f);
        for _ in 0..20 {
            press(&mut s, "+");
        }
        assert_eq!(s.layout.tree_percent, TREE_MIN_PERCENT);
        for _ in 0..20 {
            press(&mut s, "_");
        }
        assert_eq!(s.layout.tree_percent, TREE_MAX_PERCENT);
        press(&mut s, "Z");
        assert!(s.layout.zoom);
        assert_eq!(s.focus, Pane::Diff);
    }
}
