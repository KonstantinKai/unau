//! Values of the `[ui]` section.

use serde::Deserialize;

/// Who paints behind the text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Background {
    /// The theme's own `background`, so any theme reads right on any terminal.
    #[default]
    Theme,
    /// The terminal's, transparency included.
    Terminal,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineNumbers {
    #[default]
    Both,
    New,
}

/// How a pane that scrolls shows that more is out of view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScrollStyle {
    /// A thumb on the pane's right border, as long as the share in view.
    #[default]
    Scrollbar,
    /// `↑3 ↓12` on the pane's bottom border: lines out of view above and below.
    Hint,
}

#[cfg(test)]
mod tests {
    use super::super::Config;
    use super::ScrollStyle;

    #[test]
    fn the_scroll_style_is_a_scrollbar_unless_set_to_hints() {
        let mut config = Config::default();
        assert_eq!(config.scroll, ScrollStyle::Scrollbar);
        config.merge("[ui]\nscroll = \"hint\"\n").unwrap();
        assert_eq!(config.scroll, ScrollStyle::Hint);
        assert!(config.merge("[ui]\nscroll = \"arrows\"\n").is_err());
    }
}
