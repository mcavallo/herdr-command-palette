//! Plugin action rows: one per action from `plugin.action.list`, titled and
//! bound by the matching `[[keys.command]]` entry of type `plugin_action`.
//!
//! The manifest's action `title` is deliberately not used: an action the
//! user hasn't bound shows its qualified id, which names the plugin too.

use crate::domain::config::{Binding, CommandType, CustomCommand, HerdrConfig};
use crate::domain::herdr::PluginAction;
use crate::domain::non_empty;
use crate::domain::row::{Kind, Row};

/// One row per plugin action, in Herdr's order, except this plugin's own
/// actions: `open` and the repeat actions, which are meant for
/// key bindings (repeating from the palette is just picking the row).
///
/// Title: the binding's `description`, else the qualified id. Shortcut:
/// the binding's `key`. Footer: the qualified id.
#[must_use]
pub fn rows(actions: &[PluginAction], config: &HerdrConfig, plugin_id: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    for action in actions
        .iter()
        .filter(|action| action.plugin_id != plugin_id)
    {
        let id = action.qualified_id();
        let binding = binding(&id, config);
        let title = binding
            .and_then(|command| non_empty(command.description.as_deref()))
            .unwrap_or(&id)
            .to_owned();
        let shortcut = binding
            .and_then(|command| command.key.as_ref())
            .map(Binding::display)
            .unwrap_or_default();
        rows.push(
            Row::new(Kind::Plugin, id.clone(), title)
                .with_shortcut(shortcut)
                .with_footer_text(id),
        );
    }
    rows
}

/// The `[[keys.command]]` entry of type `plugin_action` that runs the
/// action `id`; the last one wins if there are several.
fn binding<'a>(id: &str, config: &'a HerdrConfig) -> Option<&'a CustomCommand> {
    config.keys.command.iter().rev().find(|command| {
        command.kind == CommandType::PluginAction && command.command.as_deref() == Some(id)
    })
}

#[cfg(test)]
mod tests {
    use crate::domain::fixtures::plugin_action;

    use super::*;

    fn action(plugin_id: &str, action_id: &str) -> PluginAction {
        plugin_action(plugin_id, action_id, "Manifest title")
    }

    const CONFIG: &str = r#"
        [[keys.command]]
        key = "prefix+ctrl+g"
        type = "plugin_action"
        command = "demo.hello"
        description = "first"

        [[keys.command]]
        key = "prefix+ctrl+h"
        type = "plugin_action"
        command = "demo.hello"
        description = "hello"

        [[keys.command]]
        type = "shell"
        command = "demo.bye"
        description = "not a binding"
    "#;

    #[test]
    fn titles_bound_actions_with_the_binding_description_and_key() {
        let config = HerdrConfig::from_toml(CONFIG).unwrap();

        let rows = rows(&[action("demo", "hello")], &config, "mmc.command-palette");

        assert_eq!(
            rows,
            [Row::new(Kind::Plugin, "demo.hello", "hello")
                .with_shortcut("prefix+ctrl+h")
                .with_footer_text("demo.hello")]
        );
    }

    #[test]
    fn shows_the_qualified_id_for_unbound_actions() {
        let config = HerdrConfig::from_toml(CONFIG).unwrap();

        let rows = rows(&[action("demo", "bye")], &config, "mmc.command-palette");

        assert_eq!(
            rows,
            [Row::new(Kind::Plugin, "demo.bye", "demo.bye").with_footer_text("demo.bye")]
        );
    }

    #[test]
    fn skips_the_palettes_own_actions() {
        let actions = [
            action("mmc.command-palette", "open"),
            action("mmc.command-palette", "repeat-command"),
            action("mmc.command-palette-extra", "open"),
        ];

        let rows = rows(&actions, &HerdrConfig::default(), "mmc.command-palette");

        let ids: Vec<_> = rows.iter().map(|row| row.payload.as_str()).collect();
        assert_eq!(ids, ["mmc.command-palette-extra.open"]);
    }
}
