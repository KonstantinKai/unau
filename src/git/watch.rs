//! A cheap fingerprint of the working tree, so changes made outside unau (an
//! agent, another editor, `git add` in another shell) are noticed without
//! re-reading every diff.

use super::{Repo, snapshot::under_review_dir};
use anyhow::Result;
use std::hash::{DefaultHasher, Hash, Hasher};

/// Changes whenever the index or a changed or untracked file does. One
/// `git status` lists those files with the index blob of each, which catches
/// staging even when only some lines of a file move; each file's size and
/// modification time catch a second edit to a file that was already changed.
/// The review folder is left out, or every annotation unau writes would look
/// like an outside change.
pub fn fingerprint(repo: &Repo) -> Result<u64> {
    // Without optional locks, git does not rewrite the index to refresh its
    // stat cache, which would change what the next call sees and race an
    // agent's own git commands.
    let status = repo.run(&[
        "--no-optional-locks",
        "status",
        "--porcelain=v2",
        "-z",
        "--untracked-files=all",
        "--no-renames",
    ])?;
    let configured = repo.review_dir_in_tree();
    let mut hasher = DefaultHasher::new();
    for entry in status.split('\0').filter(|e| !e.is_empty()) {
        let Some(path) = entry_path(entry) else {
            continue;
        };
        if under_review_dir(path, configured.as_deref()) {
            continue;
        }
        entry.hash(&mut hasher);
        if let Ok(meta) = std::fs::symlink_metadata(repo.root.join(path)) {
            meta.len().hash(&mut hasher);
            meta.modified().ok().hash(&mut hasher);
        }
    }
    Ok(hasher.finish())
}

/// The path of one `--porcelain=v2` entry: the last of nine fields for a
/// changed file, of eleven for an unmerged one, and the rest of the line for
/// untracked and ignored files.
fn entry_path(entry: &str) -> Option<&str> {
    match entry.as_bytes().first()? {
        b'1' => entry.splitn(9, ' ').nth(8),
        b'u' => entry.splitn(11, ' ').nth(10),
        b'?' | b'!' => entry.get(2..),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::fixture::Fixture;

    #[test]
    fn edits_new_files_and_staging_change_the_fingerprint() {
        let f = Fixture::new();
        f.write("a.txt", "one\ntwo\n");
        f.commit_all("init");
        let clean = fingerprint(&f.repo).unwrap();
        assert_eq!(fingerprint(&f.repo).unwrap(), clean);
        f.write("a.txt", "ONE\ntwo\n");
        let edited = fingerprint(&f.repo).unwrap();
        assert_ne!(edited, clean);
        // A second edit to a file already changed keeps its status line, so
        // only its size gives it away here (times can share a clock tick).
        f.write("a.txt", "ONE\nTWO!\n");
        let again = fingerprint(&f.repo).unwrap();
        assert_ne!(again, edited);
        f.git(&["add", "a.txt"]);
        let staged = fingerprint(&f.repo).unwrap();
        assert_ne!(staged, again);
        f.write("new.txt", "hi\n");
        assert_ne!(fingerprint(&f.repo).unwrap(), staged);
    }

    #[test]
    fn writing_review_files_leaves_the_fingerprint_alone() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit_all("init");
        f.write("a.txt", "two\n");
        let before = fingerprint(&f.repo).unwrap();
        f.write(".unau/2026-10-16-00.md", "# Review\n");
        assert_eq!(fingerprint(&f.repo).unwrap(), before);
    }

    #[test]
    fn porcelain_entries_give_their_paths() {
        let changed = "1 .M N... 100644 100644 100644 abc abc src/a b.rs";
        assert_eq!(entry_path(changed), Some("src/a b.rs"));
        assert_eq!(entry_path("? new dir/x.txt"), Some("new dir/x.txt"));
        let unmerged = "u UU N... 100644 100644 100644 100644 h1 h2 h3 c.rs";
        assert_eq!(entry_path(unmerged), Some("c.rs"));
        assert_eq!(entry_path("# branch.oid abc"), None);
    }
}
