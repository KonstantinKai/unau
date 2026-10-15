//! Folding directories in the files pane, one at a time or all at once, and
//! the flat view.

use super::{AppState, Pane};
use crate::tree::{self, Entry};
use std::collections::HashSet;

impl AppState {
    pub fn fold(&mut self) {
        match self.focus {
            Pane::Diff => return self.expand(),
            Pane::Comments => return self.focus_diff(),
            Pane::Run => return,
            Pane::Tree => {}
        }
        match self.tree.get(self.tree_cursor).map(|row| row.entry.clone()) {
            Some(Entry::Dir { path, folded }) => {
                if folded {
                    self.folded.remove(&path);
                } else {
                    self.folded.insert(path);
                }
                self.rebuild_tree();
            }
            Some(Entry::File(_)) => self.focus_diff(),
            None => {}
        }
    }

    /// Folds every directory, nested ones included, so opening a top
    /// directory shows its children still folded.
    pub fn fold_all(&mut self) {
        let refs: Vec<&str> = self.paths.iter().map(String::as_str).collect();
        // Directory rows as the unfolded, non-flat tree names them, since
        // single-child chains collapse into one row.
        self.folded = tree::build(&refs, &HashSet::new(), false)
            .into_iter()
            .filter_map(|row| match row.entry {
                Entry::Dir { path, .. } => Some(path),
                Entry::File(_) => None,
            })
            .collect();
        self.rebuild_tree();
        self.scroll_to_cursor();
    }

    pub fn unfold_all(&mut self) {
        self.folded.clear();
        self.rebuild_tree();
        self.scroll_to_cursor();
    }

    pub fn toggle_flat(&mut self) {
        let current = self.file.as_ref().and_then(|f| f.path().map(str::to_owned));
        self.flat = !self.flat;
        self.rebuild_tree();
        if let Some(at) = current.and_then(|p| self.tree_index(&p)) {
            self.tree_cursor = at;
        }
        self.scroll_to_cursor();
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        app::tests::{press, state},
        git::fixture::Fixture,
    };

    fn nested() -> Fixture {
        let f = Fixture::new();
        f.write("src/app/a.rs", "a\n");
        f.write("src/ui/b.rs", "b\n");
        f.write("top.rs", "t\n");
        f.commit_all("init");
        f.write("src/app/a.rs", "A\n");
        f.write("src/ui/b.rs", "B\n");
        f.write("top.rs", "T\n");
        f
    }

    fn labels(s: &crate::app::AppState) -> Vec<String> {
        s.tree.iter().map(|r| r.label.clone()).collect()
    }

    #[test]
    fn folding_a_directory_hides_its_files() {
        let f = Fixture::new();
        f.write("src/a.rs", "a\n");
        f.write("b.rs", "b\n");
        f.commit_all("init");
        f.write("src/a.rs", "A\n");
        f.write("b.rs", "B\n");
        let mut s = state(&f);
        assert_eq!(s.tree.len(), 3);
        s.tree_cursor = 0;
        press(&mut s, "\n");
        assert_eq!(s.tree.len(), 2);
        press(&mut s, "-");
        assert_eq!(s.tree.len(), 2);
        assert!(s.flat);
    }

    #[test]
    fn fold_all_folds_nested_directories_and_unfold_all_opens_them() {
        let f = nested();
        let mut s = state(&f);
        assert_eq!(s.tree.len(), 6);
        press(&mut s, "W");
        assert_eq!(labels(&s), ["src/", "top.rs"]);
        s.focus = crate::app::Pane::Tree;
        s.tree_cursor = 0;
        press(&mut s, "\n");
        assert_eq!(labels(&s), ["src/", "app/", "ui/", "top.rs"]);
        press(&mut s, "E");
        assert_eq!(s.tree.len(), 6);
    }

    #[test]
    fn a_cursor_hidden_by_folding_moves_to_its_visible_directory() {
        let f = nested();
        let mut s = state(&f);
        s.tree_cursor = s.tree.iter().position(|r| r.label == "b.rs").unwrap();
        press(&mut s, "W");
        assert_eq!(s.tree[s.tree_cursor].label, "src/");
    }
}
