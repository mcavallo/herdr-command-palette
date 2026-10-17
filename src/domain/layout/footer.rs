//! The footer: the focused row's description, or the palette's key hints,
//! with the toggle key's `[^/]` right-aligned on the last line, and in
//! Jump to the focused pane's git branch before its cwd.
//!
//! fzf asks for it through a callback on every focus change and on the
//! toggle key; the caller keeps the sticky [`FooterMode`] between calls.
//! Each category has a fixed number of lines in both modes (Commands 1,
//! Jump to 2), so toggling never moves the list, and in both layouts
//! too, so a resize doesn't either.

use std::fmt;
use std::str::FromStr;

use crate::domain::color::Color;
use crate::domain::row::{Category, FooterLine, Kind, PART_SEPARATOR};
use crate::domain::style::TextColors;

use super::row_line::tag;
use super::{
    ELLIPSIS, GAP, GUTTER, LayoutMode, RESET, clean, inner_width, truncate, truncate_start, width,
};

/// The palette's own keys, worded like Herdr's keybinds modal.
/// The toggle key has its own hint, [`KEYS_HINT`].
pub(crate) const KEY_HINTS: &str = "↑↓/pgup/pgdn · category tab/s-tab · select enter · close esc";

/// The toggle key (`ctrl-/`), right-aligned on the footer's last line in
/// both modes so it's always visible.
const KEYS_HINT: &str = "[^/]";

/// Joins the branch and the cwd on Jump to's last line.
const BRANCH_SEPARATOR: &str = " · ";

/// Columns of Jump to's last line that aren't shared between the branch
/// and the cwd: ` · ` (3), one space and `[^/]` (4). The branch gets at
/// most 30% of the rest.
const BRANCH_RESERVED: usize = 8;

/// A git branch (or short hash) for Jump to's last line, before the cwd,
/// and its color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FooterBranch<'a> {
    /// The name or short hash to show.
    pub label: &'a str,
    /// Its color; `None` leaves it in the footer's color.
    pub color: Option<Color>,
}

/// What the footer shows. Sticky: toggling survives cursor moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FooterMode {
    /// The focused row's description (the default).
    #[default]
    Description,
    /// The palette's key hints (`KEY_HINTS`).
    Keys,
}

impl FooterMode {
    /// Every mode.
    pub(crate) const ALL: [Self; 2] = [Self::Description, Self::Keys];

    /// The other mode: what the toggle key switches to.
    #[must_use]
    pub fn toggled(self) -> Self {
        match self {
            Self::Description => Self::Keys,
            Self::Keys => Self::Description,
        }
    }

    /// Name for a state file: `desc` or `keys`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Description => "desc",
            Self::Keys => "keys",
        }
    }
}

impl fmt::Display for FooterMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Text that names no [`FooterMode`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown footer mode `{0}` (expected `desc` or `keys`)")]
pub struct ParseFooterModeError(pub String);

impl FromStr for FooterMode {
    type Err = ParseFooterModeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|mode| mode.as_str() == s)
            .ok_or_else(|| ParseFooterModeError(s.to_owned()))
    }
}

/// The footer of `category` in a popup `columns` wide, laid out as
/// `layout` says, its lines joined with `\n` (fzf resizes the footer to
/// fit); `branch` goes before the cwd in Jump to's description mode and
/// is ignored elsewhere.
///
/// `lines` are the focused row's footer lines (fzf's `{4}` parsed back,
/// see [`footer_lines`](super::footer_lines()) and
/// [`tags::parse`](crate::domain::tags::parse())). In
/// [`FooterMode::Keys`], or when the row's first line is empty or
/// missing, the footer shows `KEY_HINTS` instead.
///
/// Each category has a fixed number of lines: Commands 1, Jump to 2
/// (missing lines are empty, extra ones dropped), in both layouts. How a
/// line is cut is set by its place in that layout (`cut_of`): Jump to's
/// line 1 is a pane's location, cut workspace first (`fit_location`); its
/// line 2 is the cwd, cut from the left (its end matters), after the
/// `branch` when there is one (`<branch> · <cwd>`, see `with_branch`);
/// every other line, and the key hints, is cut at the end, all with `…`.
/// A line's parts are joined with ` / `. In [`LayoutMode::Full`] the last
/// line always ends with `KEYS_HINT` right-aligned; the text before it
/// gives up its room to keep a 2-column gap, down to nothing in a narrow
/// popup. [`LayoutMode::Minimal`] has no `KEYS_HINT` (the toggle key
/// still works): every line gets the whole width. Every line
/// starts after a 1-column gutter and never reaches the last column; a
/// line with no room for its text is empty, and a last line with
/// no room for the hint too. Control characters become spaces and tags
/// are removed, so the line count holds.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::layout::{self, FooterBranch, FooterMode, LayoutMode};
/// use herdr_command_palette::domain::row::{Category, FooterLine};
///
/// let full = LayoutMode::Full;
/// let close = [FooterLine::from_text("Close the pane")];
/// assert_eq!(
///     layout::footer(FooterMode::Description, Category::Commands, &close, None, 30, full),
///     " Close the pane          [^/]",
/// );
/// let pane = [FooterLine::from_parts(["api", "build", "w1:p2"]), FooterLine::from_text("~/src/api")];
/// assert_eq!(
///     layout::footer(FooterMode::Description, Category::JumpTo, &pane, None, 30, full),
///     " api / build / w1:p2\n ~/src/api               [^/]",
/// );
/// let main = FooterBranch { label: "main", color: None };
/// assert_eq!(
///     layout::footer(FooterMode::Description, Category::JumpTo, &pane, Some(main), 30, full),
///     " api / build / w1:p2\n main · ~/src/api        [^/]",
/// );
/// // Minimal: the same lines, no `[^/]`.
/// assert_eq!(
///     layout::footer(FooterMode::Description, Category::Commands, &close, None, 12, LayoutMode::Minimal),
///     " Close the…",
/// );
/// ```
#[must_use]
pub fn footer(
    mode: FooterMode,
    category: Category,
    lines: &[FooterLine],
    branch: Option<FooterBranch<'_>>,
    columns: usize,
    layout: LayoutMode,
) -> String {
    let inner = inner_width(columns);
    let shows_keys = mode == FooterMode::Keys || lines.first().is_none_or(FooterLine::is_empty);
    let count = line_count(category);
    (0..count)
        .map(|index| {
            // Key hints are one text on the first line; the row's lines
            // belong to its description.
            let (parts, cut) = if shows_keys {
                let parts = if index == 0 {
                    vec![KEY_HINTS.to_owned()]
                } else {
                    Vec::new()
                };
                (parts, Cut::End)
            } else {
                // Cleaned again (rows already are): a newline would add a line.
                let parts = lines
                    .get(index)
                    .map(|line| line.parts().iter().map(|part| clean(part)).collect())
                    .unwrap_or_default();
                (parts, cut_of(category, index))
            };
            // Only the cwd line has a branch, and only in description mode.
            let branch = branch.filter(|_| cut == Cut::Cwd);
            let is_last = index + 1 == count;
            if is_last && layout == LayoutMode::Full {
                last_line(&parts, cut, branch, inner)
            } else {
                line(&parts, cut, branch, inner)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The cwd that `category`'s footer shows for the row `lines` in
/// description mode, as the row wrote it (home as `~`): Jump to's line 2,
/// when line 1 isn't empty (else the footer shows the key hints). `None`
/// when there is none, so nothing to look a branch up for.
#[must_use]
pub fn shown_cwd(category: Category, lines: &[FooterLine]) -> Option<String> {
    let index = (0..line_count(category)).find(|&index| cut_of(category, index) == Cut::Cwd)?;
    if lines.first().is_none_or(FooterLine::is_empty) {
        return None;
    }
    lines
        .get(index)
        .filter(|line| !line.is_empty())
        .map(FooterLine::to_text)
}

/// How many lines `category`'s footer has, in both modes.
fn line_count(category: Category) -> usize {
    match category {
        Category::Commands => 1,
        Category::JumpTo => 2,
    }
}

/// How line `index` (from 0) of `category`'s footer is cut: set in code by
/// its place in the layout, not by the row.
fn cut_of(category: Category, index: usize) -> Cut {
    match (category, index) {
        (Category::JumpTo, 0) => Cut::Location,
        (Category::JumpTo, 1) => Cut::Cwd,
        _ => Cut::End,
    }
}

/// How a footer line that doesn't fit is cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cut {
    /// Keep the start, `…` at the end.
    End,
    /// A cwd: keep the end, `…` at the start; after the branch when
    /// there is one (`with_branch`).
    Cwd,
    /// A pane's location: the workspace first (`fit_location`).
    Location,
}

/// `parts` joined with ` / ` and cut to `room` columns as `cut` says.
fn fit_parts(parts: &[String], cut: Cut, room: usize) -> String {
    match cut {
        Cut::End => truncate(&parts.join(PART_SEPARATOR), room),
        Cut::Cwd => truncate_start(&parts.join(PART_SEPARATOR), room),
        Cut::Location => fit_location(parts, room),
    }
}

/// A footer line without `KEYS_HINT` (every line but a full layout's
/// last): `parts` cut as `cut` says (after `branch`, when given) to
/// `inner` columns.
fn line(parts: &[String], cut: Cut, branch: Option<FooterBranch<'_>>, inner: usize) -> String {
    if inner == 0 {
        return String::new();
    }
    let shown = match branch {
        Some(branch) => with_branch(&parts.join(PART_SEPARATOR), branch, inner, inner).0,
        None => fit_parts(parts, cut, inner),
    };
    format!("{GUTTER}{shown}")
}

/// A pane's location (`[<workspace>, <tab>, <pane id>]`, or no tab) joined
/// with ` / ` and cut to `room` columns so that ` / <tab> / <pane id>`
/// stays: the workspace is cut with `…` first, down to `…`
/// alone. When even `… / <tab> / <pane id>` doesn't fit, the whole
/// location is cut from the left, keeping its end (`…ab / w1:p3`). A
/// location of one part is cut at the end. The parts come apart as the
/// row built them, so a tab label with ` / ` in it stays whole.
fn fit_location(parts: &[String], room: usize) -> String {
    let location = parts.join(PART_SEPARATOR);
    if width(&location) <= room {
        return location;
    }
    let Some((workspace, place)) = parts.split_first().filter(|(_, place)| !place.is_empty())
    else {
        return truncate(&location, room);
    };
    let tail = format!("{PART_SEPARATOR}{}", place.join(PART_SEPARATOR));
    match room.checked_sub(width(&tail)) {
        Some(left) if left >= width(ELLIPSIS) => format!("{}{tail}", truncate(workspace, left)),
        _ => truncate_start(&location, room),
    }
}

/// The last line of a [`LayoutMode::Full`] footer: `parts` cut as `cut` says (after `branch`, when
/// given), then `KEYS_HINT` right-aligned. The hint is never cut: the
/// text gives up its room first; without room for the hint itself the
/// line is empty.
fn last_line(parts: &[String], cut: Cut, branch: Option<FooterBranch<'_>>, inner: usize) -> String {
    let hint_width = width(KEYS_HINT);
    if inner < hint_width {
        return String::new();
    }
    let room = inner.saturating_sub(hint_width + GAP);
    let (shown, shown_width) = if let Some(branch) = branch {
        with_branch(&parts.join(PART_SEPARATOR), branch, inner, room)
    } else {
        let shown = fit_parts(parts, cut, room);
        let shown_width = width(&shown);
        (shown, shown_width)
    };
    // `shown` fits in `room`, so this doesn't saturate; if it ever did,
    // the line would only lose its padding.
    let pad = inner.saturating_sub(shown_width + hint_width);
    format!("{GUTTER}{shown}{:pad$}{KEYS_HINT}", "")
}

/// `<branch> · <cwd>` in `room` columns of a line `inner` wide, and its
/// width (the branch's color escapes take none). The branch is cut at
/// the end to `branch_cap`; the cwd gets the rest, cut from the left.
/// No room for the branch (a narrow popup) or nothing visible in it
/// (zero-width characters): the cwd alone; no cwd: the branch alone.
/// Neither has a separator.
fn with_branch(cwd: &str, branch: FooterBranch<'_>, inner: usize, room: usize) -> (String, usize) {
    let label = truncate(&clean(branch.label), branch_cap(inner));
    // Width, not length: zero-width characters fit a cap of 0, and a
    // separator with no room for it would overflow the line.
    if width(&label) == 0 {
        let cwd = truncate_start(cwd, room);
        let cwd_width = width(&cwd);
        return (cwd, cwd_width);
    }
    let label_width = width(&label);
    let painted = match branch.color {
        Some(color) => format!("{}{label}{RESET}", color.to_ansi_fg()),
        None => label,
    };
    if cwd.is_empty() {
        return (painted, label_width);
    }
    // At least 2 columns whenever the branch shows (`branch_cap`), so the
    // cwd never vanishes behind a dangling separator.
    let cwd_room = room.saturating_sub(label_width + width(BRANCH_SEPARATOR));
    let cwd = truncate_start(cwd, cwd_room);
    let shown_width = label_width + width(BRANCH_SEPARATOR) + width(&cwd);
    (format!("{painted}{BRANCH_SEPARATOR}{cwd}"), shown_width)
}

/// Whether a footer `columns` wide has room for a git branch: no lookup
/// is worth running otherwise.
pub(crate) fn fits_branch(columns: usize) -> bool {
    branch_cap(inner_width(columns)) > 0
}

/// The widest a branch gets on a line `inner` wide: 30% (rounded down)
/// of what `BRANCH_RESERVED` leaves; 71 inner columns give 18.
fn branch_cap(inner: usize) -> usize {
    inner.saturating_sub(BRANCH_RESERVED) * 3 / 10
}

/// `footer` (from [`footer()`]) with the label of a `kind` row's leading
/// tag (`custom` of `custom · `, or `custom` alone) in the `tags` color;
/// the ` · ` and the description keep the footer's color. Anything
/// else comes back unchanged: no `tags` color, a kind without a tag, key
/// hints, or a tag cut short. Escapes take no columns, so the layout holds.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::layout;
/// use herdr_command_palette::domain::row::Kind;
/// use herdr_command_palette::domain::style::TextColors;
/// use herdr_command_palette::domain::color::Color;
///
/// let colors = TextColors { muted: None, tags: Some(Color::Ansi(5)), branch: None };
/// assert_eq!(
///     layout::color_tag(" custom · lazygit", Kind::Custom, colors),
///     " \x1b[38;5;5mcustom\x1b[0m · lazygit",
/// );
/// ```
#[must_use]
pub fn color_tag(footer: &str, kind: Kind, colors: TextColors) -> String {
    let (Some(color), Some(name)) = (colors.tags, tag(kind)) else {
        return footer.to_owned();
    };
    let Some(text) = footer.strip_prefix(GUTTER) else {
        return footer.to_owned();
    };
    // The tag is the whole text or its first word.
    let Some(rest) = text
        .strip_prefix(name)
        .filter(|rest| rest.is_empty() || rest.starts_with(' '))
    else {
        return footer.to_owned();
    };
    format!("{GUTTER}{}{name}{RESET}{rest}", color.to_ansi_fg())
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use proptest::prelude::*;

    use crate::domain::color::Color;
    use crate::domain::fixtures::{CORPUS, text};

    use super::*;

    /// [`footer`] in the full layout.
    fn full_footer(
        mode: FooterMode,
        category: Category,
        lines: &[FooterLine],
        branch: Option<FooterBranch<'_>>,
        columns: usize,
    ) -> String {
        footer(mode, category, lines, branch, columns, LayoutMode::Full)
    }

    fn commands(mode: FooterMode, description: &str, columns: usize) -> String {
        full_footer(
            mode,
            Category::Commands,
            &[FooterLine::from_text(description)],
            None,
            columns,
        )
    }

    /// `location` written as `ws / tab / id`, one part per segment.
    fn parts(location: &str) -> Vec<String> {
        location.split(PART_SEPARATOR).map(str::to_owned).collect()
    }

    fn jump_to(mode: FooterMode, location: &str, cwd: &str, columns: usize) -> String {
        let lines = [
            FooterLine::from_parts(parts(location)),
            FooterLine::from_text(cwd),
        ];
        full_footer(mode, Category::JumpTo, &lines, None, columns)
    }

    #[test]
    fn right_aligns_the_toggle_hint() {
        assert_eq!(
            commands(FooterMode::Description, "Split the pane", 30),
            " Split the pane          [^/]",
        );
    }

    #[test]
    fn cuts_a_long_description_before_the_hint() {
        let description = "x".repeat(40);

        assert_eq!(
            commands(FooterMode::Description, &description, 30),
            format!(" {}…  [^/]", "x".repeat(21)),
        );
    }

    #[test]
    fn shows_key_hints_and_the_toggle_hint_in_keys_mode() {
        assert_eq!(
            commands(FooterMode::Keys, "desc", 76),
            format!(" {KEY_HINTS}{:10}[^/]", ""),
        );
    }

    #[test]
    fn shows_key_hints_without_description() {
        assert_eq!(
            commands(FooterMode::Description, "", 76),
            commands(FooterMode::Keys, "", 76),
        );
    }

    #[test]
    fn key_hints_and_toggle_hint_fit_the_default_popup() {
        // The 76-column popup leaves 73 usable columns, 71 inside
        // the gutters.
        assert!(width(KEY_HINTS) + GAP + width(KEYS_HINT) <= 71);
    }

    #[test]
    fn toggle_hint_wins_in_a_narrow_popup() {
        assert_eq!(commands(FooterMode::Keys, "", 12), " ↑↓/…  [^/]");
        assert_eq!(commands(FooterMode::Description, "abcdef", 9), " …  [^/]");
        assert_eq!(commands(FooterMode::Description, "abcdef", 8), "   [^/]");
        assert_eq!(commands(FooterMode::Description, "abcdef", 6), " [^/]");
    }

    #[test]
    fn is_empty_without_room_for_the_toggle_hint() {
        assert_eq!(commands(FooterMode::Keys, "", 5), "");
        assert_eq!(commands(FooterMode::Keys, "", 0), "");
        assert_eq!(jump_to(FooterMode::Keys, "", "", 2), "\n");
    }

    #[test]
    fn jump_to_shows_the_cwd_on_its_own_line() {
        assert_eq!(
            jump_to(
                FooterMode::Description,
                "api / build / w2:p2",
                "~/src/api",
                40
            ),
            " api / build / w2:p2\n ~/src/api                         [^/]",
        );
    }

    #[test]
    fn jump_to_cuts_a_long_cwd_from_the_left() {
        let cwd = "~/Code/herdr-command-palette/src/domain/layout";

        assert_eq!(
            jump_to(FooterMode::Description, "ws / tab / w1:p1", cwd, 30),
            " ws / tab / w1:p1\n …tte/src/domain/layout  [^/]",
        );
    }

    /// Jump to's footer for `cwd` with the branch `label`, uncolored.
    fn with_branch_shown(label: &str, cwd: &str, columns: usize) -> String {
        let lines = [
            FooterLine::from_parts(["ws", "tab", "w1:p1"]),
            FooterLine::from_text(cwd),
        ];
        let branch = FooterBranch { label, color: None };
        full_footer(
            FooterMode::Description,
            Category::JumpTo,
            &lines,
            Some(branch),
            columns,
        )
    }

    /// The last line of `footer`.
    fn last(footer: &str) -> &str {
        footer.split('\n').next_back().unwrap()
    }

    #[test]
    fn fits_a_branch_from_12_inner_columns() {
        assert!(!fits_branch(13));
        assert!(fits_branch(14));
    }

    #[test]
    fn jump_to_shows_the_branch_before_the_cwd() {
        assert_eq!(
            last(&with_branch_shown("main", "~/src/api", 40)),
            " main · ~/src/api                  [^/]"
        );
    }

    #[test]
    fn caps_the_branch_at_30_percent() {
        // The 76-column popup: fzf has 73 columns, 71 inside the gutters,
        // 63 without ` · `, a space and `[^/]`, 18 for the branch.
        let branch = "feature/a-very-long-branch-name";
        let cwd = "~/Code/herdr-command-palette/src/domain/layout";

        let line = with_branch_shown(branch, cwd, 73);

        assert_eq!(
            last(&line),
            " feature/a-very-lo… · …ode/herdr-command-palette/src/domain/layout  [^/]"
        );
        assert_eq!(branch_cap(71), 18);
    }

    #[test]
    fn keeps_a_branch_under_the_cap_whole_and_gives_the_cwd_the_rest() {
        let cwd = "~/Code/herdr-command-palette/src/domain/layout";

        assert_eq!(
            last(&with_branch_shown("main", cwd, 30)),
            " main · …/domain/layout  [^/]"
        );
    }

    #[test]
    fn shows_the_cwd_alone_without_room_for_the_branch() {
        // 11 inside the gutters: a cap of 0.
        assert_eq!(
            last(&with_branch_shown("main", "~/src/x", 13)),
            " …rc/x  [^/]"
        );
        assert_eq!(branch_cap(11), 0);
        assert_eq!(branch_cap(12), 1);
    }

    #[test]
    fn shows_the_cwd_alone_for_a_zero_width_branch() {
        // A zero-width branch takes no room: the cwd gets the line without
        // overflowing its padding.
        assert_eq!(last(&with_branch_shown("\u{200b}", "~/src/x", 6)), " [^/]");
        assert_eq!(
            last(&with_branch_shown("\u{200b}", "~/src/x", 20)),
            " ~/src/x       [^/]"
        );
    }

    #[test]
    fn shows_the_branch_alone_without_a_cwd() {
        assert_eq!(
            last(&with_branch_shown("main", "", 30)),
            " main                    [^/]"
        );
    }

    #[test]
    fn colors_the_branch_only() {
        let lines = [
            FooterLine::from_parts(["ws", "w1:p1"]),
            FooterLine::from_text("~/src"),
        ];
        let branch = FooterBranch {
            label: "main",
            color: Some(Color::Ansi(5)),
        };

        let shown = full_footer(
            FooterMode::Description,
            Category::JumpTo,
            &lines,
            Some(branch),
            30,
        );

        insta::assert_snapshot!(last(&shown).replace('\x1b', "^["), @" ^[[38;5;5mmain^[[0m · ~/src            [^/]");
    }

    #[test]
    fn cleans_the_branch() {
        assert_eq!(
            last(&with_branch_shown("ma\nin{hcp:line}", "~", 30)),
            " ma in · ~               [^/]"
        );
    }

    #[test]
    fn ignores_the_branch_in_keys_mode_and_in_commands() {
        let lines = [
            FooterLine::from_parts(["ws", "w1:p1"]),
            FooterLine::from_text("~/src"),
        ];
        let branch = Some(FooterBranch {
            label: "main",
            color: None,
        });

        for (mode, category) in [
            (FooterMode::Keys, Category::JumpTo),
            (FooterMode::Description, Category::Commands),
        ] {
            assert_eq!(
                full_footer(mode, category, &lines, branch, 40),
                full_footer(mode, category, &lines, None, 40)
            );
        }
    }

    #[test]
    fn finds_the_cwd_the_footer_shows() {
        let pane = [
            FooterLine::from_parts(["ws", "w1:p1"]),
            FooterLine::from_text("~/src"),
        ];
        let no_location = [FooterLine::from_text(""), FooterLine::from_text("~/src")];
        let no_cwd = [
            FooterLine::from_parts(["ws", "w1:p1"]),
            FooterLine::from_text(""),
        ];

        assert_eq!(shown_cwd(Category::JumpTo, &pane), Some("~/src".to_owned()));
        assert_eq!(shown_cwd(Category::Commands, &pane), None);
        assert_eq!(shown_cwd(Category::JumpTo, &no_location), None);
        assert_eq!(shown_cwd(Category::JumpTo, &no_cwd), None);
        assert_eq!(shown_cwd(Category::JumpTo, &pane[..1]), None);
        assert_eq!(shown_cwd(Category::JumpTo, &[]), None);
    }

    #[test]
    fn jump_to_without_cwd_shows_only_the_hint() {
        assert_eq!(
            jump_to(FooterMode::Description, "ws / tab / w1:p1", "", 30),
            " ws / tab / w1:p1\n                         [^/]",
        );
    }

    #[test]
    fn jump_to_cuts_the_first_line_at_the_full_width() {
        let description = "x".repeat(40);

        assert_eq!(
            jump_to(FooterMode::Description, &description, "", 30),
            format!(" {}…\n {:24}[^/]", "x".repeat(27), ""),
        );
    }

    #[test]
    fn jump_to_cuts_the_workspace_first() {
        let location = "herdr-command-palette-worktree / 1 / w1:p3";

        assert_eq!(
            jump_to(FooterMode::Description, location, "", 30),
            format!(" herdr-command-p… / 1 / w1:p3\n {:24}[^/]", ""),
        );
    }

    #[test]
    fn jump_to_cuts_the_workspace_down_to_the_ellipsis() {
        assert_eq!(
            fit_location(&parts("workspace / logs / w1:p3"), 16),
            "… / logs / w1:p3"
        );
    }

    #[test]
    fn jump_to_keeps_the_end_when_tab_and_id_do_not_fit() {
        assert_eq!(
            fit_location(&parts("workspace / logs / w1:p3"), 15),
            "…/ logs / w1:p3"
        );
        assert_eq!(
            fit_location(&parts("workspace / logs / w1:p3"), 13),
            "…logs / w1:p3"
        );
        assert_eq!(fit_location(&parts("workspace / w1:p3"), 7), "… w1:p3");
    }

    #[test]
    fn jump_to_cuts_the_workspace_of_a_location_without_tab() {
        assert_eq!(
            fit_location(&parts("workspace / w1:p3"), 12),
            "wor… / w1:p3"
        );
    }

    #[test]
    fn jump_to_keeps_a_fitting_location_whole() {
        assert_eq!(
            fit_location(&parts("api / logs / w1:p3"), 18),
            "api / logs / w1:p3"
        );
    }

    #[test]
    fn keeps_a_tab_label_with_a_separator_whole() {
        let location = ["workspace", "feat / x", "w1:p3"].map(str::to_owned);

        assert_eq!(fit_location(&location, 22), "wo… / feat / x / w1:p3");
        assert_eq!(fit_location(&location, 20), "… / feat / x / w1:p3");
        assert_eq!(fit_location(&location, 19), "…/ feat / x / w1:p3");
    }

    #[test]
    fn cuts_a_location_of_one_part_at_the_end() {
        assert_eq!(fit_location(&parts("workspace"), 5), "work…");
        assert_eq!(fit_location(&[], 5), "");
    }

    #[test]
    fn missing_lines_are_empty_and_extra_lines_dropped() {
        let one = [FooterLine::from_parts(["api", "w1:p3"])];
        let three = [
            FooterLine::from_text("a"),
            FooterLine::from_text("b"),
            FooterLine::from_text("c"),
        ];

        assert_eq!(
            full_footer(FooterMode::Description, Category::JumpTo, &one, None, 30),
            format!(" api / w1:p3\n {:24}[^/]", "")
        );
        assert_eq!(
            full_footer(
                FooterMode::Description,
                Category::Commands,
                &three,
                None,
                30
            ),
            format!(" a{:23}[^/]", "")
        );
    }

    #[test]
    fn shows_key_hints_when_the_first_line_is_empty() {
        let lines = [FooterLine::from_text(""), FooterLine::from_text("~/src")];

        assert_eq!(
            full_footer(FooterMode::Description, Category::JumpTo, &lines, None, 76),
            full_footer(FooterMode::Keys, Category::JumpTo, &[], None, 76),
        );
    }

    #[test]
    fn cuts_lines_by_their_place_in_the_layout() {
        assert_eq!(cut_of(Category::JumpTo, 0), Cut::Location);
        assert_eq!(cut_of(Category::JumpTo, 1), Cut::Cwd);
        assert_eq!(cut_of(Category::JumpTo, 2), Cut::End);
        assert_eq!(cut_of(Category::Commands, 0), Cut::End);
    }

    #[test]
    fn jump_to_keys_mode_puts_the_hint_alone_on_line_two() {
        assert_eq!(
            jump_to(FooterMode::Keys, "ws / tab / w1:p1", "~/src", 76),
            format!(" {KEY_HINTS}\n {:70}[^/]", ""),
        );
    }

    #[test]
    fn line_count_depends_on_the_category_only() {
        for category in Category::ALL {
            for mode in FooterMode::ALL {
                for (description, detail) in [("", ""), ("d", ""), ("d", "~/x")] {
                    let lines = [
                        FooterLine::from_text(description),
                        FooterLine::from_text(detail),
                    ];
                    let shown = full_footer(mode, category, &lines, None, 40);

                    let lines = match category {
                        Category::Commands => 1,
                        Category::JumpTo => 2,
                    };
                    assert_eq!(shown.split('\n').count(), lines);
                }
            }
        }
    }

    /// [`footer`] in the minimal layout.
    fn minimal_footer(
        mode: FooterMode,
        category: Category,
        lines: &[FooterLine],
        branch: Option<FooterBranch<'_>>,
        columns: usize,
    ) -> String {
        footer(mode, category, lines, branch, columns, LayoutMode::Minimal)
    }

    #[test]
    fn minimal_footer_has_no_toggle_hint() {
        let close = [FooterLine::from_text("Close the pane")];

        assert_eq!(
            minimal_footer(
                FooterMode::Description,
                Category::Commands,
                &close,
                None,
                30
            ),
            " Close the pane"
        );
        assert_eq!(
            minimal_footer(
                FooterMode::Description,
                Category::Commands,
                &close,
                None,
                10
            ),
            " Close t…"
        );
    }

    #[test]
    fn minimal_footer_keeps_jump_tos_two_lines() {
        let lines = [
            FooterLine::from_parts(["herdr-command-palette", "1", "w1:p3"]),
            FooterLine::from_text("~/Code/herdr-command-palette/src"),
        ];

        assert_eq!(
            minimal_footer(FooterMode::Description, Category::JumpTo, &lines, None, 24),
            " herdr-com… / 1 / w1:p3\n …r-command-palette/src"
        );
    }

    #[test]
    fn minimal_footer_shows_the_branch_before_the_cwd() {
        let lines = [
            FooterLine::from_parts(["api", "w1:p3"]),
            FooterLine::from_text("~/Code/herdr-command-palette/src"),
        ];
        let main = FooterBranch {
            label: "main",
            color: None,
        };

        let shown = minimal_footer(
            FooterMode::Description,
            Category::JumpTo,
            &lines,
            Some(main),
            30,
        );

        assert_eq!(last(&shown), " main · …-command-palette/src");
    }

    #[test]
    fn minimal_footer_cuts_key_hints_and_keeps_the_line_count() {
        assert_eq!(
            minimal_footer(FooterMode::Keys, Category::Commands, &[], None, 20),
            " ↑↓/pgup/pgdn · ca…"
        );
        assert_eq!(
            minimal_footer(FooterMode::Keys, Category::JumpTo, &[], None, 20),
            " ↑↓/pgup/pgdn · ca…\n "
        );
    }

    #[test]
    fn minimal_footer_is_empty_without_room() {
        let close = [FooterLine::from_text("Close the pane")];

        assert_eq!(
            minimal_footer(FooterMode::Description, Category::Commands, &close, None, 2),
            ""
        );
        assert_eq!(
            minimal_footer(FooterMode::Keys, Category::JumpTo, &[], None, 0),
            "\n"
        );
    }

    fn tags(color: Option<Color>) -> TextColors {
        TextColors {
            muted: None,
            tags: color,
            branch: None,
        }
    }

    #[test]
    fn colors_the_tag_label_only() {
        let line = commands(FooterMode::Description, "plugin · Say hello", 40);

        let colored = color_tag(&line, Kind::Plugin, tags(Some(Color::Ansi(2))));

        // The label is its own span; ` · ` and the description are not.
        insta::assert_snapshot!(colored.replace('\x1b', "^["), @" ^[[38;5;2mplugin^[[0m · Say hello                [^/]");
    }

    #[test]
    fn colors_a_tag_without_description() {
        let line = commands(FooterMode::Description, "palette", 40);

        let colored = color_tag(&line, Kind::Palette, tags(Some(Color::Reset)));

        assert!(
            colored.starts_with(" \x1b[39mpalette\x1b[0m "),
            "{colored:?}"
        );
    }

    #[test]
    fn leaves_untagged_or_uncolored_footers_alone() {
        let custom = commands(FooterMode::Description, "custom · make", 40);
        let keys = commands(FooterMode::Keys, "custom · make", 76);
        let color = tags(Some(Color::Ansi(2)));

        assert_eq!(color_tag(&custom, Kind::Custom, tags(None)), custom);
        assert_eq!(color_tag(&custom, Kind::Builtin, color), custom);
        assert_eq!(color_tag(&keys, Kind::Custom, color), keys);
        assert_eq!(color_tag("", Kind::Custom, color), "");
    }

    #[test]
    fn toggles_between_modes() {
        assert_eq!(FooterMode::default().toggled(), FooterMode::Keys);
        assert_eq!(FooterMode::Keys.toggled(), FooterMode::Description);
    }

    #[test]
    fn round_trips_mode_names() {
        for mode in [FooterMode::Description, FooterMode::Keys] {
            assert_eq!(mode.to_string().parse(), Ok(mode));
        }
    }

    #[test]
    fn rejects_unknown_mode_names() {
        assert_eq!(
            "both".parse::<FooterMode>(),
            Err(ParseFooterModeError("both".to_owned())),
        );
        assert_eq!(
            ParseFooterModeError("both".to_owned()).to_string(),
            "unknown footer mode `both` (expected `desc` or `keys`)",
        );
    }

    #[test]
    fn snapshot_footer_corpus() {
        let mut out = String::new();
        for description in CORPUS {
            for columns in [30, 20, 12] {
                let line = commands(FooterMode::Description, description, columns);
                writeln!(out, "{columns:>2} |{line}|").unwrap();
                let lines = jump_to(
                    FooterMode::Description,
                    "ws / tab / w1:p1",
                    description,
                    columns,
                );
                for line in lines.split('\n') {
                    writeln!(out, "{columns:>2} |{line}|").unwrap();
                }
            }
        }
        writeln!(out, "76 |{}|", commands(FooterMode::Keys, "", 76)).unwrap();
        for line in jump_to(FooterMode::Keys, "", "", 76).split('\n') {
            writeln!(out, "76 |{line}|").unwrap();
        }
        insta::assert_snapshot!(out);
    }

    fn mode(keys: bool) -> FooterMode {
        if keys {
            FooterMode::Keys
        } else {
            FooterMode::Description
        }
    }

    fn category(jump_to: bool) -> Category {
        if jump_to {
            Category::JumpTo
        } else {
            Category::Commands
        }
    }

    proptest! {
        #[test]
        fn every_line_fits_inside_the_gutters(
            description in text(),
            detail in text(),
            keys in any::<bool>(),
            jump in any::<bool>(),
            columns in 0usize..90,
        ) {
            let lines = [FooterLine::from_parts(parts(&description)), FooterLine::from_text(detail)];
            let shown = full_footer(mode(keys), category(jump), &lines, None, columns);

            for line in shown.split('\n') {
                prop_assert!(width(line) <= columns.saturating_sub(1));
                prop_assert!(line.is_empty() || line.starts_with(GUTTER));
            }
        }

        #[test]
        fn hint_is_flush_right_on_the_last_line(
            description in text(),
            detail in text(),
            keys in any::<bool>(),
            jump in any::<bool>(),
            columns in 6usize..90,
        ) {
            let lines = [FooterLine::from_parts(parts(&description)), FooterLine::from_text(detail)];
            let shown = full_footer(mode(keys), category(jump), &lines, None, columns);

            let last = shown.split('\n').next_back().unwrap();
            prop_assert!(last.ends_with(KEYS_HINT));
            prop_assert_eq!(width(last), columns - 1);
            prop_assert_eq!(shown.matches(KEYS_HINT).count(), 1);
        }

        #[test]
        fn location_keeps_its_tab_and_pane_id_while_they_fit(
            workspace in text(),
            tab in text(),
            pane in 1u16..200,
            columns in 0usize..90,
        ) {
            // Cleaned as the footer cleans it; a tab with a separator of its
            // own stays whole.
            let tab = clean(&tab);
            let id = format!("w1:p{pane}");
            let sep = PART_SEPARATOR;
            let lines = [
                FooterLine::from_parts([workspace, tab.clone(), id.clone()]),
                FooterLine::from_text(""),
            ];

            let shown = full_footer(FooterMode::Description, Category::JumpTo, &lines, None, columns);

            let first = shown.split('\n').next().unwrap();
            let inner = columns.saturating_sub(2);
            // Room for `…` before the kept end.
            let id_tail = format!("{sep}{id}");
            if width(&id_tail) < inner {
                prop_assert!(first.ends_with(&id_tail), "{first:?}");
            }
            let tail = format!("{sep}{tab}{sep}{id}");
            if width(&tail) < inner {
                prop_assert!(first.ends_with(&tail), "{first:?}");
            }
        }

        #[test]
        fn branch_line_fits_keeps_the_hint_and_the_cap(
            branch in text(),
            cwd in text(),
            columns in 0usize..90,
        ) {
            let lines = [FooterLine::from_parts(["ws", "w1:p1"]), FooterLine::from_text(cwd.clone())];
            let paint = Color::Ansi(5).to_ansi_fg();
            let label = FooterBranch { label: &branch, color: Some(Color::Ansi(5)) };

            let shown = full_footer(FooterMode::Description, Category::JumpTo, &lines, Some(label), columns);

            let last = last(&shown);
            // The branch is the painted span; the rest is plain.
            let (before, painted, after) = match last.split_once(&paint) {
                Some((before, rest)) => {
                    let (painted, after) = rest.split_once(RESET).unwrap();
                    (before, painted, after)
                }
                None => (last, "", ""),
            };
            let plain = format!("{before}{painted}{after}");
            let inner = columns.saturating_sub(2);
            prop_assert!(width(&plain) <= columns.saturating_sub(1));
            if inner >= width(KEYS_HINT) {
                prop_assert!(plain.ends_with(KEYS_HINT));
                prop_assert_eq!(width(&plain), columns - 1);
                prop_assert_eq!(plain.matches(KEYS_HINT).count(), 1);
            }
            prop_assert!(width(painted) <= branch_cap(inner));
            // A shown branch never leaves a dangling separator: the cwd
            // keeps at least 2 columns after it.
            if !painted.is_empty() && !clean(&cwd).trim().is_empty() {
                let rest = after.strip_prefix(BRANCH_SEPARATOR).unwrap();
                let rest = rest.strip_suffix(KEYS_HINT).unwrap();
                prop_assert!(width(rest) >= 2, "{last:?}");
                prop_assert!(!rest.trim().is_empty(), "{last:?}");
            }
        }

        #[test]
        fn minimal_lines_fit_without_the_hint_and_keep_their_count(
            description in text(),
            detail in text(),
            branch in proptest::option::of(text()),
            keys in any::<bool>(),
            jump in any::<bool>(),
            columns in 0usize..90,
        ) {
            let lines = [FooterLine::from_parts(parts(&description)), FooterLine::from_text(detail)];
            let branch = branch.as_deref().map(|label| FooterBranch { label, color: None });
            let category = category(jump);

            let shown = minimal_footer(mode(keys), category, &lines, branch, columns);

            prop_assert_eq!(shown.split('\n').count(), line_count(category));
            prop_assert!(!shown.contains(KEYS_HINT));
            for line in shown.split('\n') {
                prop_assert!(width(line) <= columns.saturating_sub(1));
                prop_assert!(line.is_empty() || line.starts_with(GUTTER));
            }
        }

        #[test]
        fn cwd_cut_keeps_its_end(detail in text(), columns in 6usize..90) {
            let shown = jump_to(FooterMode::Description, "d", &detail, columns);

            let last = shown.split('\n').next_back().unwrap();
            let text = last
                .strip_prefix(GUTTER)
                .and_then(|rest| rest.strip_suffix(KEYS_HINT))
                .unwrap()
                .trim_end_matches(' ');
            let detail = clean(&detail);
            let detail = detail.trim_end_matches(' ');
            // 2 gutters, the hint and the gap: no room left for any text.
            let has_room = columns > 2 + width(KEYS_HINT) + GAP;
            match text.strip_prefix('…') {
                Some(kept) => prop_assert!(detail.ends_with(kept)),
                None if has_room => prop_assert_eq!(text, detail),
                None => prop_assert_eq!(width(text), 0),
            }
        }
    }
}
