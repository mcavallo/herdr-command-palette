//! Parsers for the settings values serde can't read on its own: colors,
//! command row kinds and row ids. Each error names the accepted values;
//! the caller adds the key and line.

use std::fmt;

use serde::Deserialize;
use serde::de::{self, Deserializer, Visitor};

use crate::domain::catalog::NamedCommand;
use crate::domain::catalog::builtins::BUILTINS;
use crate::domain::catalog::palette::PALETTE_COMMANDS;
use crate::domain::color::{self, Color};
use crate::domain::row::{Kind, RowId};

/// The kinds `hide_kinds`, `hide_ids`, `confirm_ids` and `skip_recent_ids`
/// accept: every kind of the Commands category.
const COMMAND_KINDS: &str = "`plugin`, `builtin`, `palette` or `custom`";

/// A `[colors]` value: `"#rrggbb"`, an ANSI number 0-255, `"default"`
/// ([`Color::Reset`]) or `""` (`None`: keep the derived color).
pub(super) fn color<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Color>, D::Error> {
    deserializer.deserialize_any(ColorVisitor)
}

/// Reads a color from a string or an integer.
struct ColorVisitor;

impl Visitor<'_> for ColorVisitor {
    type Value = Option<Color>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a color: \"#rrggbb\", a number 0-255, \"default\" or \"\"")
    }

    fn visit_i64<E: de::Error>(self, number: i64) -> Result<Self::Value, E> {
        u8::try_from(number)
            .map(|index| Some(Color::Ansi(index)))
            .map_err(|_| E::custom(format!("color `{number}` is not a number 0-255")))
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<Self::Value, E> {
        match text {
            "" => Ok(None),
            "default" => Ok(Some(Color::Reset)),
            _ => hex(text).map(Some).ok_or_else(|| {
                E::custom(format!(
                    "unknown color `{text}`, expected `#rrggbb`, a number 0-255, `default` or `\"\"`"
                ))
            }),
        }
    }
}

/// `#rrggbb` (any case) as a color; `None` for anything else.
fn hex(text: &str) -> Option<Color> {
    // `parse_hex` also takes `rgb`; settings ask for all six digits.
    let digits = text.strip_prefix('#')?;
    if digits.len() != 6 {
        return None;
    }
    color::parse_hex(digits)
}

/// `hide_kinds`: a list of command row kinds.
pub(super) fn command_kinds<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Kind>, D::Error> {
    Vec::<String>::deserialize(deserializer)?
        .iter()
        .map(|name| command_kind(name).map_err(de::Error::custom))
        .collect()
}

/// `hide_ids`, `confirm_ids`, `skip_recent_ids`: a list of command row ids.
pub(super) fn row_ids<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<RowId>, D::Error> {
    Vec::<String>::deserialize(deserializer)?
        .iter()
        .map(|id| row_id(id).map_err(de::Error::custom))
        .collect()
}

/// A kind of the Commands category.
fn command_kind(name: &str) -> Result<Kind, String> {
    match name.parse::<Kind>() {
        Ok(kind) if kind != Kind::Pane => Ok(kind),
        _ => Err(format!(
            "unknown row kind `{name}`, expected {COMMAND_KINDS}"
        )),
    }
}

/// A `<kind>:<name>` command row id. Built-in and palette command names
/// must exist; plugin and custom names can't be checked (they depend on
/// what is installed and on Herdr's config).
fn row_id(id: &str) -> Result<RowId, String> {
    let Some((kind, name)) = id.split_once(':') else {
        return Err(format!(
            "row id `{id}` is not `<kind>:<name>` (see `herdr-command-palette list --ids`)"
        ));
    };
    let kind = command_kind(kind).map_err(|error| format!("row id `{id}`: {error}"))?;
    let known = match kind {
        Kind::Builtin => NamedCommand::find(&BUILTINS, name).is_some(),
        Kind::Palette => NamedCommand::find(&PALETTE_COMMANDS, name).is_some(),
        Kind::Plugin | Kind::Custom | Kind::Pane => !name.is_empty(),
    };
    if !known {
        return Err(format!(
            "unknown row id `{id}` (see `herdr-command-palette list --ids`)"
        ));
    }
    Ok(RowId::new(kind, name))
}

#[cfg(test)]
mod tests {
    use crate::domain::settings::Settings;

    use super::*;

    /// The message for `[colors] muted = <value>`.
    fn color_problem(value: &str) -> String {
        Settings::from_toml(&format!("[colors]\nmuted = {value}\n"))
            .unwrap_err()
            .message
    }

    /// The message for `[commands] <key> = <value>`.
    fn commands_problem(key: &str, value: &str) -> String {
        Settings::from_toml(&format!("[commands]\n{key} = {value}\n"))
            .unwrap_err()
            .message
    }

    #[test]
    fn colors_are_hex_numbers_default_or_empty() {
        assert_eq!(hex("#00ff7F"), Some(Color::Rgb(0, 255, 0x7f)));
        let parsed = Settings::from_toml(
            "[colors]\nmuted = 0\nmatches = 255\nbackground = \"default\"\ntags = \"\"\n",
        )
        .unwrap()
        .colors;
        assert_eq!(parsed.muted, Some(Color::Ansi(0)));
        assert_eq!(parsed.matches, Some(Color::Ansi(255)));
        assert_eq!(parsed.background, Some(Color::Reset));
        assert_eq!(parsed.tags, None);
    }

    #[test]
    fn rejects_colors_that_are_not_rrggbb() {
        for value in ["#fff", "#12345g", "red", "rgb(1, 2, 3)", "#日本語日本"] {
            assert_eq!(
                color_problem(&format!("\"{value}\"")),
                format!(
                    "unknown color `{value}`, expected `#rrggbb`, a number 0-255, `default` or `\"\"`"
                )
            );
        }
    }

    #[test]
    fn rejects_numbers_outside_0_to_255() {
        assert_eq!(color_problem("256"), "color `256` is not a number 0-255");
        assert_eq!(color_problem("-1"), "color `-1` is not a number 0-255");
    }

    #[test]
    fn rejects_other_types_of_colors() {
        assert_eq!(
            color_problem("true"),
            "invalid type: boolean `true`, expected a color: \"#rrggbb\", a number 0-255, \
             \"default\" or \"\""
        );
    }

    #[test]
    fn hide_kinds_take_command_kinds_only() {
        assert_eq!(
            commands_problem("hide_kinds", "[\"pane\"]"),
            "unknown row kind `pane`, expected `plugin`, `builtin`, `palette` or `custom`"
        );
        assert_eq!(
            commands_problem("hide_kinds", "[\"plugins\"]"),
            "unknown row kind `plugins`, expected `plugin`, `builtin`, `palette` or `custom`"
        );
        assert_eq!(
            commands_problem("hide_kinds", "\"plugin\""),
            "invalid type: string \"plugin\", expected a sequence"
        );
    }

    #[test]
    fn row_ids_need_a_kind_and_a_name() {
        assert_eq!(
            commands_problem("hide_ids", "[\"zoom\"]"),
            "row id `zoom` is not `<kind>:<name>` (see `herdr-command-palette list --ids`)"
        );
        assert_eq!(
            commands_problem("confirm_ids", "[\"pane:w1:p1\"]"),
            "row id `pane:w1:p1`: unknown row kind `pane`, expected `plugin`, `builtin`, \
             `palette` or `custom`"
        );
        assert_eq!(
            commands_problem("hide_ids", "[\"custom:\"]"),
            "unknown row id `custom:` (see `herdr-command-palette list --ids`)"
        );
    }

    #[test]
    fn builtin_and_palette_ids_must_exist() {
        assert_eq!(
            commands_problem("confirm_ids", "[\"builtin:close_everything\"]"),
            "unknown row id `builtin:close_everything` (see `herdr-command-palette list --ids`)"
        );
        assert_eq!(
            commands_problem("hide_ids", "[\"palette:teleport\"]"),
            "unknown row id `palette:teleport` (see `herdr-command-palette list --ids`)"
        );
        assert_eq!(
            row_id("palette:edit_herdr_config"),
            Ok(RowId::new(Kind::Palette, "edit_herdr_config"))
        );
    }

    #[test]
    fn plugin_and_custom_ids_are_taken_as_written() {
        assert_eq!(
            row_id("plugin:wtherdr.merge"),
            Ok(RowId::new(Kind::Plugin, "wtherdr.merge"))
        );
        assert_eq!(
            row_id("custom:Popup: lazygit"),
            Ok(RowId::new(Kind::Custom, "Popup: lazygit"))
        );
    }
}
