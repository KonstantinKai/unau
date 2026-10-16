//! Actions, their default keys, and key presses as config writes them.

use anyhow::{Result, bail};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{collections::BTreeMap, fmt};

macro_rules! actions {
    ($($variant:ident = $name:literal [$($key:literal),*] $help:literal,)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Action { $($variant,)* }

        impl Action {
            pub const ALL: &[Action] = &[$(Action::$variant,)*];

            pub fn name(self) -> &'static str {
                match self { $(Action::$variant => $name,)* }
            }

            pub fn help(self) -> &'static str {
                match self { $(Action::$variant => $help,)* }
            }

            pub(super) fn defaults(self) -> &'static [&'static str] {
                match self { $(Action::$variant => &[$($key),*],)* }
            }

            pub fn from_name(name: &str) -> Option<Action> {
                Self::ALL.iter().copied().find(|a| a.name() == name)
            }
        }
    };
}

actions! {
    Down = "down" ["j", "Down"] "next line",
    Up = "up" ["k", "Up"] "previous line",
    NextHunk = "next_hunk" ["J"] "next block of changes",
    PrevHunk = "prev_hunk" ["K"] "previous block of changes",
    Top = "top" ["gg"] "first line",
    Bottom = "bottom" ["G"] "last line",
    ScrollDown = "scroll_down" ["C-e"] "scroll down one line",
    ScrollUp = "scroll_up" ["C-y"] "scroll up one line",
    HalfPageDown = "half_page_down" ["C-d"] "half a page down",
    HalfPageUp = "half_page_up" ["C-u"] "half a page up",
    PageDown = "page_down" ["C-f"] "a page down",
    PageUp = "page_up" ["C-b"] "a page up",
    PaneLeft = "pane_left" ["h", "Left"] "focus files",
    PaneRight = "pane_right" ["l", "Right"] "focus diff",
    SwitchPane = "switch_pane" ["Tab"] "switch pane",
    FocusFiles = "focus_files" ["1"] "focus the files pane",
    FocusDiff = "focus_diff" ["2"] "focus the diff",
    FocusComments = "focus_comments" ["3"] "focus the comments pane, showing it",
    FocusRun = "focus_run" ["4"] "focus the run pane, showing it",
    Fold = "fold" ["Enter"] "fold directory / open file",
    FoldAll = "fold_all" ["W"] "fold every directory",
    UnfoldAll = "unfold_all" ["E"] "unfold every directory",
    FlatTree = "flat_tree" ["-"] "flat tree",
    Filter = "filter" ["/"] "filter files by path; Esc clears",
    Comments = "comments" ["L"] "show, focus or hide the comments pane",
    Select = "select" ["v"] "start or end a selection",
    Stage = "stage" ["s"] "stage selection or hunk",
    Unstage = "unstage" ["u"] "unstage selection or hunk",
    Discard = "discard" ["d"] "discard selection or hunk",
    StageFile = "stage_file" ["gs"] "stage the whole file",
    UnstageFile = "unstage_file" ["gu"] "unstage the whole file",
    DiscardFile = "discard_file" ["gd"] "discard the whole file",
    Comment = "comment" ["c"] "comment on lines (re-review: push back)",
    Suggestion = "suggestion" ["S"] "suggest replacement in $EDITOR",
    FileComment = "file_comment" ["C"] "comment on the file",
    GeneralComment = "general_comment" ["n"] "general comment",
    Edit = "edit" ["e"] "edit annotation",
    Delete = "delete" ["x"] "delete annotation",
    ToggleKind = "toggle_kind" ["q"] "toggle fix / question",
    Expand = "expand" ["z"] "expand hidden lines nearest the cursor",
    Grow = "grow" ["+"] "grow diff pane",
    Shrink = "shrink" ["_"] "shrink diff pane",
    Zoom = "zoom" ["Z"] "zoom diff pane",
    ToggleSplit = "toggle_split" ["t"] "unified / side by side",
    Run = "run" ["r"] "run the configured command",
    RunPane = "run_pane" ["R"] "show or hide the run pane",
    EditConfig = "edit_config" [","] "edit your config in $EDITOR",
    Themes = "themes" ["T"] "pick a theme, previewed live, saved to your config",
    EditInstructions = "edit_instructions" ["i"] "edit the agent instructions in $EDITOR",
    OpenFile = "open_file" ["o"] "open the file in $EDITOR at the cursor line",
    Accept = "accept" ["a"] "accept fix (re-review)",
    Quit = "quit" ["Q"] "quit",
    Help = "help" ["?"] "help",
}

/// A single key press, as written in config: `j`, `G`, `Tab`, `C-d`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key {
    pub code: KeyCodeKey,
    pub ctrl: bool,
}

/// `KeyCode` restricted to what bindings can use, so it can be ordered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyCodeKey {
    Char(char),
    Tab,
    Enter,
    Esc,
    Backspace,
    Up,
    Down,
    Left,
    Right,
    /// `Enter` with Alt held, sent by terminals as `ESC CR`.
    AltEnter,
}

impl Key {
    pub fn from_event(event: KeyEvent) -> Option<Self> {
        let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
        let code = match event.code {
            KeyCode::Char(c) => KeyCodeKey::Char(c),
            KeyCode::Tab => KeyCodeKey::Tab,
            KeyCode::Enter if event.modifiers.contains(KeyModifiers::ALT) => KeyCodeKey::AltEnter,
            KeyCode::Enter => KeyCodeKey::Enter,
            KeyCode::Esc => KeyCodeKey::Esc,
            KeyCode::Backspace => KeyCodeKey::Backspace,
            KeyCode::Up => KeyCodeKey::Up,
            KeyCode::Down => KeyCodeKey::Down,
            KeyCode::Left => KeyCodeKey::Left,
            KeyCode::Right => KeyCodeKey::Right,
            _ => return None,
        };
        Some(Self { code, ctrl })
    }

    pub fn char(c: char) -> Self {
        Self {
            code: KeyCodeKey::Char(c),
            ctrl: false,
        }
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            write!(f, "C-")?;
        }
        match self.code {
            KeyCodeKey::Char(c) => write!(f, "{c}"),
            KeyCodeKey::AltEnter => write!(f, "A-Enter"),
            other => write!(f, "{other:?}"),
        }
    }
}

/// Parses `"gg"` into two presses and `"Tab"` or `"C-d"` into one.
pub fn parse_keys(spec: &str) -> Result<Vec<Key>> {
    let named = match spec {
        "Tab" => Some(KeyCodeKey::Tab),
        "Enter" => Some(KeyCodeKey::Enter),
        "Esc" => Some(KeyCodeKey::Esc),
        "Backspace" => Some(KeyCodeKey::Backspace),
        "Up" => Some(KeyCodeKey::Up),
        "Down" => Some(KeyCodeKey::Down),
        "Left" => Some(KeyCodeKey::Left),
        "Right" => Some(KeyCodeKey::Right),
        "A-Enter" => Some(KeyCodeKey::AltEnter),
        "Space" => Some(KeyCodeKey::Char(' ')),
        _ => None,
    };
    if let Some(code) = named {
        return Ok(vec![Key { code, ctrl: false }]);
    }
    if let Some(rest) = spec.strip_prefix("C-")
        && let [c] = rest.chars().collect::<Vec<_>>()[..]
    {
        return Ok(vec![Key {
            code: KeyCodeKey::Char(c),
            ctrl: true,
        }]);
    }
    if spec.is_empty() {
        bail!("empty key binding");
    }
    Ok(spec.chars().map(Key::char).collect())
}

pub fn format_keys(keys: &[Key]) -> String {
    keys.iter().map(ToString::to_string).collect()
}

/// What a sequence of presses resolves to.
#[derive(Debug, PartialEq, Eq)]
pub enum Lookup {
    Action(Action),
    /// A longer binding starts with these presses; wait for the next one.
    Pending,
    None,
}

pub fn lookup(keys: &BTreeMap<Vec<Key>, Action>, pressed: &[Key]) -> Lookup {
    if let Some(action) = keys.get(pressed) {
        return Lookup::Action(*action);
    }
    if keys.keys().any(|k| k.starts_with(pressed)) {
        Lookup::Pending
    } else {
        Lookup::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn alt_enter_is_told_apart_from_enter() {
        let alt = KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT);
        let plain = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(Key::from_event(alt).unwrap().code, KeyCodeKey::AltEnter);
        assert_eq!(Key::from_event(plain).unwrap().code, KeyCodeKey::Enter);
        let parsed = parse_keys("A-Enter").unwrap();
        assert_eq!(format_keys(&parsed), "A-Enter");
    }

    #[test]
    fn chords_wait_for_the_next_key() {
        let keys = Config::default().keys;
        let g = Key::char('g');
        assert_eq!(lookup(&keys, &[g]), Lookup::Pending);
        assert_eq!(lookup(&keys, &[g, g]), Lookup::Action(Action::Top));
        assert_eq!(
            lookup(&keys, &[g, Key::char('s')]),
            Lookup::Action(Action::StageFile)
        );
        assert_eq!(lookup(&keys, &[Key::char('y')]), Lookup::None);
    }
}
