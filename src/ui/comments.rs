//! The comments pane under the files pane.

use super::border;
use crate::{
    app::{AppState, CommentRow, Pane, comment_label},
    config::Action,
    review::Kind,
    theme::Colors,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

/// ` · n working` while an agent has items marked `[~]`, else nothing.
pub fn working(state: &AppState) -> String {
    match state.review.working_items() {
        0 => String::new(),
        n => format!(" · {n} working"),
    }
}

pub fn render(state: &AppState, frame: &mut Frame, area: Rect, colors: &Colors) {
    if !state.comments.visible {
        return;
    }
    let focused = state.focus == Pane::Comments;
    let open = state.review.open_items();
    let done = state.review.items.len() - open;
    let block = Block::new()
        .borders(Borders::ALL)
        .border_style(border(focused, colors))
        .title(super::pane_title(
            state,
            Action::FocusComments,
            &format!("comments · {open} open{} · {done} done", working(state)),
            colors,
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let pane = &state.comments;
    let lines: Vec<Line> = pane
        .rows
        .iter()
        .enumerate()
        .skip(pane.scroll)
        .take(usize::from(inner.height))
        .map(|(row, entry)| match entry {
            CommentRow::Header(name) => Line::styled(name.clone(), Style::new().fg(colors.muted)),
            CommentRow::Item(index) => {
                let item = &state.review.items[*index];
                let kind_color = match item.kind {
                    Kind::Fix => colors.fix,
                    Kind::Question => colors.question,
                    Kind::Suggestion => colors.suggestion,
                };
                let mut spans = vec![Span::raw("  ")];
                if item.done {
                    spans.push(Span::styled("✓ ", Style::new().fg(colors.done)));
                } else if item.working {
                    spans.push(Span::styled("~ ", Style::new().fg(colors.info)));
                }
                spans.push(Span::styled(
                    format!("[{}] ", item.kind.name()),
                    Style::new().fg(kind_color).add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled(
                    comment_label(item),
                    Style::new().fg(colors.text),
                ));
                let line = Line::from(spans);
                if row == pane.cursor {
                    line.style(Style::new().bg(if focused {
                        colors.cursor_bg
                    } else {
                        colors.selection_bg
                    }))
                } else {
                    line
                }
            }
        })
        .collect();
    let body = if lines.is_empty() {
        vec![Line::styled(
            "no comments yet",
            Style::new().fg(colors.muted),
        )]
    } else {
        lines
    };
    frame.render_widget(Paragraph::new(body), inner);
}
