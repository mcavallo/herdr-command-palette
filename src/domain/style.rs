//! The palette's colors: the settings' `[colors]` over what is derived
//! from Herdr's theme; the confirmation dialog's, from Herdr's theme
//! alone.
//!
//! Three consumers, three types: [`PickerColors`] are fzf's `--color`
//! slots and the error screen's background; [`TextColors`] are escapes
//! that the palette writes into its own text (shortcuts, header, footer
//! tags); [`DialogColors`] paint the confirmation dialog.
//! Rendering fzf's flags is the picker's job, placing escapes the layout's.

use crate::domain::color::Color;
use crate::domain::settings::ColorSettings;
use crate::domain::theme::ThemeColors;

/// What fzf draws, plus the error screen's background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickerColors {
    /// fzf `bg`, `bg+` and `gutter`, the error screen: Herdr's
    /// `panel_bg` unless set.
    pub background: Color,
    /// fzf `footer`: `muted`, else Herdr's `overlay0`.
    pub footer: Color,
    /// fzf `header`, the categories: Herdr's `text`. The
    /// active one is bold, the others muted, by their own escapes.
    pub header: Color,
    /// fzf `query`, the search text: Herdr's `subtext0`.
    pub query: Color,
    /// fzf `fg`, the rows: Herdr's `subtext0`.
    pub rows: Color,
    /// fzf `hl` (underline): `matches`, else the rows' color.
    pub matches: Color,
    /// fzf `hl+` (bold + underline, the selected row): `matches`, else
    /// the selected row's color.
    pub selected_matches: Color,
    /// fzf `fg+` (kept bold): the selected row's text color,
    /// Herdr's `text` unless set. Never a background: fzf paints `bg+` on
    /// its own last column too.
    pub selected: Color,
    /// fzf `input-border`, `header-border`, `footer-border`; `None` is dim.
    pub dividers: Option<Color>,
}

impl PickerColors {
    /// `colors` over the colors derived from `theme`.
    #[must_use]
    pub fn new(theme: ThemeColors, colors: &ColorSettings) -> Self {
        let rows = theme.subtext0;
        let selected = colors.selected.unwrap_or(theme.text);
        // Without `matches`, a match keeps its row's color: only the
        // underline (and bold in the selected row) marks it.
        Self {
            background: colors.background.unwrap_or(theme.panel_bg),
            footer: colors.muted.unwrap_or(theme.overlay0),
            header: theme.text,
            query: theme.subtext0,
            rows,
            matches: colors.matches.unwrap_or(rows),
            selected_matches: colors.matches.unwrap_or(selected),
            selected,
            dividers: colors.dividers,
        }
    }
}

/// Colors the palette writes into its own text as escapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextColors {
    /// Shortcuts and inactive categories; `None` is dim.
    pub muted: Option<Color>,
    /// The footer's kind tag label (`plugin`); `None` leaves it in the
    /// footer's color.
    pub tags: Option<Color>,
    /// The git branch in Jump to's footer; `None` leaves it in
    /// the footer's color.
    pub branch: Option<Color>,
}

impl TextColors {
    /// The text colors of `colors` over `theme`: the tag label defaults to
    /// Herdr's `subtext0`, a dim label color brighter than the footer's
    /// `overlay0`; the branch to Herdr's `mauve`.
    #[must_use]
    pub fn new(theme: ThemeColors, colors: &ColorSettings) -> Self {
        Self {
            muted: colors.muted,
            tags: Some(colors.tags.unwrap_or(theme.subtext0)),
            branch: Some(colors.branch.unwrap_or(theme.mauve)),
        }
    }
}

/// The confirmation dialog's colors: Herdr's close dialog,
/// with `accent` where Herdr uses `red`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogColors {
    /// Every cell's background: Herdr's `panel_bg`, as the frame Herdr
    /// draws around the popup (never the settings' `background`).
    pub background: Color,
    /// The title and the confirm button's background: Herdr's `accent`.
    pub accent: Color,
    /// The detail (label and counts lines) and the cancel button's label: Herdr's `text`.
    pub text: Color,
    /// The cancel button's background: Herdr's `surface0`.
    pub surface0: Color,
    /// The confirm button's label when [`background`](Self::background)
    /// is [`Color::Reset`] (it is the background otherwise): Herdr's
    /// `surface_dim`.
    pub surface_dim: Color,
}

impl DialogColors {
    /// The dialog's colors: Herdr's theme only, like Herdr's frame
    /// around it; the settings' `[colors]` don't apply.
    #[must_use]
    pub fn new(theme: ThemeColors) -> Self {
        Self {
            background: theme.panel_bg,
            accent: theme.accent,
            text: theme.text,
            surface0: theme.surface0,
            surface_dim: theme.surface_dim,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::theme::theme_by_name;

    use super::*;

    const THEME: ThemeColors = ThemeColors {
        panel_bg: Color::Ansi(0),
        overlay0: Color::Ansi(8),
        subtext0: Color::Ansi(7),
        mauve: Color::Ansi(5),
        text: Color::Rgb(0xcd, 0xd6, 0xf4),
        ..ThemeColors::TERMINAL_DEFAULT
    };

    #[test]
    fn derives_from_the_theme_without_settings() {
        let colors = PickerColors::new(THEME, &ColorSettings::default());

        assert_eq!(
            colors,
            PickerColors {
                background: Color::Ansi(0),
                footer: Color::Ansi(8),
                header: Color::Rgb(0xcd, 0xd6, 0xf4),
                query: Color::Ansi(7),
                rows: Color::Ansi(7),
                matches: Color::Ansi(7),
                selected_matches: Color::Rgb(0xcd, 0xd6, 0xf4),
                selected: Color::Rgb(0xcd, 0xd6, 0xf4),
                dividers: None,
            }
        );
    }

    #[test]
    fn selected_keeps_the_terminal_default_text_with_the_terminal_default() {
        let colors = PickerColors::new(ThemeColors::TERMINAL_DEFAULT, &ColorSettings::default());

        assert_eq!(colors.selected, Color::Reset);
    }

    #[test]
    fn the_terminal_theme_maps_to_herdrs_terminal_colors() {
        let terminal = theme_by_name("terminal").unwrap();

        let colors = PickerColors::new(terminal, &ColorSettings::default());

        // Herdr's `terminal`: `text` Reset, `subtext0` Gray.
        assert_eq!(
            (
                colors.header,
                colors.query,
                colors.rows,
                colors.matches,
                colors.selected_matches,
                colors.selected
            ),
            (
                Color::Reset,
                Color::Ansi(7),
                Color::Ansi(7),
                Color::Ansi(7),
                Color::Reset,
                Color::Reset
            )
        );
    }

    #[test]
    fn selected_matches_follow_the_selected_setting() {
        let settings = ColorSettings {
            selected: Some(Color::Ansi(2)),
            ..ColorSettings::default()
        };

        let colors = PickerColors::new(THEME, &settings);

        assert_eq!(
            (colors.matches, colors.selected_matches),
            (Color::Ansi(7), Color::Ansi(2))
        );
    }

    #[test]
    fn tags_and_branch_default_to_the_theme_subtext0_and_mauve() {
        let text = TextColors::new(THEME, &ColorSettings::default());

        assert_eq!(
            text,
            TextColors {
                muted: None,
                tags: Some(Color::Ansi(7)),
                branch: Some(Color::Ansi(5)),
            }
        );
    }

    #[test]
    fn settings_override_the_theme() {
        let settings = ColorSettings {
            background: Some(Color::Reset),
            muted: Some(Color::Ansi(244)),
            matches: Some(Color::Ansi(1)),
            selected: Some(Color::Ansi(2)),
            dividers: Some(Color::Ansi(3)),
            tags: Some(Color::Ansi(4)),
            branch: Some(Color::Ansi(6)),
        };

        let picker = PickerColors::new(THEME, &settings);
        let text = TextColors::new(THEME, &settings);

        assert_eq!(
            picker,
            PickerColors {
                background: Color::Reset,
                footer: Color::Ansi(244),
                header: Color::Rgb(0xcd, 0xd6, 0xf4),
                query: Color::Ansi(7),
                rows: Color::Ansi(7),
                matches: Color::Ansi(1),
                selected_matches: Color::Ansi(1),
                selected: Color::Ansi(2),
                dividers: Some(Color::Ansi(3)),
            }
        );
        assert_eq!(
            text,
            TextColors {
                muted: Some(Color::Ansi(244)),
                tags: Some(Color::Ansi(4)),
                branch: Some(Color::Ansi(6)),
            }
        );
    }

    #[test]
    fn the_dialog_takes_herdrs_tokens_and_panel_bg() {
        let theme = ThemeColors {
            accent: Color::Ansi(4),
            text: Color::Ansi(15),
            surface0: Color::Ansi(8),
            surface_dim: Color::Ansi(0),
            ..THEME
        };

        let dialog = DialogColors::new(theme);

        assert_eq!(
            dialog,
            DialogColors {
                background: Color::Ansi(0),
                accent: Color::Ansi(4),
                text: Color::Ansi(15),
                surface0: Color::Ansi(8),
                surface_dim: Color::Ansi(0),
            }
        );
    }
}
