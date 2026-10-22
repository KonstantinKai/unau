//! The go-to-line popup: a small box centred over the whole screen, with the
//! diff still readable behind it.

use super::{popup_block, prompt};
use crate::{app::AppState, theme::Colors};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

/// Wide enough for the hint under the number.
const WIDTH: u16 = 28;

pub fn render(state: &AppState, frame: &mut Frame, pane: Rect, colors: &Colors) {
    let Some(text) = &state.goto else {
        return;
    };
    let width = WIDTH.min(pane.width);
    let height = 3.min(pane.height);
    let area = Rect {
        x: pane.x + (pane.width - width) / 2,
        y: prompt::lifted_top(pane, height),
        width,
        height,
    };
    let block = popup_block(colors, colors.border_active)
        .title(Span::styled(" go to line ", Style::new().fg(colors.info)))
        .title_bottom(Span::styled(
            " Enter go · Esc cancel ",
            Style::new().fg(colors.muted),
        ));
    let line = Line::from(vec![
        Span::styled(
            text.clone(),
            Style::new().fg(colors.text).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" ", Style::new().bg(colors.text)),
    ]);
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(line).block(block), area);
}
