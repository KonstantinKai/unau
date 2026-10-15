//! Creating, editing and deleting annotations.

use super::{AppState, EditPurpose, Effect, Pane, Prompt, PromptPurpose, rereview};
use crate::{
    diff::Row,
    git::LineKind,
    review::{Item, Kind, Scope, Status},
    tree::Entry,
};

impl AppState {
    /// The selected lines as an anchor plus their quoted text. Only lines of
    /// the section under review can be annotated.
    pub fn selection_scope(&self) -> Result<(Scope, Vec<String>), String> {
        let file = self.file.as_ref().ok_or("no file open")?;
        let path = file
            .path()
            .ok_or("general comments have no lines; use the general comment key")?;
        let section = file.view.rows.get(file.cursor).and_then(Row::section);
        let Some(section) = section.filter(|s| Some(*s) == file.annotatable()) else {
            return Err("staged lines are already accepted; annotate unstaged lines".into());
        };
        let lines = &file.sections[section].diff.lines;
        let picked: Vec<usize> = file.view.rows[file.selected_rows()]
            .iter()
            .filter(|r| r.section() == Some(section))
            .flat_map(Row::lines)
            .collect();
        if picked.is_empty() {
            return Err("move the cursor onto code lines".into());
        }
        let news: Vec<usize> = picked.iter().filter_map(|i| lines[*i].new).collect();
        if let (Some(start), Some(end)) = (news.iter().min(), news.iter().max()) {
            let quote = file
                .new_text
                .get(start - 1..(*end).min(file.new_text.len()))
                .map(<[String]>::to_vec)
                .unwrap_or_default();
            return Ok((
                Scope::Lines {
                    path: path.to_owned(),
                    start: *start,
                    end: *end,
                    removed: false,
                },
                quote,
            ));
        }
        let last = *picked.iter().max().expect("picked is not empty");
        let start = lines[last..]
            .iter()
            .find_map(|l| l.new)
            .unwrap_or_else(|| lines.iter().filter_map(|l| l.new).max().unwrap_or(0) + 1);
        let mut removed: Vec<usize> = picked
            .into_iter()
            .filter(|i| lines[*i].kind == LineKind::Removed)
            .collect();
        removed.sort_unstable();
        removed.dedup();
        Ok((
            Scope::Lines {
                path: path.to_owned(),
                start,
                end: start,
                removed: true,
            },
            removed.into_iter().map(|i| lines[i].text.clone()).collect(),
        ))
    }

    pub fn comment(&mut self) {
        if let Some(item) = self
            .file
            .as_ref()
            .and_then(super::FileState::note_under_cursor)
            && self.review.items[item].done
        {
            let kind = match self.review.items[item].kind {
                Kind::Question => Kind::Question,
                Kind::Fix | Kind::Suggestion => Kind::Fix,
            };
            self.open_prompt(PromptPurpose::Reopen(item), "push back", String::new());
            if let Some(prompt) = &mut self.prompt {
                prompt.kind = kind;
            }
            return;
        }
        if self.focus == Pane::Tree {
            let file = self.key_label(crate::config::Action::FileComment);
            self.info(format!(
                "focus the diff to comment on lines; {file} comments on the file"
            ));
            return;
        }
        match self.selection_scope() {
            Ok((scope, quote)) => {
                let label = anchor_label(&scope);
                self.open_prompt(PromptPurpose::New { scope, quote }, &label, String::new());
            }
            Err(err) => self.error(err),
        }
    }

    pub fn suggestion(&mut self) -> Effect {
        match self.selection_scope() {
            Ok((Scope::Lines { removed: true, .. }, _)) => {
                self.error("a suggestion replaces new lines; select lines that exist now");
                Effect::None
            }
            Ok((scope, quote)) => {
                let suffix = scope
                    .path()
                    .and_then(|p| std::path::Path::new(p).extension())
                    .map(|e| format!(".{}", e.to_string_lossy()))
                    .unwrap_or_default();
                Effect::Editor {
                    text: join_lines(&quote),
                    suffix,
                    purpose: EditPurpose::Suggestion { scope, quote },
                }
            }
            Err(err) => {
                self.error(err);
                Effect::None
            }
        }
    }

    pub fn file_comment(&mut self) {
        let path = match self.focus {
            Pane::Diff | Pane::Comments | Pane::Run => {
                self.file.as_ref().and_then(|f| f.path().map(str::to_owned))
            }
            Pane::Tree => match self.tree.get(self.tree_cursor).map(|row| &row.entry) {
                Some(Entry::File(i)) => Some(self.paths[*i].clone()),
                _ => None,
            },
        };
        let Some(path) = path else {
            self.error("put the cursor on a file");
            return;
        };
        self.open_prompt(
            PromptPurpose::New {
                scope: Scope::File(path),
                quote: Vec::new(),
            },
            "file comment",
            String::new(),
        );
    }

    pub fn general_comment(&mut self) {
        self.open_prompt(PromptPurpose::General, "general comment", String::new());
    }

    pub fn edit(&mut self) -> Effect {
        let Some(n) = self.note_or_explain() else {
            return Effect::None;
        };
        let item = &self.review.items[n];
        if let Some(replacement) = &item.replacement {
            return Effect::Editor {
                text: join_lines(replacement),
                suffix: format!(".{}", item.lang),
                purpose: EditPurpose::Replacement(n),
            };
        }
        let prompt = Prompt {
            purpose: PromptPurpose::Edit(n),
            label: "edit".to_owned(),
            kind: item.kind,
            cursor: item.text.chars().count(),
            text: item.text.clone(),
        };
        if item.text.contains('\n') {
            return Effect::Editor {
                text: item.text.clone(),
                suffix: ".md".to_owned(),
                purpose: EditPurpose::Prompt(prompt),
            };
        }
        self.prompt = Some(prompt);
        Effect::None
    }

    pub fn delete(&mut self) {
        if let Some(n) = self.note_or_explain() {
            self.review.items.remove(n);
            self.persist();
        }
    }

    pub fn toggle_kind(&mut self) {
        let Some(n) = self.note_or_explain() else {
            return;
        };
        let item = &mut self.review.items[n];
        item.kind = match item.kind {
            Kind::Fix => Kind::Question,
            Kind::Question => Kind::Fix,
            Kind::Suggestion => {
                self.error("suggestions have no fix / question kind");
                return;
            }
        };
        self.persist();
    }

    fn note_or_explain(&mut self) -> Option<usize> {
        let note = self
            .file
            .as_ref()
            .and_then(super::FileState::note_under_cursor);
        if note.is_none() {
            self.error("put the cursor on an annotation");
        }
        note
    }

    pub fn submit_prompt(&mut self, prompt: Prompt) {
        let text = prompt.text.trim_end().to_owned();
        if text.trim().is_empty() {
            self.info("empty comment: nothing added");
            return;
        }
        match prompt.purpose {
            PromptPurpose::New { scope, quote } => {
                let path = scope.path().map(str::to_owned);
                let mut item = Item::new(prompt.kind, scope, text);
                item.set_quote(quote, self.config.quote_lines);
                if let Some(path) = path {
                    self.record_blob(&path);
                }
                self.review.items.push(item);
                if let Some(file) = &mut self.file {
                    file.anchor = None;
                }
            }
            PromptPurpose::General => {
                self.review
                    .items
                    .push(Item::new(prompt.kind, Scope::General, text));
            }
            PromptPurpose::Edit(n) => {
                let item = &mut self.review.items[n];
                item.text = text;
                if item.kind != Kind::Suggestion {
                    item.kind = prompt.kind;
                }
            }
            PromptPurpose::Reopen(n) => self.reopen(n, text, prompt.kind),
        }
        self.persist();
    }

    /// Replaces done item `n` with an open one carrying the push-back text,
    /// anchored where its lines are now.
    fn reopen(&mut self, n: usize, text: String, kind: Kind) {
        let old = self.review.items.remove(n);
        let mut item = Item::new(kind, old.scope.clone(), text);
        if let (
            Some(file),
            Scope::Lines {
                path,
                removed: false,
                ..
            },
        ) = (&self.file, &old.scope)
            && let Some(region) = rereview::find_region(&old, &file.new_text, &file.changed_lines())
        {
            item.scope = Scope::Lines {
                path: path.clone(),
                start: region.start,
                end: region.end,
                removed: false,
            };
            let lines = file.new_text[region.start - 1..region.end].to_vec();
            item.set_quote(lines, self.config.quote_lines);
        } else {
            item.quote = old.quote;
            item.elided = old.elided;
        }
        self.review.items.push(item);
        self.review.status = Status::Open;
    }

    /// Result of `$EDITOR`; `None` when the editor failed or was aborted.
    pub fn finish_edit(&mut self, purpose: EditPurpose, text: Option<String>) {
        let Some(text) = text else {
            self.info("editor closed without changes");
            return;
        };
        let lines: Vec<String> = text.lines().map(str::to_owned).collect();
        match purpose {
            EditPurpose::Suggestion { scope, quote } => {
                if lines == quote {
                    self.info("suggestion unchanged: nothing added");
                    return;
                }
                let path = scope.path().map(str::to_owned);
                let mut item = Item::new(Kind::Suggestion, scope, String::new());
                item.quote = quote;
                item.replacement = Some(lines);
                if let Some(path) = path {
                    self.record_blob(&path);
                }
                self.review.items.push(item);
                if let Some(file) = &mut self.file {
                    file.anchor = None;
                }
                self.persist();
            }
            EditPurpose::Replacement(n) => {
                if let Some(item) = self.review.items.get_mut(n) {
                    item.replacement = Some(lines);
                    self.persist();
                }
            }
            EditPurpose::Prompt(mut prompt) => {
                text.trim_end().clone_into(&mut prompt.text);
                self.submit_prompt(prompt);
            }
        }
    }
}

fn anchor_label(scope: &Scope) -> String {
    match scope {
        Scope::Lines { start, end, .. } if start == end => format!("comment :{start}"),
        Scope::Lines { start, end, .. } => format!("comment :{start}-{end}"),
        _ => "comment".to_owned(),
    }
}

fn join_lines(lines: &[String]) -> String {
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::super::tests::{press, state};
    use super::*;
    use crate::git::fixture::Fixture;

    fn repo() -> Fixture {
        let f = Fixture::new();
        f.write("a.ts", "one\ntwo\nthree\nfour\n");
        f.commit_all("init");
        f.write("a.ts", "one\nTWO\nthree\nFOUR\n");
        f
    }

    fn cursor_on_new(s: &mut AppState, new: usize) {
        let file = s.file.as_mut().unwrap();
        let line = file.sections[0]
            .diff
            .lines
            .iter()
            .position(|l| l.new == Some(new))
            .unwrap();
        file.cursor = file.view.row_of(0, line).unwrap();
        s.focus = Pane::Diff;
    }

    #[test]
    fn comment_on_a_selection_quotes_the_exact_new_side_lines() {
        let f = repo();
        let mut s = state(&f);
        cursor_on_new(&mut s, 2);
        press(&mut s, "v");
        cursor_on_new(&mut s, 4);
        press(&mut s, "cneeds work\n");
        let item = &s.review.items[0];
        assert_eq!(
            item.scope,
            Scope::Lines {
                path: "a.ts".into(),
                start: 2,
                end: 4,
                removed: false
            }
        );
        assert_eq!(item.quote, ["TWO", "three", "FOUR"]);
        let text = std::fs::read_to_string(s.review_path().unwrap()).unwrap();
        assert!(
            text.contains("- [ ] fix `:2-4` — needs work\n  ```ts\n  TWO\n  three\n  FOUR\n  ```"),
            "{text}"
        );
        assert!(text.contains("files:\n  a.ts: "), "{text}");
    }

    #[test]
    fn long_selections_are_quoted_by_their_ends_unless_the_cap_is_off() {
        let f = Fixture::new();
        f.write("a.ts", &crate::git::fixture::numbered(30));
        f.commit_all("init");
        f.write(
            "a.ts",
            &crate::git::fixture::numbered(30).replace("line ", "row "),
        );
        for (cap, kept) in [(12, 6), (0, 20)] {
            let mut s = state(&f);
            s.config.quote_lines = cap;
            cursor_on_new(&mut s, 1);
            press(&mut s, "v");
            cursor_on_new(&mut s, 20);
            press(&mut s, "csplit\n");
            let item = &s.review.items[0];
            assert_eq!(item.quote.len(), kept, "cap {cap}");
            assert_eq!(item.quote_len(), 20);
            assert_eq!(item.quote[0], "row 1");
        }
    }

    #[test]
    fn comment_on_removed_lines_anchors_before_the_next_new_line() {
        let f = repo();
        let mut s = state(&f);
        let file = s.file.as_mut().unwrap();
        let line = file.sections[0]
            .diff
            .lines
            .iter()
            .position(|l| l.text == "two" && l.kind == LineKind::Removed)
            .unwrap();
        file.cursor = file.view.row_of(0, line).unwrap();
        s.focus = Pane::Diff;
        press(&mut s, "cwhy remove\n");
        let item = &s.review.items[0];
        assert_eq!(
            item.scope,
            Scope::Lines {
                path: "a.ts".into(),
                start: 2,
                end: 2,
                removed: true
            }
        );
        assert_eq!(item.quote, ["two"]);
    }

    #[test]
    fn tab_in_the_popup_switches_between_fix_and_question() {
        let f = repo();
        let mut s = state(&f);
        cursor_on_new(&mut s, 2);
        let tab = crate::config::Key {
            code: crate::config::KeyCodeKey::Tab,
            ctrl: false,
        };
        press(&mut s, "cwhy");
        assert_eq!(s.prompt.as_ref().unwrap().kind, Kind::Fix);
        s.handle_key(tab);
        assert_eq!(s.prompt.as_ref().unwrap().kind, Kind::Question);
        press(&mut s, "?\n");
        assert_eq!(s.review.items[0].kind, Kind::Question);
        assert_eq!(s.review.items[0].text, "why?");
        press(&mut s, "noverall");
        s.handle_key(tab);
        s.handle_key(tab);
        press(&mut s, "\n");
        let general = s
            .review
            .items
            .iter()
            .find(|i| i.scope == Scope::General)
            .unwrap();
        assert_eq!(general.kind, Kind::Fix);
    }

    #[test]
    fn editing_starts_from_the_item_kind_and_tab_changes_it() {
        let f = repo();
        let mut s = state(&f);
        cursor_on_new(&mut s, 2);
        press(&mut s, "cname\n");
        let note = s
            .file
            .as_ref()
            .unwrap()
            .view
            .rows
            .iter()
            .position(|r| matches!(r, Row::Note(_)))
            .unwrap();
        s.file.as_mut().unwrap().cursor = note;
        press(&mut s, "e");
        assert_eq!(s.prompt.as_ref().unwrap().kind, Kind::Fix);
        s.handle_key(crate::config::Key {
            code: crate::config::KeyCodeKey::Tab,
            ctrl: false,
        });
        press(&mut s, "\n");
        assert_eq!(s.review.items[0].kind, Kind::Question);
        assert_eq!(s.review.items[0].text, "name");
    }

    #[test]
    fn edit_delete_and_kind_toggle_work_on_the_note_under_the_cursor() {
        let f = repo();
        let mut s = state(&f);
        cursor_on_new(&mut s, 2);
        press(&mut s, "cfirst\n");
        let note = s
            .file
            .as_ref()
            .unwrap()
            .view
            .rows
            .iter()
            .position(|r| matches!(r, Row::Note(_)))
            .unwrap();
        s.file.as_mut().unwrap().cursor = note;
        press(&mut s, "q");
        assert_eq!(s.review.items[0].kind, Kind::Question);
        press(&mut s, "e!\n");
        assert_eq!(s.review.items[0].text, "first!");
        press(&mut s, "x");
        assert!(s.review.items.is_empty());
    }

    #[test]
    fn unchanged_suggestion_creates_nothing_and_changed_one_is_saved() {
        let f = repo();
        let mut s = state(&f);
        cursor_on_new(&mut s, 2);
        let Effect::Editor {
            text,
            purpose,
            suffix,
        } = press(&mut s, "S")
        else {
            panic!("no editor effect");
        };
        assert_eq!(text, "TWO\n");
        assert_eq!(suffix, ".ts");
        s.finish_edit(purpose.clone(), Some(text));
        assert!(s.review.items.is_empty());
        s.finish_edit(purpose, Some("Two\n".into()));
        let item = &s.review.items[0];
        assert_eq!(item.kind, Kind::Suggestion);
        assert_eq!(item.replacement.as_deref(), Some(&["Two".to_owned()][..]));
        assert_eq!(item.quote, ["TWO"]);
    }

    #[test]
    fn file_and_general_comments_have_their_own_scopes() {
        let f = repo();
        let mut s = state(&f);
        press(&mut s, "Cno docs\n");
        press(&mut s, "noverall\n");
        let scopes: Vec<_> = s.review.items.iter().map(|i| i.scope.clone()).collect();
        assert_eq!(scopes, [Scope::General, Scope::File("a.ts".into())]);
        // General comments live in the comments pane, never in the file tree.
        assert_eq!(s.tree.len(), 1);
        assert_eq!(s.tree[0].label, "a.ts");
    }

    #[test]
    fn staged_lines_cannot_be_annotated() {
        let f = repo();
        f.git(&["add", "a.ts"]);
        let mut s = state(&f);
        s.focus = Pane::Diff;
        press(&mut s, "c");
        assert!(s.prompt.is_none());
        assert!(
            s.message
                .as_ref()
                .unwrap()
                .text
                .contains("already accepted")
        );
        assert!(s.review.items.is_empty());
    }
}
