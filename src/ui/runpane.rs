//! The run pane: output of a background run, newest lines at the bottom.

use super::{border, pane_title};
use crate::{
    app::{AppState, Pane, RunLog, RunStatus, follow_top, format_elapsed},
    config::Action,
    runline::{RunLine, Seg},
    theme::Colors,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use std::borrow::Cow;

/// The keys that do something right now: `r` stops a running command and
/// starts a finished one again; `G` matters only once following stopped.
fn hint(log: &RunLog) -> String {
    let r = if log.running() {
        "r stop"
    } else {
        "r run again"
    };
    let follow = if log.scroll.is_some() {
        " · G follow"
    } else {
        ""
    };
    format!(" R hide · {r}{follow} ")
}

pub fn render(state: &AppState, frame: &mut Frame, area: Rect, colors: &Colors) {
    let Some(log) = &state.run_log else {
        return;
    };
    let (status, color) = match &log.status {
        RunStatus::Running => ("running…".to_owned(), colors.info),
        RunStatus::Exited(0) => ("exited 0".to_owned(), colors.done),
        RunStatus::Exited(code) => (format!("exited {code}"), colors.error),
        RunStatus::Failed(_) => ("could not start".to_owned(), colors.error),
    };
    let mut title = pane_title(state, Action::FocusRun, "run ·", colors);
    title.spans.extend([
        Span::styled(status, Style::new().fg(color).add_modifier(Modifier::BOLD)),
        Span::styled(
            format!(" · {} · {} ", format_elapsed(log.elapsed), log.command),
            Style::new().fg(colors.muted),
        ),
    ]);
    let hint = hint(log);
    let block = Block::new()
        .borders(Borders::ALL)
        .border_style(border(state.focus == Pane::Run, colors))
        .title(title)
        .title_bottom(Span::styled(hint, Style::new().fg(colors.muted)));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    // A command that never started has only its error to show.
    let lines: Cow<[RunLine]> = match &log.status {
        RunStatus::Failed(error) => {
            let mut lines = log.lines.clone();
            lines.push(RunLine::Error(error.clone()));
            Cow::Owned(lines)
        }
        _ => Cow::Borrowed(&log.lines),
    };
    let (width, height) = (usize::from(inner.width), usize::from(inner.height));
    let top = log
        .scroll
        .unwrap_or_else(|| follow_top(&lines, width, height));
    let body: Vec<Line> = lines
        .iter()
        .skip(top)
        .flat_map(|line| line.rows(width))
        .take(height)
        .map(|row| {
            Line::from(
                row.into_iter()
                    .map(|(text, seg)| Span::styled(text, style(seg, colors)))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    frame.render_widget(Paragraph::new(body), inner);
}

fn style(seg: Seg, colors: &Colors) -> Style {
    let bold = Modifier::BOLD;
    match seg {
        Seg::Text => Style::new().fg(colors.text),
        Seg::Muted => Style::new().fg(colors.muted),
        Seg::Accent => Style::new().fg(colors.info).add_modifier(bold),
        Seg::Bold => Style::new().fg(colors.text).add_modifier(bold),
        Seg::Code => Style::new().fg(colors.hunk),
        Seg::Heading => Style::new().fg(colors.border_active).add_modifier(bold),
        Seg::Error => Style::new().fg(colors.error),
        Seg::Done => Style::new().fg(colors.done).add_modifier(bold),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn log(status: RunStatus, scroll: Option<usize>) -> RunLog {
        RunLog {
            command: "agent".into(),
            lines: Vec::new(),
            status,
            visible: true,
            elapsed: Duration::ZERO,
            scroll,
            stream: None,
        }
    }

    #[test]
    fn the_hint_offers_only_keys_that_apply_now() {
        assert_eq!(hint(&log(RunStatus::Running, None)), " R hide · r stop ");
        assert_eq!(
            hint(&log(RunStatus::Exited(0), None)),
            " R hide · r run again "
        );
        assert_eq!(
            hint(&log(RunStatus::Failed("no sh".into()), Some(3))),
            " R hide · r run again · G follow "
        );
    }
}
