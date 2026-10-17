//! The settings file the "Edit Command Palette settings" row writes on
//! first use: every key, commented out, at its default, with what it does.
//!
//! The values come from [`Settings::default()`], except those whose
//! default is a fixed word or empty (the orders, `hide_*`, the colors).
//! Either way they can't drift from the real defaults: a test uncomments
//! every line and checks the file parses to [`Settings::default()`].
//! Section headers stay uncommented (empty sections are valid), so
//! changing a value is uncommenting one line.

use std::fmt::Write as _;

use crate::domain::row::RowId;

use super::Settings;

/// The commented default `settings.toml`.
#[must_use]
pub fn default_file() -> String {
    let defaults = Settings::default();
    let commands = &defaults.commands;
    let popup = &defaults.popup;
    let history = &defaults.history;
    let confirm_ids = quoted_ids(&commands.confirm_ids);
    let skip_recent_ids = quoted_ids(&commands.skip_recent_ids);
    let mut text = String::new();
    // Writing to a String can't fail.
    let _ = write!(
        text,
        r##"# Command Palette settings (herdr-command-palette).
#
# Every key is optional: the commented values are the defaults. Uncomment
# a line to change it. Unknown keys and invalid values are errors, shown
# by the palette instead of opening. Check the file with:
#   herdr-command-palette settings check

# Editor for "Edit Herdr config" and "Edit Command Palette settings".
# Empty: $EDITOR, then vi. May include arguments ("code -w").
# editor = {editor}

# Category the palette opens on: "commands" or "jump_to".
# open_on = {open_on}

[commands]
# "recent": most recently run first, then the fixed order.
# "fixed": plugins, built-ins, palette commands, custom commands.
# order = "recent"

# Hide whole row kinds: any of "plugin", "builtin", "palette", "custom".
# hide_kinds = []

# Hide single rows by id: "<kind>:<name>", e.g. "builtin:zoom" or
# "custom:Popup: lazygit" (a custom command's description, else its
# command, else its type). `herdr-command-palette list --ids` prints
# every row's id.
# hide_ids = []

# Ask before running these rows (by id): Enter runs, Esc cancels.
# confirm_ids = [{confirm_ids}]

# Keep these rows (by id) in their fixed place in the "recent" order; they
# are still recorded, so repeat-command repeats them.
# skip_recent_ids = [{skip_recent_ids}]

[jump_to]
# "recent": most recently focused first (current pane last), then herdr's order.
# "herdr": herdr's order (workspace, tab, pane).
# order = "recent"

[popup]
# Size in cells, computed when the palette opens: the max, shrunk to keep
# a 2-column / 1-row margin on small screens, never below the min unless
# the screen itself is smaller. Values below these defaults are raised to
# them; min at most max.
# min_width = {min_width}
# max_width = {max_width}
# min_height = {min_height}
# max_height = {max_height}

[colors]
# Each: "#rrggbb", an ANSI color number 0-255 (no quotes), or "default"
# (the terminal's color). Empty: derived from herdr's theme, as below.
#
# Popup and error screen; default herdr's panel_bg. Keep it equal to
# herdr's panel color, or the column herdr reserves right of the popup shows.
# background = ""
# Footer, shortcuts, inactive categories; default herdr's overlay0 for the
# footer, dim for the rest.
# muted = ""
# Match highlight (underlined; bold in the selected row); default the
# row's color.
# matches = ""
# Selected row text color (the row stays bold); default herdr's text.
# selected = ""
# Lines around the input and the footer; default dim.
# dividers = ""
# Footer tag label ("plugin", "palette", "custom"); default herdr's subtext0.
# tags = ""
# Git branch in the Jump to footer (needs git 2.22 or newer); default
# herdr's mauve.
# branch = ""

[history]
# false: nothing is recorded, recent orders fall back to "fixed" / "herdr",
# and the repeat actions say history is off.
# enabled = {enabled}
# Forget history older than this many days (checked at most hourly, after
# the palette opens); 0 keeps everything.
# max_age_days = {max_age_days}
"##,
        editor = quote(&defaults.editor),
        open_on = quote(defaults.open_on.as_str()),
        min_width = popup.min_width,
        max_width = popup.max_width,
        min_height = popup.min_height,
        max_height = popup.max_height,
        enabled = history.enabled,
        max_age_days = history.max_age_days,
    );
    text
}

/// `ids` as the items of a TOML array of strings: `"a", "b"`.
fn quoted_ids(ids: &[RowId]) -> String {
    ids.iter()
        .map(|id| quote(&id.to_string()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `text` as a TOML basic string. The defaults are plain ASCII, where
/// Rust's `Debug` quoting and TOML's agree.
fn quote(text: &str) -> String {
    format!("{text:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The file with every `# key = value` line uncommented.
    fn uncommented(text: &str) -> String {
        text.lines()
            .map(|line| match line.strip_prefix("# ") {
                Some(setting) if setting.contains(" = ") => setting,
                _ => line,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn snapshot_default_file() {
        insta::assert_snapshot!(default_file());
    }

    #[test]
    fn is_valid_and_all_defaults_as_written() {
        assert_eq!(
            Settings::from_toml(&default_file()),
            Ok(Settings::default())
        );
    }

    #[test]
    fn its_commented_values_are_the_defaults() {
        let text = uncommented(&default_file());

        assert!(text.contains("\nmax_height = 22\n"), "{text}");
        assert_eq!(Settings::from_toml(&text), Ok(Settings::default()));
    }

    #[test]
    fn comments_every_key() {
        let text = uncommented(&default_file());
        let keys = text
            .lines()
            .filter(|line| line.contains(" = ") && !line.starts_with('#'))
            .count();

        // editor, open_on, 5 commands, 1 jump_to, 4 popup, 7 colors, 2 history.
        assert_eq!(keys, 21);
    }
}
