//! Taking the view apart into a saved session and putting it back. The file
//! itself is read and written by the caller; this is only data.

use super::{AppState, Pane, RunLog, RunStatus, Source, TREE_MAX_PERCENT, TREE_MIN_PERCENT};
use crate::session::{Focus, Layout, SavedRun, Session};
use std::time::Duration;

impl AppState {
    /// The view as it is now, for the next start.
    pub fn session(&self) -> Session {
        let mut folded: Vec<String> = self.folded.iter().cloned().collect();
        folded.sort();
        Session {
            repo: self.repo.root.to_string_lossy().into_owned(),
            file: self.file.as_ref().and_then(|f| f.path()).map(str::to_owned),
            line: self.cursor_line(),
            focus: match self.focus {
                Pane::Tree => Focus::Files,
                Pane::Diff => Focus::Diff,
                Pane::Comments => Focus::Comments,
                Pane::Run => Focus::Run,
            },
            run: self.run_log.as_ref().map(saved_run),
            comments: self.comments.visible,
            folded,
            layout: Layout {
                flat: self.flat,
                tree_percent: Some(self.layout.tree_percent),
                zoom: self.layout.zoom,
                split: self.layout.split,
            },
        }
    }

    /// Puts a saved view back. The comments and run panes only come back
    /// when an existing review is being continued; a fresh review starts
    /// without them.
    pub fn restore(&mut self, session: &Session, continuing: bool) {
        let layout = &session.layout;
        if let Some(percent) = layout.tree_percent {
            self.layout.tree_percent = percent.clamp(TREE_MIN_PERCENT, TREE_MAX_PERCENT);
        }
        self.layout.zoom = layout.zoom;
        self.layout.split = layout.split;
        self.flat = layout.flat;
        self.folded = session.folded.iter().cloned().collect();
        self.rebuild_tree();
        if let Some(path) = session.file.as_ref().filter(|p| self.paths.contains(p)) {
            self.open_source(Source::File(path.clone()));
        }
        self.rebuild_rows();
        if let Some(line) = session.line {
            self.put_cursor_near(line);
        }
        if continuing && session.comments {
            self.comments.visible = true;
            self.rebuild_comments();
        }
        if continuing && let Some(run) = &session.run {
            self.run_log = Some(restored_run(run));
        }
        let run_shown = self.run_log.as_ref().is_some_and(|l| l.visible);
        self.focus = match session.focus {
            Focus::Comments if self.comments.visible => Pane::Comments,
            Focus::Run if run_shown => Pane::Run,
            Focus::Diff if self.file.is_some() => Pane::Diff,
            _ => Pane::Tree,
        };
        self.relayout();
        self.scroll_to_cursor();
    }

    /// Diff cursor on the row showing new-side `line`, else the nearest one.
    pub(super) fn put_cursor_near(&mut self, line: usize) {
        let Some(rows) = self.file.as_ref().map(|f| f.view.rows.len()) else {
            return;
        };
        let best = (0..rows)
            .filter(|r| self.file.as_ref().is_some_and(|f| f.selectable(*r)))
            .filter_map(|r| self.row_line(r).map(|l| (l.abs_diff(line), r)))
            .min();
        if let (Some((_, row)), Some(file)) = (best, &mut self.file) {
            file.cursor = row;
        }
    }
}

/// The run as the next start shows it. Quitting stops a run still going, so
/// it is saved as stopped.
fn saved_run(log: &RunLog) -> SavedRun {
    let (exit, error) = match &log.status {
        RunStatus::Running => (Some(-1), None),
        RunStatus::Exited(code) => (Some(*code), None),
        RunStatus::Failed(error) => (None, Some(error.clone())),
    };
    SavedRun {
        command: log.command.clone(),
        exit,
        error,
        elapsed_secs: log.elapsed.as_secs(),
        visible: log.visible,
        lines: log.lines.clone(),
    }
}

fn restored_run(run: &SavedRun) -> RunLog {
    RunLog {
        command: run.command.clone(),
        lines: run.lines.clone(),
        status: match (&run.error, run.exit) {
            (Some(error), _) => RunStatus::Failed(error.clone()),
            (None, code) => RunStatus::Exited(code.unwrap_or(-1)),
        },
        visible: run.visible,
        elapsed: Duration::from_secs(run.elapsed_secs),
        scroll: None,
        stream: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::tests::{press, state},
        git::fixture::{Fixture, numbered},
    };

    fn repo() -> Fixture {
        let f = Fixture::new();
        f.write("src/a.rs", "a\n");
        f.write("src/ui/b.rs", &numbered(40));
        f.commit_all("init");
        f.write("src/a.rs", "A\n");
        f.write(
            "src/ui/b.rs",
            &numbered(40)
                .replace("line 5\n", "line five\n")
                .replace("line 30\n", "line thirty\n"),
        );
        f
    }

    fn left(f: &Fixture) -> Session {
        let mut s = state(f);
        s.tree_cursor = s.tree.iter().position(|r| r.label == "b.rs").unwrap();
        s.open_under_cursor();
        s.focus = Pane::Diff;
        press(&mut s, "J");
        press(&mut s, "L");
        press(&mut s, "t+");
        s.session()
    }

    #[test]
    fn a_continued_review_comes_back_as_it_was_left() {
        let f = repo();
        let saved = left(&f);
        assert_eq!(saved.file.as_deref(), Some("src/ui/b.rs"));
        assert_eq!(saved.line, Some(30));
        assert_eq!(saved.focus, Focus::Comments);
        let mut s = state(&f);
        s.restore(&saved, true);
        assert_eq!(s.session(), saved);
    }

    #[test]
    fn a_fresh_review_keeps_the_layout_but_not_the_comments_pane() {
        let f = repo();
        let saved = left(&f);
        let mut s = state(&f);
        s.restore(&saved, false);
        assert!(!s.comments.visible);
        assert_eq!(s.focus, Pane::Tree);
        assert!(s.layout.split && saved.layout.split);
        assert_eq!(s.cursor_line(), Some(30));
    }

    #[test]
    fn the_last_run_comes_back_with_a_continued_review_only() {
        let f = repo();
        let mut s = state(&f);
        s.run_started("agent");
        s.run_output(&["first".to_owned(), "second".to_owned()]);
        s.run_elapsed(Duration::from_secs(90));
        press(&mut s, "4");
        let saved = s.session();
        let run = saved.run.as_ref().unwrap();
        assert_eq!(run.exit, Some(-1), "quitting stops a run still going");
        assert_eq!(saved.focus, Focus::Run);

        let mut s = state(&f);
        s.restore(&saved, true);
        let log = s.run_log.as_ref().unwrap();
        assert_eq!(log.status, RunStatus::Exited(-1));
        assert_eq!(log.lines.len(), 2);
        assert_eq!(log.elapsed, Duration::from_secs(90));
        assert!(log.visible && !log.running());
        assert_eq!(s.focus, Pane::Run);

        let mut s = state(&f);
        s.restore(&saved, false);
        assert!(s.run_log.is_none());
        assert_eq!(s.focus, Pane::Tree);
    }

    #[test]
    fn a_file_without_changes_any_more_is_not_reopened() {
        let f = repo();
        let mut saved = left(&f);
        saved.file = Some("gone.rs".into());
        saved.focus = Focus::Diff;
        let mut s = state(&f);
        let before = s.file.as_ref().and_then(|f| f.path()).map(str::to_owned);
        s.restore(&saved, true);
        assert_eq!(
            s.file.as_ref().and_then(|f| f.path()).map(str::to_owned),
            before
        );
    }
}
