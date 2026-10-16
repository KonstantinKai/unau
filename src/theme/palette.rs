//! The palette every built-in family is written as, and the single way its
//! colours fill the interface slots: Catppuccin's style guide, applied to
//! each family's own colours so they all read the same way.

use super::{Colors, blend};
use ratatui::style::Color;

/// Opacity of diff tints and the selection band, within the style guide's
/// recommended 15 to 30 percent.
const TINT_PERCENT: u16 = 20;
const SELECTION_PERCENT: u16 = 25;

/// A family's colours by role, named after Catppuccin's: `base` is the
/// background, `mantle` a second background for notes, `surface1` the
/// cursor band, `overlay0` to `overlay2` greys from dim to bright, `subtext0`
/// and `text` the foregrounds, `lavender` the accent.
pub struct Palette {
    pub mauve: Color,
    pub red: Color,
    pub peach: Color,
    pub yellow: Color,
    pub green: Color,
    pub teal: Color,
    pub blue: Color,
    pub lavender: Color,
    pub text: Color,
    pub subtext0: Color,
    pub overlay2: Color,
    pub overlay1: Color,
    pub overlay0: Color,
    pub surface1: Color,
    pub base: Color,
    pub mantle: Color,
}

pub const fn colors(p: &Palette) -> Colors {
    Colors {
        background: p.base,
        text: p.text,
        muted: p.overlay1,
        subtle: p.subtext0,
        border_active: p.lavender,
        border_inactive: p.overlay0,
        cursor_bg: p.surface1,
        selection_bg: blend(p.overlay2, p.base, SELECTION_PERCENT),
        note_bg: p.mantle,
        added: p.green,
        removed: p.red,
        added_bg: blend(p.green, p.base, TINT_PERCENT),
        removed_bg: blend(p.red, p.base, TINT_PERCENT),
        hunk: p.peach,
        fix: p.yellow,
        question: p.blue,
        suggestion: p.mauve,
        done: p.green,
        error: p.red,
        warning: p.yellow,
        info: p.teal,
    }
}
