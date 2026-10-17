//! The header line: every category label, the active one bold, the others
//! muted: dim, or the settings' `muted` color.

use crate::domain::row::Category;
use crate::domain::style::TextColors;

use super::{BOLD, GUTTER, RESET, muted};

/// Blank columns between two category labels.
const SEPARATOR: &str = "  ";

/// The header for the palette while `active` is shown, the other
/// categories muted with `colors`.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::layout;
/// use herdr_command_palette::domain::row::Category;
/// use herdr_command_palette::domain::style::TextColors;
///
/// assert_eq!(
///     layout::header(Category::JumpTo, TextColors::default()),
///     " \x1b[2mCommands\x1b[0m  \x1b[1mJump to\x1b[0m",
/// );
/// ```
#[must_use]
pub fn header(active: Category, colors: TextColors) -> String {
    let inactive = muted(colors);
    let labels: Vec<String> = Category::ALL
        .into_iter()
        .map(|category| {
            let style = if category == active { BOLD } else { &inactive };
            format!("{style}{}{RESET}", category.label())
        })
        .collect();
    format!("{GUTTER}{}", labels.join(SEPARATOR))
}

#[cfg(test)]
mod tests {
    use crate::domain::color::Color;
    use crate::domain::layout::DIM;

    use super::*;

    #[test]
    fn bolds_the_active_category_and_dims_the_rest() {
        assert_eq!(
            header(Category::Commands, TextColors::default()),
            format!(" {BOLD}Commands{RESET}  {DIM}Jump to{RESET}"),
        );
    }

    #[test]
    fn mutes_the_inactive_category_with_the_settings_color() {
        let colors = TextColors {
            muted: Some(Color::Reset),
            tags: None,
            branch: None,
        };

        assert_eq!(
            header(Category::JumpTo, colors),
            format!(" \x1b[39mCommands{RESET}  {BOLD}Jump to{RESET}"),
        );
    }

    #[test]
    fn snapshot_headers() {
        let out: Vec<String> = Category::ALL
            .into_iter()
            .map(|category| {
                header(category, TextColors::default())
                    .replace(BOLD, "**")
                    .replace(DIM, "~")
                    .replace(RESET, "|")
            })
            .collect();
        insta::assert_snapshot!(out.join("\n"));
    }
}
