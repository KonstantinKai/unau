//! How a pane that scrolls shows that more is out of view: a scrollbar in
//! its last column, or `↑3 ↓12` on its bottom border (`[ui] scroll`).

use crate::{
    app::AppState,
    config::ScrollStyle,
    theme::{Colors, blend},
};

/// How much of the inactive border colour the track keeps over the
/// background, in percent.
const TRACK_PERCENT: u16 = 50;
use ratatui::{Frame, layout::Rect, style::Style, text::Line, widgets::Block};

/// What a pane scrolls through: `total` lines, `shown` of them from `top`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scroll {
    pub total: usize,
    pub top: usize,
    pub shown: usize,
}

impl Scroll {
    fn hidden(self) -> bool {
        self.total > self.shown
    }
}

/// ` ↑3 ↓12 `: how many of `total` lines are out of view above and below a
/// pane showing `height` of them from `top`, or nothing when all show.
pub fn hidden_lines(total: usize, top: usize, height: usize) -> Option<String> {
    let above = top;
    let below = total.saturating_sub(top + height);
    let mut parts = Vec::new();
    if above > 0 {
        parts.push(format!("↑{above}"));
    }
    if below > 0 {
        parts.push(format!("↓{below}"));
    }
    (!parts.is_empty()).then(|| format!(" {} ", parts.join(" ")))
}

/// `block` with the hint on its bottom border, when hints are the style.
pub fn with_hint<'a>(
    state: &AppState,
    block: Block<'a>,
    scroll: Scroll,
    colors: &Colors,
) -> Block<'a> {
    if state.config.scroll != ScrollStyle::Hint {
        return block;
    }
    match hidden_lines(scroll.total, scroll.top, scroll.shown) {
        Some(hint) => {
            block.title_bottom(Line::styled(hint, Style::new().fg(colors.muted)).right_aligned())
        }
        None => block,
    }
}

/// The part of a pane's `inner` area left for text: all of it, or all but
/// the last column while the scrollbar is the style.
pub fn text_area(state: &AppState, inner: Rect) -> Rect {
    let keep = u16::try_from(state.scrollbar_columns()).unwrap_or(0);
    Rect {
        width: inner.width.saturating_sub(keep),
        ..inner
    }
}

/// Draws the scrollbar in the last column of `inner` when it is the style
/// and something is out of view: a half-cell bar down the whole column, dim
/// for the track and brighter for the thumb, hugging the cell's right edge
/// so a sliver of space stays between it and the border.
pub fn scrollbar(
    state: &AppState,
    frame: &mut Frame,
    inner: Rect,
    scroll: Scroll,
    focused: bool,
    colors: &Colors,
) {
    if state.config.scroll != ScrollStyle::Scrollbar || !scroll.hidden() || inner.width == 0 {
        return;
    }
    let Some((top, len)) = thumb(scroll, usize::from(inner.height)) else {
        return;
    };
    let thumb_color = if focused { colors.subtle } else { colors.muted };
    // The track is the thumb's width, dimmed toward the background so the
    // thumb stands out on it in any theme.
    let track_color = blend(colors.border_inactive, colors.background, TRACK_PERCENT);
    let x = inner.x + inner.width - 1;
    let buf = frame.buffer_mut();
    for row in 0..usize::from(inner.height) {
        let y = inner.y + u16::try_from(row).unwrap_or(u16::MAX);
        let Some(cell) = buf.cell_mut((x, y)) else {
            continue;
        };
        let color = if (top..top + len).contains(&row) {
            thumb_color
        } else {
            track_color
        };
        cell.set_symbol("▐");
        cell.set_style(Style::new().fg(color));
    }
}

/// Rows of a `track`-row bar the thumb takes, as (first, count): as long as
/// the share in view, at least one row, and at the bottom exactly when the
/// last line shows.
fn thumb(scroll: Scroll, track: usize) -> Option<(usize, usize)> {
    if track == 0 || !scroll.hidden() {
        return None;
    }
    let len = ((scroll.shown * track + scroll.total / 2) / scroll.total).clamp(1, track);
    let room = track - len;
    let max_top = scroll.total - scroll.shown;
    let top = (scroll.top.min(max_top) * room + max_top / 2) / max_top;
    Some((top, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scroll(total: usize, top: usize, shown: usize) -> Scroll {
        Scroll { total, top, shown }
    }

    #[test]
    fn the_thumb_is_as_long_as_the_share_in_view_and_ends_where_the_text_does() {
        assert_eq!(thumb(scroll(100, 0, 10), 10), Some((0, 1)));
        assert_eq!(thumb(scroll(100, 90, 10), 10), Some((9, 1)));
        assert_eq!(thumb(scroll(20, 0, 10), 10), Some((0, 5)));
        assert_eq!(thumb(scroll(20, 10, 10), 10), Some((5, 5)));
        assert_eq!(thumb(scroll(20, 5, 10), 10), Some((3, 5)));
        // Everything in view: no thumb.
        assert_eq!(thumb(scroll(10, 0, 10), 10), None);
        // A huge text still gets a thumb of one row.
        assert_eq!(thumb(scroll(1_000_000, 0, 10), 10), Some((0, 1)));
    }

    #[test]
    fn the_hint_names_what_is_out_of_view_on_each_side() {
        assert_eq!(hidden_lines(10, 0, 10), None);
        assert_eq!(hidden_lines(30, 0, 10).as_deref(), Some(" ↓20 "));
        assert_eq!(hidden_lines(30, 5, 10).as_deref(), Some(" ↑5 ↓15 "));
        assert_eq!(hidden_lines(30, 20, 10).as_deref(), Some(" ↑20 "));
    }
}
