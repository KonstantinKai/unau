//! File tree for the left pane: folding, chained single-child directories,
//! and a flat list mode.

use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    /// A directory row. `path` is the full directory path, used as fold key.
    Dir { path: String, folded: bool },
    /// A changed file, by index into the snapshot.
    File(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow {
    pub depth: usize,
    pub label: String,
    pub entry: Entry,
}

#[derive(Default)]
struct Node {
    dirs: BTreeMap<String, Node>,
    files: Vec<(String, usize)>,
}

/// Rows for `paths` (indexed as given). Directories in `folded` hide their
/// contents. In flat mode every file is one row with its full path.
pub fn build(paths: &[&str], folded: &HashSet<String>, flat: bool) -> Vec<TreeRow> {
    if flat {
        let mut order: Vec<usize> = (0..paths.len()).collect();
        order.sort_by_key(|i| paths[*i]);
        return order
            .into_iter()
            .map(|i| TreeRow {
                depth: 0,
                label: paths[i].to_owned(),
                entry: Entry::File(i),
            })
            .collect();
    }
    let mut root = Node::default();
    for (i, path) in paths.iter().enumerate() {
        let mut node = &mut root;
        let mut parts: Vec<&str> = path.split('/').collect();
        let name = parts.pop().unwrap_or_default();
        for part in parts {
            node = node.dirs.entry(part.to_owned()).or_default();
        }
        node.files.push((name.to_owned(), i));
    }
    let mut rows = Vec::new();
    emit(&root, "", 0, folded, &mut rows);
    rows
}

fn emit(
    node: &Node,
    prefix: &str,
    depth: usize,
    folded: &HashSet<String>,
    rows: &mut Vec<TreeRow>,
) {
    for (name, child) in &node.dirs {
        let mut label = name.clone();
        let mut path = format!("{prefix}{name}");
        let mut child = child;
        // A directory holding nothing but one other directory shares its row.
        while child.files.is_empty() && child.dirs.len() == 1 {
            let (next_name, next) = child.dirs.iter().next().expect("one child directory");
            label = format!("{label}/{next_name}");
            path = format!("{path}/{next_name}");
            child = next;
        }
        let is_folded = folded.contains(&path);
        rows.push(TreeRow {
            depth,
            label: format!("{label}/"),
            entry: Entry::Dir {
                path: path.clone(),
                folded: is_folded,
            },
        });
        if !is_folded {
            emit(child, &format!("{path}/"), depth + 1, folded, rows);
        }
    }
    let mut files = node.files.clone();
    files.sort();
    for (name, i) in files {
        rows.push(TreeRow {
            depth,
            label: name,
            entry: Entry::File(i),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(rows: &[TreeRow]) -> Vec<String> {
        rows.iter()
            .map(|r| format!("{}{}", "  ".repeat(r.depth), r.label))
            .collect()
    }

    #[test]
    fn chains_directories_that_only_hold_one_directory() {
        let paths = [
            "a/b/c/one.rs",
            "a/b/c/two.rs",
            "x/y.rs",
            "x/z/w.rs",
            "top.rs",
        ];
        let rows = build(&paths, &HashSet::new(), false);
        assert_eq!(
            labels(&rows),
            [
                "a/b/c/", "  one.rs", "  two.rs", "x/", "  z/", "    w.rs", "  y.rs", "top.rs"
            ]
        );
    }

    #[test]
    fn a_directory_with_files_is_not_chained_into_its_child() {
        let paths = ["a/f.rs", "a/b/c/g.rs"];
        let rows = build(&paths, &HashSet::new(), false);
        assert_eq!(labels(&rows), ["a/", "  b/c/", "    g.rs", "  f.rs"]);
    }

    #[test]
    fn folded_directories_hide_their_contents() {
        let paths = ["a/b/c/one.rs", "x/y.rs"];
        let folded = HashSet::from(["a/b/c".to_owned()]);
        let rows = build(&paths, &folded, false);
        assert_eq!(labels(&rows), ["a/b/c/", "x/", "  y.rs"]);
    }

    #[test]
    fn flat_mode_lists_full_paths() {
        let rows = build(&["b/x.rs", "a.rs"], &HashSet::new(), true);
        assert_eq!(labels(&rows), ["a.rs", "b/x.rs"]);
        assert_eq!(rows[1].entry, Entry::File(0));
    }
}
