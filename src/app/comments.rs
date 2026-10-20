//! The comments pane under the files pane: every review item in one list.
//! Selecting an item shows it in the diff pane.

use super::{AppState, Pane, Source};
use crate::{
    diff::Row,
    review::{Item, Review, Scope},
};

/// Share of the left column the comments pane takes, and its smallest height.
pub const COMMENTS_PERCENT: u16 = 40;
pub const COMMENTS_MIN: u16 = 5;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommentRow {
    /// `General` or a path, above its items. The cursor skips these.
    Header(String),
    /// An item, by index into the review.
    Item(usize),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommentsPane {
    pub visible: bool,
    pub rows: Vec<CommentRow>,
    /// Row under the cursor; always an item row when there is one.
    pub cursor: usize,
    pub scroll: usize,
    /// The selected item as it was, to find it again after the review changes.
    selected: Option<Item>,
}

impl CommentsPane {
    pub fn selected_item(&self) -> Option<usize> {
        match self.rows.get(self.cursor) {
            Some(CommentRow::Item(i)) => Some(*i),
            _ => None,
        }
    }
}

/// Rows for `review`: items in written order, each group under its header.
pub fn rows(review: &Review) -> Vec<CommentRow> {
    let mut rows = Vec::new();
    let mut group: Option<Option<&str>> = None;
    for (index, item) in review.items.iter().enumerate() {
        let path = item.scope.path();
        if group != Some(path) {
            group = Some(path);
            rows.push(CommentRow::Header(path.unwrap_or("General").to_owned()));
        }
        rows.push(CommentRow::Item(index));
    }
    rows
}

/// What a row reads, after any done mark and kind badge: the anchor and the
/// first line of the text.
pub fn label(item: &Item) -> String {
    let anchor = match &item.scope {
        Scope::General => String::new(),
        Scope::File(_) => "(file) ".to_owned(),
        Scope::Lines { start, end, .. } if start == end => format!(":{start} "),
        Scope::Lines { start, end, .. } => format!(":{start}-{end} "),
    };
    let text = match (&item.replacement, item.text.lines().next()) {
        (Some(_), _) => "replace with…",
        (None, Some(first)) => first,
        (None, None) => "",
    };
    format!("{anchor}{text}")
}

impl AppState {
    /// Rows of the comments pane on screen, borders excluded.
    pub fn comments_height(&self) -> usize {
        usize::from(self.comments_pane_height().saturating_sub(2)).max(1)
    }

    /// Lines the comments pane takes from the left column, borders included.
    pub fn comments_pane_height(&self) -> u16 {
        if !self.comments.visible {
            return 0;
        }
        let column = self.layout.height.saturating_sub(1);
        (column * COMMENTS_PERCENT / 100)
            .max(COMMENTS_MIN)
            .min(column / 2)
    }

    /// `L`: show and focus the pane, focus it, or hide it.
    pub fn toggle_comments(&mut self) {
        // Zoom hides the pane, so unzoom to show it; but on the pane already
        // focused, `L` means hide, and unzooming would undo that.
        if self.layout.zoom && !(self.comments.visible && self.focus == Pane::Comments) {
            self.layout.zoom = false;
            self.relayout();
        }
        if !self.comments.visible {
            self.comments.visible = true;
            self.rebuild_comments();
            self.focus = Pane::Comments;
            self.show_selected_comment();
        } else if self.focus == Pane::Comments {
            self.comments.visible = false;
            self.focus = Pane::Tree;
        } else {
            self.focus = Pane::Comments;
            self.show_selected_comment();
        }
        self.scroll_to_cursor();
    }

    /// Focuses the comments pane, showing it first when it is hidden.
    pub fn focus_comments(&mut self) {
        if self.comments.visible && self.focus == Pane::Comments {
            return;
        }
        if self.comments.visible {
            self.leave_zoom();
            self.focus = Pane::Comments;
            self.show_selected_comment();
        } else {
            self.toggle_comments();
        }
    }

    /// Focuses the files pane.
    pub fn focus_files(&mut self) {
        self.leave_zoom();
        self.focus = Pane::Tree;
    }

    /// The left column is hidden while the diff is zoomed.
    fn leave_zoom(&mut self) {
        if self.layout.zoom {
            self.layout.zoom = false;
            self.relayout();
        }
    }

    /// Rebuilds the rows after the review changed, keeping the selection on
    /// the same item when it is still there.
    pub fn rebuild_comments(&mut self) {
        self.comments.rows = rows(&self.review);
        let same = self.comments.selected.as_ref().and_then(|selected| {
            self.comments.rows.iter().position(
                |r| matches!(r, CommentRow::Item(i) if self.review.items[*i] == *selected),
            )
        });
        self.comments.cursor = same.unwrap_or(self.comments.cursor);
        self.settle_comment_cursor(true);
    }

    fn settle_comment_cursor(&mut self, forward: bool) {
        let rows = &self.comments.rows;
        let len = rows.len();
        if len == 0 {
            self.comments.cursor = 0;
            self.comments.selected = None;
            return;
        }
        let at = self.comments.cursor.min(len - 1);
        let is_item = |r: usize| matches!(rows[r], CommentRow::Item(_));
        let order: Vec<usize> = if forward {
            (at..len).chain((0..at).rev()).collect()
        } else {
            (0..=at).rev().chain(at + 1..len).collect()
        };
        self.comments.cursor = order.into_iter().find(|r| is_item(*r)).unwrap_or(at);
        self.comments.selected = self
            .comments
            .selected_item()
            .map(|i| self.review.items[i].clone());
    }

    /// Moves the selection to `row`, or as near as an item row allows.
    pub fn select_comment_row(&mut self, row: usize, forward: bool) {
        self.comments.cursor = row;
        self.settle_comment_cursor(forward);
        self.show_selected_comment();
    }

    /// Opens the selected item's file in the diff pane, cursor on its note.
    pub fn show_selected_comment(&mut self) {
        let Some(index) = self.comments.selected_item() else {
            return;
        };
        let source = match self.review.items[index].scope.path() {
            Some(path) => Source::File(path.to_owned()),
            None => Source::General,
        };
        self.open_source(source);
        if let Some(file) = &mut self.file
            && let Some(row) = file.view.rows.iter().position(|r| *r == Row::Note(index))
        {
            file.cursor = row;
        }
        self.scroll_to_cursor();
    }

    /// Keeps the selected row inside the visible part of the list.
    pub fn scroll_comments(&mut self) {
        let height = self.comments_height();
        let pane = &mut self.comments;
        if pane.cursor < pane.scroll {
            pane.scroll = pane.cursor;
        } else if pane.cursor >= pane.scroll + height {
            pane.scroll = pane.cursor + 1 - height;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{press, state};
    use super::*;
    use crate::{config::Key, git::fixture::Fixture};

    fn repo() -> Fixture {
        let f = Fixture::new();
        f.write("src/a.rs", &crate::git::fixture::numbered(30));
        f.write("src/b.rs", "one\n");
        f.commit_all("init");
        f.write(
            "src/a.rs",
            &crate::git::fixture::numbered(30).replace("line 25", "line 25!"),
        );
        f.write("src/b.rs", "two\n");
        f
    }

    #[test]
    fn tab_walks_the_panes_in_number_order_skipping_hidden_ones() {
        let f = repo();
        let mut s = state(&f);
        let tab = Key {
            code: crate::config::KeyCodeKey::Tab,
            ctrl: false,
        };
        let walk = |s: &mut crate::app::AppState| {
            (0..4)
                .map(|_| {
                    s.handle_key(tab);
                    s.focus
                })
                .collect::<Vec<_>>()
        };
        s.focus = Pane::Tree;
        assert_eq!(
            walk(&mut s),
            [Pane::Diff, Pane::Tree, Pane::Diff, Pane::Tree]
        );
        s.comments.visible = true;
        s.run_started("agent");
        s.focus = Pane::Tree;
        assert_eq!(
            walk(&mut s),
            [Pane::Diff, Pane::Comments, Pane::Run, Pane::Tree]
        );
    }

    #[test]
    fn number_keys_focus_panes_and_show_or_unzoom_on_the_way() {
        let f = repo();
        let mut s = state(&f);
        press(&mut s, "2");
        assert_eq!(s.focus, Pane::Diff);
        press(&mut s, "3");
        assert!(s.comments.visible);
        assert_eq!(s.focus, Pane::Comments);
        press(&mut s, "Z");
        assert!(s.layout.zoom);
        press(&mut s, "1");
        assert_eq!(s.focus, Pane::Tree);
        assert!(!s.layout.zoom);
        press(&mut s, "3");
        assert_eq!(s.focus, Pane::Comments);
    }

    #[test]
    fn digits_typed_into_a_comment_stay_text() {
        let f = repo();
        let mut s = state(&f);
        press(&mut s, "2c123\n");
        assert_eq!(s.review.items[0].text, "123");
        assert_eq!(s.focus, Pane::Diff);
    }

    #[test]
    fn rows_group_items_under_headers_and_labels_read_well() {
        let f = repo();
        let mut s = state(&f);
        press(&mut s, "noverall\nCwhole file\n");
        let rows = rows(&s.review);
        assert_eq!(rows[0], CommentRow::Header("General".into()));
        assert_eq!(rows[2], CommentRow::Header("src/a.rs".into()));
        assert_eq!(label(&s.review.items[1]), "(file) whole file");
    }

    #[test]
    fn the_general_comments_stay_open_while_any_are_left() {
        let f = repo();
        let mut s = state(&f);
        press(&mut s, "nfirst\n");
        press(&mut s, "nsecond\n");
        press(&mut s, "L");
        let source = |s: &crate::app::AppState| s.file.as_ref().unwrap().source.clone();
        assert_eq!(source(&s), Source::General);
        // A re-read, as after an outside edit or a stage, keeps them open.
        s.refresh().unwrap();
        assert_eq!(source(&s), Source::General);
        // Accepting one of two done items stays on the other.
        for item in &mut s.review.items {
            item.done = true;
        }
        s.reload_file();
        let on_note = |s: &mut crate::app::AppState| {
            let file = s.file.as_mut().unwrap();
            file.cursor = (0..file.view.rows.len())
                .find(|r| matches!(file.view.rows[*r], Row::Note(_)))
                .unwrap();
        };
        s.focus = Pane::Diff;
        on_note(&mut s);
        press(&mut s, "a");
        assert_eq!(s.review.items.len(), 1);
        assert_eq!(source(&s), Source::General);
        // The last one accepted, there is nothing general left to show.
        on_note(&mut s);
        press(&mut s, "a");
        assert!(s.review.items.is_empty());
        assert!(matches!(source(&s), Source::File(_)));
    }

    #[test]
    fn selecting_a_comment_opens_its_file_with_the_cursor_on_the_note() {
        let f = repo();
        let mut s = state(&f);
        press(&mut s, "noverall\n");
        s.focus = Pane::Diff;
        press(&mut s, "cnear the end\n");
        press(&mut s, "L");
        assert!(s.comments.visible);
        assert_eq!(s.focus, Pane::Comments);
        // The first item is the general one.
        assert_eq!(s.file.as_ref().unwrap().source, Source::General);
        press(&mut s, "j");
        let file = s.file.as_ref().unwrap();
        assert_eq!(file.path(), Some("src/a.rs"));
        assert!(matches!(file.view.rows[file.cursor], Row::Note(_)));
        press(&mut s, "\n");
        assert_eq!(s.focus, Pane::Diff);
    }

    #[test]
    fn l_shows_focuses_and_hides_and_tab_cycles_through_the_pane() {
        let f = repo();
        let mut s = state(&f);
        press(&mut s, "noverall\n");
        let tab = Key {
            code: crate::config::KeyCodeKey::Tab,
            ctrl: false,
        };
        press(&mut s, "L");
        assert_eq!(s.focus, Pane::Comments);
        s.handle_key(tab);
        assert_eq!(s.focus, Pane::Tree);
        s.handle_key(tab);
        assert_eq!(s.focus, Pane::Diff);
        s.handle_key(tab);
        assert_eq!(s.focus, Pane::Comments);
        let tree_with = s.tree_height();
        press(&mut s, "L");
        assert!(!s.comments.visible);
        assert_eq!(s.focus, Pane::Tree);
        assert!(s.tree_height() > tree_with);
    }

    #[test]
    fn selection_follows_its_item_when_the_review_changes() {
        let f = repo();
        let mut s = state(&f);
        press(&mut s, "nsecond\n");
        press(&mut s, "L");
        assert_eq!(s.comments.selected_item(), Some(0));
        s.focus = Pane::Tree;
        press(&mut s, "nfirst\n");
        let selected = s.comments.selected_item().unwrap();
        assert_eq!(s.review.items[selected].text, "second");
    }
}
