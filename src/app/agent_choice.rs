//! Asking where a run goes when the configured agent already runs in this
//! repository: into one of those sessions, or into a new one. unau never
//! types into a session the user started without being told to. The first
//! answer is saved as `[run] reuse` in the repository's `.unau.toml`, so it
//! is asked once.

use super::{AppState, Effect};
use crate::{
    agents::Session,
    config::{Key, KeyCodeKey, Reuse},
    run::Place,
};
use anyhow::{Context, Result};
use std::{fs, path::Path};

/// The sessions found, then, while `[run] reuse` is still `ask`, one more
/// row: start a new session instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentChoice {
    pub sessions: Vec<Session>,
    /// A row of `sessions`, or `sessions.len()` for the new session.
    pub cursor: usize,
    /// The line to type into a running session.
    pub text: String,
    /// What a new session runs, and where.
    pub command: String,
    pub place: Place,
    /// The question is still open: offer a new session, save the answer.
    pub asking: bool,
}

impl AgentChoice {
    /// The cursor is on the last row, the new session.
    pub fn on_new(&self) -> bool {
        self.asking && self.cursor == self.sessions.len()
    }
}

impl AppState {
    /// Opens the list; nothing runs until a row is picked. `asking` adds the
    /// new-session row and saves the answer; without it the list only says
    /// which of several sessions.
    pub fn choose_agent(
        &mut self,
        sessions: Vec<Session>,
        text: String,
        command: String,
        place: Place,
    ) {
        let asking = self.config.run_reuse == Reuse::Ask;
        self.agent_choice = Some(AgentChoice {
            sessions,
            cursor: 0,
            text,
            command,
            place,
            asking,
        });
    }

    /// `j`/`k` or the arrows move, `Enter` goes, `Esc` runs nothing. The pane
    /// picked, or the new one, is remembered, so later runs go straight to it.
    pub(super) fn agent_choice_key(&mut self, key: Key) -> Effect {
        let Some(choice) = &mut self.agent_choice else {
            return Effect::None;
        };
        let last = choice.sessions.len() - usize::from(!choice.asking);
        match key.code {
            KeyCodeKey::Down | KeyCodeKey::Char('j') => {
                choice.cursor = (choice.cursor + 1).min(last);
            }
            KeyCodeKey::Up | KeyCodeKey::Char('k') => {
                choice.cursor = choice.cursor.saturating_sub(1);
            }
            KeyCodeKey::Enter => {
                let Some(choice) = self.agent_choice.take() else {
                    return Effect::None;
                };
                if choice.asking {
                    let answer = if choice.on_new() {
                        Reuse::New
                    } else {
                        Reuse::Session
                    };
                    self.save_reuse(answer);
                }
                if choice.on_new() {
                    return Effect::OpenAgent {
                        command: choice.command,
                        place: choice.place,
                    };
                }
                let Some(session) = choice.sessions.into_iter().nth(choice.cursor) else {
                    return Effect::None;
                };
                self.agent_pane = Some(session.pane.clone());
                return Effect::SendTo {
                    pane: session.pane,
                    text: choice.text,
                };
            }
            KeyCodeKey::Esc => {
                self.agent_choice = None;
                self.info("nothing run");
            }
            _ => {}
        }
        Effect::None
    }

    /// Saves the answer for this repository and applies it at once. A file
    /// that cannot be written leaves the question to be asked again.
    fn save_reuse(&mut self, answer: Reuse) {
        let path = self.config_files.repo.clone();
        match write_reuse(&path, answer) {
            Ok(()) => {
                self.config.run_reuse = answer;
                self.config_files.mark_seen();
            }
            Err(err) => self.error(format!("answer not saved: {err:#}")),
        }
    }
}

/// Writes `[run] reuse` into the `.unau.toml` at `path`, keeping every other
/// line and comment; a missing file is created with just that.
fn write_reuse(path: &Path, answer: Reuse) -> Result<()> {
    let text = if path.is_file() {
        fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?
    } else {
        String::new()
    };
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .with_context(|| format!("parse {}", path.display()))?;
    doc.entry("run")
        .or_insert_with(toml_edit::table)
        .as_table_like_mut()
        .context("[run] is not a table")?
        .insert("reuse", toml_edit::value(answer.name()));
    fs::write(path, doc.to_string()).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::super::tests::{press, state};
    use super::*;
    use crate::{git::fixture::Fixture, run::AgentPane};

    fn session(id: &str) -> Session {
        Session {
            pane: AgentPane {
                place: Place::Tmux,
                id: id.to_owned(),
            },
            label: format!("claude in {id}"),
        }
    }

    fn repo() -> Fixture {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit_all("init");
        f
    }

    #[test]
    fn picking_a_running_session_sends_to_it_and_remembers_it() {
        let f = repo();
        let mut s = state(&f);
        s.config_files.repo = f.path().join(".unau.toml");
        let ask = |s: &mut AppState, found| {
            s.choose_agent(found, "fix it".into(), "claude".into(), Place::Tmux);
        };
        ask(&mut s, vec![session("%1"), session("%2")]);
        press(&mut s, "k");
        assert_eq!(s.agent_choice.as_ref().unwrap().cursor, 0);
        press(&mut s, "j");
        assert_eq!(
            press(&mut s, "\n"),
            Effect::SendTo {
                pane: session("%2").pane,
                text: "fix it".into()
            }
        );
        assert!(s.agent_choice.is_none());
        assert_eq!(s.agent_pane, Some(session("%2").pane));
        ask(&mut s, vec![session("%1")]);
        assert_eq!(press(&mut s, "\x1b"), Effect::None);
        assert!(s.agent_choice.is_none());
    }

    #[test]
    fn the_last_row_starts_a_new_session_instead() {
        let f = repo();
        let mut s = state(&f);
        s.config_files.repo = f.path().join(".unau.toml");
        s.choose_agent(
            vec![session("%1")],
            "fix it".into(),
            "claude 'fix it'".into(),
            Place::Tmux,
        );
        // Even with a single session found, nothing is sent without a pick.
        assert!(s.agent_choice.is_some());
        press(&mut s, "jjj");
        assert!(s.agent_choice.as_ref().unwrap().on_new());
        assert_eq!(
            press(&mut s, "\n"),
            Effect::OpenAgent {
                command: "claude 'fix it'".into(),
                place: Place::Tmux
            }
        );
        assert!(
            s.agent_pane.is_none(),
            "the new pane is remembered once open"
        );
        assert_eq!(s.config.run_reuse, Reuse::New);
        assert!(f.read(".unau.toml").contains("reuse = \"new\""));
    }

    #[test]
    fn the_answer_is_saved_once_and_a_saved_one_is_not_asked_again() {
        let f = repo();
        let mut s = state(&f);
        s.config_files.repo = f.path().join(".unau.toml");
        f.write(".unau.toml", "# mine\n[review]\nquote_lines = 4\n");
        s.choose_agent(
            vec![session("%1")],
            "x".into(),
            "claude".into(),
            Place::Tmux,
        );
        press(&mut s, "\n");
        let saved = f.read(".unau.toml");
        assert!(saved.contains("# mine") && saved.contains("quote_lines = 4"));
        assert!(saved.contains("[run]\nreuse = \"session\""), "{saved}");
        // With the answer saved, a list appears only to say which of several,
        // without the new-session row, and nothing more is written.
        s.choose_agent(
            vec![session("%1"), session("%2")],
            "x".into(),
            "claude".into(),
            Place::Tmux,
        );
        press(&mut s, "jjj");
        let choice = s.agent_choice.as_ref().unwrap();
        assert_eq!(choice.cursor, 1);
        assert!(!choice.on_new());
        press(&mut s, "\n");
        assert_eq!(f.read(".unau.toml"), saved);
    }
}
