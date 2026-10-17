//! One palette row as fzf reads it: a TSV line whose third field is the
//! rendered display text, in the full or the minimal layout.

use crate::domain::row::{Category, FooterLine, Kind, Row};
use crate::domain::style::TextColors;
use crate::domain::tags;

use super::{
    GAP, GUTTER, LayoutMode, RESET, clean, inner_width, muted, replace_controls, truncate, width,
};

/// The line fzf reads for `row` in a popup `columns` wide, laid out as
/// `mode` says, the shortcut muted with `colors`: `kind \t payload \t
/// display \t footer`.
///
/// fzf shows only the display field (`--with-nth=3`, see [`display`]);
/// the footer callback gets the fourth (`{4}`): the row's [`footer_lines`]
/// in one field, lines and parts joined with tags ([`tags::serialize`]).
/// The selection is parsed back from the first two fields.
/// Every field gets its control characters replaced by spaces, so it
/// can't break the line; every field but the payload also loses its tags,
/// so it can't break the footer. The payload keeps them: fzf hands it back
/// as it was, and the selection is matched on it.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::layout::{self, LayoutMode};
/// use herdr_command_palette::domain::row::{FooterLine, Kind, Row};
/// use herdr_command_palette::domain::style::TextColors;
///
/// let row = Row::new(Kind::Builtin, "zoom", "Toggle zoom")
///     .with_shortcut("prefix+z")
///     .with_footer_text("Zoom the focused pane");
/// assert_eq!(
///     layout::row_line(&row, 26, LayoutMode::Full, TextColors::default()),
///     "builtin\tzoom\t Toggle zoom     \x1b[2mprefix+z\x1b[0m \tZoom the focused pane",
/// );
/// let pane = Row::new(Kind::Pane, "w1:p3", "api  shell")
///     .with_footer(vec![FooterLine::from_parts(["api", "1", "w1:p3"]), FooterLine::from_text("~/src")]);
/// assert!(layout::row_line(&pane, 26, LayoutMode::Full, TextColors::default())
///     .ends_with("\tapi{hcp:seg}1{hcp:seg}w1:p3{hcp:line}~/src"));
/// ```
#[must_use]
pub fn row_line(row: &Row, columns: usize, mode: LayoutMode, colors: TextColors) -> String {
    let footer: Vec<FooterLine> = footer_lines(row)
        .iter()
        .map(|line| FooterLine::from_parts(line.parts().iter().map(|part| clean(part))))
        .collect();
    format!(
        "{}\t{}\t{}\t{}",
        row.kind,
        replace_controls(&row.payload),
        display(row, columns, mode, colors),
        tags::serialize(&footer),
    )
}

/// The text fzf shows for `row` in a popup `columns` wide, laid out as
/// `mode` says.
///
/// Layout: a 1-column gutter, the title, padding, the
/// right column (the shortcut) muted (dim, or the `muted` color), another
/// gutter. The right column always shows in full; a title that doesn't
/// fit is cut with `…`, keeping a 2-column gap. The right column is at
/// least [`Row::right_width`] wide, so rows of one list with shortcuts of
/// different widths (Jump to's `claude`, `terminal`) cut their titles at
/// the same column; a shortcut narrower than that column starts where the
/// column starts (left-aligned, like Herdr's Go to), so the gap
/// after every title is the same. A Commands shortcut fills its column,
/// so it is right-aligned. When the right column alone is too
/// wide the row runs past `columns` and fzf cuts it (`--no-hscroll`).
///
/// The title is [`Row::full_title`] in [`LayoutMode::Full`]. In
/// [`LayoutMode::Minimal`] it loses its
/// [`title_column`](Row::title_column) (Jump to's workspace), and a
/// Commands row loses its right column: the title gets the whole line,
/// with no gap kept.
///
/// After the visible part comes a hidden search tail: the full title when
/// it was cut, what the minimal layout dropped (the title column, unless
/// [`Row::search_extra`] already holds it in full; the shortcut), then
/// [`Row::search_extra`]. It sits past the right edge, so fzf matches it
/// but never shows it (`--no-hscroll --ellipsis=''`).
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::layout::{self, LayoutMode};
/// use herdr_command_palette::domain::row::{Kind, Row};
/// use herdr_command_palette::domain::style::TextColors;
///
/// let row = Row::new(Kind::Builtin, "zoom", "Toggle zoom").with_shortcut("prefix+z");
/// let colors = TextColors::default();
/// assert_eq!(
///     layout::display(&row, 26, LayoutMode::Full, colors),
///     " Toggle zoom     \x1b[2mprefix+z\x1b[0m ",
/// );
/// // Minimal: the title alone, the shortcut still searchable.
/// assert_eq!(
///     layout::display(&row, 26, LayoutMode::Minimal, colors),
///     " Toggle zoom             \x1b[2m\x1b[0m prefix+z",
/// );
/// ```
#[must_use]
pub fn display(row: &Row, columns: usize, mode: LayoutMode, colors: TextColors) -> String {
    let inner = inner_width(columns);
    let Shown {
        title,
        right,
        dropped,
    } = shown(row, mode);
    let (right_text, right_width, right_column, reserved) = match &right {
        Some(right) => {
            let text_width = width(&right.text);
            let column = text_width.max(right.min_width);
            (right.text.as_str(), text_width, column, column + GAP)
        }
        // No right column (minimal Commands): no gap kept for it either.
        None => ("", 0, 0, 0),
    };

    let kept = truncate(&title, inner.saturating_sub(reserved));
    let pad = inner.saturating_sub(width(&kept) + right_column);
    // Blanks after a shortcut narrower than its column; muted like it.
    let tail = right_column - right_width;
    let cut_title = if kept == title { "" } else { title.as_str() };
    let extra = clean(&row.search_extra);
    let hidden: Vec<&str> = [cut_title]
        .into_iter()
        .chain(dropped.iter().map(String::as_str))
        .chain([extra.as_str()])
        .filter(|text| !text.is_empty())
        .collect();

    format!(
        "{GUTTER}{kept}{:pad$}{}{right_text}{:tail$}{RESET}{GUTTER}{}",
        "",
        muted(colors),
        "",
        hidden.join(" "),
    )
}

/// What a row shows in a layout, cleaned, before it is fitted to a width.
#[derive(Debug)]
struct Shown {
    /// The title as shown, title column included in the full layout.
    title: String,
    /// The right column; `None` when the layout drops it.
    right: Option<RightColumn>,
    /// What the layout dropped, for the hidden search tail.
    dropped: Vec<String>,
}

/// A row's right column: its text and the width it takes at least.
#[derive(Debug)]
struct RightColumn {
    text: String,
    min_width: usize,
}

/// What `row` shows in `mode` (see [`display`]).
fn shown(row: &Row, mode: LayoutMode) -> Shown {
    let right = RightColumn {
        text: clean(&row.shortcut),
        min_width: row.right_width,
    };
    match mode {
        LayoutMode::Full => Shown {
            title: clean(&row.full_title()),
            right: Some(right),
            dropped: Vec::new(),
        },
        LayoutMode::Minimal => {
            let mut dropped = Vec::new();
            // A cut workspace is already in the search text in full;
            // the column (padding trimmed) would only repeat it. Relies on
            // `search_extra` holding nothing else for such rows (`Row`).
            if let Some(column) = &row.title_column
                && clean(&row.search_extra).is_empty()
            {
                dropped.push(clean(column.trim_end()));
            }
            // Jump to keeps its agent column; Commands drops the shortcut.
            let right = match row.category() {
                Category::JumpTo => Some(right),
                Category::Commands => {
                    dropped.push(right.text);
                    None
                }
            };
            Shown {
                title: clean(&row.title),
                right,
                dropped,
            }
        }
    }
}

/// The footer lines for `row`: its [`Row::footer`], the first line
/// tagged with the kind for plugin, palette and custom rows (`plugin ·
/// …`), so the kind stays visible in one list. Built-ins (most rows)
/// and panes get no tag. A tagged row without a footer shows just the
/// tag.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::layout;
/// use herdr_command_palette::domain::row::{FooterLine, Kind, Row};
///
/// let row = Row::new(Kind::Custom, "0", "Lazygit").with_footer_text("lazygit");
/// assert_eq!(layout::footer_lines(&row), [FooterLine::from_text("custom · lazygit")]);
/// ```
#[must_use]
pub fn footer_lines(row: &Row) -> Vec<FooterLine> {
    let Some(tag) = tag(row.kind) else {
        return row.footer.clone();
    };
    let mut lines = row.footer.clone();
    match lines.first_mut() {
        Some(first) if !first.is_empty() => {
            let mut parts = first.parts().to_vec();
            if let Some(part) = parts.first_mut() {
                *part = format!("{tag} · {part}");
            }
            *first = FooterLine::from_parts(parts);
        }
        Some(first) => *first = FooterLine::from_text(tag),
        None => lines.push(FooterLine::from_text(tag)),
    }
    lines
}

/// The footer tag of `kind`'s rows: the kind's name for plugin, palette
/// and custom rows; built-ins (most rows) and panes have none.
pub(super) fn tag(kind: Kind) -> Option<&'static str> {
    match kind {
        Kind::Plugin | Kind::Palette | Kind::Custom => Some(kind.as_str()),
        Kind::Builtin | Kind::Pane => None,
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use proptest::prelude::*;

    use crate::domain::color::Color;
    use crate::domain::fixtures::{CORPUS, text};
    use crate::domain::layout::DIM;

    use super::*;

    /// No `[colors]` settings: shortcuts are dim.
    const PLAIN: TextColors = TextColors {
        muted: None,
        tags: None,
        branch: None,
    };

    /// The display text with the dim escapes shown as `⟨` `⟩`, for snapshots.
    fn readable(display: &str) -> String {
        display.replace(DIM, "⟨").replace(RESET, "⟩")
    }

    /// The part of a display line that fzf shows (before the hidden tail),
    /// without escapes.
    fn visible(display: &str) -> String {
        let (shown, _hidden) = display.split_once(RESET).unwrap();
        format!("{}{GUTTER}", shown.replace(DIM, ""))
    }

    fn pane(title: &str, shortcut: &str) -> Row {
        Row::new(Kind::Pane, "w1:p1", title)
            .with_shortcut(shortcut)
            .with_footer_text("desc")
    }

    /// A pane or a command row with an optional title column.
    fn row_of(
        is_pane: bool,
        title: &str,
        column: Option<&str>,
        shortcut: &str,
        right_width: usize,
    ) -> Row {
        let kind = if is_pane { Kind::Pane } else { Kind::Builtin };
        let row = Row::new(kind, "p", title)
            .with_shortcut(shortcut)
            .with_right_width(right_width);
        match column {
            Some(column) => row.with_title_column(column),
            None => row,
        }
    }

    fn layout_mode(is_minimal: bool) -> LayoutMode {
        if is_minimal {
            LayoutMode::Minimal
        } else {
            LayoutMode::Full
        }
    }

    #[test]
    fn right_column_wins_and_full_title_stays_searchable() {
        let row = pane(&"x".repeat(30), "terminal");

        let display = display(&row, 32, LayoutMode::Full, PLAIN);

        let (shown, rest) = display.split_once(DIM).unwrap();
        assert_eq!(shown, format!(" {}…  ", "x".repeat(19)));
        assert_eq!(rest, format!("terminal{RESET} {}", "x".repeat(30)));
    }

    #[test]
    fn cuts_titles_at_the_same_column_and_left_aligns_short_shortcuts() {
        let title = format!("ws  {}", "x".repeat(40));
        let terminal = pane(&title, "terminal").with_right_width(8);
        let claude = pane(&title, "claude").with_right_width(8);

        let terminal = visible(&display(&terminal, 30, LayoutMode::Full, PLAIN));
        let claude = visible(&display(&claude, 30, LayoutMode::Full, PLAIN));

        assert_eq!(terminal, format!(" ws  {}…  terminal ", "x".repeat(13)));
        assert_eq!(claude, format!(" ws  {}…  claude   ", "x".repeat(13)));
    }

    #[test]
    fn fits_without_hidden_text() {
        let row = Row::new(Kind::Builtin, "zoom", "Toggle zoom")
            .with_shortcut("prefix+z")
            .with_footer_text("d");

        assert_eq!(
            row_line(&row, 26, LayoutMode::Full, PLAIN),
            format!("builtin\tzoom\t Toggle zoom     {DIM}prefix+z{RESET} \td"),
        );
    }

    #[test]
    fn mutes_the_shortcut_with_the_settings_color() {
        let row = Row::new(Kind::Builtin, "zoom", "Zoom").with_shortcut("z");
        let colors = TextColors {
            muted: Some(Color::Ansi(8)),
            tags: None,
            branch: None,
        };

        assert_eq!(
            display(&row, 10, LayoutMode::Full, colors),
            " Zoom   \x1b[38;5;8mz\x1b[0m "
        );
    }

    #[test]
    fn appends_search_extra_after_the_cut_title() {
        let row = pane("a-very-long-workspa…  terminal · 1", "terminal")
            .with_search_extra("a-very-long-workspace-name");

        let display = display(&row, 30, LayoutMode::Full, PLAIN);

        assert!(display.ends_with(&format!(
            "{RESET} a-very-long-workspa…  terminal · 1 a-very-long-workspace-name"
        )));
    }

    #[test]
    fn appends_search_extra_when_the_title_fits() {
        let row = pane("api", "codex").with_search_extra("api-full");

        assert!(display(&row, 30, LayoutMode::Full, PLAIN).ends_with(&format!("{RESET} api-full")));
    }

    #[test]
    fn hides_the_whole_title_when_the_right_column_fills_the_row() {
        let row = pane("title", "a-very-wide-shortcut");

        assert_eq!(
            display(&row, 12, LayoutMode::Full, PLAIN),
            format!(" {DIM}a-very-wide-shortcut{RESET} title"),
        );
    }

    #[test]
    fn replaces_control_characters_in_every_field() {
        let row = Row::new(Kind::Custom, "0\t1", "a\tb")
            .with_shortcut("k\n")
            .with_footer(vec![
                FooterLine::from_text("echo\thi"),
                FooterLine::from_text("/tmp/a\nb"),
            ]);

        let line = row_line(&row, 20, LayoutMode::Full, PLAIN);

        assert_eq!(line.split('\t').count(), 4);
        assert!(line.starts_with("custom\t0 1\t a b"));
        assert!(line.ends_with("\tcustom · echo hi{hcp:line}/tmp/a b"));
    }

    #[test]
    fn removes_tags_from_every_field_but_the_payload() {
        let row = Row::new(Kind::Pane, "w1:p1{hcp:seg}", "a{hcp:line}b")
            .with_shortcut("{hcp:seg}k")
            .with_search_extra("x{hcp:seg}")
            .with_footer(vec![
                FooterLine::from_parts(["ws{hcp:seg}", "{hcp:{hcp:seg}seg}tab", "w1:p1"]),
                FooterLine::from_text("~/{hcp:line}src"),
            ]);

        let line = row_line(&row, 20, LayoutMode::Full, PLAIN);

        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(fields[1], "w1:p1{hcp:seg}");
        assert!(fields[2].starts_with(" ab"), "{line:?}");
        assert!(!fields[2].contains("{hcp:"), "{line:?}");
        assert_eq!(
            tags::parse(fields[3]),
            [
                FooterLine::from_parts(["ws", "tab", "w1:p1"]),
                FooterLine::from_text("~/src")
            ]
        );
    }

    #[test]
    fn tags_footer_for_plugin_palette_and_custom_only() {
        let text = |kind| footer_lines(&Row::new(kind, "p", "T").with_footer_text("d"));

        assert_eq!(text(Kind::Plugin), [FooterLine::from_text("plugin · d")]);
        assert_eq!(text(Kind::Palette), [FooterLine::from_text("palette · d")]);
        assert_eq!(text(Kind::Custom), [FooterLine::from_text("custom · d")]);
        assert_eq!(text(Kind::Builtin), [FooterLine::from_text("d")]);
        assert_eq!(text(Kind::Pane), [FooterLine::from_text("d")]);
    }

    #[test]
    fn footer_is_just_the_tag_without_description() {
        let empty = Row::new(Kind::Custom, "p", "T").with_footer_text("");

        assert_eq!(
            footer_lines(&Row::new(Kind::Custom, "p", "T")),
            [FooterLine::from_text("custom")]
        );
        assert_eq!(footer_lines(&empty), [FooterLine::from_text("custom")]);
        assert_eq!(footer_lines(&Row::new(Kind::Builtin, "p", "T")), []);
    }

    #[test]
    fn tags_the_first_part_of_a_line_of_parts() {
        let row = Row::new(Kind::Plugin, "p", "T").with_footer(vec![
            FooterLine::from_parts(["a", "b"]),
            FooterLine::from_text("c"),
        ]);

        assert_eq!(
            footer_lines(&row),
            [
                FooterLine::from_parts(["plugin · a", "b"]),
                FooterLine::from_text("c")
            ]
        );
    }

    /// A Jump to row with its workspace column, as `panes::rows` builds it.
    fn pane_in(workspace: &str, name: &str) -> Row {
        pane(name, "claude")
            .with_title_column(workspace)
            .with_right_width(8)
    }

    fn command(title: &str, shortcut: &str) -> Row {
        Row::new(Kind::Builtin, "zoom", title)
            .with_shortcut(shortcut)
            .with_footer_text("desc")
    }

    #[test]
    fn full_layout_shows_the_title_column_before_the_title() {
        let row = pane_in("api     ", "fix · 3");

        assert_eq!(
            visible(&display(&row, 30, LayoutMode::Full, PLAIN)),
            " api       fix · 3   claude   "
        );
    }

    #[test]
    fn minimal_layout_drops_the_workspace_column_and_keeps_the_agent() {
        let row = pane_in("api     ", "fix · 3");

        let shown = display(&row, 30, LayoutMode::Minimal, PLAIN);

        assert_eq!(visible(&shown), " fix · 3             claude   ");
        assert!(shown.ends_with(&format!("{RESET}{GUTTER}api")), "{shown:?}");
    }

    #[test]
    fn minimal_layout_cuts_names_before_the_agent_column() {
        let row = pane_in("api", &"x".repeat(30));

        let shown = display(&row, 20, LayoutMode::Minimal, PLAIN);

        assert_eq!(visible(&shown), format!(" {}…  claude   ", "x".repeat(7)));
        assert!(shown.ends_with(&format!(" {} api", "x".repeat(30))));
    }

    #[test]
    fn minimal_layout_searches_a_cut_workspace_once() {
        let row =
            pane_in("a-very-long-workspa…", "fix").with_search_extra("a-very-long-workspace-name");

        let shown = display(&row, 30, LayoutMode::Minimal, PLAIN);

        assert!(
            shown.ends_with(&format!("{RESET}{GUTTER}a-very-long-workspace-name")),
            "{shown:?}"
        );
    }

    #[test]
    fn minimal_layout_shows_a_commands_title_alone() {
        let row = command("Toggle zoom", "prefix+z");

        let shown = display(&row, 26, LayoutMode::Minimal, PLAIN);

        assert_eq!(
            shown,
            format!(" Toggle zoom{:13}{DIM}{RESET}{GUTTER}prefix+z", "")
        );
    }

    #[test]
    fn minimal_layout_gives_a_commands_title_the_whole_line() {
        let row = command("Split pane right", "prefix+v");

        let shown = display(&row, 12, LayoutMode::Minimal, PLAIN);

        // Full would keep the shortcut and a 2-column gap: no title left.
        assert_eq!(visible(&shown), " Split pan… ");
        assert!(shown.ends_with(" Split pane right prefix+v"), "{shown:?}");
    }

    #[test]
    fn minimal_layout_keeps_the_other_fields() {
        let row = command("Toggle zoom", "prefix+z");

        let line = row_line(&row, 26, LayoutMode::Minimal, PLAIN);

        assert!(line.starts_with("builtin\tzoom\t Toggle zoom "), "{line:?}");
        assert!(line.ends_with("\tdesc"), "{line:?}");
    }

    #[test]
    fn minimal_layout_keeps_a_command_without_shortcut_as_is() {
        let row = command("Toggle zoom", "");

        assert_eq!(
            display(&row, 20, LayoutMode::Minimal, PLAIN),
            display(&row, 20, LayoutMode::Full, PLAIN)
        );
    }

    #[test]
    fn snapshot_display_corpus() {
        let mut out = String::new();
        for title in CORPUS {
            for (shortcut, columns) in [("prefix+v", 24), ("prefix+v", 16), ("日本", 12), ("", 10)]
            {
                let row = pane(title, shortcut);
                let shown = display(&row, columns, LayoutMode::Full, PLAIN);
                writeln!(
                    out,
                    "{columns:>2} |{}| {:?}",
                    visible(&shown),
                    readable(&shown)
                )
                .unwrap();
            }
        }
        insta::assert_snapshot!(out);
    }

    proptest! {
        #[test]
        fn visible_part_is_exactly_the_row_width(
            title in text(),
            shortcut in text(),
            columns in 0usize..60,
        ) {
            let row = pane(&title, &shortcut);
            let right = width(&clean(&shortcut));
            prop_assume!(right + GAP + 2 <= columns);

            prop_assert_eq!(width(&visible(&display(&row, columns, LayoutMode::Full, PLAIN))), columns);
        }

        #[test]
        fn shared_right_column_keeps_width_and_gap(
            title in text(),
            shortcut in text(),
            right_width in 0usize..20,
            columns in 0usize..60,
        ) {
            let row = pane(&title, &shortcut).with_right_width(right_width);
            let title = clean(&title);
            let right = width(&clean(&shortcut));
            let column = right.max(right_width);
            prop_assume!(column + GAP + 2 <= columns);

            let shown = display(&row, columns, LayoutMode::Full, PLAIN);

            prop_assert_eq!(width(&visible(&shown)), columns);
            // Gutter, the title (cut where the column plus the gap leave
            // room), blanks, the shortcut at the column's start, blanks.
            let (left, rest) = shown.split_once(DIM).unwrap();
            let left = left.strip_prefix(GUTTER).unwrap();
            let limit = columns - 2 - column - GAP;
            let kept = truncate(&title, limit);
            let blanks = left.strip_prefix(kept.as_str()).unwrap();
            prop_assert!(blanks.chars().all(|c| c == ' '));
            // The column starts at the same place on every row...
            prop_assert_eq!(width(left), columns - 2 - column);
            let tail = format!("{}{RESET}{GUTTER}", " ".repeat(column - right));
            let expected = format!("{}{tail}", clean(&shortcut));
            prop_assert!(rest.starts_with(&expected));
            // ...at least the gap after the title, and a cut title ends
            // exactly there (one blank more when a wide character didn't
            // fit).
            prop_assert!(blanks.len() >= GAP);
            if kept != title {
                prop_assert!(blanks.len() <= GAP + 1);
            }
        }

        #[test]
        fn right_column_is_always_shown_in_full(
            title in text(),
            shortcut in text(),
            columns in 0usize..60,
        ) {
            let row = pane(&title, &shortcut);

            let shown = display(&row, columns, LayoutMode::Full, PLAIN);

            let expected = format!("{DIM}{}{RESET}{GUTTER}", clean(&shortcut));
            prop_assert!(shown.contains(&expected));
        }

        #[test]
        fn full_title_is_searchable(title in text(), columns in 0usize..60) {
            let row = pane(&title, "x");

            let shown = display(&row, columns, LayoutMode::Full, PLAIN);

            prop_assert!(shown.contains(&clean(&title)));
        }

        #[test]
        fn line_has_four_fields(title in text(), shortcut in text(), columns in 0usize..60) {
            let row = pane(&title, &shortcut).with_search_extra(title.clone());

            prop_assert_eq!(row_line(&row, columns, LayoutMode::Full, PLAIN).split('\t').count(), 4);
        }

        #[test]
        fn every_row_is_exactly_the_width_in_both_layouts(
            title in text(),
            column in proptest::option::of(text()),
            shortcut in text(),
            is_pane in any::<bool>(),
            is_minimal in any::<bool>(),
            right_width in 0usize..12,
            columns in 2usize..60,
        ) {
            let row = row_of(is_pane, &title, column.as_deref(), &shortcut, right_width);
            let mode = layout_mode(is_minimal);
            let keeps_right = !is_minimal || is_pane;
            let right = width(&clean(&shortcut)).max(right_width);
            prop_assume!(!keeps_right || right + GAP + 2 <= columns);

            prop_assert_eq!(width(&visible(&display(&row, columns, mode, PLAIN))), columns);
        }

        #[test]
        fn minimal_layout_shows_neither_the_workspace_column_nor_a_shortcut(
            title in text(),
            column in text(),
            shortcut in text(),
            is_pane in any::<bool>(),
            columns in 2usize..60,
        ) {
            let row = row_of(is_pane, &title, Some(&column), &shortcut, 0);
            let right = if is_pane { width(&clean(&shortcut)) } else { 0 };
            prop_assume!(!is_pane || right + GAP + 2 <= columns);

            let shown = display(&row, columns, LayoutMode::Minimal, PLAIN);

            // Gutter, the title alone (cut), blanks; then the agent of a
            // pane, nothing for a command.
            let (left, rest) = shown.split_once(DIM).unwrap();
            let left = left.strip_prefix(GUTTER).unwrap();
            let reserved = if is_pane { right + GAP } else { 0 };
            let kept = truncate(&clean(&title), columns - 2 - reserved);
            let blanks = left.strip_prefix(kept.as_str()).unwrap();
            prop_assert!(blanks.chars().all(|c| c == ' '));
            let agent = if is_pane { clean(&shortcut) } else { String::new() };
            let expected = format!("{agent}{RESET}{GUTTER}");
            prop_assert!(rest.starts_with(&expected));
        }

        #[test]
        fn minimal_layout_keeps_what_it_hides_searchable(
            title in text(),
            column in text(),
            full_column in text(),
            has_extra in any::<bool>(),
            shortcut in text(),
            is_pane in any::<bool>(),
            columns in 0usize..60,
        ) {
            // `search_extra`, when set, is the column's full text (`Row`).
            let mut row = row_of(is_pane, &title, Some(&column), &shortcut, 0);
            if has_extra {
                row = row.with_search_extra(&full_column);
            }
            let searched = if has_extra && !clean(&full_column).is_empty() {
                clean(&full_column)
            } else {
                clean(column.trim_end())
            };

            let shown = display(&row, columns, LayoutMode::Minimal, PLAIN);

            let (_, tail) = shown.split_once(&format!("{RESET}{GUTTER}")).unwrap();
            prop_assert!(tail.contains(&searched), "{shown:?}");
            if !is_pane {
                prop_assert!(tail.contains(&clean(&shortcut)), "{shown:?}");
            }
        }
    }
}
