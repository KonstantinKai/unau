//! The commit picker: the working tree, then the history newest first, in a
//! popup over the whole screen.

use super::{Scroll, popup_block, prompt, scroll};
use crate::{
    app::{AppState, CommitPicker},
    git::Target,
    theme::Colors,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

/// Widest the popup gets, in columns.
const MAX_WIDTH: u16 = 100;
/// Share of the screen's height the popup takes at most, in percent.
const HEIGHT_PERCENT: u16 = 60;

pub fn render(state: &AppState, frame: &mut Frame, main: Rect, colors: &Colors) {
    let Some(picker) = &state.commits else {
        return;
    };
    let shown = picker.shown();
    let rows = 1 + shown.len();
    let height = (main.height * HEIGHT_PERCENT / 100)
        .max(8)
        .min(u16::try_from(rows + 2).unwrap_or(u16::MAX))
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
    let lines = list_lines(
        state,
        picker,
        &shown,
        usize::from(width.saturating_sub(4)),
        colors,
    );
    let title = if picker.typing || !picker.filter.is_empty() {
        format!(
            " review · /{}{} ",
            picker.filter,
            if picker.typing { "▏" } else { "" }
        )
    } else {
        " review ".to_owned()
    };
    let hint = if picker.typing {
        " Enter keep filter · Esc clear "
    } else {
        " Enter open · / filter · Esc close "
    };
    let marks = Scroll {
        total: rows,
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
    // The right padding column holds the bar, as in the keys popup.
    let inside = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    scroll::scrollbar(state, frame, inside, marks, true, colors);
}

/// One line per row: the working tree, then each commit as id, subject and
/// age, `✓` on what is shown now and the cursor's row lit.
fn list_lines(
    state: &AppState,
    picker: &CommitPicker,
    shown: &[usize],
    text_width: usize,
    colors: &Colors,
) -> Vec<Line<'static>> {
    let current = match &state.target {
        Target::WorkTree => None,
        Target::Range { to, .. } => Some(to.as_str()),
    };
    let row_style = |row: usize| {
        if row == picker.cursor {
            Style::new().bg(colors.cursor_bg)
        } else {
            Style::new()
        }
    };
    let mark = |on: bool| Span::styled(if on { "✓ " } else { "  " }, Style::new().fg(colors.done));
    let mut lines = vec![
        Line::from(vec![
            mark(current.is_none()),
            Span::styled(
                "working tree",
                Style::new().fg(colors.text).add_modifier(Modifier::BOLD),
            ),
        ])
        .style(row_style(0)),
    ];
    for (n, index) in shown.iter().enumerate() {
        let commit = &picker.commits[*index];
        let when = format!("  {}", commit.when);
        let room = text_width.saturating_sub(2 + commit.short.len() + 2 + when.chars().count());
        let subject: String = if commit.subject.chars().count() > room {
            let cut: String = commit
                .subject
                .chars()
                .take(room.saturating_sub(1))
                .collect();
            format!("{cut}…")
        } else {
            format!("{:<room$}", commit.subject)
        };
        lines.push(
            Line::from(vec![
                mark(current == Some(commit.id.as_str())),
                Span::styled(format!("{}  ", commit.short), Style::new().fg(colors.hunk)),
                Span::styled(subject, Style::new().fg(colors.text)),
                Span::styled(when, Style::new().fg(colors.muted)),
            ])
            .style(row_style(n + 1)),
        );
    }
    lines
}
