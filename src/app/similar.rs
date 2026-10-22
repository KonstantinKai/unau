//! Finding quoted lines again after they were edited, not just moved: the
//! stretch near a change that reads most like the quote.

use super::rereview::Region;
use crate::review::{Item, Scope};

/// Least likeness, from 0 to 1, for edited lines to count as the quoted
/// ones; below it they are taken for other code.
const MIN_LIKENESS: f64 = 0.5;

/// The window of `new_text` the length of the quote that overlaps a changed
/// line (`changed`, new-side numbers) and reads most like the quote, when
/// it is alike enough. Ties go to the window nearest the anchor.
pub fn closest(item: &Item, new_text: &[String], changed: &[usize]) -> Option<Region> {
    let Scope::Lines { start, .. } = &item.scope else {
        return None;
    };
    let q = item.quote_len();
    if q == 0 || q > new_text.len() {
        return None;
    }
    // A capped quote is its head at the window's start and its tail at its end.
    let (head, tail) = item.elided.map_or((item.quote.as_slice(), &[][..]), |e| {
        item.quote.split_at(e.at.min(item.quote.len()))
    });
    let pairs: Vec<(&String, usize)> = head
        .iter()
        .enumerate()
        .map(|(i, line)| (line, i))
        .chain(
            tail.iter()
                .enumerate()
                .map(|(i, line)| (line, q - tail.len() + i)),
        )
        .collect();
    let mut windows: Vec<usize> = changed
        .iter()
        .filter_map(|c| c.checked_sub(1))
        .flat_map(|c| c.saturating_sub(q - 1)..=c)
        .filter(|p| p + q <= new_text.len())
        .collect();
    // Nearest the anchor first, so of equally alike windows the nearest wins;
    // the anchor itself wins whenever it is alike enough, since lines edited
    // in place are the common case.
    windows.sort_unstable_by_key(|p| (p.abs_diff(start - 1), *p));
    windows.dedup();
    let mut best: Option<(f64, usize)> = None;
    for p in windows {
        let Some(score) = likeness(pairs.iter().map(|(line, at)| (*line, &new_text[p + at])))
        else {
            continue;
        };
        if p + 1 == *start && score >= MIN_LIKENESS {
            best = Some((score, p));
            break;
        }
        if best.is_none_or(|(top, _)| score > top) {
            best = Some((score, p));
        }
    }
    best.filter(|(score, _)| *score >= MIN_LIKENESS)
        .map(|(_, p)| Region {
            start: p + 1,
            end: p + q,
        })
}

/// How alike line pairs read, from 0 to 1: shared character pairs over all
/// of them, so long lines weigh more and blank lines or lone braces nothing.
/// `None` when no line has two characters to compare.
fn likeness<'a>(pairs: impl Iterator<Item = (&'a String, &'a String)>) -> Option<f64> {
    let (mut shared, mut total) = (0usize, 0usize);
    for (then, now) in pairs {
        let (a, b) = (bigrams(then), bigrams(now));
        shared += common(&a, &b);
        total += a.len() + b.len();
    }
    #[allow(clippy::cast_precision_loss)] // line counts never reach 2^52
    (total > 0).then(|| 2.0 * shared as f64 / total as f64)
}

/// Neighbouring character pairs of `line`, indentation left out, sorted.
fn bigrams(line: &str) -> Vec<(char, char)> {
    let chars: Vec<char> = line.trim().chars().collect();
    let mut pairs: Vec<(char, char)> = chars.windows(2).map(|w| (w[0], w[1])).collect();
    pairs.sort_unstable();
    pairs
}

/// Pairs two sorted lists have in common, repeats counted.
fn common(left: &[(char, char)], right: &[(char, char)]) -> usize {
    let (mut l, mut r, mut shared) = (0, 0, 0);
    while l < left.len() && r < right.len() {
        match left[l].cmp(&right[r]) {
            std::cmp::Ordering::Less => l += 1,
            std::cmp::Ordering::Greater => r += 1,
            std::cmp::Ordering::Equal => {
                shared += 1;
                l += 1;
                r += 1;
            }
        }
    }
    shared
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::Kind;

    fn texts(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|s| (*s).to_owned()).collect()
    }

    fn item(start: usize, end: usize, quote: &[&str]) -> Item {
        let mut item = Item::new(
            Kind::Fix,
            Scope::Lines {
                path: "a.txt".into(),
                start,
                end,
                removed: false,
            },
            "x".into(),
        );
        item.quote = texts(quote);
        item
    }

    #[test]
    fn an_edited_line_that_also_moved_is_found_where_it_went() {
        // Line 2 was quoted; a line was added above it and its typo fixed.
        let now = texts(&["use a;", "fn main() {", "    let the_count = 1;", "}"]);
        let quoted = item(2, 2, &["    let teh_count = 1;"]);
        assert_eq!(
            closest(&quoted, &now, &[1, 3]),
            Some(Region { start: 3, end: 3 })
        );
    }

    #[test]
    fn unrelated_changed_lines_are_not_taken_for_the_quote() {
        let now = texts(&["completely different", "nothing alike here"]);
        assert_eq!(
            closest(&item(1, 1, &["let total = price * count;"]), &now, &[1, 2]),
            None
        );
    }

    #[test]
    fn of_two_alike_stretches_the_one_nearer_the_anchor_wins() {
        let now = texts(&["let x = 2;", "other", "other", "let x = 2;"]);
        assert_eq!(
            closest(&item(4, 4, &["let x = 1;"]), &now, &[1, 4]),
            Some(Region { start: 4, end: 4 })
        );
    }
}
