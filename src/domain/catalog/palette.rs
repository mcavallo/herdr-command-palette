//! The palette's own commands, for things no Herdr key action does: move
//! the focused pane to a new tab or workspace, edit Herdr's config
//! or the palette's settings. How each one runs is the
//! [`requests`](crate::domain::requests) module's business.

use crate::domain::commands::PaletteCommand;
use crate::domain::row::{Kind, Row};

use super::NamedCommand;

/// The palette's own commands, in palette order: the single table of
/// names, texts and what each runs.
pub(crate) const PALETTE_COMMANDS: [NamedCommand<PaletteCommand>; 4] = [
    NamedCommand {
        name: "move_pane_new_tab",
        title: "Move pane to new tab",
        description: "Move the focused pane into a new tab in this workspace",
        action: PaletteCommand::MovePaneNewTab,
    },
    NamedCommand {
        name: "move_pane_new_workspace",
        title: "Move pane to new workspace",
        description: "Move the focused pane into a new workspace",
        action: PaletteCommand::MovePaneNewWorkspace,
    },
    NamedCommand {
        name: "edit_herdr_config",
        title: "Edit Herdr config",
        description: "Open config.toml in your editor, then check and reload it",
        action: PaletteCommand::EditHerdrConfig,
    },
    NamedCommand {
        name: "edit_settings",
        title: "Edit Command Palette settings",
        description: "Open settings.toml in your editor, then check it",
        action: PaletteCommand::EditSettings,
    },
];

/// One row per palette command; they have no key binding.
#[must_use]
pub fn rows() -> Vec<Row> {
    PALETTE_COMMANDS
        .iter()
        .map(|command| command.to_row(Kind::Palette))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_the_palette_commands_without_shortcuts() {
        let rows = rows();

        let names: Vec<_> = rows.iter().map(|row| row.payload.as_str()).collect();
        assert_eq!(
            names,
            [
                "move_pane_new_tab",
                "move_pane_new_workspace",
                "edit_herdr_config",
                "edit_settings"
            ]
        );
        assert_eq!(
            rows[2],
            Row::new(Kind::Palette, "edit_herdr_config", "Edit Herdr config")
                .with_footer_text("Open config.toml in your editor, then check and reload it")
        );
    }
}
