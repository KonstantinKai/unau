//! Display model for one file: which source lines are visible, how they map
//! to display rows, where hunks start, and how long rows wrap.

mod embedded;
mod highlight;
mod rows;
mod wrap;

pub use highlight::{Highlighting, Spans, builtin_syntax_exists};
pub use rows::{NotePlacement, Row, View, build_rows};
pub use wrap::{TAB_WIDTH, display_width, wrap_ranges, wrap_words};

use crate::git::{Line, Section};
use std::ops::Range;

/// Context lines shown around each change before the reader asks for more.
pub const CONTEXT: usize = 3;
/// Lines one press of "expand nearby" reveals on each edge of a gap.
pub const EXPAND_STEP: usize = 10;

/// Visibility of one section's lines. The section already carries the whole
/// file, so expanding only flips flags; it never re-reads git.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionView {
    pub visible: Vec<bool>,
    /// Groups of changed lines, as `[first change, last change]` index ranges.
    pub hunks: Vec<Range<usize>>,
}

impl SectionView {
    pub fn new(lines: &[Line]) -> Self {
        let mut visible = vec![false; lines.len()];
        for (i, line) in lines.iter().enumerate() {
            if line.is_change() {
                let start = i.saturating_sub(CONTEXT);
                let end = (i + CONTEXT + 1).min(lines.len());
                visible[start..end].iter_mut().for_each(|v| *v = true);
            }
        }
        Self {
            visible,
            hunks: hunks(lines),
        }
    }

    /// Reveals up to `EXPAND_STEP` hidden lines on each edge of the hidden run
    /// `gap`. Returns the line to keep the cursor on.
    pub fn expand(&mut self, gap: &Range<usize>) -> usize {
        let top = gap.start..(gap.start + EXPAND_STEP).min(gap.end);
        let bottom = gap.end.saturating_sub(EXPAND_STEP).max(gap.start)..gap.end;
        // A gap at the start of the file only grows upwards from the code below
        // it; a gap at the end only grows downwards from the code above it.
        if gap.start > 0 {
            self.visible[top.clone()].iter_mut().for_each(|v| *v = true);
        }
        if gap.end < self.visible.len() {
            self.visible[bottom.clone()]
                .iter_mut()
                .for_each(|v| *v = true);
        }
        if gap.start == 0 && gap.end == self.visible.len() {
            self.visible[top.clone()].iter_mut().for_each(|v| *v = true);
        }
        if gap.start > 0 {
            top.start
        } else {
            bottom.start
        }
    }

    /// Reveals up to `EXPAND_STEP` hidden lines at one edge of the hidden run
    /// `gap`: its last lines when `at_end`, its first lines otherwise.
    pub fn expand_edge(&mut self, gap: &Range<usize>, at_end: bool) {
        let lines = if at_end {
            gap.end.saturating_sub(EXPAND_STEP).max(gap.start)..gap.end
        } else {
            gap.start..(gap.start + EXPAND_STEP).min(gap.end)
        };
        self.visible[lines].iter_mut().for_each(|v| *v = true);
    }

    /// Index of the hunk that contains line `i`, if any.
    pub fn hunk_of(&self, i: usize) -> Option<usize> {
        self.hunks.iter().position(|h| h.contains(&i))
    }
}

/// Changed lines grouped the way `git diff -U3` would group them: two runs of
/// changes join when at most `2 * CONTEXT` context lines separate them.
fn hunks(lines: &[Line]) -> Vec<Range<usize>> {
    let mut hunks: Vec<Range<usize>> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !line.is_change() {
            continue;
        }
        match hunks.last_mut() {
            Some(last) if i - last.end <= 2 * CONTEXT => last.end = i + 1,
            _ => hunks.push(i..i + 1),
        }
    }
    hunks
}

/// Visibility state for every section of a file, in section order.
pub fn section_views(sections: &[Section]) -> Vec<SectionView> {
    sections
        .iter()
        .map(|s| SectionView::new(&s.diff.lines))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{Line, LineKind};

    pub fn lines(spec: &str) -> Vec<Line> {
        let (mut old, mut new) = (0, 0);
        spec.chars()
            .map(|c| {
                let kind = match c {
                    '+' => LineKind::Added,
                    '-' => LineKind::Removed,
                    _ => LineKind::Context,
                };
                let o = (kind != LineKind::Added).then(|| {
                    old += 1;
                    old
                });
                let n = (kind != LineKind::Removed).then(|| {
                    new += 1;
                    new
                });
                Line {
                    kind,
                    old: o,
                    new: n,
                    text: format!("{c}{}", o.or(n).unwrap_or(0)),
                    no_newline: false,
                }
            })
            .collect()
    }

    #[test]
    fn shows_three_lines_of_context_around_changes() {
        let view = SectionView::new(&lines("..........+.........."));
        let shown: Vec<_> = (0..21).filter(|i| view.visible[*i]).collect();
        assert_eq!(shown, (7..14).collect::<Vec<_>>());
    }

    #[test]
    fn nearby_changes_join_one_hunk_and_distant_ones_do_not() {
        let view = SectionView::new(&lines("+......+........+"));
        assert_eq!(view.hunks, [0..8, 16..17]);
    }

    #[test]
    fn expanding_a_gap_reveals_lines_from_both_edges() {
        let mut view = SectionView::new(&lines(&format!("+{}+", ".".repeat(40))));
        view.expand(&(4..38));
        let hidden: Vec<_> = (0..42).filter(|i| !view.visible[*i]).collect();
        assert_eq!(hidden, (14..28).collect::<Vec<_>>());
    }
}
