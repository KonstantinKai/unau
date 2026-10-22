//! `:` asks for a line number and moves the diff cursor there.

use super::{AppState, Effect, Pane, Source};
use crate::config::{Key, KeyCodeKey};

/// Most digits the popup takes; no file is longer.
const MAX_DIGITS: usize = 7;

impl AppState {
    /// `:`: the line popup, when a file is open.
    pub(super) fn start_goto(&mut self) {
        match self.file.as_ref().map(|f| &f.source) {
            Some(Source::File(_)) => self.goto = Some(String::new()),
            _ => self.info("no file open"),
        }
    }

    /// A key while the popup is open: digits build the number, `Enter` goes
    /// there, `Esc` closes it.
    pub(super) fn goto_key(&mut self, key: Key) -> Effect {
        let Some(text) = &mut self.goto else {
            return Effect::None;
        };
        match key.code {
            KeyCodeKey::Char(c) if c.is_ascii_digit() && !key.ctrl && text.len() < MAX_DIGITS => {
                text.push(c);
            }
            KeyCodeKey::Backspace => {
                text.pop();
            }
            KeyCodeKey::Enter => {
                let line = text.parse::<usize>().ok();
                self.goto = None;
                if let Some(line) = line {
                    self.go_to_line(line);
                }
            }
            KeyCodeKey::Esc => self.goto = None,
            _ => {}
        }
        Effect::None
    }

    /// The row showing new-side `line`, else the nearest one shown; a line
    /// inside an omitted stretch stays folded.
    fn go_to_line(&mut self, line: usize) {
        self.focus = Pane::Diff;
        if let Some(file) = &mut self.file {
            file.anchor = None;
        }
        self.put_cursor_near(line);
        self.scroll_to_cursor();
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{press, state};
    use super::*;
    use crate::git::fixture::{Fixture, numbered};

    #[test]
    fn colon_takes_a_line_number_and_goes_there() {
        let f = Fixture::new();
        f.write("a.txt", &numbered(60));
        f.commit_all("init");
        f.write(
            "a.txt",
            &numbered(60)
                .replace("line 10\n", "line ten\n")
                .replace("line 50\n", "line fifty\n"),
        );
        let mut s = state(&f);
        s.focus = Pane::Tree;
        press(&mut s, ":5x2");
        assert_eq!(s.goto.as_deref(), Some("52"), "only digits are typed");
        press(&mut s, "\n");
        assert!(s.goto.is_none());
        assert_eq!(s.focus, Pane::Diff);
        assert_eq!(s.cursor_line(), Some(52));
        // A line in an omitted stretch lands on the nearest row shown: here
        // the separator standing for it, which `z` opens.
        press(&mut s, ":30\n");
        let file = s.file.as_ref().unwrap();
        assert!(matches!(
            file.view.rows[file.cursor],
            crate::diff::Row::Gap { .. }
        ));
        // Esc closes the popup and leaves the cursor where it was.
        let before = s.cursor_line();
        press(&mut s, ":12\x1b");
        assert!(s.goto.is_none());
        assert_eq!(s.cursor_line(), before);
    }
}
