//! Display rows and the display-row to source-line mapping.

use super::SectionView;
use crate::git::{LineKind, Section};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// Title strip above a section (`unstaged`, `staged`).
    Section(usize),
    /// Source lines of one section. Unified rows hold the same line on both
    /// sides; side-by-side rows pair a removed line with an added one.
    Code {
        section: usize,
        lines: [Option<usize>; 2],
    },
    /// Hidden context lines `start..end` of a section.
    Gap {
        section: usize,
        start: usize,
        end: usize,
    },
    /// A binary file section; there are no lines to show.
    Binary(usize),
    /// An annotation, by index into the review's items.
    Note(usize),
}

impl Row {
    /// Distinct source lines on this row.
    pub fn lines(&self) -> impl Iterator<Item = usize> {
        let pair = match self {
            Self::Code { lines, .. } => *lines,
            _ => [None, None],
        };
        let second = pair[1].filter(|n| Some(*n) != pair[0]);
        pair[0].into_iter().chain(second)
    }

    pub fn section(&self) -> Option<usize> {
        match self {
            Self::Section(s) | Self::Binary(s) => Some(*s),
            Self::Code { section, .. } | Self::Gap { section, .. } => Some(*section),
            Self::Note(_) => None,
        }
    }
}

/// Where an annotation is drawn: after `line` of `section`, or at the top of
/// the file when `line` is `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotePlacement {
    pub item: usize,
    pub section: usize,
    pub line: Option<usize>,
}

/// Rows for a file. `titles` adds a title strip per section, used when a file
/// has more than one.
pub fn build_rows(
    sections: &[Section],
    views: &[SectionView],
    split: bool,
    titles: bool,
    notes: &[NotePlacement],
) -> Vec<Row> {
    let mut rows: Vec<Row> = notes
        .iter()
        .filter(|n| n.line.is_none())
        .map(|n| Row::Note(n.item))
        .collect();
    for (s, (section, view)) in sections.iter().zip(views).enumerate() {
        if titles {
            rows.push(Row::Section(s));
        }
        if section.diff.binary {
            rows.push(Row::Binary(s));
            continue;
        }
        let lines = &section.diff.lines;
        let mut i = 0;
        while i < lines.len() {
            if !view.visible[i] {
                let start = i;
                while i < lines.len() && !view.visible[i] {
                    i += 1;
                }
                rows.push(Row::Gap {
                    section: s,
                    start,
                    end: i,
                });
                continue;
            }
            let start_row = rows.len();
            if split && lines[i].is_change() {
                let (mut old, mut new) = (Vec::new(), Vec::new());
                while i < lines.len() && view.visible[i] && lines[i].is_change() {
                    if lines[i].kind == LineKind::Removed {
                        old.push(i);
                    } else {
                        new.push(i);
                    }
                    i += 1;
                }
                for j in 0..old.len().max(new.len()) {
                    rows.push(Row::Code {
                        section: s,
                        lines: [old.get(j).copied(), new.get(j).copied()],
                    });
                }
            } else {
                rows.push(Row::Code {
                    section: s,
                    lines: [Some(i), Some(i)],
                });
                i += 1;
            }
            // Notes go under the last row that carries their line.
            let block: Vec<usize> = (start_row..rows.len()).collect();
            for row in block.into_iter().rev() {
                let carried: Vec<usize> = rows[row].lines().collect();
                let mut after = Vec::new();
                for note in notes.iter().filter(|n| n.section == s) {
                    if note.line.is_some_and(|l| carried.contains(&l)) {
                        after.push(Row::Note(note.item));
                    }
                }
                let at = rows.len().min(insert_point(&rows, row));
                for (k, note) in after.into_iter().enumerate() {
                    rows.insert(at + k, note);
                }
            }
        }
    }
    rows
}

/// Notes for a row go right after it and after any notes already placed there.
fn insert_point(rows: &[Row], row: usize) -> usize {
    let mut at = row + 1;
    while at < rows.len() && matches!(rows[at], Row::Note(_)) {
        at += 1;
    }
    at
}

/// Rows with their wrapped heights. Geometry is computed in the update step so
/// rendering never has to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    pub rows: Vec<Row>,
    /// `starts[r]` is the first visual line of row `r`; one extra entry holds the total.
    pub starts: Vec<usize>,
}

impl View {
    pub fn new(rows: Vec<Row>) -> Self {
        let starts = (0..=rows.len()).collect();
        Self { rows, starts }
    }

    pub fn layout(&mut self, height: impl Fn(&Row) -> usize) {
        self.starts.clear();
        self.starts.push(0);
        for row in &self.rows {
            let last = *self.starts.last().unwrap_or(&0);
            self.starts.push(last + height(row).max(1));
        }
    }

    pub fn total(&self) -> usize {
        *self.starts.last().unwrap_or(&0)
    }

    pub fn height(&self, row: usize) -> usize {
        self.starts.get(row + 1).unwrap_or(&0) - self.starts.get(row).unwrap_or(&0)
    }

    /// The row a visual line belongs to.
    pub fn row_at(&self, visual: usize) -> Option<usize> {
        if visual >= self.total() {
            return None;
        }
        Some(self.starts.partition_point(|s| *s <= visual) - 1)
    }

    /// The row that shows `line` of `section`.
    pub fn row_of(&self, section: usize, line: usize) -> Option<usize> {
        self.rows.iter().position(|r| {
            matches!(r, Row::Code { section: s, .. } if *s == section)
                && r.lines().any(|l| l == line)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::tests::lines;
    use crate::git::{FileDiff, SectionKind};

    fn section(spec: &str) -> Section {
        Section {
            kind: SectionKind::Unstaged,
            diff: FileDiff {
                path: "a.txt".into(),
                lines: lines(spec),
                ..FileDiff::default()
            },
        }
    }

    fn bijective(sections: &[Section], views: &[SectionView], rows: &[Row]) {
        for (s, view) in views.iter().enumerate() {
            for (i, shown) in view.visible.iter().enumerate() {
                let carriers = rows
                    .iter()
                    .filter(|r| r.section() == Some(s) && r.lines().any(|l| l == i))
                    .count();
                assert_eq!(carriers, usize::from(*shown), "line {i} of section {s}");
            }
        }
        assert!(sections.len() == views.len());
    }

    #[test]
    fn every_visible_line_maps_to_exactly_one_row_in_both_layouts() {
        let sections = [section(&format!(
            "..+-+{}-.+{}",
            ".".repeat(12),
            ".".repeat(9)
        ))];
        let mut views = crate::diff::section_views(&sections);
        for split in [false, true] {
            let rows = build_rows(&sections, &views, split, false, &[]);
            bijective(&sections, &views, &rows);
        }
        let gap = rows_gap(&build_rows(&sections, &views, false, false, &[]));
        views[0].expand(&gap);
        for split in [false, true] {
            let rows = build_rows(&sections, &views, split, false, &[]);
            bijective(&sections, &views, &rows);
        }
    }

    fn rows_gap(rows: &[Row]) -> std::ops::Range<usize> {
        rows.iter()
            .find_map(|r| match r {
                Row::Gap { start, end, .. } => Some(*start..*end),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn side_by_side_pairs_removed_and_added_lines() {
        let sections = [section(".--+.")];
        let views = crate::diff::section_views(&sections);
        let rows = build_rows(&sections, &views, true, false, &[]);
        let pairs: Vec<_> = rows
            .iter()
            .map(|r| match r {
                Row::Code { lines, .. } => *lines,
                _ => [None, None],
            })
            .collect();
        assert_eq!(
            pairs,
            [
                [Some(0), Some(0)],
                [Some(1), Some(3)],
                [Some(2), None],
                [Some(4), Some(4)]
            ]
        );
    }

    #[test]
    fn notes_sit_under_their_line_and_file_notes_on_top() {
        let sections = [section(".+.")];
        let views = crate::diff::section_views(&sections);
        let notes = [
            NotePlacement {
                item: 7,
                section: 0,
                line: Some(1),
            },
            NotePlacement {
                item: 3,
                section: 0,
                line: None,
            },
        ];
        let rows = build_rows(&sections, &views, false, false, &notes);
        assert_eq!(rows[0], Row::Note(3));
        assert_eq!(rows[3], Row::Note(7));
    }

    #[test]
    fn wrapped_rows_map_visual_lines_back_to_rows() {
        let mut view = View::new(vec![Row::Section(0); 3]);
        view.layout(|_| 2);
        assert_eq!(view.total(), 6);
        assert_eq!(view.row_at(3), Some(1));
        assert_eq!(view.row_at(6), None);
    }
}
