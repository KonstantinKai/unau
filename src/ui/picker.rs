//! The theme picker, docked at the right so the diff it recolours stays in
//! view.

use crate::{app::AppState, theme::Colors};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

const WIDTH: u16 = 40;

pub fn render(state: &AppState, frame: &mut Frame, main: Rect, colors: &Colors) {
    let Some(picker) = &state.theme_picker else {
        return;
    };
    let rows = picker.rows();
    let wanted = u16::try_from(rows.len() + 2).unwrap_or(u16::MAX);
    let height = wanted.min(main.height);
    let width = WIDTH.min(main.width);
    let area = Rect {
        x: main.x + main.width - width,
        y: main.y,
        width,
        height,
    };
    // Keep the cursor's row in view when the list is taller than the popup.
    let inner = usize::from(height.saturating_sub(2));
    let cursor_row = rows
        .iter()
        .position(|(_, entry)| *entry == Some(picker.cursor))
        .unwrap_or(0);
    let top = cursor_row.saturating_sub(inner.saturating_sub(1));
    let lines: Vec<Line> = rows
        .iter()
        .skip(top)
        .take(inner)
        .map(|(title, entry)| match (title, entry) {
            (Some(title), _) => Line::styled(
                format!(" {title}"),
                Style::new().fg(colors.muted).add_modifier(Modifier::BOLD),
            ),
            (None, Some(i)) => {
                let e = &picker.entries[*i];
                let mark = if e.name == picker.before.name {
                    "✓ "
                } else {
                    "  "
                };
                let c = &e.theme.colors;
                let mut line = Line::from(vec![
                    Span::styled(format!("  {mark}"), Style::new().fg(colors.done)),
                    Span::styled(format!("{:<24}", e.name), Style::new().fg(colors.text)),
                    Span::styled("●", Style::new().fg(c.border_active)),
                    Span::styled("●", Style::new().fg(c.added)),
                    Span::styled("●", Style::new().fg(c.removed)),
                    Span::styled("●", Style::new().fg(c.fix)),
                ]);
                if *i == picker.cursor {
                    line = line.style(Style::new().bg(colors.cursor_bg));
                }
                line
            }
            (None, None) => Line::default(),
        })
        .collect();
    let block = super::popup_block(colors, colors.border_active)
        .title(" themes ")
        .title_bottom(Span::styled(
            " Enter save · Esc cancel ",
            Style::new().fg(colors.muted),
        ));
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}
