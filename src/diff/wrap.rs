//! Soft wrapping by terminal cell width.

use std::ops::Range;
use unicode_width::UnicodeWidthChar;

/// Tabs render as this many spaces.
pub const TAB_WIDTH: usize = 4;

fn char_width(c: char) -> usize {
    if c == '\t' {
        TAB_WIDTH
    } else {
        c.width().unwrap_or(0)
    }
}

pub fn display_width(text: &str) -> usize {
    text.chars().map(char_width).sum()
}

/// Byte ranges of `text` that each fit in `width` cells. Always at least one.
pub fn wrap_ranges(text: &str, width: usize) -> Vec<Range<usize>> {
    let width = width.max(1);
    let mut parts = Vec::new();
    let (mut start, mut used) = (0, 0);
    for (i, c) in text.char_indices() {
        let w = char_width(c);
        if used + w > width && i > start {
            parts.push(start..i);
            start = i;
            used = 0;
        }
        used += w;
    }
    parts.push(start..text.len());
    parts
}

/// Byte ranges of `text` broken between words: the first fits in `first`
/// cells, the rest in `rest`. A word wider than the width is cut; the spaces
/// at a break belong to neither side. Always at least one range.
pub fn wrap_words(text: &str, first: usize, rest: usize) -> Vec<Range<usize>> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut width = first.max(1);
    loop {
        let (mut used, mut end, mut space) = (0, text.len(), None);
        for (i, c) in text[start..].char_indices() {
            let i = start + i;
            let w = char_width(c);
            if used + w > width {
                end = i;
                break;
            }
            if c == ' ' {
                space = Some(i);
            }
            used += w;
        }
        if end == text.len() {
            parts.push(start..end);
            return parts;
        }
        let cut = if text[end..].starts_with(' ') {
            end
        } else if let Some(space) = space.filter(|s| *s > start) {
            space
        } else if end > start {
            end
        } else {
            start + text[start..].chars().next().map_or(1, char::len_utf8)
        };
        parts.push(start..text[..cut].trim_end_matches(' ').len().max(start));
        start = cut + text[cut..].len() - text[cut..].trim_start_matches(' ').len();
        if start >= text.len() {
            return parts;
        }
        width = rest.max(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_wrapping_breaks_at_spaces_and_cuts_only_long_words() {
        let text = "one two three";
        let parts: Vec<&str> = wrap_words(text, 8, 8)
            .into_iter()
            .map(|r| &text[r])
            .collect();
        assert_eq!(parts, ["one two", "three"]);
        let text = "abcdefghij x";
        let parts: Vec<&str> = wrap_words(text, 4, 4)
            .into_iter()
            .map(|r| &text[r])
            .collect();
        assert_eq!(parts, ["abcd", "efgh", "ij x"]);
        assert_eq!(wrap_words("", 4, 4).first(), Some(&(0..0)));
    }

    #[test]
    fn wraps_at_cell_width_and_counts_wide_characters() {
        assert_eq!(wrap_ranges("abcdef", 4), [0..4, 4..6]);
        assert_eq!(wrap_ranges("", 4).first(), Some(&(0..0)));
        assert_eq!(display_width("日本"), 4);
        assert_eq!(wrap_ranges("日本語", 4).len(), 2);
    }
}
