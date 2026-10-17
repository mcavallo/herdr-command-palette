//! The parts of Herdr's `config.toml` the palette reads: key bindings
//! (`[keys]`), custom commands (`[[keys.command]]`), the theme
//! (`[theme]`) and `[ui] confirm_close`.
//!
//! Parsing is lenient: unknown keys are ignored (no `deny_unknown_fields`,
//! unlike the plugin's settings), and bindings of any other
//! shape (`[keys.indexed]`, `zoom = 3`, a `[[keys.command]]` `key = 3`)
//! are kept as [`Binding::Other`] and treated as unset (a built-in then
//! shows Herdr's default key). It fails only on invalid TOML or a wrong
//! type in `[[keys.command]]` `type`/`command`/`description`/`width`/
//! `height` or in `[theme]`. A `[ui]` of another shape counts as unset
//! (Herdr's default). Finding and reading the file is the
//! [`ConfigSource`](crate::ports::ConfigSource) adapter's job.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Herdr's `config.toml`, the parts the palette uses. A missing file is
/// [`HerdrConfig::default()`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct HerdrConfig {
    /// `[keys]`: bindings and custom commands.
    pub keys: KeysConfig,
    /// `[theme]`: theme name and overrides.
    pub theme: ThemeConfig,
    /// `[ui]`: the settings the palette copies from Herdr's UI.
    #[serde(deserialize_with = "or_default")]
    pub ui: UiConfig,
}

impl HerdrConfig {
    /// Parses the text of a `config.toml`.
    ///
    /// # Errors
    ///
    /// Returns the TOML error (with line and column) when the text is not
    /// valid TOML, or has a wrong type in `[[keys.command]]`
    /// `type`/`command`/`description`/`width`/`height` or in `[theme]`.
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }
}

/// `[ui]`, the part the palette uses (Herdr `src/config/model.rs`
/// `UiConfig`, v0.9.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// `confirm_close` as written; see [`confirm_close`](Self::confirm_close).
    #[serde(rename = "confirm_close", deserialize_with = "or_default")]
    confirm_close_value: Option<bool>,
}

impl UiConfig {
    /// `confirm_close`: whether Herdr asks before closing a workspace
    /// (and refuses `tab.close` / `pane.close` that would close a
    /// worktree group). Unset or not a boolean: `true`, Herdr's
    /// default.
    #[must_use]
    pub fn confirm_close(&self) -> bool {
        self.confirm_close_value.unwrap_or(true)
    }

    /// A `[ui]` with `confirm_close` set.
    #[must_use]
    pub fn with_confirm_close(confirm_close: bool) -> Self {
        Self {
            confirm_close_value: Some(confirm_close),
        }
    }
}

/// Deserializes a `T`, or `T::default()` when the value has another
/// shape: Herdr would reject the file and use its defaults, which these
/// defaults stand for.
fn or_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    // `untagged`: serde tries `Valid` first, then accepts anything.
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Shape<T> {
        Valid(T),
        Other(serde::de::IgnoredAny),
    }
    Ok(match Shape::<T>::deserialize(deserializer)? {
        Shape::Valid(value) => value,
        Shape::Other(_) => T::default(),
    })
}

/// `[keys]`: action bindings plus `[[keys.command]]` entries.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct KeysConfig {
    /// `[[keys.command]]` entries, in file order. Their index is the
    /// payload of custom rows.
    #[serde(default)]
    pub command: Vec<CustomCommand>,
    /// Every other `[keys]` entry: action name -> binding, e.g.
    /// `zoom = ["prefix+z", "prefix+f"]`. Overrides Herdr's default keys.
    /// Entries that aren't bindings (the `[keys.indexed]` table) are
    /// [`Binding::Other`].
    // `flatten` collects the remaining keys of the table into this map.
    #[serde(flatten)]
    pub bindings: BTreeMap<String, Binding>,
}

/// A key binding: one key or several (`"prefix+z"` or `["prefix+z", "prefix+f"]`).
/// An empty string means "unbound".
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
// `untagged`: serde tries each variant in order until one fits.
#[serde(untagged)]
pub enum Binding {
    /// A single key.
    One(String),
    /// Several keys.
    Many(Vec<String>),
    /// Anything else, e.g. Herdr's `[keys.indexed]` table (Herdr
    /// `src/config/model.rs`, v0.9.3) or a field a newer Herdr adds. Not a
    /// binding: lookups treat it as absent, so it can't break the palette.
    // `deserialize_with` lets this unit variant accept (and drop) any value.
    #[serde(deserialize_with = "ignore")]
    Other,
}

/// Accepts any value and drops it, for [`Binding::Other`].
fn ignore<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<(), D::Error> {
    serde::de::IgnoredAny::deserialize(deserializer).map(|_| ())
}

impl Binding {
    /// The keys, comma separated, skipping empty ones (`prefix+z, prefix+f`);
    /// empty for [`Binding::Other`].
    #[must_use]
    pub fn display(&self) -> String {
        match self {
            Self::Other => String::new(),
            Self::One(key) => key.clone(),
            Self::Many(keys) => keys
                .iter()
                .filter(|key| !key.is_empty())
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", "),
        }
    }
}

/// One `[[keys.command]]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct CustomCommand {
    /// `type`: what the entry does; defaults to shell.
    #[serde(rename = "type")]
    pub kind: CommandType,
    /// `command`: shell command line, or qualified action id for
    /// [`CommandType::PluginAction`].
    pub command: Option<String>,
    /// `description`: shown as the row title.
    pub description: Option<String>,
    /// `key`: the binding, if any.
    pub key: Option<Binding>,
    /// `width` of a popup command.
    pub width: Option<Dimension>,
    /// `height` of a popup command.
    pub height: Option<Dimension>,
}

/// The `type` of a `[[keys.command]]` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandType {
    /// Run in the background with `sh -c`.
    #[default]
    Shell,
    /// Run in a temporary split pane.
    Pane,
    /// Run in a popup.
    Popup,
    /// Invoke a plugin action (listed with the plugin rows, not as custom).
    PluginAction,
    /// A type this version doesn't know; such entries are skipped.
    #[serde(other)]
    Other,
}

impl CommandType {
    /// The name used in `config.toml` (`other` for unknown types).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Pane => "pane",
            Self::Popup => "popup",
            Self::PluginAction => "plugin_action",
            Self::Other => "other",
        }
    }
}

/// A popup size as written in the config: cells (`80`) or text (`"90%"`).
/// Passed to `plugin.pane.open` unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Dimension {
    /// A number of cells.
    Cells(u32),
    /// Anything else Herdr accepts, e.g. a percentage.
    Text(String),
}

/// `[theme]`: which theme Herdr uses and its overrides (Herdr
/// `src/config/theme.rs` `ThemeConfig`, v0.9.3).
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    /// `name`, e.g. `catppuccin` (Herdr's default when unset).
    pub name: Option<String>,
    /// `auto_switch`: follow the terminal's light/dark appearance and use
    /// [`dark_name`](Self::dark_name) or [`light_name`](Self::light_name).
    pub auto_switch: bool,
    /// `dark_name`: the theme for a dark appearance; unset, the dark
    /// sibling of `name`.
    pub dark_name: Option<String>,
    /// `light_name`: the theme for a light appearance; unset, the light
    /// sibling of `name`.
    pub light_name: Option<String>,
    /// `[theme.custom]` and its `[theme.custom.dark]` / `[theme.custom.light]`
    /// tables.
    pub custom: CustomTheme,
}

/// `[theme.custom]`: token overrides on top of the theme, plus per-appearance
/// overrides applied after them when `auto_switch` is on.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct CustomTheme {
    /// The tokens set directly in `[theme.custom]`.
    // `flatten` reads this struct's fields from `[theme.custom]` itself.
    #[serde(flatten)]
    pub tokens: ThemeTokens,
    /// `[theme.custom.dark]`: applied when `auto_switch` picks dark.
    pub dark: ThemeTokens,
    /// `[theme.custom.light]`: applied when `auto_switch` picks light.
    pub light: ThemeTokens,
}

/// The theme tokens the palette uses, as written (`"#181825"`, `"reset"`).
/// Herdr's other tokens (`red`, `crust`, ...) are ignored, whatever their
/// type.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default)]
pub struct ThemeTokens {
    /// `panel_bg`: the popup's background.
    pub panel_bg: Option<String>,
    /// `overlay0`: the footer.
    pub overlay0: Option<String>,
    /// `subtext0`: the rows, the query and the footer's kind tag.
    pub subtext0: Option<String>,
    /// `accent`: the confirmation dialog's title and confirm button.
    pub accent: Option<String>,
    /// `text`: the categories, the selected row and the confirmation
    /// dialog's detail and cancel button.
    pub text: Option<String>,
    /// `surface0`: the cancel button's background.
    pub surface0: Option<String>,
    /// `surface_dim`: the confirm button's text when `panel_bg` is `reset`.
    pub surface_dim: Option<String>,
    /// `mauve`: the git branch in Jump to's footer.
    pub mauve: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Shared test configuration with a theme.
    const CONFIG: &str = r#"
        onboarding = false

        [theme]
        name = "Tokyo Night"
        [theme.custom]
        panel_bg = "reset"

        [keys]
        prefix = "ctrl+b"
        split_vertical = "prefix+backslash"
        zoom = ["prefix+z", "", "prefix+f"]
        close_pane = ""

        [[keys.command]]
        key = "prefix+ctrl+g"
        type = "plugin_action"
        command = "demo.hello"
        description = "hello"

        [[keys.command]]
        type = "popup"
        command = "lazygit"
        width = "90%"
        height = 30

        [[keys.command]]
        command = "notify-send hi"

        [[keys.command]]
        type = "teleport"
        command = "x"
    "#;

    #[test]
    fn parses_keys_commands_and_theme() {
        let config = HerdrConfig::from_toml(CONFIG).unwrap();
        let bindings = &config.keys.bindings;
        assert_eq!(bindings["zoom"].display(), "prefix+z, prefix+f");
        assert_eq!(bindings["split_vertical"].display(), "prefix+backslash");
        assert_eq!(bindings["close_pane"].display(), "");
        assert!(!bindings.contains_key("command"));

        let commands = &config.keys.command;
        assert_eq!(commands.len(), 4);
        assert_eq!(commands[0].kind, CommandType::PluginAction);
        assert_eq!(
            commands[0].key.as_ref().map(Binding::display).as_deref(),
            Some("prefix+ctrl+g")
        );
        assert_eq!(commands[1].width, Some(Dimension::Text("90%".into())));
        assert_eq!(commands[1].height, Some(Dimension::Cells(30)));
        assert_eq!(commands[2].kind, CommandType::Shell);
        assert_eq!(commands[2].description, None);
        assert_eq!(commands[3].kind, CommandType::Other);

        assert_eq!(config.theme.name.as_deref(), Some("Tokyo Night"));
        assert_eq!(
            config.theme.custom.tokens.panel_bg.as_deref(),
            Some("reset")
        );
    }

    #[test]
    fn parses_auto_switch_and_per_appearance_overrides() {
        let toml = r##"
            [theme]
            auto_switch = true
            dark_name = "nord"
            light_name = "dawn"
            [theme.custom]
            overlay0 = "#111111"
            mauve = "magenta"
            crust = 3
            [theme.custom.dark]
            panel_bg = "#000000"
            accent = "blue"
            [theme.custom.light]
            subtext0 = "reset"
            surface_dim = "black"
            red = "red"
        "##;

        let theme = HerdrConfig::from_toml(toml).unwrap().theme;

        assert_eq!(
            theme,
            ThemeConfig {
                name: None,
                auto_switch: true,
                dark_name: Some("nord".to_owned()),
                light_name: Some("dawn".to_owned()),
                custom: CustomTheme {
                    tokens: ThemeTokens {
                        overlay0: Some("#111111".to_owned()),
                        mauve: Some("magenta".to_owned()),
                        ..ThemeTokens::default()
                    },
                    dark: ThemeTokens {
                        panel_bg: Some("#000000".to_owned()),
                        accent: Some("blue".to_owned()),
                        ..ThemeTokens::default()
                    },
                    light: ThemeTokens {
                        subtext0: Some("reset".to_owned()),
                        surface_dim: Some("black".to_owned()),
                        ..ThemeTokens::default()
                    },
                },
            }
        );
    }

    /// Herdr v0.9.3 `KeysConfig` has a nested `indexed` table.
    const INDEXED: &str = r#"
        [keys]
        zoom = "prefix+f"
        [keys.indexed]
        tabs = "alt"
        workspaces = "ctrl+alt"
        agents = "alt+shift"
    "#;

    #[test]
    fn keeps_non_binding_keys_entries_like_indexed_as_other() {
        let config = HerdrConfig::from_toml(INDEXED).unwrap();

        assert_eq!(
            config.keys.bindings["zoom"],
            Binding::One("prefix+f".into())
        );
        assert_eq!(config.keys.bindings["indexed"], Binding::Other);
        assert_eq!(Binding::Other.display(), "");
    }

    #[test]
    fn accepts_values_of_any_type_in_keys() {
        let config = HerdrConfig::from_toml("[keys]\nfuture = 3\nmixed = [\"a\", 1]\n").unwrap();

        assert_eq!(config.keys.bindings["future"], Binding::Other);
        assert_eq!(config.keys.bindings["mixed"], Binding::Other);
    }

    #[test]
    fn reads_confirm_close_defaulting_to_herdrs_true() {
        let ui = |toml: &str| HerdrConfig::from_toml(toml).unwrap().ui;

        assert!(ui("").confirm_close());
        assert!(ui("[ui]\nsidebar_width = 30\n").confirm_close());
        assert!(!ui("[ui]\nconfirm_close = false\n").confirm_close());
        assert!(ui("[ui]\nconfirm_close = true\n").confirm_close());
        assert_eq!(
            ui("[ui]\nconfirm_close = false\n"),
            UiConfig::with_confirm_close(false)
        );
    }

    #[test]
    fn a_ui_of_another_shape_is_herdrs_default() {
        let ui = |toml: &str| HerdrConfig::from_toml(toml).unwrap().ui;

        assert!(ui("[ui]\nconfirm_close = \"no\"\n").confirm_close());
        assert!(ui("ui = 3\n").confirm_close());
    }

    #[test]
    fn an_empty_config_is_the_default() {
        let config = HerdrConfig::from_toml("").unwrap();

        assert_eq!(config, HerdrConfig::default());
    }

    #[test]
    fn invalid_toml_is_an_error_naming_the_line() {
        let error = HerdrConfig::from_toml("[keys\n").unwrap_err();

        assert!(error.to_string().contains("line 1"), "{error}");
    }

    #[test]
    fn command_types_have_config_names() {
        let names: Vec<_> = [
            CommandType::Shell,
            CommandType::Pane,
            CommandType::Popup,
            CommandType::PluginAction,
            CommandType::Other,
        ]
        .map(CommandType::as_str)
        .into();

        assert_eq!(names, ["shell", "pane", "popup", "plugin_action", "other"]);
    }

    #[test]
    fn dimensions_serialize_as_cells_or_text() {
        assert_eq!(
            serde_json::to_value(Dimension::Cells(80)).unwrap(),
            json!(80)
        );
        assert_eq!(
            serde_json::to_value(Dimension::Text("90%".into())).unwrap(),
            json!("90%")
        );
    }
}
