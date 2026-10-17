//! The confirmation dialog of a row listed in the settings' `confirm_ids`: a
//! copy of Herdr's close dialog, `accent` where Herdr uses `red`, to
//! match the popup frame Herdr draws in `accent`.
//!
//! Mirrors `render_confirm_close_overlay`, `row` and `button` in Herdr
//! `src/client/shell/overlays.rs`, v0.9.3, inside the frame Herdr draws
//! around the popup.

use crate::domain::color::Color;
use crate::domain::dialog::{DialogLine, DialogText};
use crate::domain::style::DialogColors;

use super::{BOLD, RESET, clean, cut, truncate, width};

/// The confirm button's label, centered in [`CONFIRM_WIDTH`] columns.
const CONFIRM_LABEL: &str = " ↵ confirm ";
/// The confirm button's width (Herdr's).
const CONFIRM_WIDTH: usize = 13;
/// The cancel button's label, as wide as [`CANCEL_WIDTH`].
const CANCEL_LABEL: &str = " esc cancel ";
/// The cancel button's width (Herdr's).
const CANCEL_WIDTH: usize = 12;
/// Columns between the two buttons (Herdr's).
const BUTTON_GAP: usize = 2;

/// The dialog's lines for a pane `columns` wide, each exactly `columns`
/// wide, every cell on `colors.background`. With a detail, five lines
/// (the 64 by 7 popup's inside):
///
/// 1. ` <title>` in bold `accent`.
/// 2. ` <label>` in `text`.
/// 3. ` <counts>` in `text`.
/// 4. Blank.
/// 5. The buttons, centered, 2 columns apart: ` ↵ confirm ` (bold,
///    the background, or `surface_dim` when that is
///    [`Color::Reset`], on `accent`) and ` esc cancel ` (bold `text` on
///    `surface0`).
///
/// Without a detail, four lines (the 64 by 6 popup's inside, Herdr's
/// size): the title, two blank lines, the buttons. The buttons stay on
/// the fourth line, as in Herdr's dialog (title, detail, blank,
/// buttons). Herdr's detail is one line, `<label> — <counts>`; the
/// palette splits label and counts, so a long label is cut without losing the
/// counts, which takes
/// the fifth line.
///
/// Each line is cut with `…` to fit: the title in its
/// [`text`](DialogLine::text) only, so its suffix (`?`) shows while it
/// fits (Herdr clips the whole line instead); the label and the
/// counts as a whole. The buttons, on a pane narrower than both, are cut
/// like Herdr cuts them.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::dialog::{DialogLine, DialogText};
/// use herdr_command_palette::domain::layout;
/// use herdr_command_palette::domain::style::DialogColors;
/// use herdr_command_palette::domain::color::Color;
///
/// let text = DialogText { title: DialogLine::new("Close tab?"), detail: None };
/// let colors = DialogColors {
///     background: Color::Reset,
///     accent: Color::Ansi(4),
///     text: Color::Ansi(7),
///     surface0: Color::Ansi(8),
///     surface_dim: Color::Ansi(0),
/// };
///
/// let lines = layout::dialog(&text, 30, colors);
/// let [title, blank, second_blank, buttons] = lines.as_slice() else {
///     panic!("a dialog without detail has four lines");
/// };
/// assert_eq!(title, &format!("\x1b[38;5;4m\x1b[1m Close tab?{:19}\x1b[0m", ""));
/// assert_eq!(blank, second_blank);
/// assert!(buttons.contains("\x1b[48;5;4m\x1b[38;5;0m\x1b[1m  ↵ confirm  \x1b[0m"));
/// ```
#[must_use]
pub fn dialog(text: &DialogText, columns: usize, colors: DialogColors) -> Vec<String> {
    let background = colors.background;
    let line = |content: &DialogLine, fg: Color, is_bold: bool| {
        let shown = fit_line(content, columns);
        let pad = columns.saturating_sub(width(&shown));
        paint(&format!("{shown}{:pad$}", ""), fg, background, is_bold)
    };
    let blank = paint(&format!("{:columns$}", ""), colors.text, background, false);
    let title = line(&text.title, colors.accent, true);
    let buttons = buttons(columns, colors);
    match &text.detail {
        Some(detail) => vec![
            title,
            line(&DialogLine::new(&detail.label), colors.text, false),
            line(&DialogLine::new(&detail.counts), colors.text, false),
            blank,
            buttons,
        ],
        None => vec![title, blank.clone(), blank, buttons],
    }
}

/// ` <text><suffix>` in at most `columns`. Only the text is cut (with
/// `…`) while the suffix fits after at least one column of it; on a pane
/// narrower than that, the whole line is cut.
fn fit_line(line: &DialogLine, columns: usize) -> String {
    let suffix = clean(&line.suffix);
    let text = clean(&line.text);
    let fixed = 1 + width(&suffix);
    if columns <= fixed {
        return truncate(&format!(" {text}{suffix}"), columns);
    }
    format!(" {}{suffix}", truncate(&text, columns - fixed))
}

/// The button line: both buttons centered, cut at `columns` the way
/// Herdr's `row` and `button` cut them, background around them.
fn buttons(columns: usize, colors: DialogColors) -> String {
    let contrast = match colors.background {
        Color::Reset => colors.surface_dim,
        color => color,
    };
    let total = CONFIRM_WIDTH + BUTTON_GAP + CANCEL_WIDTH;
    let left = columns.saturating_sub(total) / 2;
    let confirm_x = left;
    let cancel_x = left + CONFIRM_WIDTH + BUTTON_GAP;
    let confirm = button(
        CONFIRM_LABEL,
        CONFIRM_WIDTH,
        columns.saturating_sub(confirm_x),
    );
    let cancel = button(CANCEL_LABEL, CANCEL_WIDTH, columns.saturating_sub(cancel_x));
    let gap = BUTTON_GAP.min(columns.saturating_sub(confirm_x + width(&confirm)));
    let right = columns.saturating_sub(left + width(&confirm) + gap + width(&cancel));
    let blank = |count: usize| {
        paint(
            &format!("{:count$}", ""),
            colors.text,
            colors.background,
            false,
        )
    };
    [
        blank(left),
        paint(&confirm, contrast, colors.accent, true),
        blank(gap),
        paint(&cancel, colors.text, colors.surface0, true),
        blank(right),
    ]
    .concat()
}

/// A button's cells: `label` centered in `button_width` columns, both cut
/// to the `room` left on the line (Herdr's `button`: the label starts at
/// the button's left edge once it no longer fits).
fn button(label: &str, button_width: usize, room: usize) -> String {
    let button_width = button_width.min(room);
    let label = cut(label, button_width);
    let left = (button_width - width(label)) / 2;
    let right = button_width - width(label) - left;
    format!("{:left$}{label}{:right$}", "", "")
}

/// `text` in `fg` on `bg`, bold when asked, ending with a reset; empty
/// text paints nothing.
fn paint(text: &str, fg: Color, bg: Color, is_bold: bool) -> String {
    if text.is_empty() {
        return String::new();
    }
    let bold = if is_bold { BOLD } else { "" };
    format!("{}{}{bold}{text}{RESET}", bg.to_ansi_bg(), fg.to_ansi_fg())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use crate::domain::dialog::DialogDetail;
    use crate::domain::fixtures;

    use super::*;

    const COLORS: DialogColors = DialogColors {
        background: Color::Rgb(30, 30, 46),
        accent: Color::Ansi(4),
        text: Color::Ansi(7),
        surface0: Color::Ansi(8),
        surface_dim: Color::Ansi(0),
    };

    /// `line` without its escapes: what the cells show.
    fn cells(line: &str) -> String {
        let mut out = String::new();
        let mut chars = line.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                // Skip `[...m`.
                for c in chars.by_ref() {
                    if c == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    /// A dialog titled `title`, with the detail `(label, counts)` if any.
    fn text(title: &str, detail: Option<(&str, &str)>) -> DialogText {
        DialogText {
            title: DialogLine::new(title),
            detail: detail.map(|(label, counts)| DialogDetail::new(label, counts)),
        }
    }

    #[test]
    fn draws_herdrs_dialog_in_its_popup() {
        let lines = dialog(
            &text("Close workspace?", Some(("myproj", "3 panes"))),
            61,
            COLORS,
        );

        insta::assert_snapshot!(
            lines
                .iter()
                .map(|line| format!("{line:?}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    /// The five lines of a dialog with a detail.
    fn with_detail(text: &DialogText, columns: usize) -> [String; 5] {
        dialog(text, columns, COLORS).try_into().unwrap()
    }

    /// The four lines of a dialog without detail.
    fn title_only(text: &DialogText, columns: usize, colors: DialogColors) -> [String; 4] {
        dialog(text, columns, colors).try_into().unwrap()
    }

    #[test]
    fn every_line_fills_the_width() {
        for columns in [0, 1, 5, 14, 20, 27, 28, 61, 80] {
            for detail in [Some(("a", "2 workspaces, 1 pane")), None] {
                let lines = dialog(&text("Close worktree group?", detail), columns, COLORS);

                for line in &lines {
                    assert_eq!(width(&cells(line)), columns, "{columns}: {line:?}");
                }
            }
        }
    }

    #[test]
    fn has_five_lines_with_a_detail_and_four_without() {
        assert_eq!(dialog(&text("T", Some(("L", "C"))), 20, COLORS).len(), 5);
        assert_eq!(dialog(&text("T", None), 20, COLORS).len(), 4);
    }

    #[test]
    fn puts_label_and_counts_on_their_own_lines_after_one_space() {
        let [title, label, counts, ..] =
            with_detail(&text("Close tab?", Some(("api", "1 pane"))), 20);

        assert_eq!(cells(&title), " Close tab?         ");
        assert_eq!(cells(&label), " api                ");
        assert_eq!(cells(&counts), " 1 pane             ");
    }

    #[test]
    fn paints_title_bold_accent_and_detail_text_on_the_background() {
        let [title, label, counts, blank, _] = with_detail(&text("T", Some(("L", "C"))), 3);

        assert_eq!(
            title,
            format!("\x1b[48;2;30;30;46m\x1b[38;5;4m{BOLD} T {RESET}")
        );
        assert_eq!(label, format!("\x1b[48;2;30;30;46m\x1b[38;5;7m L {RESET}"));
        assert_eq!(counts, format!("\x1b[48;2;30;30;46m\x1b[38;5;7m C {RESET}"));
        assert_eq!(blank, format!("\x1b[48;2;30;30;46m\x1b[38;5;7m   {RESET}"));
    }

    #[test]
    fn puts_two_blank_lines_before_the_buttons_without_a_detail() {
        let [title, blank, second_blank, buttons] =
            title_only(&text("Close tab?", None), 20, COLORS);

        assert_eq!(cells(&title), " Close tab?         ");
        assert_eq!(
            blank,
            format!("\x1b[48;2;30;30;46m\x1b[38;5;7m{:20}{RESET}", "")
        );
        assert_eq!(second_blank, blank);
        assert_eq!(cells(&buttons), "  ↵ confirm     esc ");
    }

    #[test]
    fn cuts_and_cleans_title_label_and_counts() {
        let [title, label, counts, ..] = with_detail(
            &text("Close\tworkspace?", Some(("a\nlong label", "2 workspaces"))),
            10,
        );

        assert_eq!(cells(&title), " Close wo…");
        assert_eq!(cells(&label), " a long l…");
        assert_eq!(cells(&counts), " 2 worksp…");
    }

    #[test]
    fn cuts_only_the_title_text_and_keeps_its_suffix() {
        let text = DialogText {
            title: DialogLine::new("Deploy to production").with_suffix("?"),
            detail: None,
        };

        let [title, ..] = title_only(&text, 18, COLORS);

        assert_eq!(cells(&title), " Deploy to produ…?");
    }

    #[test]
    fn cuts_the_whole_title_when_the_suffix_leaves_no_room_for_text() {
        let title = |columns| {
            let text = DialogText {
                title: DialogLine::new("Close").with_suffix(" now?"),
                detail: None,
            };
            cells(&dialog(&text, columns, COLORS)[0])
        };

        assert_eq!(title(8), " C… now?");
        assert_eq!(title(7), " … now?");
        assert_eq!(title(6), " Clos…");
        assert_eq!(title(1), "…");
    }

    #[test]
    fn centers_the_buttons_two_apart() {
        let [.., buttons] = title_only(&text("x", None), 61, COLORS);

        assert_eq!(
            cells(&buttons),
            format!("{:17}  ↵ confirm     esc cancel {:17}", "", "")
        );
    }

    #[test]
    fn paints_the_buttons() {
        let [.., buttons] = title_only(&text("x", None), 27, COLORS);

        assert_eq!(
            buttons,
            format!(
                "\x1b[48;5;4m\x1b[38;2;30;30;46m{BOLD}  ↵ confirm  {RESET}\
                 \x1b[48;2;30;30;46m\x1b[38;5;7m  {RESET}\
                 \x1b[48;5;8m\x1b[38;5;7m{BOLD} esc cancel {RESET}"
            )
        );
    }

    #[test]
    fn confirm_label_uses_surface_dim_on_the_terminal_background() {
        let colors = DialogColors {
            background: Color::Reset,
            ..COLORS
        };

        let [.., buttons] = title_only(&text("x", None), 27, colors);

        assert!(buttons.starts_with(&format!("\x1b[48;5;4m\x1b[38;5;0m{BOLD}")));
    }

    #[test]
    fn cuts_the_buttons_like_herdr_on_a_narrow_pane() {
        let cut = |columns| cells(&dialog(&text("x", None), columns, COLORS)[3]);

        assert_eq!(cut(20), "  ↵ confirm     esc ");
        assert_eq!(cut(14), "  ↵ confirm   ");
        assert_eq!(cut(5), " ↵ co");
        assert_eq!(cut(0), "");
    }

    /// User text, short or long (labels, row titles): wide, combining,
    /// control and zero-width characters included.
    fn any_text() -> impl Strategy<Value = String> {
        prop_oneof![
            fixtures::text(),
            fixtures::text().prop_map(|text| text.repeat(6)),
        ]
    }

    /// Title suffixes.
    fn any_suffix() -> impl Strategy<Value = &'static str> {
        proptest::sample::select(vec!["", "?", " now?"])
    }

    /// Counts the dialog shows: `1 pane`, `N panes`, `W workspaces, N panes`.
    fn any_counts() -> impl Strategy<Value = String> {
        let panes = proptest::sample::select(vec!["0 panes", "1 pane", "2 panes", "12345 panes"]);
        (panes, proptest::option::of(2usize..1000)).prop_map(|(panes, workspaces)| {
            workspaces.map_or_else(
                || panes.to_owned(),
                |count| format!("{count} workspaces, {panes}"),
            )
        })
    }

    proptest! {
        #[test]
        fn the_title_suffix_shows_whole_when_it_fits(
            text in any_text(),
            suffix in any_suffix(),
            columns in 0usize..100,
        ) {
            let line = DialogLine::new(text.clone()).with_suffix(suffix);
            let [title, ..] = title_only(&DialogText { title: line, detail: None }, columns, COLORS);

            let shown = cells(&title);
            if columns > 1 + width(suffix) {
                prop_assert!(shown.trim_end_matches(' ').ends_with(suffix.trim_end()), "{:?}", shown);
                prop_assert!(shown.starts_with(' '));
            }
            let whole = format!(" {}{suffix}", clean(&text));
            if width(&whole) <= columns {
                prop_assert!(shown.starts_with(&whole), "{:?}", shown);
            }
        }

        #[test]
        fn the_label_is_cut_with_an_ellipsis_only_when_too_long(
            label in any_text(),
            columns in 1usize..100,
        ) {
            let detail = DialogDetail::new(label.clone(), "3 panes");
            let text = DialogText { title: DialogLine::new("Close workspace?"), detail: Some(detail) };
            let [_, shown, ..] = with_detail(&text, columns);

            let shown = cells(&shown);
            let whole = format!(" {}", clean(&label));
            if width(&whole) <= columns {
                prop_assert!(shown.starts_with(&whole), "{:?}", shown);
            } else {
                prop_assert!(shown.trim_end_matches(' ').ends_with('…'), "{:?}", shown);
            }
        }

        #[test]
        fn the_counts_are_cut_only_when_they_cannot_fit(
            counts in any_counts(),
            columns in 1usize..100,
        ) {
            let detail = DialogDetail::new("api", counts.clone());
            let text = DialogText { title: DialogLine::new("Close workspace?"), detail: Some(detail) };
            let [_, _, shown, ..] = with_detail(&text, columns);

            let shown = cells(&shown);
            let whole = format!(" {counts}");
            if width(&whole) <= columns {
                prop_assert_eq!(shown.trim_end_matches(' '), whole);
            } else {
                prop_assert!(shown.ends_with('…'), "{:?}", shown);
            }
        }

        #[test]
        fn every_line_is_exactly_the_pane_width(
            title in any_text(),
            detail in proptest::option::of((any_text(), any_counts())),
            columns in 0usize..100,
        ) {
            let text = DialogText {
                title: DialogLine::new(title).with_suffix("?"),
                detail: detail.map(|(label, counts)| DialogDetail::new(label, counts)),
            };
            let lines = dialog(&text, columns, COLORS);

            for line in &lines {
                prop_assert_eq!(width(&cells(line)), columns, "{:?}", line);
            }
        }
    }
}
