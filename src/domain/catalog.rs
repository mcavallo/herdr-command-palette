//! Row sources: everything the palette lists, in a fixed order
//! (plugin actions, built-ins, palette commands, custom commands, then
//! panes for Jump to).
//!
//! Each source is a pure function from Herdr data (plugin actions,
//! [`HerdrConfig`], [`Snapshot`]) to [`Row`]s. Fetching that data, and
//! deciding what to do when Herdr is unreachable, is the caller's job: a
//! source whose data is missing just contributes no rows.

pub mod builtins;
pub mod custom;
pub mod palette;
pub mod panes;
pub mod plugins;

use std::path::Path;

use crate::domain::config::HerdrConfig;
use crate::domain::herdr::{PluginAction, Snapshot};
use crate::domain::row::{Kind, Row};

/// A command known by name, with hand-written text and what it runs: a
/// built-in or one of the palette's own commands.
///
/// One table of these per kind ([`builtins::BUILTINS`],
/// `palette::PALETTE_COMMANDS`) is the single source of truth: the
/// catalog lists rows from it and
/// [`requests`](crate::domain::requests) parses names through it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NamedCommand<A> {
    /// Name used as the row payload and by `run` (`split_vertical`).
    pub name: &'static str,
    /// Row title (`Split pane right`).
    pub title: &'static str,
    /// Footer text.
    pub description: &'static str,
    /// What running it does ([`Builtin`](crate::domain::commands::Builtin)
    /// or [`PaletteCommand`](crate::domain::commands::PaletteCommand)).
    pub action: A,
}

impl<A: Copy> NamedCommand<A> {
    /// The entry of `table` called `name`, if any: the one lookup behind
    /// the `FromStr` of every action type.
    #[must_use]
    pub fn find(table: &'static [Self], name: &str) -> Option<&'static Self> {
        table.iter().find(|command| command.name == name)
    }

    /// A row of `kind` for this command, without a shortcut.
    fn to_row(self, kind: Kind) -> Row {
        Row::new(kind, self.name, self.title).with_footer_text(self.description)
    }
}

/// Everything the palette's rows are built from.
#[derive(Debug, Clone, Copy)]
pub struct Sources<'a> {
    /// This plugin's id; its own `open` action is not listed.
    pub plugin_id: &'a str,
    /// Enabled plugin actions (`plugin.action.list`); empty if unavailable.
    pub plugin_actions: &'a [PluginAction],
    /// Herdr's `config.toml`.
    pub config: &'a HerdrConfig,
    /// The session, for Jump to rows; `None` if Herdr is unreachable.
    pub snapshot: Option<&'a Snapshot>,
    /// The user's home directory, shown as `~` in pane working directories.
    pub home: Option<&'a Path>,
}

/// Every palette row, in order: plugin actions, built-ins,
/// palette commands, custom commands, then panes (Jump to).
#[must_use]
pub fn rows(sources: &Sources<'_>) -> Vec<Row> {
    let mut rows = plugins::rows(sources.plugin_actions, sources.config, sources.plugin_id);
    rows.extend(builtins::rows(sources.config));
    rows.extend(palette::rows());
    rows.extend(custom::rows(sources.config));
    if let Some(snapshot) = sources.snapshot {
        rows.extend(panes::rows(snapshot, sources.home));
    }
    rows
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::domain::fixtures::plugin_actions;
    use crate::domain::row::FooterLine;

    use super::*;

    /// Shared test configuration from `tests/fixtures/config.toml`.
    const CONFIG: &str = include_str!("../../tests/fixtures/config.toml");

    /// Rows as TSV without the category column:
    /// kind, payload, title, shortcut, description[, extra search text].
    fn tsv(rows: &[Row]) -> String {
        let mut out = String::new();
        for row in rows {
            let lines: Vec<String> = row.footer.iter().map(FooterLine::to_text).collect();
            let title = row.full_title();
            let mut fields = vec![
                row.kind.as_str(),
                &row.payload,
                &title,
                &row.shortcut,
                lines.first().map_or("", String::as_str),
            ];
            // Jump to's cwd, the footer's second line; commands have none.
            fields.extend(
                lines
                    .iter()
                    .skip(1)
                    .map(String::as_str)
                    .filter(|line| !line.is_empty()),
            );
            if !row.search_extra.is_empty() {
                fields.push(&row.search_extra);
            }
            out.push_str(&fields.join("\t"));
            out.push('\n');
        }
        out
    }

    /// A small session: two workspaces (one with a long name), a two-pane
    /// tab, one agent.
    fn snapshot() -> Snapshot {
        serde_json::from_value(json!({
            "workspaces": [
                {"workspace_id": "w1", "label": "herdr-command-palette"},
                {"workspace_id": "w2", "label": "api"},
            ],
            "tabs": [
                {"tab_id": "w1:t1", "workspace_id": "w1", "label": "1"},
                {"tab_id": "w2:t1", "workspace_id": "w2", "label": "1"},
            ],
            "panes": [
                {"pane_id": "w1:p1", "workspace_id": "w1", "tab_id": "w1:t1",
                 "cwd": "/home/me/dotfiles"},
                {"pane_id": "w1:p2", "workspace_id": "w1", "tab_id": "w1:t1", "cwd": "/tmp"},
                {"pane_id": "w2:p1", "workspace_id": "w2", "tab_id": "w2:t1",
                 "foreground_cwd": "/home/me"},
            ],
            "agents": [{"pane_id": "w1:p2", "agent": "claude"}],
        }))
        .unwrap()
    }

    // An unavailable Herdr session leaves command rows intact and omits panes.
    #[test]
    fn lists_commands_without_a_session() {
        let config = HerdrConfig::from_toml(CONFIG).unwrap();
        let actions = plugin_actions();
        let sources = Sources {
            plugin_id: "mmc.command-palette",
            plugin_actions: &actions,
            config: &config,
            snapshot: None,
            home: None,
        };

        let rows = rows(&sources);

        assert!(rows.iter().all(|row| row.kind != Kind::Pane));
        insta::assert_snapshot!(tsv(&rows));
    }

    #[test]
    fn appends_jump_to_rows_after_the_commands() {
        let config = HerdrConfig::default();
        let snapshot = snapshot();
        let sources = Sources {
            plugin_id: "mmc.command-palette",
            plugin_actions: &[],
            config: &config,
            snapshot: Some(&snapshot),
            home: Some(Path::new("/home/me")),
        };

        let rows = rows(&sources);

        let first_pane = rows.iter().position(|row| row.kind == Kind::Pane).unwrap();
        assert_eq!(first_pane, rows.len() - 3);
        insta::assert_snapshot!(tsv(&rows[first_pane..]));
    }
}
