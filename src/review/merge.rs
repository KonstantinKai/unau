//! Three-way merge between what the tool last wrote, what it holds now, and
//! what is on disk, so an agent editing the file while the tool runs never
//! loses its ticks.

use super::{Item, Review, Status};

/// What makes two items the same item across edits. Ticks, agent prose and
/// quoted lines are not part of it, so the agent may change those freely.
fn identity(item: &Item) -> (&super::Scope, super::Kind, &str, Option<&Vec<String>>) {
    (
        &item.scope,
        item.kind,
        item.text.as_str(),
        item.replacement.as_ref(),
    )
}

fn position(items: &[Item], item: &Item) -> Option<usize> {
    items.iter().position(|i| identity(i) == identity(item))
}

/// `base` is the review as last written, `ours` the tool's current review and
/// `theirs` the file on disk now. The agent's ticks, status and prose win;
/// the tool's additions, deletions and edits since `base` apply on top.
pub fn merge(base: &Review, ours: &Review, theirs: &Review) -> Review {
    let mut merged = theirs.clone();
    for item in &base.items {
        if position(&ours.items, item).is_none()
            && let Some(at) = position(&merged.items, item)
        {
            merged.items.remove(at);
        }
    }
    for item in &ours.items {
        if position(&base.items, item).is_none() && position(&merged.items, item).is_none() {
            merged.items.push(item.clone());
        }
    }
    for (path, sha) in &ours.files {
        merged
            .files
            .entry(path.clone())
            .or_insert_with(|| sha.clone());
    }
    if ours.status != base.status && theirs.status == base.status {
        merged.status = ours.status;
    }
    if merged.status == Status::Done && merged.open_items() > 0 {
        merged.status = Status::Partial;
    }
    merged.normalize();
    merged
}

#[cfg(test)]
mod tests {
    use super::super::{Kind, Scope};
    use super::*;

    fn item(text: &str) -> Item {
        Item::new(Kind::Fix, Scope::General, text.into())
    }

    fn review(items: &[Item]) -> Review {
        let mut r = Review::new("2026-10-14-00", "HEAD");
        r.items = items.to_vec();
        r
    }

    #[test]
    fn agent_ticks_survive_a_tool_write() {
        let base = review(&[item("a"), item("b")]);
        let mut theirs = base.clone();
        theirs.items[0].done = true;
        theirs.status = Status::Partial;
        let mut ours = base.clone();
        ours.items.push(item("c"));
        let merged = merge(&base, &ours, &theirs);
        let texts: Vec<_> = merged
            .items
            .iter()
            .map(|i| (i.text.as_str(), i.done))
            .collect();
        assert_eq!(texts, [("a", true), ("b", false), ("c", false)]);
        assert_eq!(merged.status, Status::Partial);
    }

    #[test]
    fn tool_deletions_apply_on_top_of_agent_edits() {
        let base = review(&[item("a"), item("b")]);
        let mut theirs = base.clone();
        theirs.items[1].done = true;
        let ours = review(&[item("b")]);
        let merged = merge(&base, &ours, &theirs);
        assert_eq!(merged.items.len(), 1);
        assert!(merged.items[0].done);
    }

    #[test]
    fn tool_reopening_sets_status_back_to_open() {
        let mut base = review(&[item("a")]);
        base.status = Status::Partial;
        let mut ours = base.clone();
        ours.status = Status::Open;
        let merged = merge(&base, &ours, &base);
        assert_eq!(merged.status, Status::Open);
    }
}
