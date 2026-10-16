//! Colour themes. Every colour the interface draws is one of the slots of
//! [`Colors`]; a theme fills them and names its syntax colours.

// Colours are written as `0xrrggbb` to match the `#rrggbb` codes they come from;
// digit separators would hide that.
#![allow(clippy::unreadable_literal)]

mod catppuccin;
mod families;
mod palette;
mod user;

pub use catppuccin::{
    LATTE as CATPPUCCIN_LATTE, LATTE_TMTHEME, MACCHIATO as CATPPUCCIN_MACCHIATO, MACCHIATO_TMTHEME,
};
pub use user::{resolve, user_theme_names};

use anyhow::{Result, bail};
use ratatui::style::Color;
use std::{borrow::Cow, sync::Arc};

/// Where a theme's syntax colours come from.
#[derive(Clone, Debug, PartialEq)]
pub enum Syntax {
    CatppuccinMacchiato,
    CatppuccinLatte,
    /// One of syntect's built-in themes, by name.
    Builtin(Cow<'static, str>),
    /// One of bat's syntax themes, embedded by two-face.
    Embedded(two_face::theme::EmbeddedThemeName),
    /// A `.tmTheme` bundled in `assets/themes/`.
    Vendored(Vendored),
    /// A `.tmTheme` from a user theme, parsed at startup.
    Loaded(Arc<syntect::highlighting::Theme>),
}

/// Syntax themes bundled as files, each with its license beside it in
/// `assets/themes/`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vendored {
    TokyoNightNight,
    TokyoNightDay,
    RosePineMain,
    RosePineDawn,
}

impl Vendored {
    pub fn tm_theme(self) -> &'static [u8] {
        match self {
            Self::TokyoNightNight => include_bytes!("../../assets/themes/tokyonight-night.tmTheme"),
            Self::TokyoNightDay => include_bytes!("../../assets/themes/tokyonight-day.tmTheme"),
            Self::RosePineMain => include_bytes!("../../assets/themes/rosepine-main.tmTheme"),
            Self::RosePineDawn => include_bytes!("../../assets/themes/rosepine-dawn.tmTheme"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colors {
    /// What the whole screen is filled with, unless the terminal's own
    /// background is asked for.
    pub background: Color,
    pub text: Color,
    pub muted: Color,
    pub subtle: Color,
    pub border_active: Color,
    pub border_inactive: Color,
    pub cursor_bg: Color,
    pub selection_bg: Color,
    pub note_bg: Color,
    pub added: Color,
    pub removed: Color,
    pub added_bg: Color,
    pub removed_bg: Color,
    pub hunk: Color,
    pub fix: Color,
    pub question: Color,
    pub suggestion: Color,
    pub done: Color,
    pub error: Color,
    pub warning: Color,
    pub info: Color,
}

macro_rules! slots {
    ($($slot:ident),* $(,)?) => {
        /// Slot names as written in `[theme.colors]`.
        pub const SLOTS: &[&str] = &[$(stringify!($slot)),*];

        impl Colors {
            /// Sets the slot called `name`.
            pub fn set(&mut self, name: &str, color: Color) -> Result<()> {
                match name {
                    $(stringify!($slot) => self.$slot = color,)*
                    _ => bail!(
                        "unknown colour slot in [theme.colors]: {name}; slots: {}",
                        SLOTS.join(", ")
                    ),
                }
                Ok(())
            }
        }
    };
}

slots!(
    background,
    text,
    muted,
    subtle,
    border_active,
    border_inactive,
    cursor_bg,
    selection_bg,
    note_bg,
    added,
    removed,
    added_bg,
    removed_bg,
    hunk,
    fix,
    question,
    suggestion,
    done,
    error,
    warning,
    info,
);

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub name: Cow<'static, str>,
    pub colors: Colors,
    pub syntax: Syntax,
}

impl Default for Theme {
    fn default() -> Self {
        catppuccin::MACCHIATO
    }
}

/// Every built-in theme, by its `<theme>.<flavor>` name.
pub const BUILTIN: &[Theme] = &[
    catppuccin::MACCHIATO,
    catppuccin::LATTE,
    DEFAULT_DARK,
    DEFAULT_LIGHT,
    families::GRUVBOX_DARK_THEME,
    families::GRUVBOX_LIGHT_THEME,
    families::DRACULA_THEME,
    families::NORD_THEME,
    families::SOLARIZED_DARK_THEME,
    families::SOLARIZED_LIGHT_THEME,
    families::ONEHALF_DARK_THEME,
    families::ONEHALF_LIGHT_THEME,
    families::TOKYONIGHT_NIGHT_THEME,
    families::TOKYONIGHT_DAY_THEME,
    families::ROSEPINE_MAIN_THEME,
    families::ROSEPINE_DAWN_THEME,
];

impl Theme {
    /// Dark when its text is lighter than mid grey: light text on a dark
    /// background.
    pub fn is_dark(&self) -> bool {
        let Color::Rgb(r, g, b) = self.colors.text else {
            return true;
        };
        // Relative luminance weights, in thousandths.
        u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114 > 128_000
    }
}

/// A built-in theme by name, ignoring user themes.
pub fn by_name(name: &str) -> Result<Theme> {
    if let Some(theme) = BUILTIN.iter().find(|t| t.name == name) {
        return Ok(theme.clone());
    }
    let known: Vec<&str> = BUILTIN.iter().map(|t| t.name.as_ref()).collect();
    bail!("unknown theme {name}; built in: {}", known.join(", "))
}

/// `#rrggbb` to a colour.
pub fn parse_hex(text: &str) -> Result<Color> {
    let hex = text.strip_prefix('#').unwrap_or_default();
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("not a #rrggbb colour: {text}");
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0);
    Ok(Color::Rgb(byte(0), byte(2), byte(4)))
}

/// `#rrggbb` at compile time, for palette tables.
pub const fn hex(value: u32) -> Color {
    let [_, r, g, b] = value.to_be_bytes();
    Color::Rgb(r, g, b)
}

/// `top` laid over `base` at `percent` opacity. Terminals have no
/// transparency, so tinted backgrounds are mixed ahead of time.
pub const fn blend(top: Color, base: Color, percent: u16) -> Color {
    let (Color::Rgb(tr, tg, tb), Color::Rgb(br, bg, bb)) = (top, base) else {
        return top;
    };
    Color::Rgb(
        mix(tr, br, percent),
        mix(tg, bg, percent),
        mix(tb, bb, percent),
    )
}

// A weighted mean of two bytes is itself at most 255, so the narrowing is lossless.
#[allow(clippy::cast_possible_truncation)]
const fn mix(top: u8, base: u8, percent: u16) -> u8 {
    let percent = if percent > 100 { 100 } else { percent };
    ((top as u16 * percent + base as u16 * (100 - percent) + 50) / 100) as u8
}

/// The colours unau shipped with before themes existed.
pub const DEFAULT_DARK: Theme = Theme {
    name: Cow::Borrowed("default.dark"),
    syntax: Syntax::Builtin(Cow::Borrowed("base16-ocean.dark")),
    colors: Colors {
        background: hex(0x1c2028),
        text: hex(0xdcdcdc),
        muted: hex(0x787878),
        subtle: hex(0x9a9a9a),
        border_active: hex(0x50c8c8),
        border_inactive: hex(0x787878),
        cursor_bg: hex(0x464e64),
        selection_bg: hex(0x373e50),
        note_bg: hex(0x232830),
        added: hex(0x78c878),
        removed: hex(0xdc6e6e),
        added_bg: hex(0x142d1e),
        removed_bg: hex(0x37191e),
        hunk: hex(0xe6be5a),
        fix: hex(0xe6be5a),
        question: hex(0x78aae6),
        suggestion: hex(0xbe8ce6),
        done: hex(0x78c878),
        error: hex(0xf06464),
        warning: hex(0xe6be5a),
        info: hex(0x50c8c8),
    },
};

pub const DEFAULT_LIGHT: Theme = Theme {
    name: Cow::Borrowed("default.light"),
    syntax: Syntax::Builtin(Cow::Borrowed("InspiredGitHub")),
    colors: Colors {
        background: hex(0xffffff),
        text: hex(0x1e1e1e),
        muted: hex(0x828282),
        subtle: hex(0x5a5a5a),
        border_active: hex(0x00788c),
        border_inactive: hex(0x828282),
        cursor_bg: hex(0xbecdeb),
        selection_bg: hex(0xd2dcf0),
        note_bg: hex(0xeef0f4),
        added: hex(0x1e7828),
        removed: hex(0xaa2828),
        added_bg: hex(0xdcf5e1),
        removed_bg: hex(0xfae1e1),
        hunk: hex(0xa06e00),
        fix: hex(0xa06e00),
        question: hex(0x1e5aaa),
        suggestion: hex(0x783caa),
        done: hex(0x1e7828),
        error: hex(0xbe1e1e),
        warning: hex(0xa06e00),
        info: hex(0x00788c),
    },
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_theme_resolves_by_name() {
        for theme in BUILTIN {
            assert_eq!(&by_name(&theme.name).unwrap(), theme);
        }
        let err = by_name("catppuccin.espresso").unwrap_err().to_string();
        assert!(err.contains("catppuccin.espresso") && err.contains("catppuccin.latte"));
    }

    #[test]
    fn each_builtin_theme_knows_whether_it_is_dark() {
        let dark: Vec<&str> = BUILTIN
            .iter()
            .filter(|t| t.is_dark())
            .map(|t| t.name.as_ref())
            .collect();
        assert_eq!(
            dark,
            [
                "catppuccin.macchiato",
                "default.dark",
                "gruvbox.dark",
                "dracula",
                "nord",
                "solarized.dark",
                "onehalf.dark",
                "tokyonight.night",
                "rosepine.main",
            ]
        );
    }

    #[test]
    fn every_builtin_background_matches_its_kind() {
        let lightness = |c: Color| match c {
            Color::Rgb(r, g, b) => u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114,
            _ => 0,
        };
        for theme in BUILTIN {
            let dark_ground = lightness(theme.colors.background) < 128_000;
            assert_eq!(dark_ground, theme.is_dark(), "{}", theme.name);
        }
        assert_eq!(CATPPUCCIN_LATTE.colors.background, hex(0xeff1f5));
    }

    #[test]
    fn hex_parses_only_six_digit_colours() {
        assert_eq!(parse_hex("#b7bdf8").unwrap(), Color::Rgb(0xb7, 0xbd, 0xf8));
        for bad in ["b7bdf8", "#b7bdf", "#zzzzzz", "#b7bdf8a"] {
            assert!(parse_hex(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn blending_mixes_toward_the_base() {
        let base = hex(0x000000);
        assert_eq!(blend(hex(0xffffff), base, 20), Color::Rgb(51, 51, 51));
        assert_eq!(blend(hex(0xffffff), base, 100), hex(0xffffff));
    }

    #[test]
    fn every_slot_name_can_be_set_and_unknown_ones_fail() {
        let mut colors = Theme::default().colors;
        for slot in SLOTS {
            colors.set(slot, hex(0x010203)).unwrap();
        }
        assert_eq!(colors.text, hex(0x010203));
        assert!(
            colors
                .set("sidebar", hex(0))
                .unwrap_err()
                .to_string()
                .contains("sidebar")
        );
    }
}
