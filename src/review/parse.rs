//! Markdown to review. Accepts files an agent has edited by hand: `[X]`,
//! `*` bullets, reordered sections, and prose the tool does not understand,
//! which is kept verbatim.

use super::write::{IN_PLACE_OF, REMOVED_LANG, REPLACE_WITH};
use super::{Elision, INSTRUCTIONS_HEADING, Item, Kind, Review, Scope, Status};
use anyhow::{Result, bail};

pub fn parse(text: &str) -> Result<Review> {
    let lines: Vec<&str> = text.lines().collect();
    let mut review = Review::new("", "HEAD");
    review.tool.clear();
    let mut i = 0;
    if lines.first().map(|l| l.trim_end()) == Some("---") {
        i = 1;
        let mut in_files = false;
        while i < lines.len() && lines[i].trim_end() != "---" {
            let line = lines[i];
            i += 1;
            if in_files && line.starts_with(' ') {
                if let Some((path, sha)) = line.trim().rsplit_once(": ") {
                    review.files.insert(path.to_owned(), sha.trim().to_owned());
                }
                continue;
            }
            in_files = false;
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.split(" #").next().unwrap_or_default().trim();
            match key.trim() {
                "tool" => value.clone_into(&mut review.tool),
                "status" => {
                    let word = value.split_whitespace().next().unwrap_or_default();
                    review.status = Status::from_name(word).unwrap_or_default();
                }
                "base" => {
                    value
                        .split_whitespace()
                        .next()
                        .unwrap_or("HEAD")
                        .clone_into(&mut review.base);
                }
                "files" => in_files = true,
                _ => {}
            }
        }
        if i == lines.len() {
            bail!("review file frontmatter is not closed with ---");
        }
        i += 1;
    }
    let mut section: Option<Option<String>> = None;
    while i < lines.len() {
        let line = lines[i];
        if let Some(name) = line.strip_prefix("# Review ") {
            name.trim().clone_into(&mut review.name);
            i += 1;
        } else if section.is_none() && line.trim_end() == INSTRUCTIONS_HEADING {
            i += 1;
            let mut text = Vec::new();
            while i < lines.len() && lines[i].starts_with('>') {
                let quoted = &lines[i][1..];
                text.push(quoted.strip_prefix(' ').unwrap_or(quoted));
                i += 1;
            }
            review.instructions = trim_blank(&text);
        } else if let Some(heading) = line.strip_prefix("## ") {
            let heading = heading.trim();
            section = Some((heading != "General").then(|| heading.to_owned()));
            i += 1;
        } else if let (Some(path), Some((mark, rest))) = (&section, checkbox(line)) {
            let end = body_end(&lines, i + 1);
            match item(
                path.as_deref(),
                mark == Mark::Done,
                rest,
                &lines[i + 1..end],
            ) {
                Some(mut item) => {
                    item.working = mark == Mark::Working;
                    review.items.push(item);
                }
                None => review
                    .extra
                    .extend(lines[i..end].iter().map(|l| (*l).to_owned())),
            }
            i = end;
        } else {
            if !line.trim().is_empty() {
                review.extra.push(line.to_owned());
            }
            i += 1;
        }
    }
    review.normalize();
    Ok(review)
}

/// Lines joined, without blank lines at either end.
fn trim_blank(lines: &[&str]) -> String {
    let start = lines
        .iter()
        .position(|l| !l.trim().is_empty())
        .unwrap_or(lines.len());
    let end = lines
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .map_or(start, |e| e + 1);
    lines[start..end].join("\n")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Open,
    /// `[~]`: an agent is on it.
    Working,
    Done,
}

/// `- [ ] rest`, `- [~] rest` or `* [x] rest` → (mark, rest).
fn checkbox(line: &str) -> Option<(Mark, &str)> {
    let rest = line
        .strip_prefix("- [")
        .or_else(|| line.strip_prefix("* ["))?;
    let mut chars = rest.chars();
    let mark = chars.next()?;
    let rest = chars.as_str().strip_prefix("] ")?;
    match mark {
        ' ' => Some((Mark::Open, rest)),
        '~' => Some((Mark::Working, rest)),
        'x' | 'X' => Some((Mark::Done, rest)),
        _ => None,
    }
}

/// Index of the first line after an item's body: the next item or heading.
fn body_end(lines: &[&str], mut i: usize) -> usize {
    while i < lines.len() && checkbox(lines[i]).is_none() && !lines[i].starts_with('#') {
        i += 1;
    }
    while i > 0 && lines.get(i - 1).is_some_and(|l| l.trim().is_empty()) {
        i -= 1;
    }
    i
}

fn split_text(rest: &str) -> Option<(&str, &str)> {
    rest.split_once(" — ").or_else(|| rest.split_once(" -- "))
}

fn item(path: Option<&str>, done: bool, rest: &str, body: &[&str]) -> Option<Item> {
    let (kind, scope, text) = match path {
        None => {
            let (kind, text) = rest.split_once(": ")?;
            (Kind::from_name(kind.trim())?, Scope::General, text)
        }
        Some(path) => {
            let (head, text) = split_text(rest)?;
            let (kind, anchor) = head.split_once(' ')?;
            let kind = Kind::from_name(kind)?;
            let anchor = anchor.trim();
            let scope = if anchor == "(file)" {
                Scope::File(path.to_owned())
            } else {
                let range = anchor.strip_prefix("`:")?.strip_suffix('`')?;
                let (start, end) = if let Some((a, b)) = range.split_once('-') {
                    (a.parse().ok()?, b.parse().ok()?)
                } else {
                    let n = range.parse().ok()?;
                    (n, n)
                };
                Scope::Lines {
                    path: path.to_owned(),
                    start,
                    end,
                    removed: false,
                }
            };
            (kind, scope, text)
        }
    };
    let mut item = Item::new(kind, scope, text.to_owned());
    item.done = done;
    let mut text_lines = Vec::new();
    let mut blocks: Vec<(String, Vec<String>)> = Vec::new();
    let mut i = 0;
    let mut in_text = true;
    while i < body.len() {
        let line = body[i];
        if let Some((ticks, lang)) = fence_open(line) {
            in_text = false;
            let mut content = Vec::new();
            i += 1;
            while i < body.len() && body[i].trim_end() != format!("  {ticks}") {
                content.push(body[i].strip_prefix("  ").unwrap_or(body[i]).to_owned());
                i += 1;
            }
            blocks.push((lang.to_owned(), content));
            i += 1;
        } else if line.trim() == IN_PLACE_OF && !blocks.is_empty() {
            in_text = false;
            i += 1;
        } else if in_text && (line.is_empty() || line.starts_with("  ")) {
            text_lines.push(line.strip_prefix("  ").unwrap_or(line));
            i += 1;
        } else {
            item.extra.extend(body[i..].iter().map(|l| (*l).to_owned()));
            break;
        }
    }
    while text_lines.last().is_some_and(|l| l.is_empty()) {
        text_lines.pop();
    }
    for line in text_lines {
        item.text.push('\n');
        item.text.push_str(line);
    }
    let mut blocks = blocks.into_iter();
    if kind == Kind::Suggestion && item.text == REPLACE_WITH {
        item.text.clear();
        if let Some((lang, replacement)) = blocks.next() {
            item.lang = lang;
            item.replacement = Some(replacement);
        }
    }
    if let Some((lang, quote)) = blocks.next() {
        read_quote(&mut item, lang, quote);
    }
    Some(item)
}

/// Fills in an item's quote from its code block: the capped-quote marker, and
/// whether the lines are removed ones.
fn read_quote(item: &mut Item, lang: String, mut quote: Vec<String>) {
    // Suggestion quotes are never capped, so a marker-like line there is code.
    if item.kind != Kind::Suggestion
        && let Some((at, count)) = quote
            .iter()
            .enumerate()
            .find_map(|(i, l)| Elision::parse_marker(l).map(|n| (i, n)))
    {
        quote.remove(at);
        item.elided = Some(Elision { at, count });
    }
    let removed_file = item
        .scope
        .path()
        .and_then(|p| std::path::Path::new(p).extension())
        .is_some_and(|e| e.eq_ignore_ascii_case("diff") || e.eq_ignore_ascii_case("patch"));
    let removed = lang == REMOVED_LANG
        && !removed_file
        && !quote.is_empty()
        && quote.iter().all(|l| l.starts_with('-'));
    if let (true, Scope::Lines { removed: r, .. }) = (removed, &mut item.scope) {
        *r = true;
        item.quote = quote.iter().map(|l| l[1..].to_owned()).collect();
    } else {
        if item.replacement.is_none() {
            item.lang = lang;
        }
        item.quote = quote;
    }
}

/// `  ```lang` → (ticks, lang).
fn fence_open(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("  ")?;
    let ticks = rest.len() - rest.trim_start_matches('`').len();
    (ticks >= 3).then(|| (&rest[..ticks], rest[ticks..].trim()))
}

#[cfg(test)]
mod tests {
    use super::super::write;
    use super::*;

    fn sample() -> Review {
        let mut review = Review::new("2026-10-14-00", "HEAD");
        review.files.insert("src/a.rs".into(), "3f2a".into());
        review.items.push(Item::new(
            Kind::Fix,
            Scope::General,
            "split the test".into(),
        ));
        let mut lines = Item::new(
            Kind::Question,
            Scope::Lines {
                path: "src/a.rs".into(),
                start: 42,
                end: 48,
                removed: false,
            },
            "why\n\nsecond paragraph".into(),
        );
        lines.quote = vec![
            "fn a() {".into(),
            String::new(),
            "    /// ```".into(),
            "}".into(),
        ];
        lines.done = true;
        review.items.push(lines);
        let mut removed = Item::new(
            Kind::Fix,
            Scope::Lines {
                path: "src/a.rs".into(),
                start: 50,
                end: 50,
                removed: true,
            },
            "keep this".into(),
        );
        removed.quote = vec!["gone();".into()];
        review.items.push(removed);
        let mut suggestion = Item::new(
            Kind::Suggestion,
            Scope::Lines {
                path: "src/a.rs".into(),
                start: 61,
                end: 61,
                removed: false,
            },
            String::new(),
        );
        suggestion.quote = vec!["let x = 1;".into()];
        suggestion.replacement = Some(vec!["let x = 2;".into()]);
        review.items.push(suggestion);
        let mut file = Item::new(Kind::Fix, Scope::File("src/a.rs".into()), "docs".into());
        file.extra = vec!["Agent: done in 3f2b".into()];
        review.items.push(file);
        review.items.push(Item::new(
            Kind::Fix,
            Scope::File("README.md".into()),
            "typo".into(),
        ));
        review.normalize();
        review
    }

    #[test]
    fn parsing_written_review_gives_it_back() {
        let review = sample();
        let text = write(&review);
        assert_eq!(parse(&text).unwrap(), review, "{text}");
        assert_eq!(write(&parse(&text).unwrap()), text);
    }

    #[test]
    fn single_line_anchor_is_written_without_a_range() {
        let text = write(&sample());
        assert!(
            text.contains("- [ ] suggestion `:61` — replace with"),
            "{text}"
        );
        assert!(text.contains("- [x] question `:42-48` — why"), "{text}");
        assert!(text.contains("  ```diff\n  -gone();\n  ```"), "{text}");
    }

    #[test]
    fn agent_edits_are_understood_and_unknown_lines_kept() {
        let mut text = write(&sample());
        text = text.replace("- [ ] fix: split", "* [X] fix: split");
        text = text.replace("status: open", "status: partial   # agent");
        text.push_str("\nNotes from the agent.\n");
        let review = parse(&text).unwrap();
        assert!(review.items[0].done);
        assert_eq!(review.status, Status::Partial);
        let rewritten = write(&review);
        assert!(rewritten.contains("Notes from the agent."));
        assert_eq!(parse(&rewritten).unwrap(), review);
    }

    #[test]
    fn items_an_agent_is_working_on_and_the_in_progress_status_round_trip() {
        let mut text = write(&sample());
        text = text.replace("- [ ] fix: split", "- [~] fix: split");
        text = text.replace("status: open", "status: in-progress");
        let review = parse(&text).unwrap();
        assert!(review.items[0].working && !review.items[0].done);
        assert_eq!(review.working_items(), 1);
        assert_eq!(review.status, Status::InProgress);
        let rewritten = write(&review);
        assert!(rewritten.contains("- [~] fix: split"), "{rewritten}");
        assert!(rewritten.contains("status: in-progress"), "{rewritten}");
        assert_eq!(parse(&rewritten).unwrap(), review);
    }

    #[test]
    fn capped_quotes_round_trip_and_suggestions_are_never_read_as_capped() {
        let lines: Vec<String> = (1..=20).map(|n| format!("line {n}")).collect();
        let mut review = sample();
        let mut long = Item::new(
            Kind::Fix,
            Scope::Lines {
                path: "src/b.rs".into(),
                start: 1,
                end: 20,
                removed: false,
            },
            "split this".into(),
        );
        long.set_quote(lines.clone(), 12);
        let mut gone = Item::new(
            Kind::Fix,
            Scope::Lines {
                path: "src/b.rs".into(),
                start: 30,
                end: 30,
                removed: true,
            },
            "why removed".into(),
        );
        gone.set_quote(lines.clone(), 12);
        let mut suggestion = Item::new(
            Kind::Suggestion,
            Scope::Lines {
                path: "src/b.rs".into(),
                start: 40,
                end: 40,
                removed: false,
            },
            String::new(),
        );
        suggestion.quote = vec!["⋯ 3 more lines ⋯".into()];
        suggestion.replacement = Some(vec!["x".into()]);
        review.items.extend([long, gone, suggestion]);
        review.normalize();
        let text = write(&review);
        assert!(
            text.contains("  line 3\n  ⋯ 14 more lines ⋯\n  line 18\n"),
            "{text}"
        );
        assert!(
            text.contains("  -line 3\n  ⋯ 14 more lines ⋯\n  -line 18\n"),
            "{text}"
        );
        assert_eq!(parse(&text).unwrap(), review);
    }

    #[test]
    fn custom_instructions_round_trip_with_blank_lines_and_quotes() {
        let mut review = sample();
        review.instructions = "Run `pnpm test` first.\n\n- one\n  - nested\n> quoted".into();
        let text = write(&review);
        assert!(text.contains("> **Agent instructions**\n>\n> Run `pnpm test` first.\n>\n> - one\n>   - nested\n> > quoted\n"), "{text}");
        assert_eq!(parse(&text).unwrap(), review);
    }

    #[test]
    fn reordered_sections_parse_to_the_same_items() {
        let review = sample();
        let text = write(&review);
        let (head, general) = text.split_once("\n## General\n").unwrap();
        let (general, files) = general.split_once("\n## ").unwrap();
        let moved = format!("{head}\n## {files}\n## General\n{general}");
        assert_eq!(parse(&moved).unwrap().items, review.items);
    }
}
