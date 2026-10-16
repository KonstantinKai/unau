//! Catppuccin Macchiato and Latte, from the official palette, with colours
//! placed the way Catppuccin's style guide places them.

use super::{
    Syntax, Theme, hex,
    palette::{Palette, colors},
};
use std::borrow::Cow;

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
    use crate::theme::blend;

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
