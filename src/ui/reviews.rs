//! The review picker: every review in the review folder, newest first, in a
//! popup over the whole screen.

use super::{Scroll, popup_block, prompt, scroll};
use crate::{
    app::{AppState, ReviewEntry},
    review::Status,
    theme::Colors,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

/// Widest the popup gets, in columns.
const MAX_WIDTH: u16 = 110;
/// Share of the screen's height the popup takes at most, in percent.
const HEIGHT_PERCENT: u16 = 60;

pub fn render(state: &AppState, frame: &mut Frame, main: Rect, colors: &Colors) {
    let Some(picker) = &state.reviews else {
        return;
    };
    let shown = picker.shown();
    let height = (main.height * HEIGHT_PERCENT / 100)
        .max(8)
        .min(u16::try_from(shown.len().max(1) + 2).unwrap_or(u16::MAX))
        .min(main.height);
    let width = main.width.saturating_sub(4).min(MAX_WIDTH);
    let area = Rect {
        x: main.x + (main.width - width) / 2,
        y: prompt::lifted_top(main, height),
        width,
        height,
    };
    let visible = usize::from(height.saturating_sub(2));
    let top = picker.cursor.saturating_sub(visible.saturating_sub(1));
    let text_width = usize::from(width.saturating_sub(4));
    let open = state.review_path();
    let lines: Vec<Line> = if shown.is_empty() {
        vec![Line::styled(
            "  no review matches",
            Style::new().fg(colors.muted),
        )]
    } else {
        shown
            .iter()
            .enumerate()
            .map(|(row, index)| {
                let entry = &picker.reviews[*index];
                let line = row_line(entry, open == Some(&entry.path), text_width, colors);
                if row == picker.cursor {
                    line.style(Style::new().bg(colors.cursor_bg))
                } else {
                    line
                }
            })
            .collect()
    };
    let title = if picker.typing || !picker.filter.is_empty() {
        format!(
            " reviews · /{}{} ",
            picker.filter,
            if picker.typing { "▏" } else { "" }
        )
    } else {
        " reviews ".to_owned()
    };
    let hint = if picker.typing {
        " Enter keep filter · Esc clear "
    } else {
        " Enter open · / filter · Esc close "
    };
    let marks = Scroll {
        total: shown.len(),
        top,
        shown: visible,
    };
    let block = scroll::with_hint(
        state,
        popup_block(colors, colors.border_active)
            .title(Span::styled(title, Style::new().fg(colors.info)))
            .title_bottom(Span::styled(hint, Style::new().fg(colors.muted))),
        marks,
        colors,
    );
    let lines: Vec<Line> = lines.into_iter().skip(top).take(visible).collect();
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
    // The right padding column holds the bar, as in the other popups.
    let inside = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    scroll::scrollbar(state, frame, inside, marks, true, colors);
}

/// `✓ 2026-11-26-01  done         80ae391..d6e00f3  0 open · 1 done  first comment…`
fn row_line(entry: &ReviewEntry, open: bool, width: usize, colors: &Colors) -> Line<'static> {
    let (status, color) = match entry.status {
        Status::Open => ("open", colors.text),
        Status::InProgress => ("in progress", colors.info),
        Status::Partial => ("partial", colors.warning),
        Status::Done => ("done", colors.done),
    };
    let what = if entry.target == "HEAD" {
        "working tree".to_owned()
    } else {
        entry.target.clone()
    };
    let accepted = match entry.accepted {
        0 => String::new(),
        n => format!(" · {n} accepted"),
    };
    let counts = format!("{} open · {} done{accepted}", entry.open, entry.done);
    let fixed = format!("{}  {:<12} {:<17} {counts}  ", entry.name, status, what);
    let room = width.saturating_sub(2 + fixed.chars().count());
    let first: String = if entry.first.chars().count() > room {
        let cut: String = entry.first.chars().take(room.saturating_sub(1)).collect();
        format!("{cut}…")
    } else {
        entry.first.clone()
    };
    Line::from(vec![
        Span::styled(if open { "✓ " } else { "  " }, Style::new().fg(colors.done)),
        Span::styled(format!("{}  ", entry.name), Style::new().fg(colors.hunk)),
        Span::styled(format!("{status:<12} "), Style::new().fg(color)),
        Span::styled(format!("{what:<17} "), Style::new().fg(colors.muted)),
        Span::styled(format!("{counts}  "), Style::new().fg(colors.muted)),
        Span::styled(first, Style::new().fg(colors.text)),
    ])
}
