//! Accepting a fix in re-review: staging the hunks at a done item's lines and
//! marking it accepted, so it stays in the review as a record.

use super::{
    AppState,
    rereview::{Region, find_region, touched_new},
};
use crate::{
    git::{Change, SectionKind, apply},
    review::Scope,
};

impl AppState {
    /// Accepts the done item under the cursor: stages the hunks that touch its
    /// lines and marks it accepted, so it stays in the review as a record.
    pub fn accept(&mut self) {
        let Some(file) = &self.file else {
            return;
        };
        let Some(n) = file.note_under_cursor() else {
            self.error("put the cursor on a done annotation");
            return;
        };
        let item = self.review.items[n].clone();
        if !item.done {
            self.error("not done yet: the agent has not ticked this item");
            return;
        }
        if item.accepted {
            self.info("already accepted");
            return;
        }
        let region = match &item.scope {
            Scope::Lines { .. } => {
                let found = find_region(&item, &file.new_text, &file.changed_lines());
                let Some(region) = found else {
                    self.error("anchor lost: stage the fix with s, then delete the item with x");
                    return;
                };
                Some(region)
            }
            Scope::File(_) => Some(Region {
                start: 1,
                end: usize::MAX,
            }),
            Scope::General => None,
        };
        let section = file
            .sections
            .iter()
            .position(|s| s.kind == SectionKind::Unstaged);
        let mut staged = 0;
        if let (Some(region), Some(s), false) = (region, section, self.target.is_range()) {
            let diff = &file.sections[s].diff;
            let mut selected = vec![false; diff.lines.len()];
            let new_lines = touched_new(diff);
            for hunk in &file.views[s].hunks {
                let touches = hunk.clone().any(|i| {
                    diff.lines[i].is_change() && (region.start..=region.end).contains(&new_lines[i])
                });
                if touches {
                    for i in hunk.clone() {
                        selected[i] = diff.lines[i].is_change();
                    }
                }
            }
            if diff.binary && region.end == usize::MAX {
                selected = vec![true; diff.lines.len().max(1)];
            }
            staged = selected.iter().filter(|s| **s).count();
            if staged > 0
                && let Err(err) = apply(&self.repo, diff, file.untracked, Change::Stage, &selected)
            {
                self.error(format!("{err:#}"));
                return;
            }
        }
        self.review.items[n].accepted = true;
        if let Err(err) = self.refresh() {
            self.error(format!("{err:#}"));
        }
        self.persist();
        if self.message.is_none() {
            if staged > 0 {
                self.info(format!("accepted: staged {staged} line(s)"));
            } else {
                self.info("accepted: its fix was already staged");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        NoteStyle, Pane, Store,
        tests::{press, state},
    };
    use crate::{diff::Row, git::fixture::Fixture, review::write};

    /// A review written at one point, then the agent edits the file and ticks
    /// the item; the tool reopens it with `--review`.
    fn reviewed(f: &Fixture) -> super::super::AppState {
        let body = crate::git::fixture::numbered(20);
        reviewed_as(f, &body.replace("line 5\n", "line 5 good\nline 5b\n"))
    }

    /// As `reviewed`, with the agent leaving the file as `fixed`.
    fn reviewed_as(f: &Fixture, fixed: &str) -> super::super::AppState {
        let body = crate::git::fixture::numbered(20);
        f.write("a.txt", &body);
        f.commit_all("init");
        f.write("a.txt", &body.replace("line 5\n", "line 5 bad\n"));
        let mut s = state(f);
        s.focus = Pane::Diff;
        comment_on(&mut s, "line 5 bad", "name it better");
        reopen_ticked(f, &s, fixed)
    }

    fn comment_on(s: &mut super::super::AppState, line: &str, text: &str) {
        let file = s.file.as_mut().unwrap();
        let at = file.sections[0]
            .diff
            .lines
            .iter()
            .position(|l| l.text == line)
            .unwrap();
        file.cursor = file.view.row_of(0, at).unwrap();
        press(s, &format!("c{text}\n"));
    }

    /// The agent leaves the file as `fixed` and ticks every item; the review
    /// is reopened.
    fn reopen_ticked(
        f: &Fixture,
        s: &super::super::AppState,
        fixed: &str,
    ) -> super::super::AppState {
        let path = s.review_path().unwrap().clone();
        f.write("a.txt", fixed);
        let mut review = s.review.clone();
        for item in &mut review.items {
            item.done = true;
        }
        std::fs::write(&path, write(&review)).unwrap();
        let (review, store) = super::super::persist::open(path).unwrap();
        let mut s = super::super::AppState::new(
            f.repo.clone(),
            crate::git::Target::WorkTree,
            crate::git::snapshot(&f.repo, &crate::git::Target::WorkTree).unwrap(),
            crate::config::Config::default(),
            review,
            store,
        );
        assert!(matches!(s.store, Store::File { path: Some(_), .. }));
        s.resize(120, 40);
        s.focus = Pane::Diff;
        s
    }

    /// Cursor on the note of the first item not accepted yet.
    fn on_waiting_note(s: &mut super::super::AppState) {
        let accepted: Vec<bool> = s.review.items.iter().map(|i| i.accepted).collect();
        let file = s.file.as_mut().unwrap();
        file.cursor = file
            .view
            .rows
            .iter()
            .position(|r| matches!(r, Row::Note(n) if !accepted[*n]))
            .unwrap();
    }

    /// Whether the row above the note of item `n` shows `text`.
    fn note_under(s: &super::super::AppState, n: usize, text: &str) -> bool {
        let file = s.file.as_ref().unwrap();
        let note = file.view.rows.iter().position(|r| *r == Row::Note(n));
        note.is_some_and(|note| {
            matches!(
                file.view.rows[note - 1],
                Row::Code { section, lines } if lines.iter().flatten().any(|i| {
                    file.sections[section].diff.lines[*i].text == text
                })
            )
        })
    }

    #[test]
    fn reopened_review_shows_done_items_with_what_changed() {
        let f = Fixture::new();
        let s = reviewed(&f);
        assert!(s.changed_since_review.contains("a.txt"));
        let file = s.file.as_ref().unwrap();
        let notes = &file.notes[&0];
        assert!(matches!(
            notes[0].style,
            NoteStyle::Header { done: true, .. }
        ));
        assert!(
            notes
                .iter()
                .any(|l| l.style == NoteStyle::Added && l.text.contains("line 5 good"))
        );
        assert!(
            notes
                .iter()
                .any(|l| l.style == NoteStyle::Removed && l.text.contains("line 5 bad"))
        );
    }

    #[test]
    fn accepting_a_done_item_stages_its_hunk_and_keeps_it_accepted() {
        let f = Fixture::new();
        let mut s = reviewed(&f);
        on_waiting_note(&mut s);
        press(&mut s, "a");
        assert!(s.review.items[0].accepted, "{:?}", s.message);
        assert!(f.git(&["diff", "--cached"]).contains("+line 5 good"));
        assert!(s.complete());
        // Written as accepted, still shown under its now staged line, and a
        // second `a` leaves it be.
        let text = std::fs::read_to_string(s.review_path().unwrap()).unwrap();
        assert!(text.contains("- [x] accepted fix"), "{text}");
        assert!(note_under(&s, 0, "line 5 good"));
        let file = s.file.as_mut().unwrap();
        file.cursor = file
            .view
            .rows
            .iter()
            .position(|r| *r == Row::Note(0))
            .unwrap();
        press(&mut s, "a");
        assert_eq!(s.message.as_ref().unwrap().text, "already accepted");
    }

    #[test]
    fn a_fix_that_edited_and_moved_the_quoted_line_is_found_and_accepted() {
        let f = Fixture::new();
        let fixed = crate::git::fixture::numbered(20)
            .replace("line 2\n", "line 2\nnew a\nnew b\n")
            .replace("line 5\n", "line 5 better\n");
        let mut s = reviewed_as(&f, &fixed);
        let shown = &s.file.as_ref().unwrap().notes[&0];
        assert!(
            !shown.iter().any(|l| l.text.contains("anchor lost")),
            "{shown:?}"
        );
        // The note sits under the line's new place, two lines down.
        assert!(note_under(&s, 0, "line 5 better"));
        on_waiting_note(&mut s);
        press(&mut s, "a");
        assert!(s.review.items[0].accepted, "{:?}", s.message);
        assert!(f.git(&["diff", "--cached"]).contains("+line 5 better"));
    }

    #[test]
    fn staging_a_done_items_lines_by_hand_accepts_it() {
        let f = Fixture::new();
        let mut s = reviewed(&f);
        on_waiting_note(&mut s);
        s.file.as_mut().unwrap().cursor -= 1;
        press(&mut s, "s");
        assert!(s.confirm.is_none(), "a done item needs no question");
        assert_eq!(s.review.items.len(), 1);
        assert!(s.review.items[0].accepted);
    }

    #[test]
    fn an_item_whose_rewritten_fix_is_already_staged_is_found_and_closes() {
        let f = Fixture::new();
        let mut s = reviewed(&f);
        // Accepting a neighbour stages the whole hunk, this fix with it.
        f.git(&["add", "a.txt"]);
        s.refresh().unwrap();
        let shown = &s.file.as_ref().unwrap().notes[&0];
        assert!(
            !shown.iter().any(|l| l.text.contains("anchor lost")),
            "{shown:?}"
        );
        on_waiting_note(&mut s);
        press(&mut s, "a");
        assert!(s.review.items[0].accepted, "{:?}", s.message);
        // The last item closing completes the review, which says so instead.
        assert!(s.complete());
    }

    #[test]
    fn pushing_back_replaces_the_done_item_with_an_open_one() {
        let f = Fixture::new();
        let mut s = reviewed(&f);
        on_waiting_note(&mut s);
        press(&mut s, "cstill unclear\n");
        assert_eq!(s.review.items.len(), 1);
        let item = &s.review.items[0];
        assert!(!item.done);
        assert_eq!(item.text, "still unclear");
        assert_eq!(s.review.status, crate::review::Status::Open);
        let text = std::fs::read_to_string(s.review_path().unwrap()).unwrap();
        assert!(text.contains("status: open"), "{text}");
    }
}
