//! Drawing. Reads `&AppState` and never mutates it; every number it needs
//! was computed in the update step.

mod agent_choice;
mod comments;
mod confirm;
mod diff;
mod picker;
mod prompt;
mod runpane;

use crate::{
    app::{AppState, Level, Pane},
    config::{Action, Background},
    git::FileStatus,
    theme::{Colors, blend},
    tree::Entry,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph},
};

pub fn render(state: &AppState, frame: &mut Frame) {
    let colors = &state.layout.theme.colors;
    // The theme's own ground first, so its colours never sit on a terminal
    // background of the other kind.
    frame.render_widget(Block::new().style(surface(state, colors)), frame.area());
    let [main, status] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());
    let diff_area = if state.layout.zoom {
        main
    } else {
        let tree_width = main.width * state.layout.tree_percent / 100;
        let [left, diff_area] =
            Layout::horizontal([Constraint::Length(tree_width), Constraint::Min(1)]).areas(main);
        let [tree, comments_area] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(state.comments_pane_height()),
        ])
        .areas(left);
        render_tree(state, frame, tree, colors);
        comments::render(state, frame, comments_area, colors);
        diff_area
    };
    let [diff_area, run_area] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(state.run_pane_height()),
    ])
    .areas(diff_area);
    runpane::render(state, frame, run_area, colors);
    diff::render(state, frame, diff_area, colors);
    prompt::render(state, frame, diff_area, main, colors);
    // A yes/no question or the help takes the whole screen's attention; the
    // comment popup and the theme picker leave it bright, since what is
    // behind them is the point.
    if state.confirm.is_some() || state.agent_choice.is_some() {
        dim(state, frame, main, colors);
    }
    confirm::render(state, frame, main, colors);
    agent_choice::render(state, frame, main, colors);
    picker::render(state, frame, main, colors);
    render_status(state, frame, status, colors);
    if state.help {
        dim(state, frame, frame.area(), colors);
        render_help(state, frame, colors);
    }
}

/// `[key] ` for the key that focuses a pane, or nothing when it is unbound.
pub fn pane_number(state: &AppState, action: Action) -> String {
    let key = state.key_label(action);
    if key.is_empty() {
        String::new()
    } else {
        format!("[{key}] ")
    }
}

/// A pane title: its focus key in an accent colour, apart from the text, so
/// a number in the text never reads as part of it.
pub fn pane_title(state: &AppState, action: Action, text: &str, colors: &Colors) -> Line<'static> {
    Line::from(vec![
        Span::raw(" "),
        Span::styled(
            pane_number(state, action),
            Style::new().fg(colors.info).add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("{text} ")),
    ])
}

/// The ground everything is drawn on: the theme's background, or nothing so
/// the terminal's shows through.
pub fn surface(state: &AppState, colors: &Colors) -> Style {
    match state.config.background {
        Background::Theme => Style::new().bg(colors.background),
        Background::Terminal => Style::new(),
    }
}

/// How strongly a popup's surface is tinted with the accent, in percent.
const RAISED_PERCENT: u16 = 14;

/// Columns of space between a popup's border and its text, on each side.
pub const POPUP_PADDING: u16 = 1;

/// A popup's frame: a surface raised off the screen by an accent tint, a
/// rounded border in `accent`, and a cell of padding each side, so it reads
/// as on top of whatever is behind it.
pub fn popup_block<'a>(colors: &Colors, accent: Color) -> Block<'a> {
    Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(accent))
        .padding(Padding::horizontal(POPUP_PADDING))
        .style(Style::new().fg(colors.text).bg(blend(
            colors.border_active,
            colors.background,
            RAISED_PERCENT,
        )))
}

/// Fades what is already drawn in `area`, for a popup that wants the
/// screen's attention to itself.
fn dim(state: &AppState, frame: &mut Frame, area: Rect, colors: &Colors) {
    let faded = Style::new()
        .fg(colors.muted)
        .remove_modifier(Modifier::BOLD);
    let faded = match state.config.background {
        Background::Theme => faded.bg(colors.background),
        Background::Terminal => faded,
    };
    frame.buffer_mut().set_style(area, faded);
}

pub fn border(focused: bool, colors: &Colors) -> Style {
    Style::new().fg(if focused {
        colors.border_active
    } else {
        colors.border_inactive
    })
}

fn render_tree(state: &AppState, frame: &mut Frame, area: Rect, colors: &Colors) {
    let focused = state.focus == Pane::Tree;
    let block = Block::new()
        .borders(Borders::ALL)
        .border_style(border(focused, colors))
        .title(pane_title(
            state,
            Action::FocusFiles,
            &files_title(state),
            colors,
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let open: Option<&str> = state.file.as_ref().and_then(|f| f.path());
    let lines: Vec<Line> = state
        .tree
        .iter()
        .enumerate()
        .skip(state.tree_scroll)
        .take(usize::from(inner.height))
        .map(|(i, row)| {
            let mut spans = vec![Span::raw("  ".repeat(row.depth))];
            match &row.entry {
                Entry::Dir { folded, path } => {
                    let marker = if *folded { "▸ " } else { "▾ " };
                    spans.push(Span::styled(marker, Style::new().fg(colors.muted)));
                    spans.push(Span::styled(
                        row.label.clone(),
                        Style::new().fg(colors.subtle),
                    ));
                    spans.extend(dir_badges(state, path, colors));
                }
                Entry::File(index) => {
                    let path = &state.paths[*index];
                    let style = if open == Some(path.as_str()) {
                        Style::new().fg(colors.text).add_modifier(Modifier::BOLD)
                    } else {
                        Style::new().fg(colors.text)
                    };
                    spans.push(status_letter(state, path, colors));
                    spans.push(Span::styled(row.label.clone(), style));
                    spans.extend(badges(state, path, colors));
                }
            }
            let mut line = Line::from(spans);
            if i == state.tree_cursor {
                line = line.style(Style::new().bg(if focused {
                    colors.cursor_bg
                } else {
                    colors.selection_bg
                }));
            }
            line
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

/// `files`, then the filter and how many of the files it shows.
fn files_title(state: &AppState) -> String {
    let filter = &state.filter;
    if !filter.is_set() && !filter.typing {
        return "files".to_owned();
    }
    let caret = if filter.typing { "▏" } else { "" };
    let count = if state.paths.is_empty() {
        "no match".to_owned()
    } else {
        format!("{}/{}", state.paths.len(), state.all_paths)
    };
    format!("files · /{}{caret} {count}", filter.text)
}

/// The git status column before a file name; blank for a file that is only
/// listed for its annotations.
fn status_letter(state: &AppState, path: &str, colors: &Colors) -> Span<'static> {
    let Some(entry) = state.snapshot.find(path).map(|i| &state.snapshot.files[i]) else {
        return Span::raw("  ");
    };
    let status = entry.status();
    let color = match status {
        FileStatus::Modified => colors.warning,
        FileStatus::Added => colors.added,
        FileStatus::Deleted => colors.removed,
        FileStatus::Untracked => colors.info,
    };
    Span::styled(format!("{} ", status.letter()), Style::new().fg(color))
}

fn stage_badges(unstaged: bool, staged: bool, colors: &Colors) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    if unstaged {
        spans.push(Span::styled(" ●", Style::new().fg(colors.warning)));
    }
    if staged {
        spans.push(Span::styled(" ✓", Style::new().fg(colors.done)));
    }
    spans
}

/// A directory's staging badges, from every file under it: a directory
/// whose files are all staged reads like a staged file.
fn dir_badges(state: &AppState, dir: &str, colors: &Colors) -> Vec<Span<'static>> {
    let prefix = format!("{dir}/");
    let under: Vec<_> = state
        .snapshot
        .files
        .iter()
        .filter(|f| f.path.starts_with(&prefix))
        .collect();
    stage_badges(
        under.iter().any(|f| f.has_unstaged()),
        under.iter().any(|f| f.has_staged()),
        colors,
    )
}

fn badges(state: &AppState, path: &str, colors: &Colors) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    if let Some(entry) = state.snapshot.find(path).map(|i| &state.snapshot.files[i]) {
        spans = stage_badges(entry.has_unstaged(), entry.has_staged(), colors);
    }
    let notes = state
        .review
        .items
        .iter()
        .filter(|i| i.scope.path() == Some(path))
        .count();
    if notes > 0 {
        spans.push(Span::styled(
            // Many fonts draw the pencil wider than one cell; the space keeps
            // the count from running into it.
            format!(" ✎ {notes}"),
            Style::new().fg(colors.question),
        ));
    }
    if state.changed_since_review.contains(path) {
        spans.push(Span::styled(" Δ", Style::new().fg(colors.suggestion)));
    }
    spans
}

fn render_status(state: &AppState, frame: &mut Frame, area: Rect, colors: &Colors) {
    let line = if let Some(message) = &state.message {
        let color = match message.level {
            Level::Info => colors.info,
            Level::Error => colors.error,
        };
        Line::styled(message.text.clone(), Style::new().fg(color))
    } else {
        let open = state.review.open_items();
        let done = state.review.items.len() - open;
        let file = state.review_path().and_then(|p| p.file_name()).map_or_else(
            || "no review file yet".to_owned(),
            |n| n.to_string_lossy().into_owned(),
        );
        Line::styled(
            format!(
                "{} · {open} open{} · {done} done · {file} · {} help · {} quit",
                state.target.base(),
                comments::working(state),
                state.key_label(Action::Help),
                state.key_label(Action::Quit),
            ),
            Style::new().fg(colors.muted),
        )
    };
    let used = line.width();
    frame.render_widget(Paragraph::new(line), area);
    let notice = state
        .update
        .as_ref()
        .map(crate::update::UpdateStatus::notice);
    let (notice, version) = right_end(notice, used, area.width);
    let Some(version) = version else {
        return;
    };
    let mut spans = Vec::new();
    if let Some(notice) = notice {
        spans.push(Span::styled(notice, Style::new().fg(colors.done)));
        spans.push(Span::styled(" · ", Style::new().fg(colors.muted)));
    }
    spans.push(Span::styled(version, Style::new().fg(colors.muted)));
    let corner = Line::from(spans);
    let width = u16::try_from(corner.width()).unwrap_or(area.width);
    let corner_area = Rect {
        x: area.x + area.width - width,
        width,
        ..area
    };
    frame.render_widget(Paragraph::new(corner), corner_area);
}

/// What fits at the status line's right end after the `used` columns, with a
/// gap of two: the update notice and `v<version>`, else the version alone,
/// else nothing. The status text always wins.
fn right_end(notice: Option<String>, used: usize, width: u16) -> (Option<String>, Option<String>) {
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    let fits = |columns: usize| used + 2 + columns <= usize::from(width);
    match notice {
        Some(notice) if fits(notice.len() + 3 + version.len()) => (Some(notice), Some(version)),
        _ if fits(version.len()) => (None, Some(version)),
        _ => (None, None),
    }
}

/// Keys that do nothing in a commit or range.
fn disabled_in_range(action: Action) -> bool {
    matches!(action, Action::Stage | Action::Unstage | Action::Discard)
}

fn render_help(state: &AppState, frame: &mut Frame, colors: &Colors) {
    let area = frame.area();
    let width = area.width.min(72 + 2 * POPUP_PADDING);
    let height = area
        .height
        .min(u16::try_from(Action::ALL.len() + 4).unwrap_or(u16::MAX));
    let popup = Rect {
        x: area.x + (area.width - width) / 2,
        // Help is tall, so it is centred plainly rather than lifted like the
        // small popups.
        y: area.y + (area.height - height) / 2,
        width,
        height,
    };
    let lines: Vec<Line> = help_lines(state)
        .into_iter()
        .map(|(keys, help, disabled)| {
            let style = if disabled {
                Style::new()
                    .fg(colors.muted)
                    .add_modifier(Modifier::CROSSED_OUT)
            } else {
                Style::new().fg(colors.text)
            };
            Line::from(vec![
                Span::styled(
                    format!("{keys:>12}  "),
                    Style::new().fg(colors.border_active),
                ),
                Span::styled(help, style),
            ])
        })
        .collect();
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(popup_block(colors, colors.border_active).title(" keys · any key closes ")),
        popup,
    );
}

/// `(keys, description, disabled)` for every action, as the help shows them.
pub fn help_lines(state: &AppState) -> Vec<(String, String, bool)> {
    Action::ALL
        .iter()
        .map(|action| {
            let disabled = state.target.is_range() && disabled_in_range(*action);
            let mut help = action.help().to_owned();
            if disabled {
                help.push_str(" (disabled in range mode)");
            }
            (state.key_label(*action), help, disabled)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::tests::state, git::fixture::Fixture};

    fn repo() -> Fixture {
        let f = Fixture::new();
        f.write("src/a.rs", "fn a() {}\nfn b() {}\n");
        f.commit_all("init");
        f.write("src/a.rs", "fn a() { 1 }\nfn b() {}\n");
        f
    }

    #[test]
    fn the_update_notice_and_version_give_way_to_the_status_text() {
        let version = format!("v{}", env!("CARGO_PKG_VERSION"));
        let notice = "Update available, restart to update".to_owned();
        let both = u16::try_from(40 + 2 + notice.len() + 3 + version.len()).unwrap();
        assert_eq!(
            right_end(Some(notice.clone()), 40, both),
            (Some(notice.clone()), Some(version.clone()))
        );
        // One column short for both: the version stays, the notice goes.
        assert_eq!(
            right_end(Some(notice.clone()), 41, both),
            (None, Some(version.clone()))
        );
        let room = u16::try_from(40 + 2 + version.len()).unwrap();
        assert_eq!(right_end(None, 40, room), (None, Some(version)));
        assert_eq!(right_end(None, 41, room), (None, None));
        assert_eq!(right_end(Some(notice), 0, 5), (None, None));
    }

    #[test]
    fn annotation_badge_leaves_a_space_between_pencil_and_count() {
        let f = repo();
        let mut s = state(&f);
        crate::app::tests::press(&mut s, "Cone\n");
        let colors = s.layout.theme.colors;
        let text: String = badges(&s, "src/a.rs", &colors)
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(text.contains(" ✎ 1"), "{text:?}");
    }

    #[test]
    fn a_directory_shows_staged_only_once_every_file_under_it_is_staged() {
        let f = repo();
        f.write("src/deep/b.rs", "fn b() {}\n");
        let mut s = state(&f);
        let colors = s.layout.theme.colors;
        let text = |s: &AppState| -> String {
            dir_badges(s, "src", &colors)
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        };
        assert_eq!(text(&s), " ●");
        f.git(&["add", "src/a.rs"]);
        s.refresh().unwrap();
        assert_eq!(text(&s), " ● ✓");
        f.git(&["add", "src"]);
        s.refresh().unwrap();
        assert_eq!(text(&s), " ✓");
    }

    #[test]
    fn the_files_title_shows_the_filter_and_its_count() {
        let f = repo();
        let mut s = state(&f);
        assert_eq!(files_title(&s), "files");
        crate::app::tests::press(&mut s, "/a.r");
        assert_eq!(files_title(&s), "files · /a.r▏ 1/1");
        crate::app::tests::press(&mut s, "x");
        assert_eq!(files_title(&s), "files · /a.rx▏ no match");
    }

    #[test]
    fn the_theme_paints_the_ground_unless_the_terminal_is_asked_for() {
        let f = repo();
        let mut s = state(&f);
        let colors = s.layout.theme.colors;
        assert_eq!(surface(&s, &colors).bg, Some(colors.background));
        s.config.merge("[ui]\nbackground = \"terminal\"\n").unwrap();
        assert_eq!(surface(&s, &colors).bg, None);
    }

    #[test]
    fn pane_titles_carry_the_key_that_focuses_them() {
        let f = repo();
        let mut s = state(&f);
        assert_eq!(pane_number(&s, Action::FocusFiles), "[1] ");
        assert_eq!(pane_number(&s, Action::FocusDiff), "[2] ");
        assert_eq!(pane_number(&s, Action::FocusComments), "[3] ");
        s.config.merge("[keys]\nfocus_diff = \"0\"\n").unwrap();
        assert_eq!(pane_number(&s, Action::FocusDiff), "[0] ");
    }

    #[test]
    fn help_marks_stage_keys_disabled_in_range_mode() {
        let f = repo();
        f.commit_all("second");
        let mut s = state(&f);
        s.target = f.repo.commit_range("HEAD").unwrap();
        let lines = help_lines(&s);
        let stage = lines.iter().find(|(k, ..)| k == "s").unwrap();
        assert!(stage.2 && stage.1.contains("disabled"));
        let comment = lines.iter().find(|(k, ..)| k == "c").unwrap();
        assert!(!comment.2);
    }
}
