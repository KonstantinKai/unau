//! Where a run goes when the agent already runs here: one of those sessions,
//! or a new one. A popup centred over the screen like a question.

use super::{popup_block, prompt};
use crate::{app::AppState, theme::Colors};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};

pub fn render(state: &AppState, frame: &mut Frame, pane: Rect, colors: &Colors) {
    let Some(choice) = &state.agent_choice else {
        return;
    };
    let new_row = format!("+ new session: {}", choice.command);
    let lines: Vec<Line> = choice
        .sessions
        .iter()
        .map(|session| session.label.as_str())
        .chain(choice.asking.then_some(new_row.as_str()))
        .enumerate()
        .map(|(i, label)| {
            let picked = i == choice.cursor;
            let style = if picked {
                Style::new()
                    .fg(colors.text)
                    .bg(colors.cursor_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(colors.text)
            };
            Line::styled(format!("{} {label}", if picked { "›" } else { " " }), style)
        })
        .collect();
    let count = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    let area = prompt::place(pane, None, count);
    let title = if choice.asking {
        " the agent already runs here: send to it? (asked once) "
    } else {
        " send to which session? "
    };
    let block = popup_block(colors, colors.info)
        .title(Span::styled(title, Style::new().fg(colors.info)))
        .title_bottom(Span::styled(
            " j/k move · Enter go · Esc cancel ",
            Style::new().fg(colors.muted),
        ));
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}
