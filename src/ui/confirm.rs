//! Yes/no questions, in a popup centred over the diff pane.

use super::{POPUP_PADDING, popup_block, prompt};
use crate::{app::AppState, diff::wrap_ranges, theme::Colors};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

/// `pane` is the whole screen above the status line: questions are about the
/// app, not about one pane.
pub fn render(state: &AppState, frame: &mut Frame, pane: Rect, colors: &Colors) {
    let Some(confirm) = &state.confirm else {
        return;
    };
    let width = usize::from(
        pane.width
            .saturating_sub(4)
            .clamp(1, prompt::MAX_WIDTH)
            .saturating_sub(2 + 2 * POPUP_PADDING),
    )
    .max(1);
    let lines: Vec<Line> = wrap_ranges(&confirm.question, width)
        .into_iter()
        .map(|r| Line::styled(confirm.question[r].to_owned(), Style::new().fg(colors.text)))
        .collect();
    let count = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    let area = prompt::place(pane, None, count);
    let block = popup_block(colors, colors.warning)
        .title(Span::styled(" confirm ", Style::new().fg(colors.warning)))
        .title_bottom(Span::styled(
            " y yes · any other key no ",
            Style::new().fg(colors.muted),
        ));
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}
