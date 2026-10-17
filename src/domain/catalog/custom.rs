//! Custom command rows: `[[keys.command]]` entries of type shell, pane or
//! popup. Payload is the entry's index among all
//! `[[keys.command]]` entries, so `run` finds it again in the config; the
//! row id names it by `description`, else `command`, else the type, so
//! settings survive config edits.

use crate::domain::config::{Binding, CommandType, HerdrConfig};
use crate::domain::non_empty;
use crate::domain::row::{Kind, Row};

/// One row per shell, pane or popup entry, in config order. Plugin action
/// entries (listed with the plugin rows) and unknown types are skipped.
///
/// Title: `description`, else the type. Shortcut: `key`. Footer:
/// `command`, else the type. Id: `custom:<description>`, else
/// `custom:<command>`, else `custom:<type>`.
#[must_use]
pub fn rows(config: &HerdrConfig) -> Vec<Row> {
    let mut rows = Vec::new();
    for (index, command) in config.keys.command.iter().enumerate() {
        let kind = command.kind;
        if !matches!(
            kind,
            CommandType::Shell | CommandType::Pane | CommandType::Popup
        ) {
            continue;
        }
        let title = non_empty(command.description.as_deref()).unwrap_or(kind.as_str());
        let footer = non_empty(command.command.as_deref()).unwrap_or(kind.as_str());
        // The type as a last resort: settings reject an empty name
        // (`custom:`), so the row must have one to be referenced.
        let id_name = non_empty(command.description.as_deref())
            .or(non_empty(command.command.as_deref()))
            .unwrap_or(kind.as_str());
        let shortcut = command
            .key
            .as_ref()
            .map(Binding::display)
            .unwrap_or_default();
        rows.push(
            Row::new(Kind::Custom, index.to_string(), title)
                .with_shortcut(shortcut)
                .with_footer_text(footer)
                .with_id_name(id_name),
        );
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows_for(toml: &str) -> Vec<Row> {
        rows(&HerdrConfig::from_toml(toml).unwrap())
    }

    #[test]
    fn indexes_rows_by_position_among_all_commands() {
        let rows = rows_for(
            r#"
            [[keys.command]]
            type = "plugin_action"
            command = "demo.hello"

            [[keys.command]]
            type = "teleport"
            command = "x"

            [[keys.command]]
            key = ["prefix+alt+l", "prefix+g"]
            type = "popup"
            command = "lazygit"
            description = "Popup: lazygit"
            "#,
        );

        assert_eq!(
            rows,
            [Row::new(Kind::Custom, "2", "Popup: lazygit")
                .with_shortcut("prefix+alt+l, prefix+g")
                .with_footer_text("lazygit")
                .with_id_name("Popup: lazygit")]
        );
    }

    #[test]
    fn falls_back_to_the_type_for_title_footer_and_id() {
        let rows = rows_for(
            r#"
            [[keys.command]]
            command = "notify-send hi"

            [[keys.command]]
            type = "pane"
            description = ""
            command = ""
            "#,
        );

        assert_eq!(
            rows,
            [
                Row::new(Kind::Custom, "0", "shell")
                    .with_footer_text("notify-send hi")
                    .with_id_name("notify-send hi"),
                Row::new(Kind::Custom, "1", "pane")
                    .with_footer_text("pane")
                    .with_id_name("pane"),
            ]
        );
    }
}
