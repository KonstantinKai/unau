//! Narrowing the files pane to paths that contain some text, typed live.

use super::{AppState, Effect};
use crate::{
    config::{Key, KeyCodeKey},
    tree::Entry,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileFilter {
    /// Shown paths contain this, ignoring case; empty shows every path.
    pub text: String,
    /// Keys go to the filter instead of the bindings.
    pub typing: bool,
}

impl FileFilter {
    pub fn is_set(&self) -> bool {
        !self.text.is_empty()
    }
}

impl AppState {
    /// `/`: the files pane, with the filter open for typing.
    pub fn start_filter(&mut self) {
        self.focus_files();
        self.filter.typing = true;
    }

    /// A key while the filter is open: edit it, keep it, or drop it.
    pub fn filter_key(&mut self, key: Key) -> Effect {
        match key.code {
            KeyCodeKey::Char(c) if !key.ctrl => self.filter.text.push(c),
            KeyCodeKey::Backspace => {
                self.filter.text.pop();
            }
            KeyCodeKey::Char('w') => {
                let chars: Vec<char> = self.filter.text.chars().collect();
                let start = super::input::word_start(&chars, chars.len());
                self.filter.text = chars[..start].iter().collect();
            }
            KeyCodeKey::Enter => {
                self.filter.typing = false;
                return Effect::None;
            }
            KeyCodeKey::Esc => {
                self.filter.typing = false;
                self.filter.text.clear();
            }
            _ => return Effect::None,
        }
        self.apply_filter();
        Effect::None
    }

    /// Drops a filter set earlier, showing every file again.
    pub fn clear_filter(&mut self) {
        self.filter = FileFilter::default();
        self.apply_filter();
    }

    /// Rebuilds the tree for the filter, keeping the cursor on its file when
    /// that is still shown, else on the first file.
    fn apply_filter(&mut self) {
        let open = self.file.as_ref().and_then(|f| f.path()).map(str::to_owned);
        self.rebuild_tree();
        let at = open.and_then(|p| self.tree_index(&p)).or_else(|| {
            self.tree
                .iter()
                .position(|row| matches!(row.entry, Entry::File(_)))
        });
        self.tree_cursor = at.unwrap_or(0);
        self.open_under_cursor();
        self.scroll_to_cursor();
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{
        Pane,
        tests::{press, state},
    };
    use crate::git::fixture::Fixture;

    fn repo() -> Fixture {
        let f = Fixture::new();
        for p in [
            "src/app/Cart.tsx",
            "src/ui/List.tsx",
            "docs/cart.md",
            "top.rs",
        ] {
            f.write(p, "a\n");
        }
        f.commit_all("init");
        for p in [
            "src/app/Cart.tsx",
            "src/ui/List.tsx",
            "docs/cart.md",
            "top.rs",
        ] {
            f.write(p, "b\n");
        }
        f
    }

    #[test]
    fn typing_narrows_the_tree_to_matching_paths_ignoring_case() {
        let f = repo();
        let mut s = state(&f);
        press(&mut s, "/CART");
        assert!(s.filter.typing);
        assert_eq!(s.focus, Pane::Tree);
        let mut shown = s.paths.clone();
        shown.sort();
        assert_eq!(shown, ["docs/cart.md", "src/app/Cart.tsx"]);
        assert_eq!(s.all_paths, 4);
        assert!(
            s.file
                .as_ref()
                .and_then(|f| f.path())
                .is_some_and(|p| shown.iter().any(|s| s == p)),
            "the diff follows a shown file"
        );
        press(&mut s, "\n");
        assert!(!s.filter.typing && s.filter.is_set());
        press(&mut s, "j");
        assert_eq!(s.paths.len(), 2, "keys act normally once kept");
    }

    #[test]
    fn escape_clears_while_typing_and_after() {
        let f = repo();
        let mut s = state(&f);
        press(&mut s, "/tsx\x1b");
        assert!(!s.filter.is_set() && !s.filter.typing);
        assert_eq!(s.paths.len(), 4);
        press(&mut s, "/tsx\n");
        assert_eq!(s.paths.len(), 2);
        press(&mut s, "\x1b");
        assert_eq!(s.paths.len(), 4);
    }

    #[test]
    fn staging_a_folder_under_a_filter_stages_only_the_shown_files() {
        let f = repo();
        let mut s = state(&f);
        press(&mut s, "/cart\n");
        s.tree_cursor = s
            .tree
            .iter()
            .position(|r| r.label.starts_with("src"))
            .unwrap();
        press(&mut s, "s");
        assert_eq!(
            f.git(&["diff", "--cached", "--name-only"]),
            "src/app/Cart.tsx\n"
        );
    }

    #[test]
    fn ctrl_w_deletes_a_word() {
        let f = repo();
        let mut s = state(&f);
        let ctrl = |c| crate::config::Key {
            code: crate::config::KeyCodeKey::Char(c),
            ctrl: true,
        };
        press(&mut s, "/src app");
        s.handle_key(ctrl('w'));
        assert_eq!(s.filter.text, "src ");
        s.handle_key(ctrl('w'));
        assert_eq!(s.filter.text, "");
        assert!(s.filter.typing);
    }

    #[test]
    fn backspace_widens_and_a_miss_shows_nothing() {
        let f = repo();
        let mut s = state(&f);
        press(&mut s, "/zz");
        assert!(s.tree.is_empty());
        let backspace = crate::config::Key {
            code: crate::config::KeyCodeKey::Backspace,
            ctrl: false,
        };
        s.handle_key(backspace);
        s.handle_key(backspace);
        assert_eq!(s.paths.len(), 4);
    }
}
