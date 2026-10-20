//! Key dispatch: prompt line, confirmations, help, then bound actions.

use super::{AppState, EditPurpose, Effect, Pending, Prompt, navigate::Scroll};
use crate::{
    config::{Action, Key, KeyCodeKey, Lookup, lookup},
    review::Kind,
};

impl AppState {
    pub fn handle_key(&mut self, key: Key) -> Effect {
        if self.prompt.is_some() {
            return self.prompt_key(key);
        }
        if self.theme_picker.is_some() {
            return self.theme_picker_key(key);
        }
        if self.agent_choice.is_some() {
            return self.agent_choice_key(key);
        }
        if self.filter.typing {
            return self.filter_key(key);
        }
        if let Some(confirm) = self.confirm.take() {
            if key == Key::char('y') {
                return self.run_pending(confirm.pending);
            }
            self.info("cancelled");
            return Effect::None;
        }
        if self.help {
            self.help = false;
            return Effect::None;
        }
        if key.code == KeyCodeKey::Esc
            && self.pressed.is_empty()
            && self.focus == super::Pane::Tree
            && self.filter.is_set()
        {
            self.clear_filter();
            return Effect::None;
        }
        if key.code == KeyCodeKey::Esc && self.pressed.is_empty() {
            if let Some(file) = &mut self.file {
                file.anchor = None;
            }
            self.message = None;
            return Effect::None;
        }
        self.pressed.push(key);
        match lookup(&self.config.keys, &self.pressed) {
            Lookup::Pending => Effect::None,
            Lookup::None => {
                self.pressed.clear();
                Effect::None
            }
            Lookup::Action(action) => {
                self.pressed.clear();
                self.message = None;
                self.dispatch(action)
            }
        }
    }

    fn dispatch(&mut self, action: Action) -> Effect {
        match action {
            Action::Down => self.move_by(1),
            Action::Up => self.move_by(-1),
            Action::NextHunk => self.jump_hunk(true),
            Action::PrevHunk => self.jump_hunk(false),
            Action::Top => self.move_to(false),
            Action::Bottom => self.move_to(true),
            Action::ScrollDown => self.scroll(Scroll::Line, true),
            Action::ScrollUp => self.scroll(Scroll::Line, false),
            Action::HalfPageDown => self.scroll(Scroll::HalfPage, true),
            Action::HalfPageUp => self.scroll(Scroll::HalfPage, false),
            Action::PageDown => self.scroll(Scroll::Page, true),
            Action::PageUp => self.scroll(Scroll::Page, false),
            Action::PaneLeft => self.focus = super::Pane::Tree,
            Action::PaneRight | Action::FocusDiff => self.focus_diff(),
            Action::FocusFiles => self.focus_files(),
            Action::FocusComments => self.focus_comments(),
            Action::FocusRun => self.focus_run(),
            Action::SwitchPane => {
                self.focus = self.next_pane();
                if self.focus == super::Pane::Comments {
                    self.show_selected_comment();
                }
            }
            Action::Comments => self.toggle_comments(),
            Action::Fold => self.fold(),
            Action::FoldAll => self.fold_all(),
            Action::UnfoldAll => self.unfold_all(),
            Action::FlatTree => self.toggle_flat(),
            Action::Filter => self.start_filter(),
            Action::Select => self.toggle_select(),
            Action::Stage => return self.stage_key(crate::git::Change::Stage, false),
            Action::Unstage => return self.stage_key(crate::git::Change::Unstage, false),
            Action::Discard => return self.stage_key(crate::git::Change::Discard, false),
            Action::StageFile => return self.stage_key(crate::git::Change::Stage, true),
            Action::UnstageFile => return self.stage_key(crate::git::Change::Unstage, true),
            Action::DiscardFile => return self.stage_key(crate::git::Change::Discard, true),
            Action::Comment => self.comment(),
            Action::Suggestion => return self.suggestion(),
            Action::FileComment => self.file_comment(),
            Action::GeneralComment => self.general_comment(),
            Action::Edit => return self.edit(),
            Action::Delete => self.delete(),
            Action::ToggleKind => self.toggle_kind(),
            Action::Expand => self.expand(),
            Action::Grow => self.resize_tree(false),
            Action::Shrink => self.resize_tree(true),
            Action::Zoom => {
                self.layout.zoom = !self.layout.zoom;
                if self.layout.zoom {
                    self.focus_diff();
                }
                self.relayout();
            }
            Action::ToggleSplit => {
                // Rows differ between the layouts; keep the same line, not row.
                let line = self.cursor_line();
                self.layout.split = !self.layout.split;
                self.rebuild_rows();
                if let Some(line) = line {
                    self.put_cursor_near(line);
                }
                self.scroll_to_cursor();
            }
            Action::Run => return self.run(),
            Action::RunPane => return self.toggle_run_pane(),
            Action::EditConfig => return self.edit_config(),
            Action::Themes => self.open_theme_picker(),
            Action::EditInstructions => return self.edit_instructions(),
            Action::OpenFile => return self.open_file(),
            Action::Accept => self.accept(),
            Action::Quit => return self.quit(),
            Action::Help => self.help = true,
        }
        Effect::None
    }

    /// The pane after the focused one in pane-number order (files, diff,
    /// comments, run), skipping panes that cannot take focus now.
    fn next_pane(&self) -> super::Pane {
        use super::Pane;
        const ORDER: [Pane; 4] = [Pane::Tree, Pane::Diff, Pane::Comments, Pane::Run];
        let open = |pane: &Pane| match pane {
            Pane::Tree => true,
            Pane::Diff => self.file.is_some(),
            Pane::Comments => self.comments.visible,
            Pane::Run => self.run_log.as_ref().is_some_and(|l| l.visible),
        };
        let at = ORDER.iter().position(|p| *p == self.focus).unwrap_or(0);
        ORDER
            .iter()
            .cycle()
            .skip(at + 1)
            .take(ORDER.len())
            .find(|p| open(p))
            .copied()
            .unwrap_or(Pane::Tree)
    }

    fn quit(&mut self) -> Effect {
        if !self.config.confirm_quit {
            return Effect::Quit;
        }
        let mut stakes = Vec::new();
        let open = self.review.open_items();
        if open > 0 && !self.ran && self.review_path().is_some() {
            stakes.push(format!("{open} open item(s), never run"));
        }
        if self.run_log.as_ref().is_some_and(super::RunLog::running) {
            stakes.push("a command is still running and will be stopped".to_owned());
        }
        let question = if stakes.is_empty() {
            "Quit unau?".to_owned()
        } else {
            format!("Quit unau? {}.", stakes.join("; "))
        };
        self.ask(question, Pending::Quit);
        Effect::None
    }

    pub fn ask(&mut self, question: String, pending: Pending) {
        self.confirm = Some(super::Confirm { question, pending });
    }

    fn run_pending(&mut self, pending: Pending) -> Effect {
        match pending {
            Pending::Quit => Effect::Quit,
            Pending::StopRun => Effect::StopRun,
            Pending::Stage(requests) => {
                self.perform_stage(&requests);
                Effect::None
            }
        }
    }

    fn run(&mut self) -> Effect {
        let Some(command) = self.config.run.clone() else {
            self.error("no [run] command configured");
            return Effect::None;
        };
        let Some(path) = self.review_path() else {
            self.error("nothing to run: the review is empty");
            return Effect::None;
        };
        let file = path.display().to_string();
        let place = match crate::run::place(self.config.run_in, self.multiplexers) {
            Ok(place) => place,
            Err(err) => {
                self.error(err);
                return Effect::None;
            }
        };
        if place == crate::run::Place::Pane
            && self.run_log.as_ref().is_some_and(super::RunLog::running)
        {
            self.ask_stop_run();
            return Effect::None;
        }
        self.ran = true;
        // Typed into the agent's prompt, not a shell, so the path goes in as is.
        let send = self
            .config
            .run_send
            .as_ref()
            .map(|send| send.replace("{file}", &file));
        let pane = self.agent_pane.clone().filter(|pane| pane.place == place);
        Effect::Run {
            command: command.replace("{file}", &shell_quote(&file)),
            place,
            send,
            pane,
        }
    }

    fn prompt_key(&mut self, key: Key) -> Effect {
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

    pub fn open_prompt(&mut self, purpose: super::PromptPurpose, label: &str, text: String) {
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

/// Quotes a path for `sh -c`.
pub fn shell_quote(text: &str) -> String {
    if text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-+:@".contains(c))
    {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
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
    fn run_substitutes_the_absolute_review_path() {
        let f = repo();
        let mut s = state(&f);
        s.config.run = Some("agent fix {file}".into());
        assert_eq!(press(&mut s, "r"), Effect::None);
        press(&mut s, "nsomething\n");
        let Effect::Run { command, place, .. } = press(&mut s, "r") else {
            panic!("no run effect");
        };
        let path = s.review_path().unwrap().display().to_string();
        assert_eq!(command, format!("agent fix {}", shell_quote(&path)));
        assert!(std::path::Path::new(&path).is_absolute());
        assert_eq!(place, crate::run::Place::Foreground);
        assert!(s.ran);
        s.multiplexers.tmux = true;
        let Effect::Run { place, .. } = press(&mut s, "r") else {
            panic!("no run effect");
        };
        assert_eq!(place, crate::run::Place::Tmux);
        s.config.run_in = crate::config::RunIn::Herdr;
        assert_eq!(press(&mut s, "r"), Effect::None);
        assert!(
            s.message
                .as_ref()
                .unwrap()
                .text
                .contains("not a Herdr pane")
        );
    }

    #[test]
    fn with_send_set_later_runs_go_to_the_agents_pane_as_typed_text() {
        let f = repo();
        let mut s = state(&f);
        s.config.run = Some("claude".into());
        s.config.run_send = Some("fix every open item in {file}".into());
        s.multiplexers.tmux = true;
        press(&mut s, "nsomething\n");
        // Typed at the agent's prompt, so the path is not quoted for a shell.
        let path = s.review_path().unwrap().display().to_string();
        let line = format!("fix every open item in {path}");
        let Effect::Run { send, pane, .. } = press(&mut s, "r") else {
            panic!("no run effect");
        };
        assert_eq!(send.as_deref(), Some(line.as_str()));
        assert_eq!(pane, None, "nothing remembered: a session is looked for");
        let remembered = crate::run::AgentPane {
            place: crate::run::Place::Tmux,
            id: "%7".into(),
        };
        s.agent_pane = Some(remembered.clone());
        let Effect::Run { pane, .. } = press(&mut s, "r") else {
            panic!("no run effect");
        };
        assert_eq!(pane, Some(remembered));
        s.multiplexers.zellij = true;
        s.config.run_in = crate::config::RunIn::Zellij;
        let Effect::Run { pane, .. } = press(&mut s, "r") else {
            panic!("no run effect");
        };
        assert_eq!(pane, None, "a pane of another multiplexer is not used");
        s.config.run_in = crate::config::RunIn::Tmux;
        s.config.run_send = None;
        let Effect::Run { send, .. } = press(&mut s, "r") else {
            panic!("no run effect");
        };
        assert_eq!(send, None, "without send every run opens a pane");
    }

    #[test]
    fn a_second_run_in_the_pane_offers_to_stop_the_first() {
        let f = repo();
        let mut s = state(&f);
        s.config.run = Some("agent {file}".into());
        s.config.run_in = crate::config::RunIn::Pane;
        press(&mut s, "nsomething\n");
        let Effect::Run { place, .. } = press(&mut s, "r") else {
            panic!("no run effect");
        };
        assert_eq!(place, crate::run::Place::Pane);
        s.run_started("agent");
        assert_eq!(press(&mut s, "r"), Effect::None);
        assert!(s.confirm.is_some());
        assert_eq!(press(&mut s, "y"), Effect::StopRun);
        assert_eq!(press(&mut s, "R"), Effect::None);
        assert!(!s.run_log.as_ref().unwrap().visible);
    }

    #[test]
    fn quit_asks_by_default_and_names_what_is_at_stake() {
        let f = repo();
        let mut s = state(&f);
        assert_eq!(press(&mut s, "Q"), Effect::None);
        assert_eq!(s.confirm.as_ref().unwrap().question, "Quit unau?");
        assert_eq!(press(&mut s, "n"), Effect::None);
        assert!(s.confirm.is_none());
        press(&mut s, "nsomething\n");
        s.run_started("agent");
        press(&mut s, "Q");
        let question = &s.confirm.as_ref().unwrap().question;
        assert!(question.contains("1 open item(s), never run"), "{question}");
        assert!(question.contains("will be stopped"), "{question}");
        assert_eq!(press(&mut s, "y"), Effect::Quit);
    }

    #[test]
    fn quit_without_confirmation_when_switched_off() {
        let f = repo();
        let mut s = state(&f);
        s.config.merge("[ui]\nconfirm_quit = false\n").unwrap();
        press(&mut s, "nsomething\n");
        assert_eq!(press(&mut s, "Q"), Effect::Quit);
        assert!(s.confirm.is_none());
    }

    #[test]
    fn chords_resolve_and_unknown_keys_are_ignored() {
        let f = repo();
        let mut s = state(&f);
        s.focus = super::super::Pane::Diff;
        press(&mut s, "G");
        let bottom = s.file.as_ref().unwrap().cursor;
        press(&mut s, "gg");
        assert!(s.file.as_ref().unwrap().cursor <= bottom);
        press(&mut s, "y");
        assert!(s.pressed.is_empty());
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

    #[test]
    fn shell_quote_leaves_plain_paths_alone() {
        assert_eq!(shell_quote("/a/b.md"), "/a/b.md");
        assert_eq!(shell_quote("/a b/it's.md"), r"'/a b/it'\''s.md'");
    }
}
