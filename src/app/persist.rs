//! Writing the review file after every change, and picking up edits an agent
//! makes to it while the tool runs.

use super::AppState;
use crate::review::{self, Review, merge, parse, write};
use anyhow::{Context, Result};
use std::{fs, path::PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Store {
    /// `.revmark/<name>.md`. `path` is `None` until the first annotation;
    /// `synced` is the file's text as last written or read.
    File {
        path: Option<PathBuf>,
        synced: String,
    },
    /// `--out -`: nothing is written until the tool exits.
    Stdout,
}

impl AppState {
    /// Writes the review, merging in whatever changed on disk since the last
    /// write. Called after every annotation change.
    pub fn persist(&mut self) {
        if let Err(err) = self.try_persist() {
            self.error(format!("could not write the review: {err:#}"));
        }
        self.after_review_change();
    }

    fn try_persist(&mut self) -> Result<()> {
        self.review.normalize();
        let Store::File { path, synced } = &mut self.store else {
            return Ok(());
        };
        if path.is_none() {
            if self.review.is_empty() {
                return Ok(());
            }
            let dir = self.repo.review_dir();
            fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
            let name = review::next_name(&dir, &review::today());
            self.review.name.clone_from(&name);
            *path = Some(dir.join(format!("{name}.md")));
        }
        let file = path.as_ref().expect("path set above");
        let disk = fs::read_to_string(file).unwrap_or_default();
        if !synced.is_empty() && disk != *synced {
            let base = parse(synced)?;
            let theirs = parse(&disk)?;
            self.review = merge(&base, &self.review, &theirs);
        }
        let text = write(&self.review);
        fs::write(file, &text).with_context(|| format!("write {}", file.display()))?;
        *synced = text;
        Ok(())
    }

    /// Adopts edits made to the review file by someone else. Called on every
    /// idle tick. Returns whether anything changed.
    pub fn poll_review(&mut self) -> bool {
        let Store::File {
            path: Some(file),
            synced,
        } = &mut self.store
        else {
            return false;
        };
        let Ok(disk) = fs::read_to_string(&*file) else {
            return false;
        };
        if disk == *synced {
            return false;
        }
        let (Ok(base), Ok(theirs)) = (parse(synced), parse(&disk)) else {
            return false;
        };
        self.review = merge(&base, &self.review, &theirs);
        *synced = disk;
        self.after_review_change();
        true
    }

    /// Path of the review file, once it exists.
    pub fn review_path(&self) -> Option<&PathBuf> {
        match &self.store {
            Store::File { path, .. } => path.as_ref(),
            Store::Stdout => None,
        }
    }

    /// Records the blob of `path` the first time an item is added to it, so a
    /// re-review can tell whether the file changed since.
    pub fn record_blob(&mut self, path: &str) {
        if self.review.files.contains_key(path) {
            return;
        }
        if let Ok(blob) = self.repo.blob(&self.target, path) {
            self.review.files.insert(path.to_owned(), blob);
        }
    }

    fn after_review_change(&mut self) {
        let before = self.tree.len();
        self.rebuild_tree();
        if self.tree.len() != before {
            self.open_under_cursor();
        }
        self.rebuild_rows();
        if self.comments.visible {
            self.rebuild_comments();
        }
        if self.complete() {
            self.info("review complete: no open items and nothing left unstaged");
        }
    }

    /// The final review, for `--out -`.
    pub fn final_review(&self) -> Option<String> {
        (!self.review.is_empty()).then(|| write(&self.review))
    }
}

/// Loads an existing review for `--review`, as a store and its review.
pub fn open(path: PathBuf) -> Result<(Review, Store)> {
    let (review, text) = review::load(&path).with_context(|| format!("read {}", path.display()))?;
    Ok((
        review,
        Store::File {
            path: Some(path),
            synced: text,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::super::tests::{press, state};
    use crate::git::fixture::Fixture;

    #[test]
    fn first_annotation_creates_the_review_file_and_every_change_rewrites_it() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit_all("init");
        f.write("a.txt", "two\n");
        let mut s = state(&f);
        assert!(!f.path().join(crate::git::REVIEW_DIR).exists());
        press(&mut s, "nfirst\n");
        let path = s.review_path().unwrap().clone();
        assert!(
            path.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .ends_with("-00.md")
        );
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("- [ ] fix: first")
        );
        press(&mut s, "nsecond\n");
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("- [ ] fix: second")
        );
    }

    #[test]
    fn stdout_mode_writes_nothing_and_prints_the_review_at_the_end() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit_all("init");
        f.write("a.txt", "two\n");
        let mut s = state(&f);
        s.store = super::Store::Stdout;
        assert!(s.final_review().is_none());
        press(&mut s, "nprinted\n");
        assert!(!f.path().join(crate::git::REVIEW_DIR).exists());
        assert!(s.final_review().unwrap().contains("- [ ] fix: printed"));
    }

    #[test]
    fn agent_ticks_made_while_running_survive_the_next_write() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit_all("init");
        f.write("a.txt", "two\n");
        let mut s = state(&f);
        press(&mut s, "nfirst\n");
        let path = s.review_path().unwrap().clone();
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.replace("- [ ] fix: first", "- [x] fix: first")).unwrap();
        press(&mut s, "nsecond\n");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("- [x] fix: first"), "{text}");
        assert!(text.contains("- [ ] fix: second"), "{text}");
    }

    #[test]
    fn tool_never_writes_done_status() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit_all("init");
        f.write("a.txt", "two\n");
        let mut s = state(&f);
        press(&mut s, "nonly\n");
        s.review.items[0].done = true;
        s.persist();
        let text = std::fs::read_to_string(s.review_path().unwrap()).unwrap();
        assert!(text.contains("status: open"), "{text}");
    }
}
