//! Which of the two layouts a popup width gets: full, or minimal for a
//! popup narrowed well below its `[popup] min_width`.
//!
//! The popup only gets narrower than at open (Herdr never grows it past
//! the size it was opened with), so the full layout is what a normal open
//! shows; the minimal one is for a terminal (or Herdr's area) shrunk while
//! the palette is open, or a palette opened on a small screen.

use crate::domain::popup;

/// Below this share of the settings' `[popup] min_width`, in percent of
/// the popup's outer width, the palette uses [`LayoutMode::Minimal`].
const MINIMAL_BELOW_PERCENT: usize = 75;

/// How much the rows and the footer show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayoutMode {
    /// Every column: Commands' shortcuts, Jump to's workspace column,
    /// `[^/]` on the footer's last line.
    #[default]
    Full,
    /// For a narrow popup: Commands rows show the title only, Jump to rows
    /// the name and the agent (no workspace column); what they drop stays
    /// searchable. Footers keep their lines, cut to fit, without `[^/]`.
    Minimal,
}

impl LayoutMode {
    /// The layout for an fzf `columns` wide (`$FZF_COLUMNS`, or the
    /// terminal's width at open) in a popup whose settings' `[popup]
    /// min_width` is `min_width` (after the floor): minimal when
    /// the popup's outer width ([`popup::outer_width`]) is under 75% of
    /// `min_width`.
    ///
    /// # Examples
    ///
    /// ```
    /// use herdr_command_palette::domain::layout::LayoutMode;
    ///
    /// // The default `min_width` (76): minimal from an outer width of 56.
    /// assert_eq!(LayoutMode::for_width(54, 76), LayoutMode::Full);
    /// assert_eq!(LayoutMode::for_width(53, 76), LayoutMode::Minimal);
    /// ```
    #[must_use]
    pub fn for_width(columns: usize, min_width: u16) -> Self {
        // Integers only: `outer < min_width * 75%`, without rounding.
        let outer = popup::outer_width(columns).saturating_mul(100);
        if outer < usize::from(min_width) * MINIMAL_BELOW_PERCENT {
            Self::Minimal
        } else {
            Self::Full
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::settings::Settings;

    use super::*;

    /// The narrowest fzf width that keeps the full layout for `min_width`.
    fn narrowest_full(min_width: u16) -> usize {
        (0..usize::from(min_width))
            .find(|&columns| LayoutMode::for_width(columns, min_width) == LayoutMode::Full)
            .unwrap()
    }

    #[test]
    fn is_full_from_75_percent_of_the_default_min_width() {
        // 76 * 75% = 57: an outer width of 57 is full, 56 is minimal.
        assert_eq!(LayoutMode::for_width(54, 76), LayoutMode::Full);
        assert_eq!(LayoutMode::for_width(53, 76), LayoutMode::Minimal);
        assert_eq!(narrowest_full(76), 54);
    }

    #[test]
    fn is_full_at_exactly_75_percent() {
        // 100 * 75% = 75: an outer width of 75 (fzf 72) is not under it.
        assert_eq!(LayoutMode::for_width(72, 100), LayoutMode::Full);
        assert_eq!(LayoutMode::for_width(71, 100), LayoutMode::Minimal);
    }

    #[test]
    fn uses_the_floored_min_width() {
        // A `min_width` below the default is raised to it at load.
        let settings = Settings::from_toml("[popup]\nmin_width = 40\n").unwrap();

        let min_width = settings.popup.min_width;

        assert_eq!(min_width, 76);
        assert_eq!(narrowest_full(min_width), 54);
    }

    #[test]
    fn follows_a_custom_min_width() {
        let settings = Settings::from_toml("[popup]\nmin_width = 120\nmax_width = 140\n").unwrap();

        // 120 * 75% = 90: outer 90 (fzf 87) is full.
        assert_eq!(narrowest_full(settings.popup.min_width), 87);
    }

    #[test]
    fn is_minimal_without_columns_and_full_on_huge_ones() {
        assert_eq!(LayoutMode::for_width(0, 76), LayoutMode::Minimal);
        assert_eq!(LayoutMode::for_width(usize::MAX, 76), LayoutMode::Full);
        assert_eq!(LayoutMode::default(), LayoutMode::Full);
    }
}
