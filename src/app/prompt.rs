//! The comment popup's line editing: typing, moving, deleting words and
//! lines, new lines, pasting, and handing the text to `$EDITOR`.

use super::{AppState, EditPurpose, Effect, Prompt, PromptPurpose};
use crate::{
    config::{Key, KeyCodeKey},
    review::Kind,
};

impl AppState {
    /// A key while the comment popup is open.
    pub(super) fn prompt_key(&mut self, key: Key) -> Effect {
        let Some(prompt) = &mut self.prompt else {
            return Effect::None;
        };
        let chars: Vec<char> = prompt.text.chars().collect();
        match (key.code, key.ctrl) {
            (KeyCodeKey::Tab, _) => {
                prompt.kind = match prompt.kind {
                    Kind::Fix => Kind::Question,
                    Kind::Question => Kind::Fix,
                    Kind::Suggestion => Kind::Suggestion,
                };
            }
            (KeyCodeKey::Esc, _) => {
                self.prompt = None;
                self.info("cancelled");
            }
            (KeyCodeKey::Enter, _) => {
                let prompt = self.prompt.take().expect("prompt checked above");
                self.submit_prompt(prompt);
            }
            (KeyCodeKey::Char('e'), true) => {
                let prompt = self.prompt.take().expect("prompt checked above");
                return Effect::Editor {
                    text: prompt.text.clone(),
                    suffix: ".md".to_owned(),
                    purpose: EditPurpose::Prompt(prompt),
                };
            }
            (KeyCodeKey::Backspace, _) if prompt.cursor > 0 => {
                let mut chars = chars;
                chars.remove(prompt.cursor - 1);
                prompt.text = chars.into_iter().collect();
                prompt.cursor -= 1;
            }
            (KeyCodeKey::Char('w'), true) => {
                let start = word_start(&chars, prompt.cursor);
                prompt.text = chars[..start]
                    .iter()
                    .chain(&chars[prompt.cursor..])
                    .collect();
                prompt.cursor = start;
            }
            (KeyCodeKey::Left, _) => prompt.cursor = prompt.cursor.saturating_sub(1),
            (KeyCodeKey::Right, _) => prompt.cursor = (prompt.cursor + 1).min(chars.len()),
            (KeyCodeKey::AltEnter, _) | (KeyCodeKey::Char('j'), true) => {
                let mut chars = chars;
                chars.insert(prompt.cursor, '\n');
                prompt.text = chars.into_iter().collect();
                prompt.cursor += 1;
            }
            (KeyCodeKey::Char('a'), true) => prompt.cursor = 0,
            (KeyCodeKey::Char('u'), true) => {
                prompt.text = chars[prompt.cursor..].iter().collect();
                prompt.cursor = 0;
            }
            (KeyCodeKey::Char(c), false) => {
                let mut chars = chars;
                chars.insert(prompt.cursor, c);
                prompt.text = chars.into_iter().collect();
                prompt.cursor += 1;
            }
            _ => {}
        }
        Effect::None
    }

    pub fn open_prompt(&mut self, purpose: PromptPurpose, label: &str, text: String) {
        self.prompt = Some(Prompt {
            purpose,
            label: label.to_owned(),
            kind: Kind::Fix,
            cursor: text.chars().count(),
            text,
        });
    }

    /// Text pasted into the terminal goes into the prompt line, newlines included.
    pub fn paste(&mut self, text: &str) {
        if let Some(prompt) = &mut self.prompt {
            let mut chars: Vec<char> = prompt.text.chars().collect();
            let pasted: Vec<char> = text.replace("\r\n", "\n").chars().collect();
            let at = prompt.cursor.min(chars.len());
            chars.splice(at..at, pasted.iter().copied());
            prompt.cursor = at + pasted.len();
            prompt.text = chars.into_iter().collect();
        }
    }
}

/// Where the word before `cursor` starts, as shells see it for `C-w`: the
/// whitespace right before the cursor, then the run of non-whitespace.
pub(super) fn word_start(chars: &[char], cursor: usize) -> usize {
    let mut at = cursor.min(chars.len());
    while at > 0 && chars[at - 1].is_whitespace() {
        at -= 1;
    }
    while at > 0 && !chars[at - 1].is_whitespace() {
        at -= 1;
    }
    at
}

#[cfg(test)]
mod tests {
    use super::super::tests::{press, state};
    use super::*;
    use crate::git::fixture::Fixture;

    fn repo() -> Fixture {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit_all("init");
        f.write("a.txt", "two\n");
        f
    }

    #[test]
    fn alt_enter_or_ctrl_j_adds_a_line_and_enter_saves_the_whole_comment() {
        let f = repo();
        let mut s = state(&f);
        press(&mut s, "nfirst");
        s.handle_key(Key {
            code: KeyCodeKey::AltEnter,
            ctrl: false,
        });
        press(&mut s, "second");
        s.handle_key(Key {
            code: KeyCodeKey::Char('j'),
            ctrl: true,
        });
        press(&mut s, "third\n");
        assert!(s.prompt.is_none());
        assert_eq!(s.review.items[0].text, "first\nsecond\nthird");
    }

    #[test]
    fn prompt_edits_text_and_escape_cancels() {
        let f = repo();
        let mut s = state(&f);
        press(&mut s, "nabc");
        s.handle_key(Key {
            code: KeyCodeKey::Backspace,
            ctrl: false,
        });
        assert_eq!(s.prompt.as_ref().unwrap().text, "ab");
        press(&mut s, "\x1b");
        assert!(s.prompt.is_none());
        assert!(s.review.is_empty());
    }

    #[test]
    fn ctrl_w_deletes_the_word_before_the_cursor_in_the_comment_popup() {
        let f = repo();
        let mut s = state(&f);
        let ctrl = |c| Key {
            code: KeyCodeKey::Char(c),
            ctrl: true,
        };
        press(&mut s, "nfix the  loo");
        s.handle_key(ctrl('w'));
        assert_eq!(s.prompt.as_ref().unwrap().text, "fix the  ");
        s.handle_key(ctrl('w'));
        assert_eq!(s.prompt.as_ref().unwrap().text, "fix ");
        // In the middle: only what is before the cursor goes.
        let left = Key {
            code: KeyCodeKey::Left,
            ctrl: false,
        };
        press(&mut s, "it");
        s.handle_key(left);
        s.handle_key(left);
        s.handle_key(ctrl('w'));
        let prompt = s.prompt.as_ref().unwrap();
        assert_eq!((prompt.text.as_str(), prompt.cursor), ("it", 0));
        s.handle_key(ctrl('w'));
        assert_eq!(
            s.prompt.as_ref().unwrap().text,
            "it",
            "nothing before the start"
        );
    }

    #[test]
    fn word_start_skips_spaces_then_the_word() {
        let chars: Vec<char> = "ab  cd ".chars().collect();
        assert_eq!(word_start(&chars, 7), 4);
        assert_eq!(word_start(&chars, 4), 0);
        assert_eq!(word_start(&chars, 0), 0);
    }
}
