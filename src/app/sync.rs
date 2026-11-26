//! Reading the repository again: after unau changed it, when something
//! outside unau did, and when the window comes back into focus.

use super::{AppState, Source};
use crate::git::{Target, snapshot};
use anyhow::Result;

impl AppState {
    /// Reads the repository again and rebuilds everything that depends on it.
    pub fn refresh(&mut self) -> Result<()> {
        self.worktree_print = self.current_print();
        self.snapshot = snapshot(&self.repo, &self.target)?;
        // Cached files hold the old diff.
        self.file_cache.clear();
        self.check_blobs();
        self.rebuild_tree();
        let source = self.file.as_ref().map(|f| f.source.clone());
        // General has no row in the files pane; it stays open while the
        // review still has general comments to show.
        let still_there = |s: &Source| match s {
            Source::General => self
                .review
                .items
                .iter()
                .any(|i| i.scope == crate::review::Scope::General),
            Source::File(_) => self
                .tree
                .iter()
                .any(|t| self.tree_source(t).as_ref() == Some(s)),
        };
        if source.as_ref().is_some_and(still_there) {
            self.reload_file();
        } else {
            self.file = None;
            self.open_under_cursor();
        }
        Ok(())
    }

    /// Re-reads the repository when something outside unau changed the
    /// working tree or the index since it was last read. Waits while a comment
    /// is being typed, a question is asked or lines are selected, since moving
    /// the lines under those would lose the user's place; the change is picked
    /// up once they close. Returns whether it re-read.
    pub fn poll_worktree(&mut self) -> bool {
        // Nobody is looking while another window has focus; the check that
        // runs when focus comes back catches up.
        if !self.window_focused {
            return false;
        }
        let busy = self.prompt.is_some()
            || self.goto.is_some()
            || self.confirm.is_some()
            || self.file.as_ref().is_some_and(|f| f.anchor.is_some());
        if busy {
            return false;
        }
        let now = self.current_print();
        if now.is_none() || now == self.worktree_print {
            return false;
        }
        if self.worktree_print.is_none() {
            // The first look only records where things stand.
            self.worktree_print = now;
            return false;
        }
        if let Err(err) = self.refresh() {
            self.error(format!("{err:#}"));
        }
        true
    }

    /// The terminal window gained or lost focus. Coming back checks the
    /// working tree at once, so what changed meanwhile is there on first look.
    pub fn window_focus(&mut self, gained: bool) {
        self.window_focused = gained;
        if gained {
            self.poll_worktree();
        }
    }

    /// A commit or a range never changes, so only the working tree is watched.
    fn current_print(&self) -> Option<u64> {
        match self.target {
            Target::WorkTree => crate::git::fingerprint(&self.repo).ok(),
            Target::Range { .. } => None,
        }
    }

    /// Compares each annotated file with the blob recorded in the review.
    pub fn check_blobs(&mut self) {
        self.changed_since_review = self
            .review
            .files
            .iter()
            .filter(|(path, sha)| {
                self.repo
                    .blob(&self.target, path)
                    .is_ok_and(|now| now != **sha)
            })
            .map(|(path, _)| path.clone())
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{press, state};
    use crate::git::fixture::Fixture;

    #[test]
    fn changes_made_outside_unau_are_read_in_unless_the_user_is_mid_comment() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit_all("init");
        f.write("a.txt", "two\n");
        let mut s = state(&f);
        assert!(!s.poll_worktree(), "the first look only records");
        assert!(!s.poll_worktree());
        f.write("b.txt", "an agent wrote this\n");
        assert!(s.poll_worktree());
        assert_eq!(s.paths, ["a.txt", "b.txt"]);
        f.write("c.txt", "and this\n");
        press(&mut s, "2c");
        assert!(s.prompt.is_some());
        assert!(!s.poll_worktree());
        press(&mut s, "\x1b");
        assert!(s.poll_worktree());
        assert_eq!(s.paths, ["a.txt", "b.txt", "c.txt"]);
        f.write(".unau/2026-10-16-00.md", "# Review\n");
        assert!(!s.poll_worktree());
    }

    #[test]
    fn the_check_pauses_while_another_window_has_focus_and_catches_up_on_return() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit_all("init");
        f.write("a.txt", "two\n");
        let mut s = state(&f);
        s.poll_worktree();
        s.window_focus(false);
        f.write("b.txt", "written while away\n");
        assert!(!s.poll_worktree());
        assert_eq!(s.paths, ["a.txt"]);
        s.window_focus(true);
        assert_eq!(s.paths, ["a.txt", "b.txt"]);
    }
}
