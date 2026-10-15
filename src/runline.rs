//! Lines of run output by kind, and how each one reads: an agent's prose as
//! Markdown, tool calls with the tool name set apart, and every line wrapped
//! at word boundaries with list items hanging under their text. Pure data;
//! the run pane maps each `Seg` to a colour.

use crate::diff::wrap_words;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunLine {
    /// Output as the command printed it.
    Plain(String),
    /// An agent's own words, Markdown.
    Prose(String),
    /// A line inside a fenced code block of the agent's words.
    Code(String),
    /// A tool call: what it used and on what.
    Tool { name: String, subject: String },
    /// A failed tool call.
    Error(String),
    /// Start and end of a session.
    Status(String),
}

/// How a piece of a line is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seg {
    Text,
    Muted,
    Accent,
    Bold,
    Code,
    Heading,
    Error,
    Done,
}

/// A line ready to draw: its pieces, and how far rows after the first are
/// indented.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub segs: Vec<(String, Seg)>,
    pub indent: usize,
}

impl RunLine {
    pub fn shown(&self) -> Shown {
        let one = |text: &str, seg| Shown {
            segs: vec![(text.to_owned(), seg)],
            indent: 0,
        };
        match self {
            Self::Plain(text) => one(text, Seg::Text),
            Self::Code(text) => one(text, Seg::Code),
            Self::Error(text) => one(text, Seg::Error),
            Self::Prose(text) => markdown(text),
            Self::Tool { name, subject } => Shown {
                segs: vec![
                    ("· ".to_owned(), Seg::Muted),
                    (name.clone(), Seg::Accent),
                    (format!(" {subject}"), Seg::Text),
                ],
                indent: 2,
            },
            Self::Status(text) => {
                let (word, seg) = if text.starts_with("· finished") {
                    ("finished", Seg::Done)
                } else if text.starts_with("· failed") {
                    ("failed", Seg::Error)
                } else {
                    return one(text, Seg::Muted);
                };
                let rest = &text["· ".len() + word.len()..];
                Shown {
                    segs: vec![
                        ("· ".to_owned(), Seg::Muted),
                        (word.to_owned(), seg),
                        (rest.to_owned(), Seg::Muted),
                    ],
                    indent: 2,
                }
            }
        }
    }

    /// The line as screen rows `width` cells wide, continuation rows
    /// indented.
    pub fn rows(&self, width: usize) -> Vec<Vec<(String, Seg)>> {
        self.shown().rows(width)
    }
}

impl Shown {
    pub fn rows(&self, width: usize) -> Vec<Vec<(String, Seg)>> {
        let text: String = self.segs.iter().map(|(t, _)| t.as_str()).collect();
        let indent = if self.indent * 2 < width {
            self.indent
        } else {
            0
        };
        let mut rows = Vec::new();
        for (k, range) in wrap_words(&text, width, width - indent)
            .into_iter()
            .enumerate()
        {
            let mut row = Vec::new();
            if k > 0 && indent > 0 {
                row.push((" ".repeat(indent), Seg::Text));
            }
            let mut at = 0;
            for (piece, seg) in &self.segs {
                let span = at..at + piece.len();
                at = span.end;
                let start = span.start.max(range.start);
                let end = span.end.min(range.end);
                if start < end {
                    row.push((text[start..end].to_owned(), *seg));
                }
            }
            rows.push(row);
        }
        rows
    }
}

/// One line of Markdown: a heading, a list item or a paragraph line, with
/// bold and code spans inside.
fn markdown(line: &str) -> Shown {
    let body = line.trim_start();
    let lead = &line[..line.len() - body.len()];
    let mut segs = Vec::new();
    if !lead.is_empty() {
        segs.push((lead.to_owned(), Seg::Text));
    }
    let hashes = body.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes)
        && let Some(title) = body[hashes..].strip_prefix(' ')
    {
        inline(title, Seg::Heading, &mut segs);
        return Shown { segs, indent: 0 };
    }
    if let Some(item) = body.strip_prefix("- ").or_else(|| body.strip_prefix("* ")) {
        segs.push(("• ".to_owned(), Seg::Muted));
        inline(item, Seg::Text, &mut segs);
        return Shown {
            segs,
            indent: lead.len() + 2,
        };
    }
    let digits = body.chars().take_while(char::is_ascii_digit).count();
    if digits > 0
        && let Some(item) = body[digits..].strip_prefix(". ")
    {
        let marker = &body[..digits + 2];
        segs.push((marker.to_owned(), Seg::Muted));
        inline(item, Seg::Text, &mut segs);
        return Shown {
            segs,
            indent: lead.len() + marker.len(),
        };
    }
    inline(body, Seg::Text, &mut segs);
    Shown {
        segs,
        indent: lead.len(),
    }
}

/// `**bold**` and `` `code` `` spans of `text`; a marker without its
/// closing pair stays as written.
fn inline(text: &str, base: Seg, segs: &mut Vec<(String, Seg)>) {
    let mut plain = String::new();
    let mut bold = false;
    let style = |bold: bool| {
        if bold && base == Seg::Text {
            Seg::Bold
        } else {
            base
        }
    };
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if let Some(after) = rest.strip_prefix('`')
            && let Some(end) = after.find('`')
        {
            flush(&mut plain, style(bold), segs);
            if end > 0 {
                segs.push((after[..end].to_owned(), Seg::Code));
            }
            rest = &after[end + 1..];
            continue;
        }
        if let Some(after) = rest.strip_prefix("**")
            && (bold || after.contains("**"))
        {
            flush(&mut plain, style(bold), segs);
            bold = !bold;
            rest = after;
            continue;
        }
        plain.push(c);
        rest = &rest[c.len_utf8()..];
    }
    flush(&mut plain, style(bold), segs);
}

fn flush(plain: &mut String, seg: Seg, segs: &mut Vec<(String, Seg)>) {
    if !plain.is_empty() {
        segs.push((std::mem::take(plain), seg));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prose(text: &str) -> Vec<(String, Seg)> {
        RunLine::Prose(text.to_owned()).shown().segs
    }

    fn seg(text: &str, seg: Seg) -> (String, Seg) {
        (text.to_owned(), seg)
    }

    #[test]
    fn markdown_bold_code_lists_and_headings_lose_their_markers() {
        assert_eq!(
            prose("**Change:** in `guide.mdx`, done"),
            [
                seg("Change:", Seg::Bold),
                seg(" in ", Seg::Text),
                seg("guide.mdx", Seg::Code),
                seg(", done", Seg::Text),
            ]
        );
        assert_eq!(
            prose("  - nested `x`"),
            [
                seg("  ", Seg::Text),
                seg("• ", Seg::Muted),
                seg("nested ", Seg::Text),
                seg("x", Seg::Code),
            ]
        );
        assert_eq!(prose("## Gates"), [seg("Gates", Seg::Heading)]);
        assert_eq!(
            prose("2 ** 3 and a ` tick"),
            [seg("2 ** 3 and a ` tick", Seg::Text)]
        );
    }

    #[test]
    fn rows_break_between_words_and_list_items_hang() {
        let line = RunLine::Prose("- one two three four".to_owned());
        let rows: Vec<String> = line
            .rows(10)
            .iter()
            .map(|row| row.iter().map(|(t, _)| t.as_str()).collect())
            .collect();
        assert_eq!(rows, ["• one two", "  three", "  four"]);
    }

    #[test]
    fn tools_and_endings_are_set_apart() {
        let tool = RunLine::Tool {
            name: "edit".into(),
            subject: "src/a.rs".into(),
        };
        assert_eq!(tool.shown().segs[1], seg("edit", Seg::Accent));
        let done = RunLine::Status("· finished · 7 turns".into()).shown();
        assert_eq!(done.segs[1], seg("finished", Seg::Done));
        assert_eq!(done.segs[2], seg(" · 7 turns", Seg::Muted));
    }
}
