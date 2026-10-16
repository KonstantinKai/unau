//! The open file: its rows, notes, wrapping and highlighting, all computed in
//! the update step so rendering only reads.

use super::{AppState, Pane, rereview};
use crate::{
    config::LineNumbers,
    diff::{self, Highlighting, Row, SectionView, View, wrap_ranges},
    git::{LineKind, Section, SectionKind},
    review::{Item, Kind, Scope},
    theme::Syntax,
};
use std::{collections::BTreeMap, ops::Range};

/// Files whose built state is kept after switching away, so going back is
/// instant and keeps expanded context and highlighting.
pub const FILE_CACHE: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    General,
    File(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteStyle {
    Header {
        kind: Kind,
        done: bool,
        working: bool,
    },
    Text,
    Same,
    Removed,
    Added,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteLine {
    pub text: String,
    pub style: NoteStyle,
}

#[derive(Clone, Debug)]
pub struct FileState {
    pub source: Source,
    pub sections: Vec<Section>,
    pub untracked: bool,
    pub views: Vec<SectionView>,
    pub view: View,
    /// The file on the new side, line by line, for quoting and re-finding anchors.
    pub new_text: Vec<String>,
    /// Display lines per note row, by item index.
    pub notes: BTreeMap<usize, Vec<NoteLine>>,
    /// Highlighting, filled in as lines come into view.
    pub highlight: Highlighting,
    /// Width of the line number columns.
    pub digits: usize,
    pub cursor: usize,
    /// First visual line shown.
    pub scroll: usize,
    /// Row where the selection started.
    pub anchor: Option<usize>,
}

impl FileState {
    pub fn path(&self) -> Option<&str> {
        match &self.source {
            Source::File(path) => Some(path),
            Source::General => None,
        }
    }

    /// Index of the section annotations attach to.
    pub fn annotatable(&self) -> Option<usize> {
        self.sections.iter().position(|s| s.kind.annotatable())
    }

    /// New-side lines the changes touch: unstaged ones, and staged ones moved
    /// to working-tree numbers, so a fix staged by accepting a neighbour is
    /// still found at its anchor.
    pub fn changed_lines(&self) -> Vec<usize> {
        let unstaged = self.annotatable().map(|s| &self.sections[s].diff);
        let mut lines = unstaged.map(rereview::changed_lines).unwrap_or_default();
        if let Some(staged) = self.sections.iter().find(|s| s.kind == SectionKind::Staged) {
            let staged_lines = rereview::changed_lines(&staged.diff).into_iter();
            match unstaged {
                Some(diff) => {
                    lines.extend(staged_lines.filter_map(rereview::index_to_worktree(diff)));
                }
                None => lines.extend(staged_lines),
            }
            lines.sort_unstable();
            lines.dedup();
        }
        lines
    }

    pub fn selectable(&self, row: usize) -> bool {
        !matches!(self.view.rows.get(row), Some(Row::Section(_)) | None)
    }

    /// Moves the cursor off rows that cannot hold it, in the given direction first.
    pub fn settle_cursor(&mut self, forward: bool) {
        let len = self.view.rows.len();
        if len == 0 {
            self.cursor = 0;
            return;
        }
        self.cursor = self.cursor.min(len - 1);
        let ahead: Vec<usize> = if forward {
            (self.cursor..len).chain((0..self.cursor).rev()).collect()
        } else {
            (0..=self.cursor)
                .rev()
                .chain(self.cursor + 1..len)
                .collect()
        };
        if let Some(row) = ahead.into_iter().find(|r| self.selectable(*r)) {
            self.cursor = row;
        }
    }

    /// The note row under the cursor, as an item index.
    pub fn note_under_cursor(&self) -> Option<usize> {
        match self.view.rows.get(self.cursor) {
            Some(Row::Note(item)) => Some(*item),
            _ => None,
        }
    }

    /// Rows from the selection anchor to the cursor, or just the cursor.
    pub fn selected_rows(&self) -> Range<usize> {
        let anchor = self.anchor.unwrap_or(self.cursor);
        anchor.min(self.cursor)..anchor.max(self.cursor) + 1
    }

    /// Highlights the lines on screen and one screen below, so the next page
    /// is ready before it is scrolled to.
    pub fn highlight_screen(&mut self, height: usize, theme: &Syntax) {
        let Some(path) = self.path().map(str::to_owned) else {
            return;
        };
        let first = self.view.row_at(self.scroll).unwrap_or(0);
        let last = self
            .view
            .row_at(self.scroll + 2 * height)
            .unwrap_or(self.view.rows.len());
        let wanted: Vec<(usize, usize)> = self.view.rows[first..last.min(self.view.rows.len())]
            .iter()
            .filter_map(|row| row.section().map(|s| (s, row)))
            .flat_map(|(s, row)| row.lines().map(move |i| (s, i)))
            .collect();
        self.highlight
            .ensure(&path, &self.sections, &self.views, wanted, theme);
    }
}

impl AppState {
    /// Shows whatever the tree cursor is on in the diff pane.
    pub fn open_under_cursor(&mut self) {
        let Some(item) = self.tree.get(self.tree_cursor).cloned() else {
            self.file = None;
            return;
        };
        let Some(source) = self.tree_source(&item) else {
            return;
        };
        self.open_source(source);
    }

    /// Shows `source` in the diff pane unless it is already there, and puts
    /// the tree cursor on it when the tree shows it.
    pub fn open_source(&mut self, source: Source) {
        if self.file.as_ref().is_some_and(|f| f.source == source) {
            return;
        }
        let row = self
            .tree
            .iter()
            .position(|t| self.tree_source(t).as_ref() == Some(&source));
        if let Some(row) = row {
            self.tree_cursor = row;
        }
        if let Some(left) = self.file.take() {
            self.file_cache.retain(|f| f.source != left.source);
            self.file_cache.insert(0, left);
            self.file_cache.truncate(FILE_CACHE);
        }
        let cached = self.file_cache.iter().position(|f| f.source == source);
        if let Some(at) = cached {
            // The review may have changed since: only the note rows need redoing.
            self.file = Some(self.file_cache.remove(at));
            self.rebuild_rows();
        } else {
            self.file = Some(self.build_file(source));
            self.relayout();
        }
    }

    /// Rebuilds the open file from the current snapshot and review, keeping
    /// cursor and expanded context.
    pub fn reload_file(&mut self) {
        let Some(old) = self.file.take() else {
            return;
        };
        let mut file = self.build_file(old.source.clone());
        if file.sections == old.sections {
            file.views = old.views;
        }
        file.cursor = old.cursor;
        file.scroll = old.scroll;
        file.anchor = old.anchor.filter(|_| file.sections == old.sections);
        self.file = Some(file);
        self.rebuild_rows();
        if let Some(file) = &mut self.file {
            file.settle_cursor(false);
        }
        self.relayout();
    }

    fn build_file(&self, source: Source) -> FileState {
        let entry = match &source {
            Source::File(path) => self.snapshot.find(path).map(|i| &self.snapshot.files[i]),
            Source::General => None,
        };
        let sections = entry.map(|e| e.sections.clone()).unwrap_or_default();
        let new_text = match &source {
            Source::File(path) => self
                .repo
                .new_side(&self.target, path)
                .unwrap_or_default()
                .lines()
                .map(str::to_owned)
                .collect(),
            Source::General => Vec::new(),
        };
        let digits = sections
            .iter()
            .flat_map(|s| &s.diff.lines)
            .filter_map(|l| l.old.max(l.new))
            .max()
            .unwrap_or(1)
            .to_string()
            .len();
        let highlight = match &source {
            Source::File(path) => Highlighting::new(path, &sections),
            Source::General => Highlighting::default(),
        };
        let mut file = FileState {
            views: diff::section_views(&sections),
            untracked: entry.is_some_and(|e| e.untracked),
            sections,
            source,
            view: View::default(),
            new_text,
            notes: BTreeMap::new(),
            highlight,
            digits,
            cursor: 0,
            scroll: 0,
            anchor: None,
        };
        let rows = rows_for(&mut file, &self.review.items, self.layout.split);
        file.notes = rows.1;
        file.view = View::new(rows.0);
        // Start on the first change, not on a title or context line.
        file.cursor = first_change(&file).unwrap_or(0);
        file.settle_cursor(true);
        file
    }

    /// Recomputes rows after the review or the visibility changed.
    pub fn rebuild_rows(&mut self) {
        let split = self.layout.split;
        let items = self.review.items.clone();
        if let Some(file) = &mut self.file {
            let current = file.view.rows.get(file.cursor).copied();
            let (rows, notes) = rows_for(file, &items, split);
            file.view = View::new(rows);
            file.notes = notes;
            if let Some(current) = current
                && let Some(at) = file.view.rows.iter().position(|r| *r == current)
            {
                file.cursor = at;
            }
            file.settle_cursor(true);
        }
        self.relayout();
    }

    /// Wraps every row for the current width and keeps the cursor on screen.
    pub fn relayout(&mut self) {
        let (width, _) = self.diff_size();
        let split = self.layout.split;
        let numbers = self.config.line_numbers;
        if let Some(file) = &mut self.file {
            let code = code_widths(width, file.digits, split, numbers);
            let note_width = width.saturating_sub(4).max(1);
            let FileState {
                view,
                sections,
                notes,
                ..
            } = file;
            view.layout(|row| match row {
                Row::Code { section, lines } => {
                    let text = |i: Option<usize>, w: usize| {
                        i.map_or(1, |i| {
                            wrap_ranges(&sections[*section].diff.lines[i].text, w).len()
                        })
                    };
                    if split {
                        text(lines[0], code[0]).max(text(lines[1], code[1]))
                    } else {
                        text(lines[0], code[0])
                    }
                }
                Row::Note(item) => notes.get(item).map_or(1, |lines| {
                    lines
                        .iter()
                        .map(|l| wrap_ranges(&l.text, note_width).len())
                        .sum()
                }),
                _ => 1,
            });
        }
        self.scroll_to_cursor();
    }

    pub fn scroll_to_cursor(&mut self) {
        let (_, height) = self.diff_size();
        if let Some(file) = &mut self.file {
            let top = file.view.starts.get(file.cursor).copied().unwrap_or(0);
            let bottom = top + file.view.height(file.cursor);
            if top < file.scroll {
                file.scroll = top;
            } else if bottom > file.scroll + height {
                file.scroll = bottom.saturating_sub(height).min(top);
            }
            file.highlight_screen(height, &self.layout.theme.syntax);
        }
        let tree_height = self.tree_height();
        if self.tree_cursor < self.tree_scroll {
            self.tree_scroll = self.tree_cursor;
        } else if self.tree_cursor >= self.tree_scroll + tree_height {
            self.tree_scroll = self.tree_cursor + 1 - tree_height;
        }
        self.scroll_comments();
    }

    /// The tree row for `path`, unfolding nothing.
    pub fn tree_index(&self, path: &str) -> Option<usize> {
        self.tree.iter().position(
            |row| matches!(row.entry, crate::tree::Entry::File(i) if self.paths[i] == path),
        )
    }

    pub fn focus_diff(&mut self) {
        if self.file.is_some() {
            self.focus = Pane::Diff;
        }
    }
}

/// Columns left for code after the gutter: `[old side, new side]`; both equal
/// in the unified layout.
pub fn code_widths(width: usize, digits: usize, split: bool, numbers: LineNumbers) -> [usize; 2] {
    if split {
        let half = width.saturating_sub(1) / 2;
        let w = half.saturating_sub(digits + 3).max(1);
        [
            w,
            width
                .saturating_sub(1 + half)
                .saturating_sub(digits + 3)
                .max(1),
        ]
    } else {
        let gutter = match numbers {
            LineNumbers::Both => 2 * digits + 4,
            LineNumbers::New => digits + 3,
        };
        let w = width.saturating_sub(gutter).max(1);
        [w, w]
    }
}

fn first_change(file: &FileState) -> Option<usize> {
    file.view.rows.iter().position(|row| match row {
        Row::Code { section, lines } => lines
            .iter()
            .flatten()
            .any(|i| file.sections[*section].diff.lines[*i].is_change()),
        _ => false,
    })
}

/// Rows and note text for `file` with the review's items placed in it.
fn rows_for(
    file: &mut FileState,
    items: &[Item],
    split: bool,
) -> (Vec<Row>, BTreeMap<usize, Vec<NoteLine>>) {
    let mut placements = Vec::new();
    let mut notes = BTreeMap::new();
    let target = file.annotatable();
    let changed = file.changed_lines();
    for (index, item) in items.iter().enumerate() {
        let belongs = match (&file.source, item.scope.path()) {
            (Source::General, None) => true,
            (Source::File(path), Some(p)) => path == p,
            _ => false,
        };
        if !belongs {
            continue;
        }
        let region = rereview::find_region(item, &file.new_text, &changed);
        let line = match (&item.scope, target) {
            (
                Scope::Lines {
                    removed,
                    start,
                    end,
                    ..
                },
                Some(s),
            ) => {
                let (start, end) = region.map_or((*start, *end), |r| (r.start, r.end));
                note_line(&file.sections[s].diff.lines, start, end, *removed)
            }
            _ => None,
        };
        if let (Some(line), Some(s)) = (line, target) {
            file.views[s].visible[line] = true;
        }
        placements.push(diff::NotePlacement {
            item: index,
            section: target.unwrap_or(0),
            line,
        });
        notes.insert(index, note_lines(item, region, &file.new_text));
    }
    // Name the sections whenever staged lines are shown, so accepted lines are never mistaken for open ones.
    let titles = file
        .sections
        .iter()
        .any(|s| s.kind == crate::git::SectionKind::Staged);
    (
        diff::build_rows(&file.sections, &file.views, split, titles, &placements),
        notes,
    )
}

/// The section line a note sits under: the last anchored new-side line, or
/// for removed lines the last of them before new-side line `start`.
fn note_line(lines: &[crate::git::Line], start: usize, end: usize, removed: bool) -> Option<usize> {
    if removed {
        let next = lines
            .iter()
            .position(|l| l.new == Some(start))
            .unwrap_or(lines.len());
        (next > 0 && lines[next - 1].kind == LineKind::Removed).then(|| next - 1)
    } else {
        lines.iter().position(|l| l.new == Some(end))
    }
}

fn note_lines(item: &Item, region: Option<rereview::Region>, new_text: &[String]) -> Vec<NoteLine> {
    let mut text = item.text.lines();
    let first = match item.kind {
        Kind::Suggestion => "replace with".to_owned(),
        _ => text.next().unwrap_or_default().to_owned(),
    };
    let mut lines = vec![NoteLine {
        text: first,
        style: NoteStyle::Header {
            kind: item.kind,
            done: item.done,
            working: item.working,
        },
    }];
    lines.extend(text.map(|t| NoteLine {
        text: t.to_owned(),
        style: NoteStyle::Text,
    }));
    if let Some(replacement) = &item.replacement {
        lines.extend(replacement.iter().map(|t| NoteLine {
            text: t.clone(),
            style: NoteStyle::Added,
        }));
    }
    let lines_scope = matches!(item.scope, Scope::Lines { removed: false, .. });
    if lines_scope && region.is_none() {
        lines.push(NoteLine {
            text: "anchor lost: the quoted lines are gone".to_owned(),
            style: NoteStyle::Warning,
        });
    }
    // A ticked item shows what its lines were against what they are now.
    if item.done
        && lines_scope
        && let Some(region) = region
    {
        let now = &new_text[region.start - 1..region.end.min(new_text.len())];
        lines.extend(rereview::quote_diff(item, now));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::super::tests::{press, state};
    use super::*;
    use crate::git::fixture::Fixture;

    #[test]
    fn opening_a_file_puts_the_cursor_on_its_first_change() {
        let f = Fixture::new();
        let body = crate::git::fixture::numbered(30);
        f.write("a.txt", &body);
        f.commit_all("init");
        f.write("a.txt", &body.replace("line 20\n", "line twenty\n"));
        let state = state(&f);
        let file = state.file.as_ref().unwrap();
        let Row::Code { lines, .. } = file.view.rows[file.cursor] else {
            panic!("cursor not on code");
        };
        let line = &file.sections[0].diff.lines[lines[0].unwrap()];
        assert_eq!(line.kind, LineKind::Removed);
        assert_eq!(file.new_text.len(), 30);
        assert!(file.view.total() >= file.view.rows.len());
    }

    #[test]
    fn opening_a_long_file_highlights_about_a_screen_not_the_file() {
        let f = Fixture::new();
        f.write("big.rs", "fn keep() {}\n");
        f.commit_all("init");
        let body = crate::git::fixture::numbered(20_000).replace("line ", "fn f() -> u32 { ") + "}";
        f.write("big.rs", &body);
        let mut s = state(&f);
        let (_, height) = s.diff_size();
        let file = s.file.as_ref().unwrap();
        let done = file.highlight.highlighted();
        assert!(
            done > 0 && done <= 3 * height,
            "{done} lines highlighted for a {height}-line pane"
        );
        let Row::Code { section, lines } = file.view.rows[file.cursor] else {
            panic!("cursor not on code");
        };
        assert!(!file.highlight.spans[section][lines[0].unwrap()].is_empty());
        s.focus = super::super::Pane::Diff;
        press(&mut s, "G");
        let file = s.file.as_ref().unwrap();
        let Row::Code { section, lines } = file.view.rows[file.cursor] else {
            panic!("cursor not on code");
        };
        assert!(!file.highlight.spans[section][lines[0].unwrap()].is_empty());
    }

    #[test]
    fn switching_back_to_a_file_reuses_it_until_the_diff_is_reread() {
        let f = Fixture::new();
        f.write("a.txt", &crate::git::fixture::numbered(60));
        f.write("b.txt", "one\n");
        f.commit_all("init");
        f.write(
            "a.txt",
            &crate::git::fixture::numbered(60).replace("line 30\n", "line thirty\n"),
        );
        f.write("b.txt", "two\n");
        let mut s = state(&f);
        s.focus = super::super::Pane::Diff;
        press(&mut s, "z");
        let shown = |s: &super::super::AppState| {
            s.file.as_ref().unwrap().views[0]
                .visible
                .iter()
                .filter(|v| **v)
                .count()
        };
        let expanded = shown(&s);
        s.open_source(Source::File("b.txt".into()));
        assert_eq!(s.file_cache.len(), 1);
        s.open_source(Source::File("a.txt".into()));
        assert_eq!(shown(&s), expanded, "expanded context kept");
        assert_eq!(s.file_cache[0].source, Source::File("b.txt".into()));
        s.refresh().unwrap();
        assert!(s.file_cache.is_empty());
    }

    #[test]
    fn split_widths_share_the_pane() {
        assert_eq!(code_widths(81, 2, true, LineNumbers::Both), [35, 35]);
        assert_eq!(code_widths(80, 2, false, LineNumbers::Both), [72, 72]);
        assert_eq!(code_widths(80, 2, false, LineNumbers::New), [75, 75]);
    }
}
