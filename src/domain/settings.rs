//! The plugin's own settings: `settings.toml` in Herdr's plugin config dir
//! as typed values with their defaults.
//!
//! Every key is optional; a missing file or key means the default. Parsing
//! is strict (`deny_unknown_fields`, closed sets of values) and a problem
//! names the dotted key and its line, so the palette can show it on its
//! error screen. Finding and reading the file is the
//! [`SettingsSource`](crate::ports::SettingsSource) adapter's job.
//!
//! - [`template`]: the commented file the "Edit Command Palette settings"
//!   row writes on first use.

mod key_path;
pub mod template;
mod values;

use std::fmt;

use serde::Deserialize;

use crate::domain::color::Color;
use crate::domain::popup::Bounds;
use crate::domain::row::{Category, Kind, Row, RowId};

/// Everything in `settings.toml`. [`Settings::default()`] is a missing file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// `editor`: the editor of the edit rows (Herdr config, these
    /// settings), arguments allowed (`code -w`). Empty: `$EDITOR`, then
    /// `vi`.
    pub editor: String,
    /// `open_on`: the category the palette opens on.
    pub open_on: Category,
    /// `[commands]`.
    pub commands: CommandSettings,
    /// `[jump_to]`.
    pub jump_to: JumpToSettings,
    /// `[popup]`: the palette popup's size bounds.
    pub popup: PopupSettings,
    /// `[colors]`: overrides of the colors derived from Herdr's theme.
    pub colors: ColorSettings,
    /// `[history]`: recording and pruning of recent use.
    pub history: HistorySettings,
}

/// `[commands]`: order, hidden rows, confirmations and rows kept out of
/// the recent order of the Commands category.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CommandSettings {
    /// `order`: recently run first, or the fixed order.
    pub order: CommandOrder,
    /// `hide_kinds`: row kinds left out of the palette (never `pane`).
    #[serde(deserialize_with = "values::command_kinds")]
    pub hide_kinds: Vec<Kind>,
    /// `hide_ids`: single rows left out of the palette.
    #[serde(deserialize_with = "values::row_ids")]
    pub hide_ids: Vec<RowId>,
    /// `confirm_ids`: rows that ask before running.
    #[serde(deserialize_with = "values::row_ids")]
    pub confirm_ids: Vec<RowId>,
    /// `skip_recent_ids`: rows that keep their fixed place in the
    /// "recent" order; still recorded, so `repeat-command` repeats them.
    #[serde(deserialize_with = "values::row_ids")]
    pub skip_recent_ids: Vec<RowId>,
}

impl Default for CommandSettings {
    fn default() -> Self {
        Self {
            order: CommandOrder::default(),
            hide_kinds: Vec::new(),
            hide_ids: Vec::new(),
            confirm_ids: vec![RowId::new(Kind::Builtin, "close_workspace")],
            skip_recent_ids: vec![RowId::new(Kind::Builtin, "reload_config")],
        }
    }
}

impl CommandSettings {
    /// Whether `row` is left out of the palette: its kind is in
    /// `hide_kinds` or its id in `hide_ids`. Pane rows never are.
    #[must_use]
    pub fn hides(&self, row: &Row) -> bool {
        self.hide_kinds.contains(&row.kind) || self.hide_ids.contains(&row.id)
    }

    /// Whether running `row` from the palette asks first (`confirm_ids`).
    #[must_use]
    pub fn confirms(&self, row: &Row) -> bool {
        self.confirm_ids.contains(&row.id)
    }

    /// Whether `row` keeps its fixed place in the "recent" order
    /// (`skip_recent_ids`).
    #[must_use]
    pub fn skips_recent(&self, row: &Row) -> bool {
        self.skip_recent_ids.contains(&row.id)
    }
}

/// `[commands] order`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandOrder {
    /// `recent`: most recently run first, then the fixed order.
    #[default]
    Recent,
    /// `fixed`: plugins, built-ins, palette commands, custom commands.
    Fixed,
}

/// `[jump_to]`: order of the Jump to category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct JumpToSettings {
    /// `order`: recently focused first, or Herdr's order.
    pub order: JumpToOrder,
}

/// `[jump_to] order`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JumpToOrder {
    /// `recent`: most recently focused first (current pane last), then
    /// Herdr's order.
    #[default]
    Recent,
    /// `herdr`: Herdr's order (workspace, tab, pane).
    Herdr,
}

/// `[popup]`: size bounds in cells. Defaults: Herdr's
/// keybinds modal, 76 columns, 16 to 22 rows. The defaults are also the
/// floor: a smaller value is raised to its default, silently
/// ([`floored`](Self::floored)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PopupSettings {
    /// `min_width`, in columns.
    pub min_width: u16,
    /// `max_width`, in columns.
    pub max_width: u16,
    /// `min_height`, in rows.
    pub min_height: u16,
    /// `max_height`, in rows.
    pub max_height: u16,
}

impl Default for PopupSettings {
    fn default() -> Self {
        Self {
            min_width: 76,
            max_width: 76,
            min_height: 16,
            max_height: 22,
        }
    }
}

impl PopupSettings {
    /// These bounds with each one below its default raised to the default:
    /// the palette needs that room, and the popup still shrinks to fit a
    /// small screen ([`popup_size`](crate::domain::popup::popup_size)).
    ///
    /// # Examples
    ///
    /// ```
    /// use herdr_command_palette::domain::settings::PopupSettings;
    ///
    /// let small = PopupSettings { min_width: 0, max_width: 40, min_height: 10, max_height: 30 };
    /// let floored = small.floored();
    /// assert_eq!((floored.min_width, floored.max_width), (76, 76));
    /// assert_eq!((floored.min_height, floored.max_height), (16, 30));
    /// ```
    #[must_use]
    pub fn floored(self) -> Self {
        let floor = Self::default();
        Self {
            min_width: self.min_width.max(floor.min_width),
            max_width: self.max_width.max(floor.max_width),
            min_height: self.min_height.max(floor.min_height),
            max_height: self.max_height.max(floor.max_height),
        }
    }

    /// The width bounds.
    #[must_use]
    pub fn width(&self) -> Bounds {
        Bounds {
            min: self.min_width,
            max: self.max_width,
        }
    }

    /// The height bounds.
    #[must_use]
    pub fn height(&self) -> Bounds {
        Bounds {
            min: self.min_height,
            max: self.max_height,
        }
    }
}

/// `[colors]`: each `None` (empty or missing) keeps the color derived from
/// Herdr's theme; `"default"` is [`Color::Reset`] (the terminal's color).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ColorSettings {
    /// `background`: popup and error screen; default Herdr's `panel_bg`.
    #[serde(deserialize_with = "values::color")]
    pub background: Option<Color>,
    /// `muted`: footer, shortcuts, inactive categories; default Herdr's
    /// `overlay0` for the footer, dim for the rest.
    #[serde(deserialize_with = "values::color")]
    pub muted: Option<Color>,
    /// `matches`: match highlight (underlined; bold in the selected
    /// row); default the row's color.
    #[serde(deserialize_with = "values::color")]
    pub matches: Option<Color>,
    /// `selected`: the selected row's text color (kept bold);
    /// default Herdr's `text`.
    #[serde(deserialize_with = "values::color")]
    pub selected: Option<Color>,
    /// `dividers`: the lines around the input and the footer; default dim.
    #[serde(deserialize_with = "values::color")]
    pub dividers: Option<Color>,
    /// `tags`: the footer's kind tag label (`plugin`); default Herdr's
    /// `subtext0`.
    #[serde(deserialize_with = "values::color")]
    pub tags: Option<Color>,
    /// `branch`: the git branch in Jump to's footer; default Herdr's
    /// `mauve`.
    #[serde(deserialize_with = "values::color")]
    pub branch: Option<Color>,
}

/// `[history]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HistorySettings {
    /// `enabled`: `false` turns recording off (hooks exit at once, recent
    /// orders fall back to fixed/Herdr, repeat actions say so).
    pub enabled: bool,
    /// `max_age_days`: history older than this is pruned; 0 keeps
    /// everything.
    pub max_age_days: u32,
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_age_days: 90,
        }
    }
}

/// What is wrong with a settings file: where (dotted key, line) and what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsProblem {
    /// The dotted key (`popup.min_width`), when the problem has one.
    pub key: Option<String>,
    /// The 1-based line, when known.
    pub line: Option<usize>,
    /// What is wrong (may span lines, as TOML syntax errors do).
    pub message: String,
}

impl SettingsProblem {
    /// Where the problem is: `` `key` (line n) ``, `` `key` ``, `line n`,
    /// or `settings` when neither is known.
    #[must_use]
    pub fn location(&self) -> String {
        match (&self.key, self.line) {
            (Some(key), Some(line)) => format!("`{key}` (line {line})"),
            (Some(key), None) => format!("`{key}`"),
            (None, Some(line)) => format!("line {line}"),
            (None, None) => "settings".to_owned(),
        }
    }
}

impl fmt::Display for SettingsProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid {}: {}", self.location(), self.message)
    }
}

impl Settings {
    /// Parses and checks the text of a `settings.toml`.
    ///
    /// # Errors
    ///
    /// A [`SettingsProblem`] for invalid TOML, an unknown key, a value of
    /// the wrong type or outside its set, or a min size above its max.
    /// Sizes below the defaults aren't problems: they are raised to the
    /// defaults ([`PopupSettings::floored`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use herdr_command_palette::domain::settings::Settings;
    ///
    /// let settings = Settings::from_toml("[popup]\nmax_height = 30\n").unwrap();
    /// assert_eq!(settings.popup.max_height, 30);
    ///
    /// let settings = Settings::from_toml("[popup]\nmin_width = 0\n").unwrap();
    /// assert_eq!(settings.popup.min_width, 76);
    ///
    /// let problem = Settings::from_toml("[popup]\nmin_width = 100\n").unwrap_err();
    /// assert_eq!(
    ///     problem.to_string(),
    ///     "invalid `popup.min_width` (line 2): 100 is larger than `popup.max_width` (76)",
    /// );
    /// ```
    pub fn from_toml(text: &str) -> Result<Self, SettingsProblem> {
        let mut settings: Self = toml::from_str(text).map_err(|error| {
            let offset = error.span().map(|span| span.start);
            SettingsProblem {
                key: offset.and_then(|offset| key_path::key_at(text, offset)),
                line: offset.map(|offset| key_path::line_at(text, offset)),
                // Without the input, the message is just the problem: no
                // "TOML parse error at" banner or source excerpt.
                message: error.message().trim_end().to_owned(),
            }
        })?;
        let written = settings.popup;
        settings.popup = written.floored();
        settings
            .check(written)
            .map_err(|(key, message)| SettingsProblem {
                line: key_path::line_of_key(text, &key),
                key: Some(key),
                message,
            })?;
        Ok(settings)
    }

    /// The check a single value's type can't express: min at most max
    /// (after [`PopupSettings::floored`]; `written` is the popup before it,
    /// so a raised max says so). Returns the offending key and what is
    /// wrong.
    fn check(&self, written: PopupSettings) -> Result<(), (String, String)> {
        let popup = &self.popup;
        for (axis, min, max, written_max) in [
            ("width", popup.min_width, popup.max_width, written.max_width),
            (
                "height",
                popup.min_height,
                popup.max_height,
                written.max_height,
            ),
        ] {
            if min > max {
                let raised = if written_max == max {
                    ""
                } else {
                    ", raised to the default"
                };
                return Err((
                    format!("popup.min_{axis}"),
                    format!("{min} is larger than `popup.max_{axis}` ({max}{raised})"),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn problem(text: &str) -> SettingsProblem {
        Settings::from_toml(text).unwrap_err()
    }

    fn located(key: &str, line: usize, message: &str) -> SettingsProblem {
        SettingsProblem {
            key: Some(key.to_owned()),
            line: Some(line),
            message: message.to_owned(),
        }
    }

    #[test]
    fn an_empty_file_is_all_defaults() {
        assert_eq!(Settings::from_toml(""), Ok(Settings::default()));
    }

    #[test]
    fn defaults_follow_the_plan() {
        let settings = Settings::default();

        assert_eq!(settings.editor, "");
        assert_eq!(settings.open_on, Category::Commands);
        assert_eq!(settings.commands.order, CommandOrder::Recent);
        assert_eq!(settings.commands.hide_kinds, Vec::<Kind>::new());
        assert_eq!(settings.commands.hide_ids, Vec::<RowId>::new());
        assert_eq!(
            settings.commands.confirm_ids,
            [RowId::new(Kind::Builtin, "close_workspace")]
        );
        assert_eq!(
            settings.commands.skip_recent_ids,
            [RowId::new(Kind::Builtin, "reload_config")]
        );
        assert_eq!(settings.jump_to.order, JumpToOrder::Recent);
        assert_eq!(settings.popup.width(), Bounds { min: 76, max: 76 });
        assert_eq!(settings.popup.height(), Bounds { min: 16, max: 22 });
        assert_eq!(settings.colors, ColorSettings::default());
        assert!(settings.history.enabled);
        assert_eq!(settings.history.max_age_days, 90);
    }

    #[test]
    fn parses_every_key() {
        let text = r##"
            editor = "code -w"
            open_on = "jump_to"
            [commands]
            order = "fixed"
            hide_kinds = ["plugin", "custom"]
            hide_ids = ["builtin:zoom", "custom:Popup: lazygit"]
            confirm_ids = []
            skip_recent_ids = ["builtin:zoom", "custom:Popup: lazygit"]
            [jump_to]
            order = "herdr"
            [popup]
            min_width = 80
            max_width = 100
            min_height = 18
            max_height = 40
            [colors]
            background = "#1E1E2E"
            muted = 8
            matches = "default"
            selected = ""
            dividers = 240
            tags = "#ff0000"
            branch = 5
            [history]
            enabled = false
            max_age_days = 0
        "##;

        let settings = Settings::from_toml(text).unwrap();

        assert_eq!(settings.editor, "code -w");
        assert_eq!(settings.open_on, Category::JumpTo);
        assert_eq!(
            settings.commands,
            CommandSettings {
                order: CommandOrder::Fixed,
                hide_kinds: vec![Kind::Plugin, Kind::Custom],
                hide_ids: vec![
                    RowId::new(Kind::Builtin, "zoom"),
                    RowId::new(Kind::Custom, "Popup: lazygit"),
                ],
                confirm_ids: vec![],
                skip_recent_ids: vec![
                    RowId::new(Kind::Builtin, "zoom"),
                    RowId::new(Kind::Custom, "Popup: lazygit"),
                ],
            }
        );
        assert_eq!(settings.jump_to.order, JumpToOrder::Herdr);
        assert_eq!(settings.popup.width(), Bounds { min: 80, max: 100 });
        assert_eq!(settings.popup.height(), Bounds { min: 18, max: 40 });
        assert_eq!(
            settings.colors,
            ColorSettings {
                background: Some(Color::Rgb(0x1e, 0x1e, 0x2e)),
                muted: Some(Color::Ansi(8)),
                matches: Some(Color::Reset),
                selected: None,
                dividers: Some(Color::Ansi(240)),
                tags: Some(Color::Rgb(255, 0, 0)),
                branch: Some(Color::Ansi(5)),
            }
        );
        assert_eq!(
            settings.history,
            HistorySettings {
                enabled: false,
                max_age_days: 0
            }
        );
    }

    #[test]
    fn hides_rows_by_kind_or_id() {
        let commands = CommandSettings {
            hide_kinds: vec![Kind::Plugin],
            hide_ids: vec![RowId::new(Kind::Custom, "Git")],
            ..CommandSettings::default()
        };
        let custom = |name: &str| Row::new(Kind::Custom, "0", "t").with_id_name(name);

        assert!(commands.hides(&Row::new(Kind::Plugin, "a.b", "t")));
        assert!(commands.hides(&custom("Git")));
        assert!(!commands.hides(&custom("Other")));
        assert!(!commands.hides(&Row::new(Kind::Pane, "w1:p1", "t")));
    }

    #[test]
    fn confirms_close_workspace_by_default() {
        let commands = CommandSettings::default();

        assert!(commands.confirms(&Row::new(Kind::Builtin, "close_workspace", "t")));
        assert!(!commands.confirms(&Row::new(Kind::Builtin, "close_tab", "t")));
    }

    #[test]
    fn skips_reload_config_in_the_recent_order_by_default() {
        let commands = CommandSettings::default();

        assert!(commands.skips_recent(&Row::new(Kind::Builtin, "reload_config", "t")));
        assert!(!commands.skips_recent(&Row::new(Kind::Builtin, "zoom", "t")));
    }

    #[test]
    fn invalid_skip_recent_ids_are_named() {
        assert_eq!(
            problem("[commands]\nskip_recent_ids = [\"builtin:nope\"]\n"),
            located(
                "commands.skip_recent_ids",
                2,
                "unknown row id `builtin:nope` (see `herdr-command-palette list --ids`)"
            )
        );
    }

    #[test]
    fn unknown_keys_are_named() {
        assert_eq!(
            problem("[popup]\nwidth = 3\n"),
            located(
                "popup.width",
                2,
                "unknown field `width`, expected one of `min_width`, `max_width`, \
                 `min_height`, `max_height`"
            )
        );
    }

    #[test]
    fn the_old_accent_color_key_is_unknown() {
        assert_eq!(
            problem("[colors]\naccent = 1\n"),
            located(
                "colors.accent",
                2,
                "unknown field `accent`, expected one of `background`, `muted`, \
                 `matches`, `selected`, `dividers`, `tags`, `branch`"
            )
        );
    }

    #[test]
    fn the_old_confirm_key_is_unknown() {
        assert_eq!(
            problem("[commands]\nconfirm = []\n"),
            located(
                "commands.confirm",
                2,
                "unknown field `confirm`, expected one of `order`, `hide_kinds`, \
                 `hide_ids`, `confirm_ids`, `skip_recent_ids`"
            )
        );
    }

    #[test]
    fn unknown_sections_are_named() {
        let problem = problem("editor = \"vim\"\n\n[colours]\nmuted = 8\n");

        assert_eq!(problem.key.as_deref(), Some("colours"));
        assert_eq!(problem.line, Some(3));
        assert!(
            problem
                .message
                .starts_with("unknown field `colours`, expected one of "),
            "{}",
            problem.message
        );
    }

    #[test]
    fn wrong_types_are_named() {
        assert_eq!(
            problem("[history]\nenabled = \"yes\"\n"),
            located(
                "history.enabled",
                2,
                "invalid type: string \"yes\", expected a boolean"
            )
        );
        assert_eq!(
            problem("[popup]\nmax_height = -1\n"),
            located(
                "popup.max_height",
                2,
                "invalid value: integer `-1`, expected u16"
            )
        );
    }

    #[test]
    fn unknown_values_list_the_accepted_ones() {
        assert_eq!(
            problem("open_on = \"panes\"\n"),
            located(
                "open_on",
                1,
                "unknown variant `panes`, expected `commands` or `jump_to`"
            )
        );
        assert_eq!(
            problem("[commands]\norder = \"frecent\"\n").message,
            "unknown variant `frecent`, expected `recent` or `fixed`"
        );
        assert_eq!(
            problem("[jump_to]\norder = \"fixed\"\n").message,
            "unknown variant `fixed`, expected `recent` or `herdr`"
        );
    }

    #[test]
    fn invalid_toml_has_a_line_but_no_key() {
        let problem = problem("[popup\nmin_width = 3\n");

        assert_eq!(problem.key, None);
        assert_eq!(problem.line, Some(1));
        assert!(!problem.message.contains("TOML parse error"), "{problem}");
    }

    #[test]
    fn raises_sizes_below_the_defaults_silently() {
        for axis in ["width", "height"] {
            let text = format!("[popup]\nmin_{axis} = 0\nmax_{axis} = 1\n");

            let settings = Settings::from_toml(&text).unwrap();

            assert_eq!(settings.popup, PopupSettings::default());
        }
    }

    #[test]
    fn a_max_below_the_default_no_longer_conflicts_with_the_min() {
        let settings = Settings::from_toml("[popup]\nmax_width = 40\nmax_height = 10\n").unwrap();

        assert_eq!(settings.popup, PopupSettings::default());
    }

    #[test]
    fn keeps_sizes_at_or_above_the_defaults() {
        let popup = PopupSettings {
            min_width: 80,
            max_width: 120,
            min_height: 16,
            max_height: 40,
        };

        assert_eq!(popup.floored(), popup);
    }

    #[test]
    fn min_width_must_not_exceed_max_width() {
        assert_eq!(
            problem("[popup]\nmin_width = 100\n"),
            located(
                "popup.min_width",
                2,
                "100 is larger than `popup.max_width` (76)"
            )
        );
    }

    #[test]
    fn a_min_above_a_raised_max_says_the_max_was_raised() {
        assert_eq!(
            problem("[popup]\nmin_width = 80\nmax_width = 60\n"),
            located(
                "popup.min_width",
                2,
                "80 is larger than `popup.max_width` (76, raised to the default)"
            )
        );
    }

    #[test]
    fn min_height_must_not_exceed_max_height() {
        assert_eq!(
            problem("[popup]\nmin_height = 30\nmax_height = 25\n"),
            located(
                "popup.min_height",
                2,
                "30 is larger than `popup.max_height` (25)"
            )
        );
    }

    #[test]
    fn problems_say_where_they_are() {
        let mut problem = located(
            "popup.min_width",
            2,
            "100 is larger than `popup.max_width` (76)",
        );
        assert_eq!(
            problem.to_string(),
            "invalid `popup.min_width` (line 2): 100 is larger than `popup.max_width` (76)"
        );
        problem.line = None;
        assert_eq!(problem.location(), "`popup.min_width`");
        problem.key = None;
        assert_eq!(problem.location(), "settings");
        problem.line = Some(3);
        assert_eq!(problem.location(), "line 3");
    }

    /// A popup bound: often near or below the defaults (16 to 76), so
    /// every case of the floor comes up, sometimes anything.
    fn bound() -> impl Strategy<Value = u16> {
        prop_oneof![0u16..=24, 0u16..=100, any::<u16>()]
    }

    /// Popup bounds from four [`bound`]s.
    fn any_popup() -> impl Strategy<Value = PopupSettings> {
        (bound(), bound(), bound(), bound()).prop_map(
            |(min_width, max_width, min_height, max_height)| PopupSettings {
                min_width,
                max_width,
                min_height,
                max_height,
            },
        )
    }

    /// The four bounds of `popup` with their defaults, in one order.
    fn pairs(popup: PopupSettings) -> [(u16, u16); 4] {
        let floor = PopupSettings::default();
        [
            (popup.min_width, floor.min_width),
            (popup.max_width, floor.max_width),
            (popup.min_height, floor.min_height),
            (popup.max_height, floor.max_height),
        ]
    }

    proptest! {
        #[test]
        fn floored_bounds_are_at_least_the_defaults(popup in any_popup()) {
            for (bound, floor) in pairs(popup.floored()) {
                prop_assert!(bound >= floor);
            }
        }

        #[test]
        fn floored_keeps_bounds_at_or_above_the_defaults(popup in any_popup()) {
            let floored = pairs(popup.floored());

            for ((bound, floor), (kept, _)) in pairs(popup).into_iter().zip(floored) {
                if bound >= floor {
                    prop_assert_eq!(kept, bound);
                }
            }
        }

        #[test]
        fn floored_is_idempotent(popup in any_popup()) {
            prop_assert_eq!(popup.floored().floored(), popup.floored());
        }

        #[test]
        fn an_axis_at_or_below_the_defaults_never_fails(popup in any_popup()) {
            let floor = PopupSettings::default();
            let text = format!(
                "[popup]\nmin_width = {}\nmax_width = {}\nmin_height = {}\nmax_height = {}\n",
                popup.min_width, popup.max_width, popup.min_height, popup.max_height,
            );

            let key = Settings::from_toml(&text).err().and_then(|problem| problem.key);

            if popup.min_width <= floor.min_width && popup.max_width <= floor.max_width {
                prop_assert_ne!(key.as_deref(), Some("popup.min_width"));
            }
            if popup.min_height <= floor.min_height && popup.max_height <= floor.max_height {
                prop_assert_ne!(key.as_deref(), Some("popup.min_height"));
            }
        }
    }
}
