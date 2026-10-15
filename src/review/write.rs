//! Review to Markdown.

use super::{INSTRUCTIONS_HEADING, Item, Kind, Review, Scope};
use std::fmt::Write as _;

/// Phrase between a suggestion's replacement and the lines it replaces.
pub const IN_PLACE_OF: &str = "in place of";
/// Fixed text of a suggestion item line.
pub const REPLACE_WITH: &str = "replace with";
/// Fence language that marks a quote of removed lines.
pub const REMOVED_LANG: &str = "diff";

pub fn write(review: &Review) -> String {
    let mut out = String::new();
    out.push_str("---\n");
    let _ = writeln!(out, "tool: {}", review.tool);
    let _ = writeln!(out, "status: {}", review.status.name());
    let _ = writeln!(out, "base: {}", review.base);
    out.push_str("files:\n");
    for (path, sha) in &review.files {
        let _ = writeln!(out, "  {path}: {sha}");
    }
    out.push_str("---\n\n");
    let _ = writeln!(out, "# Review {}\n", review.name);
    // A blockquote marks the instructions off from the items for the agent and
    // for the parser, and allows lists and blank lines inside.
    out.push_str(INSTRUCTIONS_HEADING);
    out.push_str("\n>\n");
    for line in review.instructions.lines() {
        if line.is_empty() {
            out.push_str(">\n");
        } else {
            let _ = writeln!(out, "> {line}");
        }
    }
    if !review.extra.is_empty() {
        out.push('\n');
        for line in &review.extra {
            out.push_str(line);
            out.push('\n');
        }
    }
    let mut section: Option<Option<&str>> = None;
    for item in &review.items {
        let path = item.scope.path();
        if section != Some(path) {
            section = Some(path);
            let _ = writeln!(out, "\n## {}\n", path.unwrap_or("General"));
        }
        write_item(&mut out, item);
    }
    out
}

fn write_item(out: &mut String, item: &Item) {
    let check = if item.done {
        'x'
    } else if item.working {
        '~'
    } else {
        ' '
    };
    let kind = item.kind.name();
    let (first, rest) = match item.text.split_once('\n') {
        Some((first, rest)) => (first, Some(rest)),
        None => (item.text.as_str(), None),
    };
    let text = if item.kind == Kind::Suggestion && item.replacement.is_some() {
        REPLACE_WITH
    } else {
        first
    };
    match &item.scope {
        Scope::General => {
            let _ = writeln!(out, "- [{check}] {kind}: {text}");
        }
        Scope::File(_) => {
            let _ = writeln!(out, "- [{check}] {kind} (file) — {text}");
        }
        Scope::Lines { start, end, .. } => {
            let range = if start == end {
                format!(":{start}")
            } else {
                format!(":{start}-{end}")
            };
            let _ = writeln!(out, "- [{check}] {kind} `{range}` — {text}");
        }
    }
    if item.kind != Kind::Suggestion || item.replacement.is_none() {
        for line in rest.into_iter().flat_map(str::lines) {
            indent(out, line);
        }
    }
    let removed = matches!(item.scope, Scope::Lines { removed: true, .. });
    let mut quote: Vec<String> = if removed {
        item.quote.iter().map(|l| format!("-{l}")).collect()
    } else {
        item.quote.clone()
    };
    if let Some(elision) = item.elided {
        quote.insert(elision.at.min(quote.len()), elision.marker());
    }
    let lang = if removed { REMOVED_LANG } else { &item.lang };
    if let Some(replacement) = &item.replacement {
        fence(out, &item.lang, replacement);
        if !quote.is_empty() {
            indent(out, IN_PLACE_OF);
            fence(out, lang, &quote);
        }
    } else if !quote.is_empty() {
        fence(out, lang, &quote);
    }
    for line in &item.extra {
        out.push_str(line);
        out.push('\n');
    }
}

fn indent(out: &mut String, line: &str) {
    if line.is_empty() {
        out.push('\n');
    } else {
        let _ = writeln!(out, "  {line}");
    }
}

/// A fence longer than any backtick run inside it, so quoted Markdown or doc
/// comments that contain fences cannot close it early.
pub fn fence_for(lines: &[String]) -> String {
    let longest = lines
        .iter()
        .flat_map(|l| l.split(|c| c != '`'))
        .map(str::len)
        .max()
        .unwrap_or(0);
    "`".repeat((longest + 1).max(3))
}

fn fence(out: &mut String, lang: &str, lines: &[String]) {
    let ticks = fence_for(lines);
    let _ = writeln!(out, "  {ticks}{lang}");
    for line in lines {
        indent(out, line);
    }
    let _ = writeln!(out, "  {ticks}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fence_is_longer_than_any_backtick_run_inside() {
        assert_eq!(fence_for(&["plain".into()]), "```");
        assert_eq!(fence_for(&["```rust".into(), "x".into()]), "````");
        assert_eq!(fence_for(&["a ````` b".into()]), "``````");
    }
}
