//! Parser for the unified diff text `git diff` prints.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub kind: LineKind,
    /// One-based line number on the old side; `None` for added lines.
    pub old: Option<usize>,
    /// One-based line number on the new side; `None` for removed lines.
    pub new: Option<usize>,
    pub text: String,
    /// Git printed `\ No newline at end of file` right after this line.
    pub no_newline: bool,
}

impl Line {
    pub fn is_change(&self) -> bool {
        self.kind != LineKind::Context
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    pub binary: bool,
    pub new_file: bool,
    pub deleted: bool,
    pub lines: Vec<Line>,
}

/// Splits `git diff` output into one entry per file. Diffs are requested with
/// renames off, so the old and new paths of an entry are always the same.
pub fn parse(patch: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let (mut old, mut new) = (0, 0);
    let mut in_hunk = false;
    for raw in patch.split_inclusive('\n') {
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix("diff --git ") {
            files.push(FileDiff {
                path: header_path(rest),
                ..FileDiff::default()
            });
            in_hunk = false;
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if in_hunk {
            match line.as_bytes().first() {
                Some(b' ') => {
                    old += 1;
                    new += 1;
                    file.lines.push(context(old, new, &line[1..]));
                    continue;
                }
                Some(b'+') => {
                    new += 1;
                    file.lines.push(Line {
                        kind: LineKind::Added,
                        old: None,
                        new: Some(new),
                        text: line[1..].to_owned(),
                        no_newline: false,
                    });
                    continue;
                }
                Some(b'-') => {
                    old += 1;
                    file.lines.push(Line {
                        kind: LineKind::Removed,
                        old: Some(old),
                        new: None,
                        text: line[1..].to_owned(),
                        no_newline: false,
                    });
                    continue;
                }
                Some(b'\\') => {
                    if let Some(last) = file.lines.last_mut() {
                        last.no_newline = true;
                    }
                    continue;
                }
                _ if line.is_empty() => {
                    // Some tools strip the trailing space of empty context lines.
                    old += 1;
                    new += 1;
                    file.lines.push(context(old, new, ""));
                    continue;
                }
                _ => in_hunk = false,
            }
        }
        if let Some(header) = line.strip_prefix("@@ ") {
            let (o, n) = hunk_starts(header);
            old = o.saturating_sub(1);
            new = n.saturating_sub(1);
            in_hunk = true;
        } else if line.starts_with("new file mode") {
            file.new_file = true;
        } else if line.starts_with("deleted file mode") {
            file.deleted = true;
        } else if line.starts_with("Binary files ") || line == "GIT binary patch" {
            file.binary = true;
        } else if let Some(path) = line.strip_prefix("+++ b/") {
            file.path = unquote(path);
        } else if let Some(path) = line.strip_prefix("--- a/")
            && file.lines.is_empty()
        {
            file.path = unquote(path);
        }
    }
    files
}

fn context(old: usize, new: usize, text: &str) -> Line {
    Line {
        kind: LineKind::Context,
        old: Some(old),
        new: Some(new),
        text: text.to_owned(),
        no_newline: false,
    }
}

/// `a/x b/x` with both halves equal, because renames are off. Quoted paths are
/// unquoted; the `+++`/`---` lines, when present, override this guess.
fn header_path(rest: &str) -> String {
    if rest.starts_with('"')
        && let Some(end) = closing_quote(rest)
    {
        return unquote(&rest[..=end]).trim_start_matches("a/").to_owned();
    }
    let half = rest.len().saturating_sub(1) / 2;
    rest.get(..half)
        .unwrap_or(rest)
        .trim_start_matches("a/")
        .to_owned()
}

fn closing_quote(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// Reverses git's C-style path quoting (`"a/caf\303\251"`).
pub fn unquote(path: &str) -> String {
    let path = path.trim_end_matches('\t');
    let Some(inner) = path.strip_prefix('"').and_then(|p| p.strip_suffix('"')) else {
        return path.to_owned();
    };
    let mut bytes = Vec::with_capacity(inner.len());
    let mut chars = inner.bytes().peekable();
    while let Some(b) = chars.next() {
        if b != b'\\' {
            bytes.push(b);
            continue;
        }
        match chars.next() {
            Some(b'n') => bytes.push(b'\n'),
            Some(b't') => bytes.push(b'\t'),
            Some(d @ b'0'..=b'7') => {
                let mut value = u32::from(d - b'0');
                for _ in 0..2 {
                    if let Some(&next @ b'0'..=b'7') = chars.peek() {
                        value = value * 8 + u32::from(next - b'0');
                        chars.next();
                    }
                }
                bytes.push(u8::try_from(value).unwrap_or(b'?'));
            }
            Some(other) => bytes.push(other),
            None => {}
        }
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    text.strip_prefix("a/")
        .or_else(|| text.strip_prefix("b/"))
        .map_or(text.clone(), str::to_owned)
}

fn hunk_starts(header: &str) -> (usize, usize) {
    let mut parts = header.split_whitespace();
    let mut start = |prefix: char| {
        parts
            .next()
            .and_then(|p| p.strip_prefix(prefix))
            .and_then(|p| p.split(',').next())
            .and_then(|n| n.parse().ok())
            .unwrap_or(0)
    };
    let old = start('-');
    let new = start('+');
    (old.max(1), new.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "diff --git a/src/a.rs b/src/a.rs
index 1111111..2222222 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,3 +1,3 @@
 one
-two
+deux
 three
\\ No newline at end of file
diff --git a/new.txt b/new.txt
new file mode 100644
--- /dev/null
+++ b/new.txt
@@ -0,0 +1 @@
+hello
diff --git a/img.png b/img.png
Binary files a/img.png and b/img.png differ
";

    #[test]
    fn numbers_lines_on_both_sides() {
        let files = parse(PATCH);
        let a = &files[0];
        assert_eq!(a.path, "src/a.rs");
        let numbers: Vec<_> = a.lines.iter().map(|l| (l.old, l.new)).collect();
        assert_eq!(
            numbers,
            [
                (Some(1), Some(1)),
                (Some(2), None),
                (None, Some(2)),
                (Some(3), Some(3))
            ]
        );
        assert!(a.lines[3].no_newline);
    }

    #[test]
    fn recognises_new_and_binary_files() {
        let files = parse(PATCH);
        assert!(files[1].new_file);
        assert_eq!(files[1].lines[0].new, Some(1));
        assert_eq!(files[2].path, "img.png");
        assert!(files[2].binary);
    }

    #[test]
    fn unquotes_octal_escapes() {
        assert_eq!(unquote("\"b/caf\\303\\251.txt\""), "café.txt");
    }
}
