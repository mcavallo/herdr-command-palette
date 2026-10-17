//! Text layout in terminal columns: display width, truncation with `…`
//! (or a plain cut), and the palette's rendered text (row lines, header,
//! footer).
//!
//! This is the only module that measures or cuts text for display
//! (`docs/code-style.md`, "Strings and text width"). Widths come from
//! `unicode-width` 0.2, the crate Herdr uses, so our column math matches
//! Herdr's. Cuts walk grapheme clusters (`unicode-segmentation`), so an
//! emoji or an accented letter is never split.
//!
//! Accepted limits: Nerd Font icons
//! (Private Use Area) and East Asian Ambiguous characters such as `·` and
//! `…` count as 1 column, like Herdr counts them. fzf measures text on its
//! own, so the palette keeps `--no-hscroll --ellipsis=''` as a safety net.
//!
//! The submodules render what fzf shows; they are re-exported here so
//! callers write `layout::row_line(..)`:
//!
//! - [`row_line`](row_line()): the TSV line fzf reads for one row.
//! - [`header`](header()): the category line.
//! - [`footer`](footer()): description (with the git branch in Jump to)
//!   or key hints.
//! - [`dialog`](dialog()): the confirmation dialog, Herdr's close dialog
//!   copied.
//!
//! Rows and footer come in two layouts, [`LayoutMode`]: full, or minimal
//! for a popup narrowed well below its `[popup] min_width`.
//!
//! Nothing here reads fzf's environment or state files; callers pass the
//! column count, the layout mode and the footer mode in.

mod dialog;
mod footer;
mod header;
mod mode;
mod row_line;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::domain::color::Color;
use crate::domain::style::TextColors;
use crate::domain::tags;

pub use dialog::dialog;
pub(crate) use footer::fits_branch;
pub use footer::{FooterBranch, FooterMode, ParseFooterModeError, color_tag, footer, shown_cwd};
pub use header::header;
pub use mode::LayoutMode;
pub use row_line::{display, footer_lines, row_line};
// The use cases' tests compare footers against the hints.
#[cfg(test)]
pub(crate) use footer::KEY_HINTS;

/// Appended to text that was cut. One column wide.
const ELLIPSIS: &str = "…";

/// Width assumed when neither the terminal nor fzf says (`tput cols`,
/// `FZF_COLUMNS`).
pub(crate) const DEFAULT_COLUMNS: usize = 80;

/// Blank column at each side of the header, rows and footer: fzf padding
/// would shrink the divider lines too.
const GUTTER: &str = " ";

/// The columns a row or footer line has for text in a popup `columns`
/// wide: the width minus a [`GUTTER`] on each side.
fn inner_width(columns: usize) -> usize {
    columns.saturating_sub(2 * width(GUTTER))
}

/// Minimum blank columns between a cut text and what is right-aligned
/// after it: a row's title and its right column, the footer's
/// text and its last line's `[^/]`.
const GAP: usize = 2;

// SGR escape sequences fzf renders with `--ansi`.
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// The escape that starts muted text (shortcuts, inactive categories):
/// dim, or the settings' `muted` color. [`RESET`] ends it.
fn muted(colors: TextColors) -> String {
    colors
        .muted
        .map_or_else(|| DIM.to_owned(), Color::to_ansi_fg)
}

/// Terminal escapes for a full-screen message: hide the cursor, clear the
/// screen (in the current background color), move the cursor home.
pub(crate) const CLEAR_SCREEN: &str = "\x1b[?25l\x1b[2J\x1b[H";

/// Display width of `text` in terminal columns.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::layout;
///
/// assert_eq!(layout::width("abc"), 3);
/// assert_eq!(layout::width("日本"), 4); // double-width characters
/// assert_eq!(layout::width("e\u{301}"), 1); // `e` + combining accent
/// ```
#[must_use]
pub fn width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// `text` cut to at most `limit` columns, ending in `…` when cut.
///
/// Text that fits comes back unchanged. The cut falls between grapheme
/// clusters, never inside one. With `limit == 0` there is no room for the
/// ellipsis either, so cut text becomes empty.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::layout;
///
/// assert_eq!(layout::truncate("short", 10), "short");
/// assert_eq!(layout::truncate("abcdefghij", 6), "abcde…");
/// assert_eq!(layout::truncate("日本語テキスト", 6), "日本…");
/// ```
#[must_use]
pub fn truncate(text: &str, limit: usize) -> String {
    if width(text) <= limit {
        return text.to_owned();
    }
    // `checked_sub` is `None` when the ellipsis alone is too wide.
    let Some(room) = limit.checked_sub(width(ELLIPSIS)) else {
        return String::new();
    };
    format!("{}{ELLIPSIS}", cut(text, room))
}

/// `text` cut from the left to at most `limit` columns, starting with `…`
/// when cut: the end is kept (a cwd's last directories).
///
/// Like [`truncate`], mirrored: text that fits comes back unchanged, the
/// cut falls between grapheme clusters, and with `limit == 0` cut text
/// becomes empty.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::layout;
///
/// assert_eq!(layout::truncate_start("~/src", 10), "~/src");
/// assert_eq!(layout::truncate_start("~/Code/app/src/handlers", 14), "…/src/handlers");
/// ```
#[must_use]
pub fn truncate_start(text: &str, limit: usize) -> String {
    if width(text) <= limit {
        return text.to_owned();
    }
    let Some(room) = limit.checked_sub(width(ELLIPSIS)) else {
        return String::new();
    };
    format!("{ELLIPSIS}{}", cut_start(text, room))
}

/// The longest suffix of `text` at most `limit` columns wide, cut between
/// grapheme clusters: [`cut`] from the other end.
fn cut_start(text: &str, limit: usize) -> &str {
    let mut start = text.len();
    // `rev` walks the clusters from the end; each index is a boundary.
    for (index, _) in text.grapheme_indices(true).rev() {
        if width(&text[index..]) > limit {
            break;
        }
        start = index;
    }
    &text[start..]
}

/// The longest prefix of `text` at most `limit` columns wide, with no
/// `…`: cut between grapheme clusters, never inside one (Herdr's
/// `set_stringn` cuts the same way).
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::layout;
///
/// assert_eq!(layout::cut("abcdef", 4), "abcd");
/// assert_eq!(layout::cut("日本語", 3), "日");
/// assert_eq!(layout::cut("short", 10), "short");
/// ```
#[must_use]
pub fn cut(text: &str, limit: usize) -> &str {
    // Keep whole clusters while the kept prefix fits. Measuring the prefix
    // (instead of adding up cluster widths) stays exact where
    // `unicode-width` looks across clusters. Titles are short, so measuring
    // repeatedly costs nothing noticeable.
    let mut end = 0;
    for (start, cluster) in text.grapheme_indices(true) {
        let next = start + cluster.len();
        // `next` is a cluster boundary, so this slice can't split a character.
        if width(&text[..next]) > limit {
            break;
        }
        end = next;
    }
    &text[..end]
}

/// `text` truncated to `columns` and padded with spaces to exactly that
/// width: a fixed-width column (e.g. the Jump to workspace column).
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::layout;
///
/// assert_eq!(layout::fit("api", 6), "api   ");
/// assert_eq!(layout::fit("dotfiles", 6), "dotfi…");
/// ```
#[must_use]
pub fn fit(text: &str, columns: usize) -> String {
    let cut = truncate(text, columns);
    let pad = columns.saturating_sub(width(&cut));
    // `{:pad$}` pads the empty string to `pad` spaces.
    format!("{cut}{:pad$}", "")
}

/// `text` with every control character (tab, newline, escape, ...)
/// replaced by a space, so it can't break the TSV line fzf reads or inject
/// terminal escapes, and every `{hcp:<name>}` tag removed
/// ([`tags::strip`]), so it can't add a footer line or part.
/// Every user-provided field fzf shows passes here before it is
/// serialized; the payload only gets [`replace_controls`].
fn clean(text: &str) -> String {
    tags::strip(&replace_controls(text))
}

/// `text` with every control character (tab, newline, escape, ...)
/// replaced by a space, so it can't break the TSV line fzf reads or inject
/// terminal escapes. Tags stay: the payload is never parsed for them, and
/// the selection is matched back on it.
fn replace_controls(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use proptest::prelude::*;

    use crate::domain::fixtures::{CORPUS, text};

    use super::*;

    /// Whether `prefix` ends on a grapheme cluster boundary of `text`.
    fn is_cluster_boundary(text: &str, prefix: &str) -> bool {
        prefix.len() == text.len()
            || text
                .grapheme_indices(true)
                .any(|(start, _)| start == prefix.len())
    }

    #[test]
    fn measures_columns_not_bytes() {
        assert_eq!(width("abc"), 3);
        assert_eq!(width("日本"), 4);
        assert_eq!(width("e\u{301}"), 1);
        assert_eq!(width("\u{200b}"), 0);
    }

    #[test]
    fn counts_ambiguous_ellipsis_and_middle_dot_as_one_column() {
        assert_eq!(width(ELLIPSIS), 1);
        assert_eq!(width("·"), 1);
    }

    #[test]
    fn truncates_by_columns() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("abcdefghij", 6), "abcde…");
        assert_eq!(truncate("日本語テキスト", 6), "日本…");
    }

    #[test]
    fn truncates_at_grapheme_boundary() {
        assert_eq!(truncate("e\u{301}e\u{301}e\u{301}", 2), "e\u{301}…");
        assert_eq!(truncate("👨‍👩‍👧‍👦👨‍👩‍👧‍👦", 3), "👨‍👩‍👧‍👦…");
    }

    #[test]
    fn cuts_without_an_ellipsis_at_grapheme_boundaries() {
        assert_eq!(cut("abcdef", 4), "abcd");
        assert_eq!(cut("日本語", 3), "日");
        assert_eq!(cut("e\u{301}e\u{301}", 1), "e\u{301}");
        assert_eq!(cut("abc", 0), "");
    }

    #[test]
    fn truncates_from_the_left_keeping_the_end() {
        assert_eq!(truncate_start("short", 10), "short");
        assert_eq!(
            truncate_start("/home/me/src/handlers", 14),
            "…/src/handlers"
        );
        assert_eq!(truncate_start("日本語テキスト", 6), "…スト");
        assert_eq!(truncate_start("e\u{301}e\u{301}e\u{301}", 2), "…e\u{301}");
        assert_eq!(truncate_start("abc", 1), "…");
        assert_eq!(truncate_start("abc", 0), "");
    }

    #[test]
    fn leaves_no_room_for_the_ellipsis_at_zero_columns() {
        assert_eq!(truncate("abc", 0), "");
        assert_eq!(truncate("abc", 1), "…");
    }

    #[test]
    fn fits_by_padding_or_cutting() {
        assert_eq!(fit("api", 6), "api   ");
        assert_eq!(
            fit("a-very-long-workspace-name", 20),
            "a-very-long-workspa…"
        );
        assert_eq!(fit("日本語", 5), "日本…");
    }

    #[test]
    fn fits_by_display_width_without_splitting_graphemes() {
        assert_eq!(fit("日本語ワーク", 7), "日本語…");
        assert_eq!(fit("日本", 6), "日本  ");
        assert_eq!(fit("e\u{301}tude", 3), "e\u{301}t…");
        assert_eq!(fit("", 0), "");
    }

    #[test]
    fn mutes_with_dim_unless_a_color_is_set() {
        let colored = TextColors {
            muted: Some(Color::Ansi(8)),
            tags: None,
            branch: None,
        };

        assert_eq!(muted(TextColors::default()), DIM);
        assert_eq!(muted(colored), "\x1b[38;5;8m");
    }

    #[test]
    fn cleans_control_characters() {
        assert_eq!(clean("a\tb\nc\x1b[1m"), "a b c [1m");
    }

    #[test]
    fn replaces_control_characters_but_keeps_tags() {
        assert_eq!(replace_controls("a\tb{hcp:seg}c\n"), "a b{hcp:seg}c ");
    }

    #[test]
    fn cleans_tags_but_keeps_look_alikes() {
        assert_eq!(clean("a{hcp:seg}b{hcp:{hcp:line}line}c"), "abc");
        assert_eq!(
            clean("{hcp:Foo}{hcp:}{other:seg}"),
            "{hcp:Foo}{hcp:}{other:seg}"
        );
    }

    #[test]
    fn snapshot_truncate_corpus() {
        let mut out = String::new();
        for text in CORPUS {
            writeln!(out, "{text:?} ({} cols)", width(text)).unwrap();
            for limit in [0, 1, 2, 3, 5, 8, 12] {
                let cut = truncate(text, limit);
                writeln!(out, "  {limit:>2} -> {:>2} {cut:?}", width(&cut)).unwrap();
            }
        }
        insta::assert_snapshot!(out);
    }

    proptest! {
        #[test]
        fn truncated_width_fits_the_limit(text in text(), limit in 0usize..30) {
            prop_assert!(width(&truncate(&text, limit)) <= limit);
        }

        #[test]
        fn truncation_never_splits_a_cluster(text in text(), limit in 0usize..30) {
            let cut = truncate(&text, limit);
            let kept = if cut == text { &cut } else { cut.strip_suffix(ELLIPSIS).unwrap_or(&cut) };
            prop_assert!(text.starts_with(kept));
            prop_assert!(is_cluster_boundary(&text, kept));
        }

        #[test]
        fn ellipsis_only_when_cut(text in text(), limit in 0usize..30) {
            let cut = truncate(&text, limit);
            if width(&text) <= limit {
                prop_assert_eq!(cut, text);
            } else {
                prop_assert!(cut.ends_with(ELLIPSIS) || (limit == 0 && cut.is_empty()));
            }
        }

        #[test]
        fn cut_is_the_longest_fitting_cluster_prefix(text in text(), limit in 0usize..30) {
            let kept = cut(&text, limit);
            prop_assert!(width(kept) <= limit);
            prop_assert!(text.starts_with(kept));
            prop_assert!(is_cluster_boundary(&text, kept));
            // One more cluster would not fit.
            if let Some(next) = text[kept.len()..].graphemes(true).next() {
                prop_assert!(width(&text[..kept.len() + next.len()]) > limit);
            }
        }

        #[test]
        fn left_cut_fits_and_keeps_the_end(text in text(), limit in 0usize..30) {
            let cut = truncate_start(&text, limit);
            prop_assert!(width(&cut) <= limit);
            if width(&text) <= limit {
                prop_assert_eq!(cut, text);
            } else if let Some(kept) = cut.strip_prefix(ELLIPSIS) {
                prop_assert!(text.ends_with(kept));
                prop_assert!(is_cluster_boundary(&text, &text[..text.len() - kept.len()]));
                // One more cluster would not fit.
                let start = text.len() - kept.len();
                if let Some(previous) = text[..start].graphemes(true).next_back() {
                    prop_assert!(width(&text[start - previous.len()..]) > limit - 1);
                }
            } else {
                prop_assert!(limit == 0 && cut.is_empty());
            }
        }

        #[test]
        fn cleaned_text_has_no_tag_and_stays_clean(
            text in text(),
            names in proptest::collection::vec("[a-z_{}:\n]{0,5}", 0..4),
        ) {
            let mut mixed = text;
            for name in names {
                mixed = format!("{{hcp:{name}}}{mixed}{{hcp:{mixed}");
            }

            let cleaned = clean(&mixed);

            prop_assert_eq!(tags::strip(&cleaned), cleaned.clone());
            prop_assert!(!cleaned.chars().any(char::is_control));
            prop_assert_eq!(clean(&cleaned), cleaned);
        }

        #[test]
        fn fit_is_exactly_the_column_width(text in text(), columns in 0usize..30) {
            prop_assert_eq!(width(&fit(&text, columns)), columns);
        }
    }
}
