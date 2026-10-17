//! Palette rows: what the user picks from.
//!
//! A [`Row`] is one line in the palette. Its [`Kind`] says where it came
//! from and how `run` executes it; the kind also decides the [`Category`]
//! (tab) it is listed under. `run <kind> <payload>` runs a row; its
//! [`RowId`] (`<kind>:<name>`) names it in the settings.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The palette's tabs, in header order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Deserialize)]
// The settings' `open_on` spells them as `as_str` does.
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Everything that runs something: plugin actions, built-ins, palette
    /// and custom commands.
    #[default]
    Commands,
    /// One row per pane, to focus it.
    JumpTo,
}

impl Category {
    /// Every category, in the order the header shows them.
    pub(crate) const ALL: [Self; 2] = [Self::Commands, Self::JumpTo];

    /// Name used on the command line (`fzf rows commands`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Commands => "commands",
            Self::JumpTo => "jump_to",
        }
    }

    /// Label shown in the header.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Commands => "Commands",
            Self::JumpTo => "Jump to",
        }
    }

    /// The category `offset` steps away, wrapping around (`-1` is the
    /// previous one).
    #[must_use]
    // Reason: `ALL` has two entries, so the `usize` -> `i64` casts are exact.
    #[expect(clippy::cast_possible_wrap)]
    pub fn cycle(self, offset: i32) -> Self {
        // `ALL` holds every category, so `position` always finds `self`.
        let len = Self::ALL.len() as i64;
        let current = Self::ALL.iter().position(|c| *c == self).unwrap_or(0) as i64;
        // `rem_euclid` keeps the result in 0..len even for negative offsets.
        let next = (current + i64::from(offset)).rem_euclid(len);
        usize::try_from(next)
            .ok()
            .and_then(|index| Self::ALL.get(index).copied())
            .unwrap_or(self)
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Category {
    type Err = ParseRowError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|category| category.as_str() == s)
            .ok_or_else(|| ParseRowError::UnknownCategory(s.to_owned()))
    }
}

/// Where a row comes from, which decides how it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A plugin action (`plugin.action.invoke`); payload: qualified action id.
    Plugin,
    /// A Herdr key action (`split_vertical`, ...); payload: its name.
    Builtin,
    /// One of the palette's own commands; payload: its name.
    Palette,
    /// A `[[keys.command]]` entry of type shell, pane or popup; payload: its
    /// index among all `[[keys.command]]` entries.
    Custom,
    /// A pane to focus; payload: its pane id (`w1:p2`).
    Pane,
}

impl Kind {
    /// Every kind, in the order the Commands tab lists them (panes last).
    pub(crate) const ALL: [Self; 5] = [
        Self::Plugin,
        Self::Builtin,
        Self::Palette,
        Self::Custom,
        Self::Pane,
    ];

    /// Name used on the command line (`run builtin zoom`) and in row ids.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plugin => "plugin",
            Self::Builtin => "builtin",
            Self::Palette => "palette",
            Self::Custom => "custom",
            Self::Pane => "pane",
        }
    }

    /// The tab rows of this kind are listed under.
    #[must_use]
    pub fn category(self) -> Category {
        match self {
            Self::Pane => Category::JumpTo,
            Self::Plugin | Self::Builtin | Self::Palette | Self::Custom => Category::Commands,
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Kind {
    type Err = ParseRowError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == s)
            .ok_or_else(|| ParseRowError::UnknownKind(s.to_owned()))
    }
}

/// Names a row as `<kind>:<name>`, e.g. `builtin:zoom`, `pane:w1:p2` or
/// `custom:Popup: lazygit`: what `list --ids` prints and the settings'
/// `hide_ids` / `confirm_ids` take.
///
/// The name is the row's payload, except for custom commands: their
/// `description`, else their `command`, else their type (the payload is an index into
/// `[[keys.command]]`, which shifts when the config changes). Two custom
/// commands with the same description share an id. The name may itself
/// contain `:` (pane ids do); parsing splits at the first one only.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RowId {
    /// The row's kind.
    pub kind: Kind,
    /// The row's name within its kind (see above).
    pub name: String,
}

impl RowId {
    /// A row id from its parts.
    #[must_use]
    pub fn new(kind: Kind, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
        }
    }
}

impl fmt::Display for RowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind, self.name)
    }
}

impl FromStr for RowId {
    type Err = ParseRowError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (kind, name) = s
            .split_once(':')
            .ok_or_else(|| ParseRowError::MissingSeparator(s.to_owned()))?;
        Ok(Self::new(kind.parse()?, name))
    }
}

/// One palette line, before layout.
///
/// Serialized as JSON into the palette state dir at open, so `fzf rows`
/// can lay the rows out again when the popup is resized.
/// A change to the serialized shape needs a new `ROWS_JSON_VERSION`
/// (`app::fzf_callbacks`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    /// Where the row comes from; decides how it runs and its category.
    pub kind: Kind,
    /// What `run` needs to execute it (see [`Kind`]).
    pub payload: String,
    /// Main text, left-aligned, after [`title_column`](Self::title_column).
    pub title: String,
    /// A fixed-width column before the title, already padded (Jump to's
    /// workspace). The minimal layout drops it and keeps it
    /// searchable, unless [`search_extra`](Self::search_extra) is set:
    /// then that must hold the column's full text.
    pub title_column: Option<String>,
    /// Right column, dimmed: key bindings (`prefix+z, prefix+f`) for
    /// commands, the agent kind (`claude`, `terminal`) for panes. May be
    /// empty.
    pub shortcut: String,
    /// Width the right column takes at least, in terminal columns: the
    /// widest shortcut of the row's list, so every row's title is cut at
    /// the same column (Jump to's agent kinds, left-aligned in it).
    /// 0 = the shortcut's own width.
    pub right_width: usize,
    /// The focused row's footer, line by line: one
    /// line for commands, the location and the cwd for panes. May be
    /// empty.
    pub footer: Vec<FooterLine>,
    /// Searchable but never shown (e.g. a workspace name cut from the title).
    /// May be empty. On a row with a [`title_column`](Self::title_column)
    /// it is that column's full text or nothing: the minimal layout then
    /// skips the column in the search text.
    pub search_extra: String,
    /// The row's id in the settings; `<kind>:<payload>` unless set with
    /// [`with_id_name`](Self::with_id_name).
    pub id: RowId,
}

impl Row {
    /// A row with the given kind, payload and title; other fields empty.
    #[must_use]
    pub fn new(kind: Kind, payload: impl Into<String>, title: impl Into<String>) -> Self {
        let payload = payload.into();
        Self {
            kind,
            id: RowId::new(kind, payload.clone()),
            payload,
            title: title.into(),
            title_column: None,
            shortcut: String::new(),
            right_width: 0,
            footer: Vec::new(),
            search_extra: String::new(),
        }
    }

    /// Sets the name part of the row's [`id`](Self::id) (custom commands).
    #[must_use]
    pub fn with_id_name(mut self, name: impl Into<String>) -> Self {
        self.id.name = name.into();
        self
    }

    /// Sets the column before the title ([`title_column`](Self::title_column)).
    #[must_use]
    pub fn with_title_column(mut self, column: impl Into<String>) -> Self {
        self.title_column = Some(column.into());
        self
    }

    /// The title as the full layout shows it: the
    /// [`title_column`](Self::title_column), two spaces, the title; just
    /// the title without a column.
    #[must_use]
    pub fn full_title(&self) -> String {
        match &self.title_column {
            Some(column) => format!("{column}  {}", self.title),
            None => self.title.clone(),
        }
    }

    /// Sets the right column.
    #[must_use]
    pub fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = shortcut.into();
        self
    }

    /// Sets the right column's minimum width ([`right_width`](Self::right_width)).
    #[must_use]
    pub fn with_right_width(mut self, right_width: usize) -> Self {
        self.right_width = right_width;
        self
    }

    /// Sets the footer's lines.
    #[must_use]
    pub fn with_footer(mut self, footer: Vec<FooterLine>) -> Self {
        self.footer = footer;
        self
    }

    /// Sets a footer of one plain line (a command's description).
    #[must_use]
    pub fn with_footer_text(self, text: impl Into<String>) -> Self {
        self.with_footer(vec![FooterLine::from_text(text)])
    }

    /// Sets the hidden search text.
    #[must_use]
    pub fn with_search_extra(mut self, search_extra: impl Into<String>) -> Self {
        self.search_extra = search_extra.into();
        self
    }

    /// The tab this row is listed under.
    #[must_use]
    pub fn category(&self) -> Category {
        self.kind.category()
    }
}

/// Joins the parts of a footer line where it is shown as text: the
/// footer (Jump to's `<workspace> / <tab> / <pane id>`) and
/// `list`. It reads as a path.
pub(crate) const PART_SEPARATOR: &str = " / ";

/// One line of a row's footer: plain text, or parts (Jump to's location:
/// workspace, tab, pane id) that the footer cuts one by one.
/// Plain text is a line of one part.
///
/// Serialized as its list of parts; read back through
/// [`from_parts`](Self::from_parts), so an empty list still gives a line
/// of one empty part.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Vec<String>", into = "Vec<String>")]
pub struct FooterLine {
    /// Never empty: a line of no parts holds one empty part.
    parts: Vec<String>,
}

impl FooterLine {
    /// A line of plain text.
    #[must_use]
    pub fn from_text(text: impl Into<String>) -> Self {
        Self {
            parts: vec![text.into()],
        }
    }

    /// A line of `parts`; no parts gives an empty line.
    #[must_use]
    pub fn from_parts<S: Into<String>>(parts: impl IntoIterator<Item = S>) -> Self {
        Self::from_vec(parts.into_iter().map(Into::into).collect())
    }

    /// [`from_parts`](Self::from_parts) without the generics, so its branch is
    /// compiled (and covered) once.
    fn from_vec(mut parts: Vec<String>) -> Self {
        if parts.is_empty() {
            parts.push(String::new());
        }
        Self { parts }
    }

    /// The line's parts, at least one.
    #[must_use]
    pub fn parts(&self) -> &[String] {
        &self.parts
    }

    /// Whether the line shows nothing: every part is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.parts.iter().all(String::is_empty)
    }

    /// The line as text: its parts joined with ` / `.
    ///
    /// # Examples
    ///
    /// ```
    /// use herdr_command_palette::domain::row::FooterLine;
    ///
    /// assert_eq!(FooterLine::from_parts(["api", "logs", "w1:p3"]).to_text(), "api / logs / w1:p3");
    /// ```
    #[must_use]
    pub fn to_text(&self) -> String {
        self.parts.join(PART_SEPARATOR)
    }
}

impl From<Vec<String>> for FooterLine {
    fn from(parts: Vec<String>) -> Self {
        Self::from_vec(parts)
    }
}

impl From<FooterLine> for Vec<String> {
    fn from(line: FooterLine) -> Self {
        line.parts
    }
}

/// A category, kind or row id that could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseRowError {
    /// Not one of `commands`, `jump_to`.
    #[error("unknown category `{0}` (expected `commands` or `jump_to`)")]
    UnknownCategory(String),
    /// Not one of `plugin`, `builtin`, `palette`, `custom`, `pane`.
    #[error("unknown row kind `{0}` (expected `plugin`, `builtin`, `palette`, `custom` or `pane`)")]
    UnknownKind(String),
    /// A row id without the `:` between kind and name.
    #[error("row id `{0}` is not `<kind>:<name>`")]
    MissingSeparator(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_round_trip_through_their_names() {
        for category in Category::ALL {
            assert_eq!(category.as_str().parse::<Category>(), Ok(category));
            assert_eq!(category.to_string(), category.as_str());
        }
        assert_eq!(Category::JumpTo.label(), "Jump to");
        assert_eq!(Category::Commands.label(), "Commands");
        assert_eq!(
            "goto".parse::<Category>(),
            Err(ParseRowError::UnknownCategory("goto".into()))
        );
    }

    #[test]
    fn categories_cycle_both_ways() {
        assert_eq!(Category::Commands.cycle(1), Category::JumpTo);
        assert_eq!(Category::JumpTo.cycle(1), Category::Commands);
        assert_eq!(Category::Commands.cycle(-1), Category::JumpTo);
        assert_eq!(Category::Commands.cycle(0), Category::Commands);
        assert_eq!(Category::Commands.cycle(-3), Category::JumpTo);
    }

    #[test]
    fn cycles_by_extreme_offsets_without_overflow() {
        assert_eq!(Category::Commands.cycle(i32::MAX), Category::JumpTo);
        assert_eq!(Category::JumpTo.cycle(i32::MIN), Category::JumpTo);
    }

    #[test]
    fn kinds_round_trip_and_pick_their_category() {
        for kind in Kind::ALL {
            assert_eq!(kind.as_str().parse::<Kind>(), Ok(kind));
            assert_eq!(kind.to_string(), kind.as_str());
        }
        assert_eq!(Kind::Pane.category(), Category::JumpTo);
        assert_eq!(Kind::Custom.category(), Category::Commands);
        let error = "goto".parse::<Kind>().unwrap_err();
        assert_eq!(
            error.to_string(),
            "unknown row kind `goto` (expected `plugin`, `builtin`, `palette`, `custom` or `pane`)"
        );
    }

    #[test]
    fn row_ids_split_at_the_first_colon() {
        let id: RowId = "pane:w1:p2".parse().unwrap();
        assert_eq!(id, RowId::new(Kind::Pane, "w1:p2"));
        assert_eq!(id.to_string(), "pane:w1:p2");
        assert_eq!(
            "zoom".parse::<RowId>(),
            Err(ParseRowError::MissingSeparator("zoom".into()))
        );
        assert_eq!(
            "nope:x".parse::<RowId>(),
            Err(ParseRowError::UnknownKind("nope".into()))
        );
    }

    #[test]
    fn rows_build_with_optional_fields() {
        let row = Row::new(Kind::Builtin, "zoom", "Toggle zoom")
            .with_shortcut("prefix+z")
            .with_footer_text("Zoom the focused pane")
            .with_search_extra("hidden");
        assert_eq!(row.category(), Category::Commands);
        assert_eq!(row.id.to_string(), "builtin:zoom");
        assert_eq!(row.shortcut, "prefix+z");
        assert_eq!(row.footer, [FooterLine::from_text("Zoom the focused pane")]);
        assert_eq!(row.search_extra, "hidden");
        let bare = Row::new(Kind::Pane, "w1:p1", "dotfiles");
        assert!(bare.shortcut.is_empty() && bare.footer.is_empty());
    }

    #[test]
    fn footer_lines_hold_at_least_one_part() {
        let empty: [&str; 0] = [];

        assert_eq!(FooterLine::from_parts(empty), FooterLine::from_text(""));
        assert!(FooterLine::from_parts(["", ""]).is_empty());
        assert!(!FooterLine::from_parts(["", "x"]).is_empty());
    }

    #[test]
    fn footer_lines_join_their_parts_as_a_path() {
        assert_eq!(FooterLine::from_text("a / b").to_text(), "a / b");
        assert_eq!(
            FooterLine::from_parts(["a / b", "c"]).parts(),
            ["a / b", "c"]
        );
    }

    #[test]
    fn full_title_puts_the_title_column_first() {
        let pane = Row::new(Kind::Pane, "w1:p1", "fix · 3").with_title_column("api ");

        assert_eq!(pane.full_title(), "api   fix · 3");
        assert_eq!(Row::new(Kind::Builtin, "zoom", "Zoom").full_title(), "Zoom");
    }

    #[test]
    fn rows_round_trip_through_json() {
        let rows = [
            Row::new(Kind::Pane, "w1:p1", "fix · 3")
                .with_title_column("api ")
                .with_shortcut("claude")
                .with_right_width(8)
                .with_search_extra("api-long")
                .with_footer(vec![
                    FooterLine::from_parts(["api", "1", "w1:p1"]),
                    FooterLine::from_text("~/src"),
                ]),
            Row::new(Kind::Custom, "0", "Lazygit").with_id_name("Popup: lazygit"),
        ];

        let json = serde_json::to_string(&rows).unwrap();

        assert_eq!(serde_json::from_str::<Vec<Row>>(&json).unwrap(), rows);
        assert!(json.contains(r#""kind":"pane""#), "{json}");
        assert!(
            json.contains(r#""footer":[["api","1","w1:p1"],["~/src"]]"#),
            "{json}"
        );
    }

    #[test]
    fn footer_lines_read_from_json_hold_at_least_one_part() {
        let line: FooterLine = serde_json::from_str("[]").unwrap();

        assert_eq!(line, FooterLine::from_text(""));
    }

    #[test]
    fn custom_rows_name_their_id() {
        let row = Row::new(Kind::Custom, "3", "Lazygit").with_id_name("Popup: lazygit");

        assert_eq!(row.id.to_string(), "custom:Popup: lazygit");
        assert_eq!(row.payload, "3");
    }

    #[test]
    fn open_on_names_parse_like_category_names() {
        #[derive(Deserialize)]
        struct OpenOn {
            open_on: Category,
        }

        let parsed: OpenOn = toml::from_str("open_on = \"jump_to\"").unwrap();

        assert_eq!(parsed.open_on, Category::JumpTo);
        assert_eq!(Category::default(), Category::Commands);
    }
}
