//! The diff pane: header, line-number gutter, omitted-lines separator, selection
//! band, inline notes, unified or side by side.

use super::border;
use crate::{
    app::{AppState, FileState, NoteStyle, Pane, Source},
    config::{Action, LineNumbers},
    diff::{Row, Spans, TAB_WIDTH, wrap_ranges},
    git::{Line as DiffLine, LineKind},
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

pub const SEPARATOR: &str = "··· unchanged lines omitted · z: expand nearby ···";

pub fn render(state: &AppState, frame: &mut Frame, area: Rect, colors: &Colors) {
    let focused = state.focus == Pane::Diff;
    let block = Block::new()
        .borders(Borders::ALL)
        .border_style(border(focused, colors))
        .title(header(state, colors));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(file) = &state.file else {
        let text = if state.tree.is_empty() {
            "no changes to review"
        } else {
            "select a file"
        };
        frame.render_widget(
            Paragraph::new(text).style(Style::new().fg(colors.muted)),
            inner,
        );
        return;
    };
    let width = usize::from(inner.width);
    let selected = file.anchor.map(|_| file.selected_rows());
    let mut lines = Vec::new();
    for visual in file.scroll..file.scroll + usize::from(inner.height) {
        let Some(row) = file.view.row_at(visual) else {
            break;
        };
        let part = visual - file.view.starts[row];
        let mut line = draw_row(state, file, file.view.rows[row], part, width, colors);
        let band = if row == file.cursor && focused {
            Some(colors.cursor_bg)
        } else if selected.as_ref().is_some_and(|s| s.contains(&row)) || row == file.cursor {
            Some(colors.selection_bg)
        } else {
            None
        };
        if let Some(bg) = band {
            line = line.style(Style::new().bg(bg));
            for span in &mut line.spans {
                span.style = span.style.bg(colors.band_over(span.style.bg, bg));
            }
        }
        lines.push(line);
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn header(state: &AppState, colors: &Colors) -> Line<'static> {
    let mode = if state.layout.split {
        "side by side"
    } else {
        "unified"
    };
    let text = match state.file.as_ref().map(|f| &f.source) {
        Some(Source::File(path)) => {
            let n = state
                .paths
                .iter()
                .position(|p| p == path)
                .map_or(0, |i| i + 1);
            let total = state.paths.len();
            format!("{path} · {n}/{total} · {mode}")
        }
        Some(Source::General) => "General".to_owned(),
        None => "diff".to_owned(),
    };
    super::pane_title(state, Action::FocusDiff, &text, colors)
}

fn draw_row(
    state: &AppState,
    file: &FileState,
    row: Row,
    part: usize,
    width: usize,
    colors: &Colors,
) -> Line<'static> {
    match row {
        Row::Section(s) => {
            let title = format!(" {} ", file.sections[s].kind.title());
            Line::styled(
                title,
                Style::new()
                    .fg(colors.muted)
                    .add_modifier(Modifier::REVERSED),
            )
        }
        Row::Gap { .. } => Line::styled(format!("  {SEPARATOR}"), Style::new().fg(colors.hunk)),
        Row::Binary(_) => Line::styled(
            "  binary file: stage or discard it whole",
            Style::new().fg(colors.muted),
        ),
        Row::Note(item) => draw_note(file, item, part, width, colors),
        Row::Code { section, lines } => {
            let diff = &file.sections[section].diff;
            let spans = file.highlight.spans.get(section);
            let side = |i: Option<usize>| i.map(|i| (&diff.lines[i], spans.and_then(|s| s.get(i))));
            let widths = crate::app::code_widths(
                width,
                file.digits,
                state.layout.split,
                state.config.line_numbers,
            );
            if state.layout.split {
                let left = side(lines[0].filter(|i| diff.lines[*i].kind != LineKind::Added));
                let right = side(lines[1].filter(|i| diff.lines[*i].kind != LineKind::Removed));
                let mut out = half(left, true, part, widths[0], file.digits, colors);
                out.push(Span::styled("│", Style::new().fg(colors.muted)));
                out.extend(half(right, false, part, widths[1], file.digits, colors));
                Line::from(out)
            } else {
                let (line, spans) = side(lines[0]).expect("unified rows carry a line");
                let mut out = gutter(line, part, file.digits, state.config.line_numbers, colors);
                out.extend(code(line, spans, part, widths[0], colors));
                Line::from(out)
            }
        }
    }
}

fn number(n: Option<usize>, digits: usize, show: bool) -> String {
    match n {
        Some(n) if show => format!("{n:>digits$}"),
        _ => " ".repeat(digits),
    }
}

fn sign(line: &DiffLine) -> char {
    match line.kind {
        LineKind::Added => '+',
        LineKind::Removed => '-',
        LineKind::Context => ' ',
    }
}

fn gutter(
    line: &DiffLine,
    part: usize,
    digits: usize,
    numbers: LineNumbers,
    colors: &Colors,
) -> Vec<Span<'static>> {
    let first = part == 0;
    let text = match numbers {
        LineNumbers::Both => format!(
            "{} {} {} ",
            number(line.old, digits, first),
            number(line.new, digits, first),
            if first { sign(line) } else { ' ' }
        ),
        LineNumbers::New => format!(
            "{} {} ",
            number(line.new, digits, first),
            if first { sign(line) } else { ' ' }
        ),
    };
    vec![Span::styled(text, Style::new().fg(colors.muted))]
}

fn half(
    side: Option<(&DiffLine, Option<&Spans>)>,
    old: bool,
    part: usize,
    width: usize,
    digits: usize,
    colors: &Colors,
) -> Vec<Span<'static>> {
    let Some((line, spans)) = side else {
        return vec![Span::raw(" ".repeat(digits + 3 + width))];
    };
    let n = if old { line.old } else { line.new };
    let first = part == 0;
    let mut out = vec![Span::styled(
        format!(
            "{} {} ",
            number(n, digits, first),
            if first { sign(line) } else { ' ' }
        ),
        Style::new().fg(colors.muted),
    )];
    out.extend(code(line, spans, part, width, colors));
    out
}

/// One wrapped piece of a source line, highlighted and padded to `width`.
fn code(
    line: &DiffLine,
    spans: Option<&Spans>,
    part: usize,
    width: usize,
    colors: &Colors,
) -> Vec<Span<'static>> {
    let bg = match line.kind {
        LineKind::Added => Some(colors.added_bg),
        LineKind::Removed => Some(colors.removed_bg),
        LineKind::Context => None,
    };
    let base = bg.map_or(Style::new().fg(colors.text), |bg| {
        Style::new().fg(colors.text).bg(bg)
    });
    let Some(range) = wrap_ranges(&line.text, width).get(part).cloned() else {
        return vec![Span::styled(" ".repeat(width), base)];
    };
    let mut out = Vec::new();
    let mut at = range.start;
    let mut used = 0;
    let mut push = |text: &str, style: Style, used: &mut usize| {
        let text = text.replace('\t', &" ".repeat(TAB_WIDTH));
        *used += crate::diff::display_width(&text);
        out.push(Span::styled(text, style));
    };
    for (color, span) in spans.into_iter().flatten() {
        let start = span.start.max(range.start);
        let end = span.end.min(range.end);
        if start >= end {
            continue;
        }
        if at < start {
            push(&line.text[at..start], base, &mut used);
        }
        push(&line.text[start..end], base.fg(*color), &mut used);
        at = end;
    }
    if at < range.end {
        push(&line.text[at..range.end], base, &mut used);
    }
    if used < width {
        out.push(Span::styled(" ".repeat(width - used), base));
    }
    out
}

fn draw_note(
    file: &FileState,
    item: usize,
    part: usize,
    width: usize,
    colors: &Colors,
) -> Line<'static> {
    let Some(lines) = file.notes.get(&item) else {
        return Line::raw("");
    };
    let width = width.saturating_sub(4).max(1);
    let mut remaining = part;
    for (index, note) in lines.iter().enumerate() {
        let pieces = wrap_ranges(&note.text, width);
        if remaining >= pieces.len() {
            remaining -= pieces.len();
            continue;
        }
        let text = note.text[pieces[remaining].clone()].to_owned();
        let style = Style::new().bg(colors.note_bg);
        let mut spans = vec![Span::styled("    ", style)];
        match note.style {
            NoteStyle::Header {
                kind,
                done,
                working,
            } if index == 0 && remaining == 0 => {
                let color = match kind {
                    Kind::Fix => colors.fix,
                    Kind::Question => colors.question,
                    Kind::Suggestion => colors.suggestion,
                };
                if done {
                    spans.push(Span::styled("✓ ", style.fg(colors.done)));
                } else if working {
                    spans.push(Span::styled("~ ", style.fg(colors.info)));
                }
                spans.push(Span::styled(
                    format!("[{}] ", kind.name()),
                    style.fg(color).add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled(text, style.fg(colors.text)));
            }
            NoteStyle::Added => spans.push(Span::styled(text, style.fg(colors.added))),
            NoteStyle::Removed => spans.push(Span::styled(text, style.fg(colors.removed))),
            NoteStyle::Warning => spans.push(Span::styled(text, style.fg(colors.error))),
            NoteStyle::Same | NoteStyle::Text | NoteStyle::Header { .. } => {
                spans.push(Span::styled(text, style.fg(colors.muted)));
            }
        }
        return Line::from(spans);
    }
    Line::raw("")
}
