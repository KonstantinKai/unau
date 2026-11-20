//! The file as it was reviewed, kept as a git object when the first comment
//! on it was made, and a line diff from it to the file now, so an item's old
//! lines map to where they are now however the agent moved or rewrote them.

use super::rereview::{Region, quote_at};
use crate::review::{Item, Scope};
use similar::{Algorithm, DiffOp, capture_diff_slices};

#[derive(Clone, Debug)]
pub struct Reviewed {
    text: Vec<String>,
    ops: Vec<DiffOp>,
}

impl Reviewed {
    pub fn new(text: Vec<String>, now: &[String]) -> Self {
        let ops = capture_diff_slices(Algorithm::Myers, &text, now);
        Self { text, ops }
    }

    /// Where `item`'s lines are now: the lines they became, or, when they
    /// were all deleted, the line in their place. `None` when the item was not
    /// written against this copy: its quote is not at its anchor in it.
    pub fn map(&self, item: &Item) -> Option<Region> {
        let Scope::Lines {
            start,
            end,
            removed: false,
            ..
        } = &item.scope
        else {
            return None;
        };
        if item.quote.is_empty() || !quote_at(item, &self.text, start - 1) {
            return None;
        }
        // Zero-based, end exclusive.
        let (from, to) = (start - 1, *end);
        let mut lines: Option<(usize, usize)> = None;
        let mut place = None;
        let mut take = |first: usize, last: usize| {
            lines = Some(lines.map_or((first, last), |(a, b)| (a.min(first), b.max(last))));
        };
        for op in &self.ops {
            let old = op.old_range();
            let new = op.new_range();
            let overlaps = old.start < to && from < old.end;
            match op {
                DiffOp::Equal { .. } if overlaps => {
                    let skip = from.saturating_sub(old.start);
                    let keep = to.min(old.end) - old.start.max(from);
                    take(new.start + skip, new.start + skip + keep);
                }
                DiffOp::Replace { .. } if overlaps => take(new.start, new.end),
                DiffOp::Delete { .. } if overlaps => place = place.or(Some(new.start)),
                // Lines added inside the region belong to it.
                DiffOp::Insert { old_index, .. } if from < *old_index && *old_index < to => {
                    take(new.start, new.end);
                }
                _ => {}
            }
        }
        match (lines, place) {
            (Some((first, last)), _) => Some(Region {
                start: first + 1,
                end: last.max(first + 1),
            }),
            (None, Some(at)) => Some(Region {
                start: at + 1,
                end: at + 1,
            }),
            (None, None) => None,
        }
    }
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
    fn lines_rewritten_past_recognition_and_moved_are_found() {
        let then = texts(&["a", "b", "line 5 bad", "c", "d"]);
        let now = texts(&["a", "new 1", "new 2", "b", "zzz qqq", "c", "d"]);
        let reviewed = Reviewed::new(then, &now);
        assert_eq!(
            reviewed.map(&item(3, 3, &["line 5 bad"])),
            Some(Region { start: 5, end: 5 })
        );
    }

    #[test]
    fn deleted_lines_map_to_the_line_in_their_place_and_a_stale_copy_is_ignored() {
        let then = texts(&["a", "gone", "b"]);
        let now = texts(&["a", "b"]);
        let reviewed = Reviewed::new(then, &now);
        assert_eq!(
            reviewed.map(&item(2, 2, &["gone"])),
            Some(Region { start: 2, end: 2 })
        );
        // Written against another version of the file: not this copy's to map.
        assert_eq!(reviewed.map(&item(2, 2, &["other"])), None);
    }

    #[test]
    fn a_region_that_grew_takes_in_its_added_lines() {
        let then = texts(&["a", "one", "two", "b"]);
        let now = texts(&["a", "one", "extra", "two", "b"]);
        let reviewed = Reviewed::new(then, &now);
        assert_eq!(
            reviewed.map(&item(2, 3, &["one", "two"])),
            Some(Region { start: 2, end: 4 })
        );
    }
}
