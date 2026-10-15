//! The comment popup: a bordered box that floats next to the lines being
//! commented on, so they stay readable while typing.

use super::border;
use crate::{
    app::{AppState, Pane, PromptPurpose},
    diff::{display_width, wrap_ranges},
    review::{Kind, Scope},
    theme::Colors,
};

fn kind_color(kind: Kind, colors: &Colors) -> ratatui::style::Color {
    match kind {
        Kind::Fix => colors.fix,
        Kind::Question => colors.question,
        Kind::Suggestion => colors.suggestion,
    }
}
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

/// Most text lines the popup shows before it scrolls.
pub const MAX_TEXT_LINES: u16 = 10;
/// Widest the popup gets, in columns.
pub const MAX_WIDTH: u16 = 80;

/// How far a centred popup sits above the middle, in percent of the area it
/// is centred in: the eye rests above the middle of a screen, not on it.
pub const POPUP_LIFT_PERCENT: u16 = 20;

/// Top row of a popup `height` rows tall, centred in `area` and then lifted
/// by `POPUP_LIFT_PERCENT`, never above `area`'s top.
pub fn lifted_top(area: Rect, height: u16) -> u16 {
    let centred = area.y + area.height.saturating_sub(height) / 2;
    centred
        .saturating_sub(area.height * POPUP_LIFT_PERCENT / 100)
        .max(area.y)
}

/// Where the popup goes inside `pane`. `anchor` is the on-screen rows
/// `top..bottom` of the lines being commented on; the popup sits below them
/// when it fits, above them otherwise, and centred when there is no anchor.
pub fn place(pane: Rect, anchor: Option<(u16, u16)>, text_lines: u16) -> Rect {
    let width = pane.width.saturating_sub(4).clamp(1, MAX_WIDTH);
    let height = (text_lines.clamp(1, MAX_TEXT_LINES) + 2).min(pane.height);
    let x = pane.x + (pane.width - width) / 2;
    let y = match anchor {
        Some((top, bottom)) => {
            if bottom + height <= pane.bottom() {
                bottom
            } else if top >= pane.y + height {
                top - height
            } else {
                pane.bottom() - height
            }
        }
        None => lifted_top(pane, height),
    };
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Byte ranges of `text` as wrapped lines, splitting at newlines first.
fn wrapped(text: &str, width: usize) -> Vec<(usize, usize)> {
    let mut lines = Vec::new();
    let mut offset = 0;
    for part in text.split('\n') {
        for range in wrap_ranges(part, width) {
            lines.push((offset + range.start, offset + range.end));
        }
        offset += part.len() + 1;
    }
    lines
}

/// The wrapped line and column the cursor (a character index) falls on.
fn cursor_at(text: &str, lines: &[(usize, usize)], cursor: usize) -> (usize, usize) {
    let byte = text
        .char_indices()
        .nth(cursor)
        .map_or(text.len(), |(b, _)| b);
    let mut line = 0;
    for (i, (start, end)) in lines.iter().enumerate() {
        if byte >= *start && byte <= *end {
            line = i;
            // A position at the very end of a full line belongs to the next one.
            if byte == *end && lines.get(i + 1).is_some_and(|(s, _)| *s == byte) {
                line = i + 1;
            }
            break;
        }
    }
    let start = lines.get(line).map_or(0, |(s, _)| *s);
    (line, display_width(&text[start..byte]))
}

/// On-screen rows of the lines the prompt is about, if they are visible.
fn anchor(state: &AppState, pane: Rect) -> Option<(u16, u16)> {
    let anchored = match state.prompt.as_ref().map(|p| &p.purpose) {
        Some(PromptPurpose::New {
            scope: Scope::Lines { .. },
            ..
        }) => true,
        Some(PromptPurpose::Edit(_) | PromptPurpose::Reopen(_)) => state.focus == Pane::Diff,
        _ => false,
    };
    let file = state.file.as_ref().filter(|_| anchored)?;
    let rows = file.selected_rows();
    let top = file.view.starts.get(rows.start)?.checked_sub(file.scroll);
    let bottom = file.view.starts.get(rows.end)?.saturating_sub(file.scroll);
    let inner_top = pane.y + 1;
    let clamp = |v: usize| inner_top + u16::try_from(v).unwrap_or(u16::MAX).min(pane.height - 2);
    Some((clamp(top.unwrap_or(0)), clamp(bottom)))
}

/// Draws the comment popup. `pane` is the diff pane, where a popup about
/// lines sits next to them; `app` is the whole screen above the status line,
/// where a popup about no lines is centred.
pub fn render(state: &AppState, frame: &mut Frame, pane: Rect, app: Rect, colors: &Colors) {
    let Some(prompt) = &state.prompt else {
        return;
    };
    let anchor = anchor(state, pane);
    let within = if anchor.is_some() { pane } else { app };
    let text_width = usize::from(
        within
            .width
            .saturating_sub(4)
            .clamp(1, MAX_WIDTH)
            .saturating_sub(2),
    )
    .max(1);
    let lines = wrapped(&prompt.text, text_width);
    let count = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    let area = place(within, anchor, count);
    let visible = usize::from(area.height.saturating_sub(2)).max(1);
    let (cursor_line, cursor_col) = cursor_at(&prompt.text, &lines, prompt.cursor);
    let first = (cursor_line + 1).saturating_sub(visible);
    let body: Vec<Line> = lines
        .iter()
        .skip(first)
        .take(visible)
        .map(|(s, e)| Line::styled(prompt.text[*s..*e].to_owned(), Style::new().fg(colors.text)))
        .collect();
    let block = Block::new()
        .borders(Borders::ALL)
        .border_style(border(true, colors))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                prompt.kind.name(),
                Style::new()
                    .fg(kind_color(prompt.kind, colors))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" · {} · Tab fix/question ", prompt.label),
                Style::new().fg(colors.border_active),
            ),
        ]))
        .title_bottom(Span::styled(
            " Enter save · Alt-Enter newline · Esc cancel · C-e editor ",
            Style::new().fg(colors.muted),
        ))
        .style(Style::new().bg(colors.note_bg));
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(body).block(block), area);
    let x = area.x + 1 + u16::try_from(cursor_col).unwrap_or(u16::MAX);
    let y = area.y + 1 + u16::try_from(cursor_line - first).unwrap_or(0);
    frame.set_cursor_position((x.min(area.right().saturating_sub(2)), y));
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANE: Rect = Rect {
        x: 30,
        y: 0,
        width: 90,
        height: 40,
    };

    #[test]
    fn popup_sits_under_the_commented_lines_when_it_fits() {
        let area = place(PANE, Some((10, 13)), 1);
        assert_eq!((area.y, area.height), (13, 3));
        assert_eq!((area.x, area.width), (35, MAX_WIDTH));
    }

    #[test]
    fn popup_goes_above_when_there_is_no_room_below() {
        let area = place(PANE, Some((30, 38)), 4);
        assert_eq!(area.y + area.height, 30);
    }

    #[test]
    fn popup_never_covers_lines_it_can_avoid_and_never_leaves_the_pane() {
        for top in 1..39 {
            for size in 1..8 {
                let bottom = (top + size).min(39);
                let area = place(PANE, Some((top, bottom)), 3);
                assert!(area.bottom() <= PANE.bottom());
                let below = bottom + area.height <= PANE.bottom();
                let above = top >= PANE.y + area.height;
                if below || above {
                    assert!(
                        area.bottom() <= top || area.y >= bottom,
                        "covers {top}..{bottom}: {area:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn popup_without_an_anchor_sits_above_centre_and_height_is_capped() {
        let area = place(PANE, None, 40);
        assert_eq!(area.height, MAX_TEXT_LINES + 2);
        assert_eq!(
            area.y,
            (40 - area.height) / 2 - 40 * POPUP_LIFT_PERCENT / 100
        );
        let small = Rect { height: 6, ..PANE };
        assert_eq!(lifted_top(small, 5), small.y, "never above the pane");
    }

    #[test]
    fn text_wraps_at_newlines_and_width_and_the_cursor_follows() {
        let text = "abcdef\ngh";
        let lines = wrapped(text, 4);
        assert_eq!(lines, [(0, 4), (4, 6), (7, 9)]);
        assert_eq!(cursor_at(text, &lines, 0), (0, 0));
        assert_eq!(cursor_at(text, &lines, 5), (1, 1));
        assert_eq!(cursor_at(text, &lines, 7), (2, 0));
        assert_eq!(cursor_at(text, &lines, 9), (2, 2));
    }
}
