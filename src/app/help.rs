//! The keys popup: opened with `?`, scrolled with the line keys, closed with
//! `Esc` or `?` again.

use super::{AppState, Effect};
use crate::config::{Action, Key, KeyCodeKey, Lookup, lookup};

/// Share of the screen's height the popup takes at most, in percent; the
/// rest of the keys scroll.
const HELP_HEIGHT_PERCENT: u16 = 60;
/// Fewest rows the popup takes, borders included, when the screen has them.
const HELP_MIN_HEIGHT: u16 = 8;

/// Rows of the keys popup, borders included, on a screen `screen` rows tall:
/// short enough to leave the screen behind it in view, never taller than the
/// key list needs.
pub fn help_height(screen: u16) -> u16 {
    let lines = u16::try_from(Action::ALL.len()).unwrap_or(u16::MAX);
    (screen * HELP_HEIGHT_PERCENT / 100)
        .max(HELP_MIN_HEIGHT)
        .min(lines + 2)
        .min(screen)
}

impl AppState {
    pub(super) fn open_help(&mut self) {
        self.help = true;
        self.help_scroll = 0;
    }

    /// A key while the keys popup is open. Other keys do nothing, so a stray
    /// press does not close it halfway through reading.
    pub(super) fn help_key(&mut self, key: Key) -> Effect {
        let action = match lookup(&self.config.keys, &[key]) {
            Lookup::Action(action) => Some(action),
            Lookup::Pending | Lookup::None => None,
        };
        match action {
            _ if key.code == KeyCodeKey::Esc => self.help = false,
            Some(Action::Help) => self.help = false,
            Some(Action::Down) => {
                self.help_scroll = (self.help_scroll + 1).min(self.help_max_scroll());
            }
            Some(Action::Up) => self.help_scroll = self.help_scroll.saturating_sub(1),
            Some(Action::Bottom) => self.help_scroll = self.help_max_scroll(),
            _ => {}
        }
        Effect::None
    }

    /// How far the key list scrolls before its last line is at the bottom of
    /// the popup.
    pub fn help_max_scroll(&self) -> usize {
        let shown = usize::from(help_height(self.layout.height).saturating_sub(2));
        Action::ALL.len().saturating_sub(shown)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{press, state};
    use crate::{
        config::{Key, KeyCodeKey},
        git::fixture::Fixture,
    };

    #[test]
    fn the_keys_popup_scrolls_with_the_line_keys_and_closes_with_esc_or_question() {
        let f = Fixture::new();
        f.write("a.txt", "a\n");
        f.commit_all("init");
        f.write("a.txt", "b\n");
        let mut s = state(&f);
        s.resize(100, 20);
        press(&mut s, "?");
        assert!(s.help);
        press(&mut s, "jj");
        assert_eq!(s.help_scroll, 2);
        press(&mut s, "kkk");
        assert_eq!(s.help_scroll, 0, "never above the top");
        press(&mut s, "G");
        assert_eq!(s.help_scroll, s.help_max_scroll());
        press(&mut s, "j");
        assert_eq!(s.help_scroll, s.help_max_scroll(), "never past the end");
        press(&mut s, "x");
        assert!(s.help, "other keys leave it open");
        s.handle_key(Key {
            code: KeyCodeKey::Esc,
            ctrl: false,
        });
        assert!(!s.help);
        press(&mut s, "?");
        assert_eq!(s.help_scroll, 0, "opens at the top");
        press(&mut s, "?");
        assert!(!s.help);
        // A screen tall enough for every key has nothing to scroll.
        s.resize(100, 200);
        assert_eq!(s.help_max_scroll(), 0);
    }
}
