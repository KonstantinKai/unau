//! The starter config written the first time the user opens their config
//! from inside the tool.

use super::{Action, Config};
use crate::review::DEFAULT_QUOTE_LINES;
use std::fmt::Write as _;

/// A config file with every setting at its default, commented out.
pub fn template() -> String {
    let defaults = Config::default();
    let mut text = String::from(
        "# revmark configuration. Every setting is listed at its default, commented out.\n\
         # Uncomment what you want to change; revmark picks the file up when it is saved.\n\
         \n[keys]\n",
    );
    for action in Action::ALL {
        let keys: Vec<String> = defaults
            .keys_for(*action)
            .into_iter()
            .map(|k| format!("{k:?}"))
            .collect();
        let value = match keys.as_slice() {
            [one] => one.clone(),
            many => format!("[{}]", many.join(", ")),
        };
        let _ = writeln!(text, "# {} = {value}", action.name());
    }
    text.push_str(
        "\n[run]\n\
         # command = \"claude 'fix every open item in {file}'\"\n\
         # in = \"auto\"                    # auto | herdr | tmux | pane | foreground\n\
         # output = \"plain\"               # plain | claude-stream (run pane; command needs -p --output-format stream-json --verbose)\n\
         \n[ui]\n\
         # line_numbers = \"both\"          # both | new\n\
         # confirm_quit = true             # Q asks before quitting\n\
         \n[review]\n",
    );
    let _ = writeln!(
        text,
        "# quote_lines = {DEFAULT_QUOTE_LINES}               # longer quotes keep their first & last lines; 0 = never cap"
    );
    text.push_str(
        "# dir = \".revmark\"               # where reviews go: relative to the repo, ~/…, or absolute; {repo} = repo folder name\n",
    );
    text.push_str("\n[theme]\n# mode = \"auto\"                  # auto | light | dark\n");
    let _ = writeln!(text, "# dark = \"{}\"", defaults.dark_name);
    let _ = writeln!(text, "# light = \"{}\"", defaults.light_name);
    text.push_str("\n[theme.colors]                  # any slot, on top of the active theme\n");
    let _ = writeln!(text, "# border_active = \"#b7bdf8\"");
    text
}
