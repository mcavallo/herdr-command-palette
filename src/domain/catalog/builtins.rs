//! Built-in rows: Herdr key actions the palette can run, with Herdr's
//! default keys ([`DEFAULT_KEYS`])
//! overridden by the user's `[keys]`.
//!
//! Only actions with a socket equivalent are listed (Herdr has no API to
//! run a built-in by name); actions that need input or an index are left
//! out. How each one runs is the
//! [`requests`](crate::domain::requests) module's business.

use crate::domain::commands::{Builtin, Direction, Step};
use crate::domain::config::{Binding, HerdrConfig};
use crate::domain::row::{Kind, Row};

use super::NamedCommand;

/// A built-in: its `[keys]` name, palette text and what it runs.
pub type BuiltinCommand = NamedCommand<Builtin>;

/// One [`BUILTINS`] entry; shorter than a struct literal per line.
const fn builtin(
    name: &'static str,
    title: &'static str,
    description: &'static str,
    action: Builtin,
) -> BuiltinCommand {
    NamedCommand {
        name,
        title,
        description,
        action,
    }
}

/// The built-ins, in palette order; names are Herdr's `[keys]` action names.
/// Titles and descriptions are hand-written.
pub const BUILTINS: [BuiltinCommand; 33] = [
    builtin(
        "split_vertical",
        "Split pane right",
        "Split the focused pane, new pane on the right",
        Builtin::SplitVertical,
    ),
    builtin(
        "split_horizontal",
        "Split pane down",
        "Split the focused pane, new pane below",
        Builtin::SplitHorizontal,
    ),
    builtin(
        "close_pane",
        "Close pane",
        "Close the focused pane and its process",
        Builtin::ClosePane,
    ),
    builtin(
        "zoom",
        "Toggle zoom",
        "Zoom the focused pane to fill the tab, or restore it",
        Builtin::Zoom,
    ),
    builtin(
        "focus_pane_left",
        "Focus pane left",
        "Move focus to the pane on the left",
        Builtin::FocusPane(Direction::Left),
    ),
    builtin(
        "focus_pane_down",
        "Focus pane down",
        "Move focus to the pane below",
        Builtin::FocusPane(Direction::Down),
    ),
    builtin(
        "focus_pane_up",
        "Focus pane up",
        "Move focus to the pane above",
        Builtin::FocusPane(Direction::Up),
    ),
    builtin(
        "focus_pane_right",
        "Focus pane right",
        "Move focus to the pane on the right",
        Builtin::FocusPane(Direction::Right),
    ),
    builtin(
        "swap_pane_left",
        "Swap pane left",
        "Swap the focused pane with its left neighbor",
        Builtin::SwapPane(Direction::Left),
    ),
    builtin(
        "swap_pane_down",
        "Swap pane down",
        "Swap the focused pane with the pane below",
        Builtin::SwapPane(Direction::Down),
    ),
    builtin(
        "swap_pane_up",
        "Swap pane up",
        "Swap the focused pane with the pane above",
        Builtin::SwapPane(Direction::Up),
    ),
    builtin(
        "swap_pane_right",
        "Swap pane right",
        "Swap the focused pane with its right neighbor",
        Builtin::SwapPane(Direction::Right),
    ),
    builtin(
        "resize_pane_left",
        "Resize pane left",
        "Move the focused pane's border left one step",
        Builtin::ResizePane(Direction::Left),
    ),
    builtin(
        "resize_pane_down",
        "Resize pane down",
        "Move the focused pane's border down one step",
        Builtin::ResizePane(Direction::Down),
    ),
    builtin(
        "resize_pane_up",
        "Resize pane up",
        "Move the focused pane's border up one step",
        Builtin::ResizePane(Direction::Up),
    ),
    builtin(
        "resize_pane_right",
        "Resize pane right",
        "Move the focused pane's border right one step",
        Builtin::ResizePane(Direction::Right),
    ),
    builtin(
        "cycle_pane_next",
        "Next pane",
        "Focus the next pane in this tab",
        Builtin::CyclePane(Step::Next),
    ),
    builtin(
        "cycle_pane_previous",
        "Previous pane",
        "Focus the previous pane in this tab",
        Builtin::CyclePane(Step::Previous),
    ),
    builtin(
        "clear_pane",
        "Clear pane",
        "Clear the focused pane's screen and scrollback",
        Builtin::ClearPane,
    ),
    builtin(
        "edit_scrollback",
        "Edit scrollback",
        "Open the focused pane's scrollback in $EDITOR",
        Builtin::EditScrollback,
    ),
    builtin(
        "new_tab",
        "New tab",
        "Create a tab in this workspace and focus it",
        Builtin::NewTab,
    ),
    builtin(
        "previous_tab",
        "Previous tab",
        "Focus the previous tab in this workspace",
        Builtin::FocusTab(Step::Previous),
    ),
    builtin(
        "next_tab",
        "Next tab",
        "Focus the next tab in this workspace",
        Builtin::FocusTab(Step::Next),
    ),
    builtin(
        "move_tab_previous",
        "Move tab left",
        "Move the current tab one position left",
        Builtin::MoveTab(Step::Previous),
    ),
    builtin(
        "move_tab_next",
        "Move tab right",
        "Move the current tab one position right",
        Builtin::MoveTab(Step::Next),
    ),
    builtin(
        "close_tab",
        "Close tab",
        "Close the current tab and all its panes",
        Builtin::CloseTab,
    ),
    builtin(
        "new_workspace",
        "New workspace",
        "Create a workspace and focus it",
        Builtin::NewWorkspace,
    ),
    builtin(
        "close_workspace",
        "Close workspace",
        "Close the current workspace and all its tabs",
        Builtin::CloseWorkspace,
    ),
    builtin(
        "previous_workspace",
        "Previous workspace",
        "Focus the previous workspace",
        Builtin::FocusWorkspace(Step::Previous),
    ),
    builtin(
        "next_workspace",
        "Next workspace",
        "Focus the next workspace",
        Builtin::FocusWorkspace(Step::Next),
    ),
    builtin(
        "previous_agent",
        "Previous agent",
        "Focus the previous pane running an agent",
        Builtin::FocusAgent(Step::Previous),
    ),
    builtin(
        "next_agent",
        "Next agent",
        "Focus the next pane running an agent",
        Builtin::FocusAgent(Step::Next),
    ),
    builtin(
        "reload_config",
        "Reload config",
        "Re-read config.toml in the running server",
        Builtin::ReloadConfig,
    ),
];

/// Herdr's default key for each built-in that has one (action name, key).
/// Built-ins missing here are unbound by default.
///
/// Mirrors `impl Default for KeysConfig` in Herdr `src/config/model.rs`,
/// v0.9.3 (only the actions in [`BUILTINS`]).
pub const DEFAULT_KEYS: [(&str, &str); 22] = [
    ("new_workspace", "prefix+shift+n"),
    ("close_workspace", "prefix+shift+d"),
    ("reload_config", "prefix+shift+r"),
    ("new_tab", "prefix+c"),
    ("previous_tab", "prefix+p"),
    ("next_tab", "prefix+n"),
    ("close_tab", "prefix+shift+x"),
    ("edit_scrollback", "prefix+e"),
    ("focus_pane_left", "prefix+h"),
    ("focus_pane_down", "prefix+j"),
    ("focus_pane_up", "prefix+k"),
    ("focus_pane_right", "prefix+l"),
    ("swap_pane_left", "prefix+shift+h"),
    ("swap_pane_down", "prefix+shift+j"),
    ("swap_pane_up", "prefix+shift+k"),
    ("swap_pane_right", "prefix+shift+l"),
    ("cycle_pane_next", "prefix+tab"),
    ("cycle_pane_previous", "prefix+shift+tab"),
    ("split_vertical", "prefix+v"),
    ("split_horizontal", "prefix+minus"),
    ("close_pane", "prefix+x"),
    ("zoom", "prefix+z"),
];

/// Other `[keys]` names Herdr accepts for an action (action, alias).
///
/// Mirrors the `#[serde(alias = ...)]` attributes on `KeysConfig` in Herdr
/// `src/config/model.rs`, v0.9.3 (only the actions in [`BUILTINS`]).
pub const KEY_ALIASES: [(&str, &str); 1] = [("zoom", "fullscreen")];

/// Herdr's default key for the action `name`, if it has one.
fn default_key(name: &str) -> Option<&'static str> {
    action_entry(&DEFAULT_KEYS, name)
}

/// The alias Herdr also accepts for the action `name` in `[keys]`, if any.
fn key_alias(name: &str) -> Option<&'static str> {
    action_entry(&KEY_ALIASES, name)
}

/// The value for the action `name` in an (action, value) `table`.
fn action_entry(table: &[(&str, &'static str)], name: &str) -> Option<&'static str> {
    table
        .iter()
        .find(|(action, _)| *action == name)
        .map(|(_, value)| *value)
}

/// The keys bound to `name`: the user's `[keys]` entry if there is one
/// (under `name`, else under its alias in [`KEY_ALIASES`]; an empty one means
/// unbound), else Herdr's default, else none (`""`). An entry that isn't a
/// binding ([`Binding::Other`]) counts as absent.
#[must_use]
pub fn shortcut(name: &str, config: &HerdrConfig) -> String {
    let user_binding = |key: &str| match config.keys.bindings.get(key) {
        Some(Binding::Other) | None => None,
        Some(binding) => Some(binding),
    };
    match user_binding(name).or_else(|| key_alias(name).and_then(user_binding)) {
        Some(binding) => binding.display(),
        None => default_key(name).unwrap_or_default().to_owned(),
    }
}

/// One row per built-in, in [`BUILTINS`] order.
#[must_use]
pub fn rows(config: &HerdrConfig) -> Vec<Row> {
    BUILTINS
        .iter()
        .map(|builtin| {
            builtin
                .to_row(Kind::Builtin)
                .with_shortcut(shortcut(builtin.name, config))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn config(toml: &str) -> HerdrConfig {
        HerdrConfig::from_toml(toml).unwrap()
    }

    #[test]
    fn pins_a_few_known_entries() {
        assert_eq!(BUILTINS[0].name, "split_vertical");
        assert_eq!(BUILTINS[32].name, "reload_config");
        assert_eq!(
            BuiltinCommand::find(&BUILTINS, "zoom").map(|b| b.title),
            Some("Toggle zoom")
        );
        assert_eq!(BuiltinCommand::find(&BUILTINS, "goto"), None);
        assert_eq!(default_key("split_horizontal"), Some("prefix+minus"));
        assert_eq!(default_key("previous_workspace"), None);
    }

    #[test]
    fn builds_entries_from_name_title_and_description() {
        // `const` tables are built at compile time; this runs the helper.
        let entry = builtin(
            "zoom",
            "Toggle zoom",
            "Zoom the focused pane to fill the tab, or restore it",
            Builtin::Zoom,
        );

        assert_eq!(BuiltinCommand::find(&BUILTINS, "zoom"), Some(&entry));
    }

    #[test]
    fn builtin_names_are_unique() {
        let names: HashSet<_> = BUILTINS.iter().map(|builtin| builtin.name).collect();

        assert_eq!(names.len(), BUILTINS.len());
    }

    #[test]
    fn default_keys_are_unique() {
        let keyed: HashSet<_> = DEFAULT_KEYS.iter().map(|(name, _)| *name).collect();

        assert_eq!(keyed.len(), DEFAULT_KEYS.len());
    }

    #[test]
    fn default_keys_are_only_for_listed_builtins() {
        let names: HashSet<_> = BUILTINS.iter().map(|builtin| builtin.name).collect();
        let keyed: HashSet<_> = DEFAULT_KEYS.iter().map(|(name, _)| *name).collect();

        assert!(keyed.is_subset(&names));
    }

    #[test]
    fn uses_the_herdr_default_when_the_user_sets_nothing() {
        assert_eq!(shortcut("zoom", &HerdrConfig::default()), "prefix+z");
        assert_eq!(shortcut("next_agent", &HerdrConfig::default()), "");
    }

    #[test]
    fn user_bindings_override_the_default() {
        let config = config(
            r#"
            [keys]
            zoom = ["prefix+z", "", "prefix+f"]
            split_vertical = "prefix+backslash"
            next_agent = "prefix+alt+n"
            "#,
        );

        assert_eq!(shortcut("zoom", &config), "prefix+z, prefix+f");
        assert_eq!(shortcut("split_vertical", &config), "prefix+backslash");
        assert_eq!(shortcut("next_agent", &config), "prefix+alt+n");
    }

    #[test]
    fn an_empty_user_binding_means_unbound() {
        let config = config("[keys]\nclose_pane = \"\"\nzoom = []\n");

        assert_eq!(shortcut("close_pane", &config), "");
        assert_eq!(shortcut("zoom", &config), "");
    }

    #[test]
    fn ignores_keys_entries_that_are_not_bindings() {
        let config = config("[keys]\nzoom = 3\n[keys.indexed]\ntabs = \"alt\"\n");

        assert_eq!(shortcut("zoom", &config), "prefix+z");
        assert_eq!(shortcut("indexed", &config), "");
    }

    #[test]
    fn reads_the_fullscreen_alias_of_zoom() {
        let config = config("[keys]\nfullscreen = \"prefix+f\"\n");

        assert_eq!(shortcut("zoom", &config), "prefix+f");
    }

    #[test]
    fn prefers_the_action_name_over_its_alias() {
        let config = config("[keys]\nzoom = \"prefix+z\"\nfullscreen = \"prefix+f\"\n");

        assert_eq!(shortcut("zoom", &config), "prefix+z");
    }

    #[test]
    fn aliases_name_listed_builtins_and_are_not_builtins() {
        assert_eq!(key_alias("zoom"), Some("fullscreen"));
        assert_eq!(key_alias("close_pane"), None);
        for (action, alias) in KEY_ALIASES {
            assert!(
                BuiltinCommand::find(&BUILTINS, action).is_some(),
                "{action}"
            );
            assert!(BuiltinCommand::find(&BUILTINS, alias).is_none(), "{alias}");
        }
    }

    #[test]
    fn lists_every_builtin_in_order() {
        let rows = rows(&HerdrConfig::default());

        assert_eq!(rows.len(), BUILTINS.len());
        assert_eq!(
            rows[3],
            Row::new(Kind::Builtin, "zoom", "Toggle zoom")
                .with_shortcut("prefix+z")
                .with_footer_text("Zoom the focused pane to fill the tab, or restore it")
        );
    }
}
