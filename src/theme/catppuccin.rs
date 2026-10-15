//! Catppuccin Macchiato and Latte, from the official palette, with colours
//! placed the way Catppuccin's style guide places them.

use super::{Colors, Syntax, Theme, blend, hex};
use ratatui::style::Color;
use std::borrow::Cow;

/// Opacity of diff tints and the selection band, within the style guide's
/// recommended 15 to 30 percent.
const TINT_PERCENT: u16 = 20;
const SELECTION_PERCENT: u16 = 25;

/// The palette colours revmark uses; the rest of the 26 have no slot.
struct Palette {
    mauve: Color,
    red: Color,
    peach: Color,
    yellow: Color,
    green: Color,
    teal: Color,
    blue: Color,
    lavender: Color,
    text: Color,
    subtext0: Color,
    overlay2: Color,
    overlay1: Color,
    overlay0: Color,
    surface1: Color,
    base: Color,
    mantle: Color,
}

const MACCHIATO_PALETTE: Palette = Palette {
    mauve: hex(0xc6a0f6),
    red: hex(0xed8796),
    peach: hex(0xf5a97f),
    yellow: hex(0xeed49f),
    green: hex(0xa6da95),
    teal: hex(0x8bd5ca),
    blue: hex(0x8aadf4),
    lavender: hex(0xb7bdf8),
    text: hex(0xcad3f5),
    subtext0: hex(0xa5adcb),
    overlay2: hex(0x939ab7),
    overlay1: hex(0x8087a2),
    overlay0: hex(0x6e738d),
    surface1: hex(0x494d64),
    base: hex(0x24273a),
    mantle: hex(0x1e2030),
};

const LATTE_PALETTE: Palette = Palette {
    mauve: hex(0x8839ef),
    red: hex(0xd20f39),
    peach: hex(0xfe640b),
    yellow: hex(0xdf8e1d),
    green: hex(0x40a02b),
    teal: hex(0x179299),
    blue: hex(0x1e66f5),
    lavender: hex(0x7287fd),
    text: hex(0x4c4f69),
    subtext0: hex(0x6c6f85),
    overlay2: hex(0x7c7f93),
    overlay1: hex(0x8c8fa1),
    overlay0: hex(0x9ca0b0),
    surface1: hex(0xbcc0cc),
    base: hex(0xeff1f5),
    mantle: hex(0xe6e9ef),
};

const fn colors(p: &Palette) -> Colors {
    Colors {
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

pub const MACCHIATO: Theme = Theme {
    name: Cow::Borrowed("catppuccin.macchiato"),
    colors: colors(&MACCHIATO_PALETTE),
    syntax: Syntax::CatppuccinMacchiato,
};

pub const LATTE: Theme = Theme {
    name: Cow::Borrowed("catppuccin.latte"),
    colors: colors(&LATTE_PALETTE),
    syntax: Syntax::CatppuccinLatte,
};

/// The official syntax themes from `catppuccin/bat`, MIT licensed; the notice
/// ships next to them in `assets/themes/CATPPUCCIN-LICENSE`.
pub const MACCHIATO_TMTHEME: &[u8] =
    include_bytes!("../../assets/themes/catppuccin-macchiato.tmTheme");
pub const LATTE_TMTHEME: &[u8] = include_bytes!("../../assets/themes/catppuccin-latte.tmTheme");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tints_are_blended_over_the_base_colour() {
        assert_eq!(
            MACCHIATO.colors.added_bg,
            blend(hex(0xa6da95), hex(0x24273a), 20)
        );
        assert_ne!(MACCHIATO.colors.added_bg, MACCHIATO_PALETTE.green);
        assert_eq!(LATTE.colors.border_active, hex(0x7287fd));
    }
}
