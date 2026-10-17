//! Colors borrowed from Herdr's theme so the popup blends in:
//! `panel_bg` (fzf background; Herdr paints the column it reserves right of
//! the popup with it), `overlay0` (footer, like the keybinds modal
//! footer), `subtext0` (rows, query, the footer's kind tag), `text`
//! (categories, selected row) and, for the confirmation
//! dialog, `accent`, `text`, `surface0`, `surface_dim`, and
//! `mauve` (the footer's git branch), with
//! `[theme.custom]` overrides. With `[theme] auto_switch`, the theme and overrides follow
//! the terminal's [`Appearance`] as in Herdr; finding out the
//! appearance is the caller's job.
//!
//! Rendering a [`Color`] for fzf or the terminal is
//! [`color`](crate::domain::color)'s job.

use crate::domain::appearance::Appearance;
use crate::domain::color::Color;
use crate::domain::config::{ThemeConfig, ThemeTokens};
use crate::domain::non_empty;

/// The theme colors the palette uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeColors {
    /// Panel background: fzf `bg`, `bg+`, `gutter` and the error screen
    /// (hides the column Herdr paints with it), unless the settings say
    /// otherwise.
    pub panel_bg: Color,
    /// Dim text: the footer, unless the settings say otherwise.
    pub overlay0: Color,
    /// Dim labels (Herdr's settings, workspace rows): the rows, the query
    /// and the footer's kind tag (`plugin`), unless the settings say
    /// otherwise.
    pub subtext0: Color,
    /// Accent: the confirmation dialog's title and confirm button, where
    /// Herdr's close dialog uses `red`, to match the popup frame.
    pub accent: Color,
    /// Body text: the categories, the selected row and the confirmation
    /// dialog's detail and cancel button.
    pub text: Color,
    /// Raised surface: the cancel button's background.
    pub surface0: Color,
    /// Dim surface: the confirm button's text when `panel_bg` is `reset`.
    pub surface_dim: Color,
    /// Mauve: the git branch on Jump to's footer line 2,
    /// unless the settings say otherwise.
    pub mauve: Color,
}

impl ThemeColors {
    /// Terminal defaults, for a `config.toml` that can't be read.
    pub(crate) const TERMINAL_DEFAULT: Self = Self {
        panel_bg: Color::Reset,
        overlay0: Color::Reset,
        subtext0: Color::Reset,
        accent: Color::Reset,
        text: Color::Reset,
        surface0: Color::Reset,
        surface_dim: Color::Reset,
        mauve: Color::Reset,
    };
}

/// `(name, colors)` of Herdr's 18 built-in themes, by canonical name.
///
/// Copied from `Palette::*()` in Herdr `src/app/state.rs`, v0.9.3. Named
/// colors are their ANSI indexes (`Gray` 7, `DarkGray` 8, `Blue` 4).
pub const THEME_COLORS: [(&str, ThemeColors); 18] = {
    // Variant names without the `Color::` prefix keep each theme short.
    use Color::{Ansi, Reset, Rgb};
    [
        (
            "catppuccin",
            ThemeColors {
                panel_bg: Rgb(0x18, 0x18, 0x25),
                overlay0: Rgb(0x6c, 0x70, 0x86),
                subtext0: Rgb(0xa6, 0xad, 0xc8),
                accent: Rgb(0x89, 0xb4, 0xfa),
                text: Rgb(0xcd, 0xd6, 0xf4),
                surface0: Rgb(0x31, 0x32, 0x44),
                surface_dim: Rgb(0x1e, 0x1e, 0x2e),
                mauve: Rgb(0xcb, 0xa6, 0xf7),
            },
        ),
        (
            "catppuccin-latte",
            ThemeColors {
                panel_bg: Rgb(0xef, 0xf1, 0xf5),
                overlay0: Rgb(0x9c, 0xa0, 0xb0),
                subtext0: Rgb(0x6c, 0x6f, 0x85),
                accent: Rgb(0x1e, 0x66, 0xf5),
                text: Rgb(0x4c, 0x4f, 0x69),
                surface0: Rgb(0xcc, 0xd0, 0xda),
                surface_dim: Rgb(0xe6, 0xe9, 0xef),
                mauve: Rgb(0x88, 0x39, 0xef),
            },
        ),
        (
            "terminal",
            ThemeColors {
                panel_bg: Reset,
                overlay0: Ansi(7),
                subtext0: Ansi(7),
                accent: Ansi(4),
                text: Reset,
                surface0: Reset,
                surface_dim: Ansi(8),
                mauve: Ansi(7),
            },
        ),
        (
            "tokyo-night",
            ThemeColors {
                panel_bg: Rgb(0x1a, 0x1b, 0x26),
                overlay0: Rgb(0x56, 0x5f, 0x89),
                subtext0: Rgb(0xa9, 0xb1, 0xd6),
                accent: Rgb(0x7a, 0xa2, 0xf7),
                text: Rgb(0xc0, 0xca, 0xf5),
                surface0: Rgb(0x24, 0x28, 0x3b),
                surface_dim: Rgb(0x1a, 0x1b, 0x26),
                mauve: Rgb(0xbb, 0x9a, 0xf7),
            },
        ),
        (
            "tokyo-night-day",
            ThemeColors {
                panel_bg: Rgb(0xe1, 0xe2, 0xe7),
                overlay0: Rgb(0x89, 0x90, 0xb3),
                subtext0: Rgb(0x61, 0x72, 0xb0),
                accent: Rgb(0x2e, 0x7d, 0xe9),
                text: Rgb(0x37, 0x60, 0xbf),
                surface0: Rgb(0xc4, 0xc8, 0xda),
                surface_dim: Rgb(0xd2, 0xd3, 0xda),
                mauve: Rgb(0x78, 0x47, 0xbd),
            },
        ),
        (
            "dracula",
            ThemeColors {
                panel_bg: Rgb(0x28, 0x2a, 0x36),
                overlay0: Rgb(0x62, 0x72, 0xa4),
                subtext0: Rgb(0xd2, 0xd2, 0xdc),
                accent: Rgb(0xbd, 0x93, 0xf9),
                text: Rgb(0xf8, 0xf8, 0xf2),
                surface0: Rgb(0x44, 0x47, 0x5a),
                surface_dim: Rgb(0x28, 0x2a, 0x36),
                mauve: Rgb(0xff, 0x79, 0xc6),
            },
        ),
        (
            "nord",
            ThemeColors {
                panel_bg: Rgb(0x2e, 0x34, 0x40),
                overlay0: Rgb(0x4c, 0x56, 0x6a),
                subtext0: Rgb(0xd8, 0xde, 0xe9),
                accent: Rgb(0x88, 0xc0, 0xd0),
                text: Rgb(0xec, 0xef, 0xf4),
                surface0: Rgb(0x3b, 0x42, 0x52),
                surface_dim: Rgb(0x2e, 0x34, 0x40),
                mauve: Rgb(0xb4, 0x8e, 0xad),
            },
        ),
        (
            "gruvbox",
            ThemeColors {
                panel_bg: Rgb(0x28, 0x28, 0x28),
                overlay0: Rgb(0x92, 0x83, 0x74),
                subtext0: Rgb(0xd5, 0xc4, 0xa1),
                accent: Rgb(0xd7, 0x99, 0x21),
                text: Rgb(0xeb, 0xdb, 0xb2),
                surface0: Rgb(0x3c, 0x38, 0x36),
                surface_dim: Rgb(0x28, 0x28, 0x28),
                mauve: Rgb(0xd3, 0x86, 0x9b),
            },
        ),
        (
            "gruvbox-light",
            ThemeColors {
                panel_bg: Rgb(0xfb, 0xf1, 0xc7),
                overlay0: Rgb(0x92, 0x83, 0x74),
                subtext0: Rgb(0x50, 0x49, 0x45),
                accent: Rgb(0x07, 0x66, 0x78),
                text: Rgb(0x3c, 0x38, 0x36),
                surface0: Rgb(0xeb, 0xdb, 0xb2),
                surface_dim: Rgb(0xf2, 0xe5, 0xbc),
                mauve: Rgb(0x8f, 0x3f, 0x71),
            },
        ),
        (
            "one-dark",
            ThemeColors {
                panel_bg: Rgb(0x28, 0x2c, 0x34),
                overlay0: Rgb(0x5c, 0x63, 0x70),
                subtext0: Rgb(0x96, 0x9c, 0xa8),
                accent: Rgb(0x61, 0xaf, 0xef),
                text: Rgb(0xab, 0xb2, 0xbf),
                surface0: Rgb(0x2c, 0x31, 0x3a),
                surface_dim: Rgb(0x28, 0x2c, 0x34),
                mauve: Rgb(0xc6, 0x78, 0xdd),
            },
        ),
        (
            "one-light",
            ThemeColors {
                panel_bg: Rgb(0xfa, 0xfa, 0xfa),
                overlay0: Rgb(0xa0, 0xa1, 0xa7),
                subtext0: Rgb(0x68, 0x6b, 0x77),
                accent: Rgb(0x40, 0x78, 0xf2),
                text: Rgb(0x38, 0x3a, 0x42),
                surface0: Rgb(0xf0, 0xf0, 0xf1),
                surface_dim: Rgb(0xf5, 0xf5, 0xf6),
                mauve: Rgb(0xa6, 0x26, 0xa4),
            },
        ),
        (
            "solarized",
            ThemeColors {
                panel_bg: Rgb(0x00, 0x2b, 0x36),
                overlay0: Rgb(0x58, 0x6e, 0x75),
                subtext0: Rgb(0x83, 0x94, 0x96),
                accent: Rgb(0x26, 0x8b, 0xd2),
                text: Rgb(0x93, 0xa1, 0xa1),
                surface0: Rgb(0x07, 0x36, 0x42),
                surface_dim: Rgb(0x00, 0x2b, 0x36),
                mauve: Rgb(0xd3, 0x36, 0x82),
            },
        ),
        (
            "solarized-light",
            ThemeColors {
                panel_bg: Rgb(0xfd, 0xf6, 0xe3),
                overlay0: Rgb(0x93, 0xa1, 0xa1),
                subtext0: Rgb(0x83, 0x94, 0x96),
                accent: Rgb(0x26, 0x8b, 0xd2),
                text: Rgb(0x65, 0x7b, 0x83),
                surface0: Rgb(0xee, 0xe8, 0xd5),
                surface_dim: Rgb(0xee, 0xe8, 0xd5),
                mauve: Rgb(0xd3, 0x36, 0x82),
            },
        ),
        (
            "kanagawa",
            ThemeColors {
                panel_bg: Rgb(0x1f, 0x1f, 0x28),
                overlay0: Rgb(0x72, 0x71, 0x69),
                subtext0: Rgb(0xc8, 0xc3, 0xaa),
                accent: Rgb(0x7e, 0x9c, 0xd8),
                text: Rgb(0xdc, 0xd7, 0xba),
                surface0: Rgb(0x2a, 0x2a, 0x37),
                surface_dim: Rgb(0x1f, 0x1f, 0x28),
                mauve: Rgb(0x95, 0x7f, 0xb8),
            },
        ),
        (
            "kanagawa-lotus",
            ThemeColors {
                panel_bg: Rgb(0xf2, 0xec, 0xbc),
                overlay0: Rgb(0xa0, 0x9c, 0xac),
                subtext0: Rgb(0x43, 0x43, 0x6c),
                accent: Rgb(0x4d, 0x69, 0x9b),
                text: Rgb(0x54, 0x54, 0x64),
                surface0: Rgb(0xdc, 0xd5, 0xac),
                surface_dim: Rgb(0xd5, 0xce, 0xa3),
                mauve: Rgb(0x62, 0x4c, 0x83),
            },
        ),
        (
            "rose-pine",
            ThemeColors {
                panel_bg: Rgb(0x19, 0x17, 0x24),
                overlay0: Rgb(0x6e, 0x6a, 0x86),
                subtext0: Rgb(0xc8, 0xc5, 0xdc),
                accent: Rgb(0xc4, 0xa7, 0xe7),
                text: Rgb(0xe0, 0xde, 0xf4),
                surface0: Rgb(0x1f, 0x1d, 0x2e),
                surface_dim: Rgb(0x26, 0x23, 0x3a),
                mauve: Rgb(0xc4, 0xa7, 0xe7),
            },
        ),
        (
            "rose-pine-dawn",
            ThemeColors {
                panel_bg: Rgb(0xfa, 0xf4, 0xed),
                overlay0: Rgb(0x98, 0x93, 0xa5),
                subtext0: Rgb(0x79, 0x75, 0x93),
                accent: Rgb(0x90, 0x7a, 0xa9),
                text: Rgb(0x46, 0x42, 0x61),
                surface0: Rgb(0xf2, 0xe9, 0xe1),
                surface_dim: Rgb(0xf2, 0xe9, 0xe1),
                mauve: Rgb(0x90, 0x7a, 0xa9),
            },
        ),
        (
            "vesper",
            ThemeColors {
                panel_bg: Rgb(0x1a, 0x1a, 0x1a),
                overlay0: Rgb(0x5c, 0x5c, 0x5c),
                subtext0: Rgb(0xa0, 0xa0, 0xa0),
                accent: Rgb(0xff, 0xc7, 0x99),
                text: Rgb(0xff, 0xff, 0xff),
                surface0: Rgb(0x23, 0x23, 0x23),
                surface_dim: Rgb(0x10, 0x10, 0x10),
                mauve: Rgb(0xff, 0xd1, 0xa8),
            },
        ),
    ]
};

/// Other names Herdr accepts for its themes: (alias, canonical name).
///
/// Copied from `canonical_theme_name` in Herdr `src/config/theme.rs`,
/// v0.9.3.
pub const THEME_ALIASES: [(&str, &str); 14] = [
    ("catppuccin-mocha", "catppuccin"),
    ("latte", "catppuccin-latte"),
    ("light", "catppuccin-latte"),
    ("tokyonight", "tokyo-night"),
    ("tokyo-day", "tokyo-night-day"),
    ("tokyonight-day", "tokyo-night-day"),
    ("gruvbox-dark", "gruvbox"),
    ("onedark", "one-dark"),
    ("onelight", "one-light"),
    ("solarized-dark", "solarized"),
    ("lotus", "kanagawa-lotus"),
    ("rosepine", "rose-pine"),
    ("rosepine-dawn", "rose-pine-dawn"),
    ("dawn", "rose-pine-dawn"),
];

/// The dark and light theme of each family, for `auto_switch` without
/// `dark_name` / `light_name`. A theme outside these pairs is its own
/// sibling in both slots.
///
/// Copied from `sibling_theme_names` in Herdr `src/app/mod.rs`, v0.9.3.
pub const THEME_SIBLINGS: [(&str, &str); 7] = [
    ("catppuccin", "catppuccin-latte"),
    ("tokyo-night", "tokyo-night-day"),
    ("gruvbox", "gruvbox-light"),
    ("one-dark", "one-light"),
    ("solarized", "solarized-light"),
    ("kanagawa", "kanagawa-lotus"),
    ("rose-pine", "rose-pine-dawn"),
];

/// Herdr's default theme, used when `[theme] name` is unset and for
/// unknown names (in the dark slot of `auto_switch`).
const DEFAULT_THEME: &str = "catppuccin";

/// The theme Herdr falls back to for an unknown `light_name`.
const DEFAULT_LIGHT_THEME: &str = "catppuccin-latte";

/// A theme name as Herdr compares it: lowercase, spaces and `_` as `-`.
fn normalize(name: &str) -> String {
    name.to_lowercase().replace([' ', '_'], "-")
}

/// The canonical name of a built-in theme or alias; `None` for unknown
/// names.
fn canonical(name: &str) -> Option<&'static str> {
    let name = normalize(name);
    let name = THEME_ALIASES
        .iter()
        .find(|(alias, _)| *alias == name)
        .map_or(name.as_str(), |(_, canonical)| canonical);
    THEME_COLORS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(known, _)| *known)
}

/// Colors of a theme by name, as Herdr resolves it: lowercased, spaces and
/// `_` as `-`, aliases followed. `None` for an unknown theme.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::theme::theme_by_name;
///
/// let colors = theme_by_name("Tokyo Night").unwrap();
/// assert_eq!(colors.panel_bg.to_fzf(), "#1a1b26");
/// assert_eq!(theme_by_name("rose_pine"), theme_by_name("rosepine"));
/// ```
#[must_use]
pub fn theme_by_name(name: &str) -> Option<ThemeColors> {
    let name = canonical(name)?;
    THEME_COLORS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|&(_, colors)| colors)
}

/// The (dark, light) themes `auto_switch` uses for `name` when
/// `dark_name` / `light_name` are unset: its family's pair, else `name`
/// twice.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::theme::siblings;
///
/// assert_eq!(siblings("Latte"), ("catppuccin".into(), "catppuccin-latte".into()));
/// assert_eq!(siblings("nord"), ("nord".into(), "nord".into()));
/// ```
#[must_use]
pub fn siblings(name: &str) -> (String, String) {
    canonical(name)
        .and_then(|name| {
            THEME_SIBLINGS
                .iter()
                .find(|(dark, light)| *dark == name || *light == name)
        })
        .map_or_else(
            || (name.to_owned(), name.to_owned()),
            |(dark, light)| ((*dark).to_owned(), (*light).to_owned()),
        )
}

/// The palette's colors for `theme`, resolved as Herdr resolves its own
/// (Herdr `resolve_effective_theme`, `src/app/mod.rs`, v0.9.3):
///
/// 1. The theme: `name` (default `catppuccin`); with `auto_switch`,
///    `dark_name` or `light_name` by `appearance` (unknown is dark), each
///    defaulting to `name`'s [`siblings`].
/// 2. An unknown name falls back to `catppuccin` (`catppuccin-latte` in
///    the light slot), as Herdr does.
/// 3. `[theme.custom]` overrides, then, with `auto_switch`,
///    `[theme.custom.dark]` or `[theme.custom.light]`. Empty values count
///    as unset.
///
/// `appearance` is ignored without `auto_switch`.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::appearance::Appearance;
/// use herdr_command_palette::domain::config::HerdrConfig;
/// use herdr_command_palette::domain::theme::theme_colors;
///
/// let config = HerdrConfig::from_toml("[theme]\nauto_switch = true").unwrap();
/// let light = theme_colors(&config.theme, Some(Appearance::Light));
/// assert_eq!(light.panel_bg.to_fzf(), "#eff1f5"); // catppuccin-latte
/// ```
#[must_use]
pub fn theme_colors(theme: &ThemeConfig, appearance: Option<Appearance>) -> ThemeColors {
    let manual = theme.name.as_deref().unwrap_or(DEFAULT_THEME);
    let (name, fallback, mode) = if theme.auto_switch {
        let (dark, light) = siblings(manual);
        match appearance.unwrap_or(Appearance::Dark) {
            Appearance::Dark => (
                theme.dark_name.clone().unwrap_or(dark),
                DEFAULT_THEME,
                Some(&theme.custom.dark),
            ),
            Appearance::Light => (
                theme.light_name.clone().unwrap_or(light),
                DEFAULT_LIGHT_THEME,
                Some(&theme.custom.light),
            ),
        }
    } else {
        (manual.to_owned(), DEFAULT_THEME, None)
    };
    let base = theme_by_name(&name)
        .or_else(|| theme_by_name(fallback))
        // Not reached: both fallbacks are in the table (tested).
        .unwrap_or(ThemeColors::TERMINAL_DEFAULT);
    let colors = with_overrides(base, &theme.custom.tokens);
    mode.map_or(colors, |tokens| with_overrides(colors, tokens))
}

/// `colors` with the tokens set (non-empty) in `tokens` replaced.
fn with_overrides(colors: ThemeColors, tokens: &ThemeTokens) -> ThemeColors {
    let token = |value: &Option<String>| non_empty(value.as_deref()).map(Color::parse);
    ThemeColors {
        panel_bg: token(&tokens.panel_bg).unwrap_or(colors.panel_bg),
        overlay0: token(&tokens.overlay0).unwrap_or(colors.overlay0),
        subtext0: token(&tokens.subtext0).unwrap_or(colors.subtext0),
        accent: token(&tokens.accent).unwrap_or(colors.accent),
        text: token(&tokens.text).unwrap_or(colors.text),
        surface0: token(&tokens.surface0).unwrap_or(colors.surface0),
        surface_dim: token(&tokens.surface_dim).unwrap_or(colors.surface_dim),
        mauve: token(&tokens.mauve).unwrap_or(colors.mauve),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::domain::config::HerdrConfig;

    use super::*;

    fn colors_for(toml: &str) -> (String, String) {
        let config = HerdrConfig::from_toml(toml).unwrap();
        let colors = theme_colors(&config.theme, None);
        (colors.panel_bg.to_fzf(), colors.overlay0.to_fzf())
    }

    fn pair(panel_bg: &str, overlay0: &str) -> (String, String) {
        (panel_bg.to_owned(), overlay0.to_owned())
    }

    #[test]
    fn defaults_to_catppuccin() {
        assert_eq!(colors_for(""), pair("#181825", "#6c7086"));
    }

    #[test]
    fn normalises_the_name() {
        assert_eq!(
            colors_for("[theme]\nname = \"Tokyo Night\""),
            pair("#1a1b26", "#565f89")
        );
        assert_eq!(
            colors_for("[theme]\nname = \"solarized_dark\""),
            pair("#002b36", "#586e75")
        );
    }

    #[test]
    fn terminal_theme_uses_terminal_colors() {
        assert_eq!(colors_for("[theme]\nname = \"terminal\""), pair("-1", "7"));
    }

    #[test]
    fn unknown_theme_falls_back_to_catppuccin_like_herdr() {
        assert_eq!(
            colors_for("[theme]\nname = \"hotdog\""),
            pair("#181825", "#6c7086")
        );
    }

    #[test]
    fn custom_colors_apply_over_the_fallback() {
        let toml = "[theme]\nname = \"hotdog\"\n[theme.custom]\npanel_bg = \"reset\"";

        assert_eq!(colors_for(toml), pair("-1", "#6c7086"));
    }

    #[test]
    fn custom_colors_override_the_theme() {
        let toml =
            "[theme]\nname = \"nord\"\n[theme.custom]\npanel_bg = \"#000\"\noverlay0 = \"red\"";

        assert_eq!(colors_for(toml), pair("#000000", "1"));
    }

    #[test]
    fn reset_override_means_terminal_default() {
        let toml = "[theme.custom]\npanel_bg = \"reset\"\noverlay0 = \"\"";

        assert_eq!(colors_for(toml), pair("-1", "#6c7086"));
    }

    #[test]
    fn theme_table_is_complete_and_aliases_resolve() {
        let names: HashSet<&str> = THEME_COLORS.iter().map(|(name, _)| *name).collect();
        assert_eq!(names.len(), THEME_COLORS.len());
        for (alias, canonical) in THEME_ALIASES {
            assert!(names.contains(canonical), "{alias} -> {canonical}");
            assert_eq!(theme_by_name(alias), theme_by_name(canonical));
        }
    }

    #[test]
    fn pins_a_few_theme_colors() {
        let hex = |name| {
            let colors = theme_by_name(name).unwrap();
            pair(&colors.panel_bg.to_fzf(), &colors.overlay0.to_fzf())
        };
        assert_eq!(hex("dracula"), pair("#282a36", "#6272a4"));
        assert_eq!(hex("solarized"), pair("#002b36", "#586e75"));
        assert_eq!(hex("dawn"), pair("#faf4ed", "#9893a5"));
    }

    #[test]
    fn pins_the_dialog_tokens_of_a_few_themes() {
        let dialog = |name| {
            let colors = theme_by_name(name).unwrap();
            (
                colors.accent,
                colors.text,
                colors.surface0,
                colors.surface_dim,
            )
        };

        assert_eq!(
            dialog("catppuccin"),
            (
                Color::Rgb(0x89, 0xb4, 0xfa),
                Color::Rgb(0xcd, 0xd6, 0xf4),
                Color::Rgb(0x31, 0x32, 0x44),
                Color::Rgb(0x1e, 0x1e, 0x2e)
            )
        );
        // Herdr's `terminal`: `Blue`, `Reset`, `Reset`, `DarkGray`.
        assert_eq!(
            dialog("terminal"),
            (Color::Ansi(4), Color::Reset, Color::Reset, Color::Ansi(8))
        );
    }

    #[test]
    fn custom_dialog_tokens_override_the_theme() {
        let toml = "[theme.custom]\naccent = \"red\"\ntext = \"#010203\"\n\
                    surface0 = \"reset\"\nsurface_dim = \"\"";
        let config = HerdrConfig::from_toml(toml).unwrap();

        let colors = theme_colors(&config.theme, None);

        assert_eq!(
            (
                colors.accent,
                colors.text,
                colors.surface0,
                colors.surface_dim
            ),
            (
                Color::Ansi(1),
                Color::Rgb(1, 2, 3),
                Color::Reset,
                // Empty counts as unset: catppuccin's stays.
                Color::Rgb(0x1e, 0x1e, 0x2e)
            )
        );
    }

    #[test]
    fn per_appearance_dialog_tokens_apply_after_custom() {
        let toml = format!(
            "{AUTO}[theme.custom]\naccent = \"#010101\"\nsurface_dim = \"blue\"\n\
             [theme.custom.light]\naccent = \"#020202\"\ntext = \"white\"\nsurface0 = \"black\""
        );
        let config = HerdrConfig::from_toml(&toml).unwrap();

        let light = theme_colors(&config.theme, Some(Appearance::Light));

        assert_eq!(
            (light.accent, light.text, light.surface0, light.surface_dim),
            (
                Color::Rgb(2, 2, 2),
                Color::Ansi(15),
                Color::Ansi(0),
                Color::Ansi(4)
            )
        );
    }

    #[test]
    fn pins_a_few_subtext0_colors() {
        let subtext0 = |name| theme_by_name(name).unwrap().subtext0.to_fzf();
        assert_eq!(subtext0("catppuccin"), "#a6adc8");
        assert_eq!(subtext0("gruvbox-light"), "#504945");
        assert_eq!(subtext0("terminal"), "7");
    }

    #[test]
    fn custom_subtext0_overrides_the_theme() {
        let config = HerdrConfig::from_toml("[theme.custom]\nsubtext0 = \"#010203\"").unwrap();

        assert_eq!(
            theme_colors(&config.theme, None).subtext0,
            Color::Rgb(1, 2, 3)
        );
    }

    #[test]
    fn custom_mauve_overrides_the_theme() {
        let config = HerdrConfig::from_toml("[theme.custom]\nmauve = \"#010203\"").unwrap();

        assert_eq!(theme_colors(&config.theme, None).mauve, Color::Rgb(1, 2, 3));
    }

    #[test]
    fn per_appearance_mauve_applies_after_custom() {
        let toml = format!(
            "{AUTO}[theme.custom]\nmauve = \"#010101\"\n\
             [theme.custom.dark]\nmauve = \"#020202\"\n[theme.custom.light]\nmauve = \"red\""
        );
        let config = HerdrConfig::from_toml(&toml).unwrap();

        let dark = theme_colors(&config.theme, Some(Appearance::Dark));
        let light = theme_colors(&config.theme, Some(Appearance::Light));

        assert_eq!(
            (dark.mauve, light.mauve),
            (Color::Rgb(2, 2, 2), Color::Ansi(1))
        );
    }

    /// `panel_bg` of `toml`'s theme for `appearance`.
    fn panel_bg(toml: &str, appearance: Option<Appearance>) -> String {
        let config = HerdrConfig::from_toml(toml).unwrap();
        theme_colors(&config.theme, appearance).panel_bg.to_fzf()
    }

    const AUTO: &str = "[theme]\nauto_switch = true\n";

    #[test]
    fn appearance_is_ignored_without_auto_switch() {
        let toml = "[theme]\nname = \"tokyo-night\"\nlight_name = \"nord\"";

        assert_eq!(panel_bg(toml, Some(Appearance::Light)), "#1a1b26");
    }

    #[test]
    fn auto_switch_picks_the_siblings_of_name() {
        let toml = format!("{AUTO}name = \"tokyo-night-day\"");

        assert_eq!(panel_bg(&toml, Some(Appearance::Dark)), "#1a1b26");
        assert_eq!(panel_bg(&toml, Some(Appearance::Light)), "#e1e2e7");
    }

    #[test]
    fn auto_switch_defaults_to_catppuccin_and_latte() {
        assert_eq!(panel_bg(AUTO, Some(Appearance::Dark)), "#181825");
        assert_eq!(panel_bg(AUTO, Some(Appearance::Light)), "#eff1f5");
    }

    #[test]
    fn auto_switch_with_an_unknown_appearance_is_dark() {
        let toml = format!("{AUTO}name = \"gruvbox-light\"");

        assert_eq!(panel_bg(&toml, None), "#282828");
    }

    #[test]
    fn dark_and_light_names_win_over_the_siblings() {
        let toml = format!("{AUTO}name = \"gruvbox\"\ndark_name = \"nord\"\nlight_name = \"Dawn\"");

        assert_eq!(panel_bg(&toml, Some(Appearance::Dark)), "#2e3440");
        assert_eq!(panel_bg(&toml, Some(Appearance::Light)), "#faf4ed");
    }

    #[test]
    fn a_theme_without_siblings_is_used_in_both_slots() {
        let toml = format!("{AUTO}name = \"dracula\"");

        assert_eq!(panel_bg(&toml, Some(Appearance::Light)), "#282a36");
    }

    #[test]
    fn unknown_slot_names_fall_back_per_slot_like_herdr() {
        let toml = format!("{AUTO}dark_name = \"tokio-night\"\nlight_name = \"lattee\"");

        assert_eq!(panel_bg(&toml, Some(Appearance::Dark)), "#181825");
        assert_eq!(panel_bg(&toml, Some(Appearance::Light)), "#eff1f5");
    }

    #[test]
    fn an_unknown_name_falls_back_in_both_slots() {
        let toml = format!("{AUTO}name = \"hotdog\"");

        assert_eq!(panel_bg(&toml, Some(Appearance::Dark)), "#181825");
        assert_eq!(panel_bg(&toml, Some(Appearance::Light)), "#eff1f5");
    }

    #[test]
    fn per_appearance_overrides_apply_after_custom() {
        let toml = format!(
            "{AUTO}[theme.custom]\npanel_bg = \"#010101\"\n\
             [theme.custom.dark]\npanel_bg = \"#020202\"\n\
             [theme.custom.light]\npanel_bg = \"reset\"\noverlay0 = \"\""
        );
        let config = HerdrConfig::from_toml(&toml).unwrap();

        let dark = theme_colors(&config.theme, Some(Appearance::Dark));
        let light = theme_colors(&config.theme, Some(Appearance::Light));

        assert_eq!(dark.panel_bg, Color::Rgb(2, 2, 2));
        assert_eq!(light.panel_bg, Color::Reset);
        // Empty counts as unset: latte's overlay0 stays.
        assert_eq!(light.overlay0, Color::Rgb(0x9c, 0xa0, 0xb0));
    }

    #[test]
    fn per_appearance_overrides_need_auto_switch() {
        let toml = "[theme.custom.dark]\npanel_bg = \"#020202\"";

        assert_eq!(panel_bg(toml, Some(Appearance::Dark)), "#181825");
    }

    #[test]
    fn siblings_follow_aliases_and_keep_unknown_names() {
        assert_eq!(
            siblings("rosepine-dawn"),
            ("rose-pine".into(), "rose-pine-dawn".into())
        );
        assert_eq!(
            siblings("lotus"),
            ("kanagawa".into(), "kanagawa-lotus".into())
        );
        assert_eq!(siblings("hotdog"), ("hotdog".into(), "hotdog".into()));
    }

    #[test]
    fn siblings_and_fallbacks_are_built_in_themes() {
        for (dark, light) in THEME_SIBLINGS {
            assert!(theme_by_name(dark).is_some(), "{dark}");
            assert!(theme_by_name(light).is_some(), "{light}");
        }
        assert!(theme_by_name(DEFAULT_THEME).is_some());
        assert!(theme_by_name(DEFAULT_LIGHT_THEME).is_some());
    }
}
