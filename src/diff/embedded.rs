//! Files that hold other languages in blocks: Vue and Svelte components, and
//! HTML with `<script>` and `<style>`. Highlighting parses from part way into
//! a file, so it has to know which block a line is in and which language that
//! block is written in.

use crate::git::Line;
use std::path::Path;

/// Extensions whose files are made of `<template>`, `<script>` and `<style>`
/// blocks.
const BLOCK_FILES: &[&str] = &["vue", "svelte", "astro", "html", "htm"];

/// Where the block holding line `i` starts, and the extension of its language
/// (`None` for the file's own syntax). `None` overall for files that have no
/// blocks.
pub fn block_of(path: &str, lines: &[Line], i: usize) -> Option<(usize, Option<&'static str>)> {
    let extension = Path::new(path).extension()?.to_str()?;
    if !BLOCK_FILES
        .iter()
        .any(|e| e.eq_ignore_ascii_case(extension))
    {
        return None;
    }
    // Astro: a `---` fence pair at the top holds TypeScript.
    let mut floor = 0;
    if extension.eq_ignore_ascii_case("astro")
        && lines.first().is_some_and(|l| l.text.trim() == "---")
    {
        let close = lines
            .iter()
            .skip(1)
            .position(|l| l.text.trim() == "---")
            .map(|p| p + 1);
        match close {
            _ if i == 0 => return Some((0, None)),
            Some(close) if i < close => return Some((1, Some("ts"))),
            Some(close) if i == close => return Some((close, None)),
            Some(close) => floor = close + 1,
            None => return Some((1, Some("ts"))),
        }
    }
    for j in (floor..=i.min(lines.len().saturating_sub(1))).rev() {
        let text = lines[j].text.trim_start();
        let Some(tag) = block_tag(text) else {
            continue;
        };
        // A tag line itself belongs to the file's own syntax.
        if j == i {
            return Some((i, None));
        }
        return Some(match tag {
            Tag::Script => (j + 1, Some(script_language(text))),
            Tag::Style => (j + 1, Some(style_language(text))),
            Tag::Other => (j, None),
        });
    }
    Some((floor, None))
}

enum Tag {
    Script,
    Style,
    /// `<template>` or any closing tag: back to the file's own syntax.
    Other,
}

fn block_tag(text: &str) -> Option<Tag> {
    let opens = |name: &str| {
        text.strip_prefix('<')
            .and_then(|t| t.strip_prefix(name))
            .is_some_and(|rest| rest.starts_with(['>', ' ', '\t']) || rest.is_empty())
    };
    if opens("script") {
        Some(Tag::Script)
    } else if opens("style") {
        Some(Tag::Style)
    } else if opens("template")
        || ["</script", "</style", "</template"]
            .iter()
            .any(|close| text.starts_with(close))
    {
        Some(Tag::Other)
    } else {
        None
    }
}

/// The value of `lang="…"` (or `'…'`) in a tag line.
fn lang_attribute(tag: &str) -> Option<&str> {
    let rest = &tag[tag.find("lang=")? + "lang=".len()..];
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let value = &rest[1..];
    value.find(quote).map(|end| &value[..end])
}

fn script_language(tag: &str) -> &'static str {
    match lang_attribute(tag) {
        Some("ts" | "typescript") => "ts",
        Some("tsx") => "tsx",
        _ => "js",
    }
}

fn style_language(tag: &str) -> &'static str {
    match lang_attribute(tag) {
        Some("scss") => "scss",
        Some("sass") => "sass",
        Some("less") => "less",
        _ => "css",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::LineKind;

    fn lines(text: &str) -> Vec<Line> {
        text.lines()
            .enumerate()
            .map(|(n, t)| Line {
                kind: LineKind::Context,
                old: Some(n + 1),
                new: Some(n + 1),
                text: t.to_owned(),
                no_newline: false,
            })
            .collect()
    }

    #[test]
    fn lines_belong_to_the_block_above_them() {
        let file = lines(
            "<template>\n  <div/>\n</template>\n<script setup lang=\"ts\">\nconst a = 1\n</script>\n<style lang='scss'>\n.a { b: c }\n</style>",
        );
        assert_eq!(block_of("a.vue", &file, 1), Some((0, None)));
        assert_eq!(
            block_of("a.vue", &file, 3),
            Some((3, None)),
            "tag line itself"
        );
        assert_eq!(block_of("a.vue", &file, 4), Some((4, Some("ts"))));
        assert_eq!(block_of("a.vue", &file, 5), Some((5, None)));
        assert_eq!(block_of("a.vue", &file, 7), Some((7, Some("scss"))));
        assert_eq!(block_of("a.rs", &file, 4), None);
    }

    #[test]
    fn astro_frontmatter_is_typescript_and_blocks_follow_it() {
        let file = lines(
            "---\nconst title = 'x'\n---\n<h1>{title}</h1>\n<style>\nh1 { color: red }\n</style>",
        );
        assert_eq!(block_of("a.astro", &file, 0), Some((0, None)));
        assert_eq!(block_of("a.astro", &file, 1), Some((1, Some("ts"))));
        assert_eq!(block_of("a.astro", &file, 2), Some((2, None)));
        assert_eq!(block_of("a.astro", &file, 3), Some((3, None)));
        assert_eq!(block_of("a.astro", &file, 5), Some((5, Some("css"))));
        let no_frontmatter = lines("<h1>hi</h1>\n<script>\nlet a = 1\n</script>");
        assert_eq!(
            block_of("a.astro", &no_frontmatter, 2),
            Some((2, Some("js")))
        );
    }

    #[test]
    fn script_and_style_languages_default_sensibly() {
        assert_eq!(script_language("<script>"), "js");
        assert_eq!(script_language("<script lang=\"tsx\">"), "tsx");
        assert_eq!(style_language("<style scoped>"), "css");
        assert!(block_tag("<scripts>").is_none());
        assert!(block_tag("<script").is_some());
    }
}
