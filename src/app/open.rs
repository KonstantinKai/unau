//! Opening the file under review in the user's editor, at the line the
//! cursor is on.

use super::{AppState, Effect, Pane, Source};
use crate::{diff::Row, git::LineKind, review::Scope};

impl AppState {
    /// The file the focused pane points at: the tree row in the files pane,
    /// the open file elsewhere.
    fn file_to_open(&self) -> Option<String> {
        match self.focus {
            Pane::Tree => {
                let row = self.tree.get(self.tree_cursor)?;
                match self.tree_source(row)? {
                    Source::File(path) => Some(path),
                    Source::General => None,
                }
            }
            Pane::Diff | Pane::Comments | Pane::Run => {
                self.file.as_ref()?.path().map(str::to_owned)
            }
        }
    }

    /// New-side line under the diff cursor of the open file.
    pub(super) fn cursor_line(&self) -> Option<usize> {
        self.row_line(self.file.as_ref()?.cursor)
    }

    /// New-side line of diff row `row` of the open file. A removed line
    /// counts as the new line after it, a note as the start of its anchor.
    pub(super) fn row_line(&self, row: usize) -> Option<usize> {
        let file = self.file.as_ref()?;
        let line_from = |section: usize, start: usize| {
            let lines = &file.sections.get(section)?.diff.lines;
            let line = lines.get(start)?;
            match line.kind {
                LineKind::Removed => lines[start..].iter().find_map(|l| l.new),
                _ => line.new,
            }
        };
        match *file.view.rows.get(row)? {
            Row::Code { section, lines } => {
                lines.iter().flatten().find_map(|i| line_from(section, *i))
            }
            Row::Gap { section, start, .. } => line_from(section, start),
            Row::Note(item) => match &self.review.items.get(item)?.scope {
                Scope::Lines { start, .. } => Some(*start),
                _ => None,
            },
            Row::Section(_) | Row::Binary(_) => None,
        }
    }

    pub fn open_file(&mut self) -> Effect {
        let Some(path) = self.file_to_open() else {
            self.error("not a file");
            return Effect::None;
        };
        let full = self.repo.root.join(&path);
        if !full.is_file() {
            self.error(format!("{path} is not in the working tree"));
            return Effect::None;
        }
        let line = if self.file.as_ref().and_then(|f| f.path()) == Some(path.as_str()) {
            self.cursor_line()
        } else {
            None
        };
        Effect::OpenFile { path: full, line }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::tests::{press, state},
        git::fixture::Fixture,
    };

    fn repo() -> Fixture {
        let f = Fixture::new();
        f.write("src/a.rs", "one\ntwo\nthree\nfour\n");
        f.commit_all("init");
        f.write("src/a.rs", "one\ntwo\nTHREE\nfour\n");
        f
    }

    #[test]
    fn opening_from_the_diff_goes_to_the_cursor_line() {
        let f = repo();
        let mut s = state(&f);
        press(&mut s, "l");
        let Effect::OpenFile { path, line } = press(&mut s, "o") else {
            panic!("no open effect");
        };
        assert_eq!(path, f.path().join("src/a.rs"));
        // The cursor starts on the first change: the removed `three`, which
        // sits just before new line 3.
        assert_eq!(line, Some(3));
        press(&mut s, "j");
        let Effect::OpenFile { line, .. } = press(&mut s, "o") else {
            panic!("no open effect");
        };
        assert_eq!(line, Some(3));
        press(&mut s, "j");
        let Effect::OpenFile { line, .. } = press(&mut s, "o") else {
            panic!("no open effect");
        };
        assert_eq!(line, Some(4));
    }

    #[test]
    fn opening_from_the_files_pane_uses_the_file_under_the_cursor() {
        let f = repo();
        let mut s = state(&f);
        s.focus = Pane::Tree;
        s.tree_cursor = 0;
        assert_eq!(press(&mut s, "o"), Effect::None, "a directory row");
        s.tree_cursor = 1;
        assert!(matches!(press(&mut s, "o"), Effect::OpenFile { .. }));
    }

    #[test]
    fn a_deleted_file_is_not_opened() {
        let f = repo();
        std::fs::remove_file(f.path().join("src/a.rs")).unwrap();
        let mut s = state(&f);
        press(&mut s, "l");
        assert_eq!(press(&mut s, "o"), Effect::None);
        assert!(s.message.is_some());
    }
}
