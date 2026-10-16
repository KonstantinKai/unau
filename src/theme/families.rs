//! Built-in families beyond Catppuccin: Gruvbox, Dracula, Nord, Solarized,
//! One Half, Tokyo Night and Rosé Pine, each from its official palette. A
//! role a family has no colour for is blended from its own colours.

use super::{
    Syntax, Theme, Vendored, blend, hex,
    palette::{Palette, colors},
};
use std::borrow::Cow;
use two_face::theme::EmbeddedThemeName;

const fn theme(name: &'static str, palette: &Palette, syntax: Syntax) -> Theme {
    Theme {
        name: Cow::Borrowed(name),
        colors: colors(palette),
        syntax,
    }
}

const GRUVBOX_DARK: Palette = Palette {
    mauve: hex(0xd3869b),
    red: hex(0xfb4934),
    peach: hex(0xfe8019),
    yellow: hex(0xfabd2f),
    green: hex(0xb8bb26),
    teal: hex(0x8ec07c),
    blue: hex(0x83a598),
    lavender: hex(0x83a598),
    text: hex(0xebdbb2),
    subtext0: hex(0xbdae93),
    overlay2: hex(0xa89984),
    overlay1: hex(0x928374),
    overlay0: hex(0x7c6f64),
    surface1: hex(0x504945),
    base: hex(0x282828),
    mantle: hex(0x1d2021),
};

const GRUVBOX_LIGHT: Palette = Palette {
    mauve: hex(0x8f3f71),
    red: hex(0x9d0006),
    peach: hex(0xaf3a03),
    yellow: hex(0xb57614),
    green: hex(0x79740e),
    teal: hex(0x427b58),
    blue: hex(0x076678),
    lavender: hex(0x076678),
    text: hex(0x3c3836),
    subtext0: hex(0x665c54),
    overlay2: hex(0x7c6f64),
    overlay1: hex(0x928374),
    overlay0: hex(0xa89984),
    surface1: hex(0xd5c4a1),
    base: hex(0xfbf1c7),
    mantle: hex(0xf2e5bc),
};

const DRACULA: Palette = Palette {
    mauve: hex(0xff79c6),
    red: hex(0xff5555),
    peach: hex(0xffb86c),
    yellow: hex(0xf1fa8c),
    green: hex(0x50fa7b),
    teal: hex(0x8be9fd),
    blue: hex(0x8be9fd),
    lavender: hex(0xbd93f9),
    text: hex(0xf8f8f2),
    subtext0: blend(hex(0xf8f8f2), hex(0x282a36), 75),
    overlay2: hex(0x6272a4),
    overlay1: hex(0x6272a4),
    overlay0: hex(0x6272a4),
    surface1: hex(0x44475a),
    base: hex(0x282a36),
    mantle: hex(0x21222c),
};

const NORD: Palette = Palette {
    mauve: hex(0xb48ead),
    red: hex(0xbf616a),
    peach: hex(0xd08770),
    yellow: hex(0xebcb8b),
    green: hex(0xa3be8c),
    teal: hex(0x8fbcbb),
    blue: hex(0x81a1c1),
    lavender: hex(0x88c0d0),
    text: hex(0xd8dee9),
    subtext0: blend(hex(0xd8dee9), hex(0x2e3440), 75),
    overlay2: hex(0xd8dee9),
    overlay1: hex(0x616e88),
    overlay0: hex(0x4c566a),
    surface1: hex(0x434c5e),
    base: hex(0x2e3440),
    mantle: hex(0x3b4252),
};

const SOLARIZED_DARK: Palette = Palette {
    mauve: hex(0x6c71c4),
    red: hex(0xdc322f),
    peach: hex(0xcb4b16),
    yellow: hex(0xb58900),
    green: hex(0x859900),
    teal: hex(0x2aa198),
    blue: hex(0x268bd2),
    lavender: hex(0x268bd2),
    text: hex(0x93a1a1),
    subtext0: hex(0x839496),
    overlay2: hex(0x657b83),
    overlay1: hex(0x586e75),
    overlay0: hex(0x586e75),
    surface1: blend(hex(0x586e75), hex(0x002b36), 40),
    base: hex(0x002b36),
    mantle: hex(0x073642),
};

const SOLARIZED_LIGHT: Palette = Palette {
    mauve: hex(0x6c71c4),
    red: hex(0xdc322f),
    peach: hex(0xcb4b16),
    yellow: hex(0xb58900),
    green: hex(0x859900),
    teal: hex(0x2aa198),
    blue: hex(0x268bd2),
    lavender: hex(0x268bd2),
    text: hex(0x586e75),
    subtext0: hex(0x657b83),
    overlay2: hex(0x839496),
    overlay1: hex(0x93a1a1),
    overlay0: hex(0x93a1a1),
    surface1: blend(hex(0x93a1a1), hex(0xfdf6e3), 35),
    base: hex(0xfdf6e3),
    mantle: hex(0xeee8d5),
};

const ONEHALF_DARK: Palette = Palette {
    mauve: hex(0xc678dd),
    red: hex(0xe06c75),
    peach: hex(0xd19a66),
    yellow: hex(0xe5c07b),
    green: hex(0x98c379),
    teal: hex(0x56b6c2),
    blue: hex(0x61afef),
    lavender: hex(0x61afef),
    text: hex(0xdcdfe4),
    subtext0: hex(0xabb2bf),
    overlay2: hex(0x919baa),
    overlay1: hex(0x919baa),
    overlay0: hex(0x5c6370),
    surface1: hex(0x474e5d),
    base: hex(0x282c34),
    mantle: hex(0x21252b),
};

const ONEHALF_LIGHT: Palette = Palette {
    mauve: hex(0xa626a4),
    red: hex(0xe45649),
    peach: hex(0x986801),
    yellow: hex(0xc18401),
    green: hex(0x50a14f),
    teal: hex(0x0997b3),
    blue: hex(0x0184bc),
    lavender: hex(0x0184bc),
    text: hex(0x383a42),
    subtext0: hex(0x696c77),
    overlay2: hex(0x696c77),
    overlay1: hex(0x9d9d9f),
    overlay0: hex(0xa0a1a7),
    surface1: hex(0xe5e5e6),
    base: hex(0xfafafa),
    mantle: hex(0xf0f0f0),
};

const TOKYONIGHT_NIGHT: Palette = Palette {
    mauve: hex(0xbb9af7),
    red: hex(0xf7768e),
    peach: hex(0xff9e64),
    yellow: hex(0xe0af68),
    green: hex(0x9ece6a),
    teal: hex(0x7dcfff),
    blue: hex(0x7aa2f7),
    lavender: hex(0x7aa2f7),
    text: hex(0xc0caf5),
    subtext0: hex(0xa9b1d6),
    overlay2: hex(0x737aa2),
    overlay1: hex(0x565f89),
    overlay0: hex(0x414868),
    surface1: hex(0x292e42),
    base: hex(0x1a1b26),
    mantle: hex(0x16161e),
};

const TOKYONIGHT_DAY: Palette = Palette {
    mauve: hex(0x9854f1),
    red: hex(0xf52a65),
    peach: hex(0xb15c00),
    yellow: hex(0x8c6c3e),
    green: hex(0x587539),
    teal: hex(0x007197),
    blue: hex(0x2e7de9),
    lavender: hex(0x2e7de9),
    text: hex(0x3760bf),
    subtext0: hex(0x6172b0),
    overlay2: hex(0x68709a),
    overlay1: hex(0x848cb5),
    overlay0: hex(0xa1a6c5),
    surface1: hex(0xc4c8da),
    base: hex(0xe1e2e7),
    mantle: hex(0xd0d5e3),
};

const ROSEPINE_MAIN: Palette = Palette {
    mauve: hex(0xebbcba),
    red: hex(0xeb6f92),
    peach: hex(0xebbcba),
    yellow: hex(0xf6c177),
    green: hex(0x9ccfd8),
    teal: hex(0x9ccfd8),
    blue: hex(0xc4a7e7),
    lavender: hex(0xc4a7e7),
    text: hex(0xe0def4),
    subtext0: hex(0x908caa),
    overlay2: hex(0x908caa),
    overlay1: hex(0x6e6a86),
    overlay0: hex(0x524f67),
    surface1: hex(0x403d52),
    base: hex(0x191724),
    mantle: hex(0x1f1d2e),
};

const ROSEPINE_DAWN: Palette = Palette {
    mauve: hex(0x907aa9),
    red: hex(0xb4637a),
    peach: hex(0xd7827e),
    yellow: hex(0xea9d34),
    green: hex(0x56949f),
    teal: hex(0x56949f),
    blue: hex(0x286983),
    lavender: hex(0x907aa9),
    text: hex(0x575279),
    subtext0: hex(0x797593),
    overlay2: hex(0x797593),
    overlay1: hex(0x9893a5),
    overlay0: hex(0xcecacd),
    surface1: hex(0xdfdad9),
    base: hex(0xfaf4ed),
    mantle: hex(0xf2e9e1),
};

pub const GRUVBOX_DARK_THEME: Theme = theme(
    "gruvbox.dark",
    &GRUVBOX_DARK,
    Syntax::Embedded(EmbeddedThemeName::GruvboxDark),
);
pub const GRUVBOX_LIGHT_THEME: Theme = theme(
    "gruvbox.light",
    &GRUVBOX_LIGHT,
    Syntax::Embedded(EmbeddedThemeName::GruvboxLight),
);
pub const DRACULA_THEME: Theme = theme(
    "dracula",
    &DRACULA,
    Syntax::Embedded(EmbeddedThemeName::Dracula),
);
pub const NORD_THEME: Theme = theme("nord", &NORD, Syntax::Embedded(EmbeddedThemeName::Nord));
pub const SOLARIZED_DARK_THEME: Theme = theme(
    "solarized.dark",
    &SOLARIZED_DARK,
    Syntax::Embedded(EmbeddedThemeName::SolarizedDark),
);
pub const SOLARIZED_LIGHT_THEME: Theme = theme(
    "solarized.light",
    &SOLARIZED_LIGHT,
    Syntax::Embedded(EmbeddedThemeName::SolarizedLight),
);
pub const ONEHALF_DARK_THEME: Theme = theme(
    "onehalf.dark",
    &ONEHALF_DARK,
    Syntax::Embedded(EmbeddedThemeName::OneHalfDark),
);
pub const ONEHALF_LIGHT_THEME: Theme = theme(
    "onehalf.light",
    &ONEHALF_LIGHT,
    Syntax::Embedded(EmbeddedThemeName::OneHalfLight),
);
pub const TOKYONIGHT_NIGHT_THEME: Theme = theme(
    "tokyonight.night",
    &TOKYONIGHT_NIGHT,
    Syntax::Vendored(Vendored::TokyoNightNight),
);
pub const TOKYONIGHT_DAY_THEME: Theme = theme(
    "tokyonight.day",
    &TOKYONIGHT_DAY,
    Syntax::Vendored(Vendored::TokyoNightDay),
);
pub const ROSEPINE_MAIN_THEME: Theme = theme(
    "rosepine.main",
    &ROSEPINE_MAIN,
    Syntax::Vendored(Vendored::RosePineMain),
);
pub const ROSEPINE_DAWN_THEME: Theme = theme(
    "rosepine.dawn",
    &ROSEPINE_DAWN,
    Syntax::Vendored(Vendored::RosePineDawn),
);
