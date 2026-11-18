//! The text of a note under the lines an item is about: the comment, a
//! suggestion's replacement, what the agent wrote back, and for a ticked item
//! what its lines were against what they are now.

use super::{NoteLine, NoteStyle, rereview};
use crate::review::{Item, Kind, Scope};

pub(super) fn note_lines(
    item: &Item,
    region: Option<rereview::Region>,
    new_text: &[String],
) -> Vec<NoteLine> {
    let mut text = item.text.lines();
    let first = match item.kind {
        Kind::Suggestion => "replace with".to_owned(),
        _ => text.next().unwrap_or_default().to_owned(),
    };
    let mut lines = vec![NoteLine {
        text: first,
        style: NoteStyle::Header {
            kind: item.kind,
            done: item.done,
            working: item.working,
        },
    }];
    lines.extend(text.map(|t| NoteLine {
        text: t.to_owned(),
        style: NoteStyle::Text,
    }));
    if let Some(replacement) = &item.replacement {
        lines.extend(replacement.iter().map(|t| NoteLine {
            text: t.clone(),
            style: NoteStyle::Added,
        }));
    }
    lines.extend(reply(item).into_iter().map(|text| NoteLine {
        text,
        style: NoteStyle::Reply,
    }));
    let lines_scope = matches!(item.scope, Scope::Lines { removed: false, .. });
    if lines_scope && region.is_none() {
        lines.push(NoteLine {
            text: "anchor lost: the quoted lines are gone".to_owned(),
            style: NoteStyle::Warning,
        });
    }
    // A ticked item shows what its lines were against what they are now.
    if item.done
        && lines_scope
        && let Some(region) = region
    {
        let now = &new_text[region.start - 1..region.end.min(new_text.len())];
        lines.extend(rereview::quote_diff(item, now));
    }
    lines
}

/// The lines the agent wrote under an item, such as the answer to a
/// question, without the item's indent or blank lines around them; the
/// first is marked as the agent's.
fn reply(item: &Item) -> Vec<String> {
    let lines: Vec<&str> = item
        .extra
        .iter()
        .map(|l| l.strip_prefix("  ").unwrap_or(l).trim_end())
        .collect();
    let Some(first) = lines.iter().position(|l| !l.is_empty()) else {
        return Vec::new();
    };
    let last = lines.iter().rposition(|l| !l.is_empty()).unwrap_or(first);
    lines[first..=last]
        .iter()
        .enumerate()
        .map(|(i, l)| format!("{}{l}", if i == 0 { "↳ " } else { "  " }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::parse;

    #[test]
    fn the_agents_answer_under_an_item_shows_in_its_note() {
        let text = "---\ntool: unau 0.5.1\nstatus: done\nbase: HEAD\n---\n\n# Review x\n\n\
                    ## a.json\n\n\
                    - [x] question `:1-1` — why here?\n  ```json\n  \"nx\": {\n  ```\n  \
                    Answer: nx.json cannot hold\n  one project's target.\n";
        let review = parse(text).unwrap();
        let item = &review.items[0];
        let lines = note_lines(item, None, &[]);
        let replies: Vec<&str> = lines
            .iter()
            .filter(|l| l.style == NoteStyle::Reply)
            .map(|l| l.text.as_str())
            .collect();
        assert_eq!(
            replies,
            ["↳ Answer: nx.json cannot hold", "  one project's target."]
        );
    }
}
