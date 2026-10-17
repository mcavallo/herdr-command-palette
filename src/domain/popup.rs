//! Popup size for the palette: like Herdr's keybinds modal, use the max
//! size, shrink to keep a margin on small screens, but never below the min
//! unless the screen itself is smaller.
//!
//! Herdr's keybinds modal is `popup(area, 76, 22)` with a 2-column /
//! 1-row margin; the bounds default to it and come from the settings'
//! `[popup]`. The size is computed at open time (the manifest only takes
//! fixed cells or a percentage) from the focused tab's layout area, which
//! excludes the sidebar: Herdr draws popups inside it.

use crate::domain::herdr::Area;

/// Smallest and largest size along one axis, in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    /// Never smaller than this, unless the screen is.
    pub min: u16,
    /// Never larger than this.
    pub max: u16,
}

/// Columns kept free on each side.
const MARGIN_COLUMNS: u16 = 2;

/// Rows kept free above and below.
const MARGIN_ROWS: u16 = 1;

/// Columns a Herdr popup takes beyond the terminal inside it: its border
/// (one column each side) and the reserved right column. Herdr 0.9.3
/// geometry (`src/popup_size.rs`, `resolve_popup_geometry`: the inner
/// terminal is `outer - 2` wide, minus one more column); re-check on
/// Herdr upgrades (`docs/development.md`).
const HERDR_FRAME_COLUMNS: usize = 3;

/// The outer width of the Herdr popup whose terminal is `columns` wide
/// (fzf's `$FZF_COLUMNS`: the palette uses no margin or padding).
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::popup;
///
/// // The default 76-column popup leaves fzf 73.
/// assert_eq!(popup::outer_width(73), 76);
/// ```
#[must_use]
pub fn outer_width(columns: usize) -> usize {
    columns.saturating_add(HERDR_FRAME_COLUMNS)
}

/// A popup size in cells, as `plugin.pane.open` takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopupSize {
    /// Columns.
    pub width: u16,
    /// Rows.
    pub height: u16,
}

/// Popup size for a tab whose layout area is `area`, within `width` and
/// `height` bounds.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::herdr::Area;
/// use herdr_command_palette::domain::popup::{popup_size, Bounds, PopupSize};
///
/// let width = Bounds { min: 76, max: 76 };
/// let height = Bounds { min: 16, max: 22 };
/// let big = Area { x: 0, y: 0, width: 200, height: 50 };
/// assert_eq!(popup_size(big, width, height), PopupSize { width: 76, height: 22 });
///
/// // 20 rows: shrink to keep a 1-row margin.
/// let short = Area { height: 20, ..big };
/// assert_eq!(popup_size(short, width, height).height, 18);
/// ```
#[must_use]
pub fn popup_size(area: Area, width: Bounds, height: Bounds) -> PopupSize {
    PopupSize {
        width: fit(area.width, width, MARGIN_COLUMNS),
        height: fit(area.height, height, MARGIN_ROWS),
    }
}

/// Size along one axis of `total` cells, keeping `margin` cells on each side
/// while staying within `bounds`; never more than `total` or less than 1.
fn fit(total: u16, bounds: Bounds, margin: u16) -> u16 {
    let size = total
        .saturating_sub(2 * margin)
        .min(bounds.max)
        .max(bounds.min);
    size.min(total).max(1)
}

#[cfg(test)]
mod tests {
    use crate::domain::settings::PopupSettings;

    use super::*;

    /// The size for a `width` x `height` area with the default bounds
    /// (Herdr's keybinds modal).
    fn size(width: u16, height: u16) -> PopupSize {
        let bounds = PopupSettings::default();
        popup_size(
            Area {
                x: 0,
                y: 0,
                width,
                height,
            },
            bounds.width(),
            bounds.height(),
        )
    }

    #[test]
    fn uses_the_max_on_a_big_screen() {
        assert_eq!(
            size(300, 80),
            PopupSize {
                width: 76,
                height: 22
            }
        );
    }

    #[test]
    fn shrinks_height_to_keep_the_margin() {
        assert_eq!(size(300, 23).height, 21);
    }

    #[test]
    fn never_goes_below_the_min_while_the_screen_fits_it() {
        assert_eq!(
            size(78, 17),
            PopupSize {
                width: 76,
                height: 16
            }
        );
    }

    #[test]
    fn fills_a_screen_smaller_than_the_min() {
        assert_eq!(
            size(60, 10),
            PopupSize {
                width: 60,
                height: 10
            }
        );
    }

    #[test]
    fn grows_up_to_larger_bounds() {
        let area = Area {
            x: 0,
            y: 0,
            width: 300,
            height: 30,
        };

        let size = popup_size(
            area,
            Bounds { min: 40, max: 120 },
            Bounds { min: 5, max: 50 },
        );

        assert_eq!(
            size,
            PopupSize {
                width: 120,
                height: 28
            }
        );
    }

    #[test]
    fn adds_herdrs_frame_to_the_terminal_width() {
        assert_eq!(outer_width(0), 3);
        assert_eq!(outer_width(53), 56);
        assert_eq!(outer_width(usize::MAX), usize::MAX);
    }

    #[test]
    fn is_at_least_one_cell() {
        assert_eq!(
            size(0, 0),
            PopupSize {
                width: 1,
                height: 1
            }
        );
    }
}
