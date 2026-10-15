//! Launching the user's editor: `$VISUAL`, then `$EDITOR`, then `vi`.

use anyhow::{Context, Result};
use std::{io::Write, path::Path, process::Command};

/// The editor command line, as the user set it.
fn command() -> String {
    std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .ok()
        .filter(|e| !e.trim().is_empty())
        .unwrap_or_else(|| "vi".to_owned())
}

/// Runs the editor on `path`; true when it exits cleanly.
pub fn open(path: &Path) -> Result<bool> {
    open_at(path, None)
}

/// Runs the editor on `path`, at `line` when the editor is known to take
/// one; true when it exits cleanly.
pub fn open_at(path: &Path, line: Option<usize>) -> Result<bool> {
    let editor = command();
    let (before, file) = line_arguments(&editor, &path.to_string_lossy(), line);
    // The editor may carry its own arguments (`code --wait`), so the shell
    // splits it; the file and line go in as positional parameters.
    let status = Command::new("sh")
        .arg("-c")
        .arg(format!("{editor} {} \"$1\"", before.join(" ")))
        .arg("sh")
        .arg(file)
        .status()
        .with_context(|| format!("start {editor}"))?;
    Ok(status.success())
}

/// Opens the editor on a temporary file holding `text`. Returns the saved
/// text, or `None` when the editor exited with an error.
pub fn edit(text: &str, suffix: &str) -> Result<Option<String>> {
    let mut file = tempfile::Builder::new()
        .prefix("revmark-")
        .suffix(suffix)
        .tempfile()?;
    file.write_all(text.as_bytes())?;
    file.flush()?;
    if !open(file.path())? {
        return Ok(None);
    }
    Ok(Some(std::fs::read_to_string(file.path())?))
}

/// Arguments that go before the file, and the file argument itself, for
/// opening `path` at `line` in `editor`. Editors that are not recognised get
/// the plain path, since a stray `+12` would open a second file.
fn line_arguments(editor: &str, path: &str, line: Option<usize>) -> (Vec<String>, String) {
    let Some(line) = line else {
        return (Vec::new(), path.to_owned());
    };
    let program = editor
        .split_whitespace()
        .next()
        .and_then(|p| Path::new(p).file_name())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    match program.as_str() {
        "vi" | "vim" | "nvim" | "gvim" | "mvim" | "view" | "nano" | "pico" | "emacs"
        | "emacsclient" | "micro" | "kak" | "ne" | "joe" | "jed" | "mg" => {
            (vec![format!("+{line}")], path.to_owned())
        }
        "hx" | "helix" | "zed" | "subl" => (Vec::new(), format!("{path}:{line}")),
        "code" | "codium" | "code-insiders" | "cursor" | "windsurf" => {
            (vec!["--goto".to_owned()], format!("{path}:{line}"))
        }
        _ => (Vec::new(), path.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(editor: &str, line: Option<usize>) -> (Vec<String>, String) {
        line_arguments(editor, "src/a.rs", line)
    }

    #[test]
    fn terminal_editors_get_a_plus_line_argument() {
        for editor in ["nvim", "/usr/bin/vim", "nano -w", "emacsclient -t"] {
            assert_eq!(
                args(editor, Some(12)),
                (vec!["+12".to_owned()], "src/a.rs".to_owned()),
                "{editor}"
            );
        }
    }

    #[test]
    fn editors_that_read_path_colon_line_get_it_in_the_file_argument() {
        assert_eq!(args("hx", Some(3)), (Vec::new(), "src/a.rs:3".to_owned()));
        assert_eq!(
            args("code --wait", Some(3)),
            (vec!["--goto".to_owned()], "src/a.rs:3".to_owned())
        );
    }

    #[test]
    fn unknown_editors_and_no_line_get_the_plain_path() {
        assert_eq!(args("ed2000", Some(3)), (Vec::new(), "src/a.rs".to_owned()));
        assert_eq!(args("nvim", None), (Vec::new(), "src/a.rs".to_owned()));
    }
}
