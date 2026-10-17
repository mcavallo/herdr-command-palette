//! Command-line interface: the subcommands Herdr, fzf and developers invoke.
//!
//! Herdr starts this binary from the plugin manifest (`herdr-plugin.toml`);
//! each subcommand is one role. See `docs/architecture.md`, "Process model".

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use crate::domain::row::{Category, Kind};

/// Keyboard-driven command palette for Herdr.
#[derive(Debug, Parser)]
#[command(name = "herdr-command-palette", version, about, long_about = None)]
pub struct Cli {
    /// The role to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Every role the binary can run.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum Command {
    /// Open the palette popup (plugin action `open`).
    Open,
    /// Run the palette inside the popup (plugin pane `palette`).
    Palette,
    /// Callbacks fzf runs while the palette is open (internal).
    #[command(subcommand, hide = true)]
    Fzf(FzfCommand),
    /// Run one palette row.
    Run {
        /// Row kind: `plugin`, `builtin`, `palette`, `custom` or `pane`.
        kind: Kind,
        /// What to run, interpreted per kind (action id, built-in name, pane id, ...).
        payload: String,
        /// Enter was pressed in the confirmation dialog (internal).
        #[arg(long, hide = true)]
        confirmed: bool,
    },
    /// Open the `confirm` popup for a row picked in the palette (internal).
    #[command(hide = true)]
    Ask {
        /// Row kind, as for `run`.
        kind: Kind,
        /// What to run, as for `run`.
        payload: String,
        /// The row's title, for the dialog.
        title: String,
    },
    /// Ask before running a `confirm_ids` row (plugin pane `confirm`, internal).
    #[command(hide = true)]
    Confirm,
    /// Run a custom popup/pane command, keeping failures on screen (plugin pane `exec`).
    Exec,
    /// Rerun the most recent command (plugin action `repeat-command`).
    RepeatCommand,
    /// Jump again to the most recent jump target (plugin action `repeat-jump`).
    RepeatJump,
    /// Record history from Herdr event hooks.
    #[command(subcommand)]
    History(HistoryCommand),
    /// Work with the plugin's settings file (`settings.toml`).
    #[command(subcommand)]
    Settings(SettingsCommand),
    /// Print every palette row (debugging).
    List {
        /// Print each row's id, as used by `hide_ids` and `confirm_ids` in settings.
        #[arg(long)]
        ids: bool,
    },
}

/// fzf callbacks: fzf runs these through its `--bind` actions.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum FzfCommand {
    /// Print the rows of one category.
    Rows {
        /// Category to print: `commands` or `jump_to`.
        category: Category,
    },
    /// Move to another category and print the fzf actions that show it.
    Switch {
        /// How many categories to move by, e.g. `1` or `-1`.
        #[arg(allow_hyphen_values = true)]
        offset: i32,
    },
    /// Print the fzf actions that lay the rows out again after a resize.
    Resize,
    /// Print the footer text.
    Footer {
        /// What triggered the update.
        #[arg(value_enum)]
        event: FooterEvent,
        /// Kind of the focused row (fzf's `{1}`), for its footer tag; empty
        /// or unknown means none.
        #[arg(long)]
        kind: Option<String>,
        /// Footer field of the focused row (fzf's `{4}`): its lines, tagged
        /// with `{hcp:line}` and `{hcp:seg}`, if any.
        lines: Option<String>,
    },
}

/// Why fzf asks for the footer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FooterEvent {
    /// The focused row changed (or the list was reloaded).
    Focus,
    /// The user pressed the footer toggle key (`ctrl-/`).
    Toggle,
}

/// What to do with the settings file.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum SettingsCommand {
    /// Check `settings.toml`: exit 0 when valid (or missing), else print
    /// what is wrong and exit 1.
    Check {
        /// Check this file instead of the one in Herdr's plugin config dir.
        #[arg(long, value_name = "PATH")]
        file: Option<PathBuf>,
    },
}

/// History recording, run by Herdr `[[events]]` hooks.
#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum HistoryCommand {
    /// Record a pane focus (hook `pane.focused`).
    Focus,
    /// Carry a moved pane's history over to its new id (hook `pane.moved`).
    Move,
    /// Forget an earlier pane's history under a new pane's id (hook `pane.created`).
    Created,
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    fn parse(args: &[&str]) -> Command {
        let command_line = std::iter::once("herdr-command-palette").chain(args.iter().copied());
        Cli::try_parse_from(command_line)
            .expect("valid arguments")
            .command
    }

    #[test]
    fn cli_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn every_subcommand_parses() {
        let cases: [&[&str]; 18] = [
            &["open"],
            &["palette"],
            &["fzf", "rows", "commands"],
            &["fzf", "switch", "-1"],
            &["fzf", "resize"],
            &["fzf", "footer", "focus", "Split pane right"],
            &["run", "builtin", "zoom"],
            &["ask", "builtin", "zoom", "Toggle zoom"],
            &["confirm"],
            &["exec"],
            &["repeat-command"],
            &["repeat-jump"],
            &["history", "focus"],
            &["history", "move"],
            &["history", "created"],
            &["list", "--ids"],
            &["settings", "check"],
            &["settings", "check", "--file", "/cfg/settings.toml"],
        ];
        for args in cases {
            parse(args);
        }
    }

    #[test]
    fn arguments_are_captured() {
        assert_eq!(
            parse(&["fzf", "switch", "-1"]),
            Command::Fzf(FzfCommand::Switch { offset: -1 })
        );
        assert_eq!(
            parse(&["fzf", "footer", "toggle"]),
            Command::Fzf(FzfCommand::Footer {
                event: FooterEvent::Toggle,
                kind: None,
                lines: None
            })
        );
        assert_eq!(
            parse(&["run", "pane", "w1:p2"]),
            Command::Run {
                kind: Kind::Pane,
                payload: "w1:p2".into(),
                confirmed: false
            }
        );
        assert_eq!(
            parse(&["run", "--confirmed", "--", "builtin", "close_tab"]),
            Command::Run {
                kind: Kind::Builtin,
                payload: "close_tab".into(),
                confirmed: true
            }
        );
        assert_eq!(parse(&["list"]), Command::List { ids: false });
        assert_eq!(
            parse(&["settings", "check", "--file", "/e/settings.toml"]),
            Command::Settings(SettingsCommand::Check {
                file: Some("/e/settings.toml".into())
            })
        );
        assert_eq!(
            parse(&["fzf", "rows", "jump_to"]),
            Command::Fzf(FzfCommand::Rows {
                category: Category::JumpTo
            })
        );
    }

    #[test]
    fn takes_leading_dash_user_text_after_double_dash() {
        assert_eq!(
            parse(&[
                "fzf",
                "footer",
                "focus",
                "--kind",
                "",
                "--",
                "-scratch{hcp:seg}w1:p1{hcp:line}-dir"
            ]),
            Command::Fzf(FzfCommand::Footer {
                event: FooterEvent::Focus,
                kind: Some(String::new()),
                lines: Some("-scratch{hcp:seg}w1:p1{hcp:line}-dir".into())
            })
        );
        assert_eq!(
            parse(&["ask", "--", "custom", "1", "-x"]),
            Command::Ask {
                kind: Kind::Custom,
                payload: "1".into(),
                title: "-x".into()
            }
        );
        assert_eq!(
            parse(&["run", "--", "plugin", "-odd.id"]),
            Command::Run {
                kind: Kind::Plugin,
                payload: "-odd.id".into(),
                confirmed: false
            }
        );
    }

    #[test]
    fn rejects_unknown_kinds_and_categories() {
        let kind = Cli::try_parse_from(["herdr-command-palette", "run", "nope", "x"]);
        let category = Cli::try_parse_from(["herdr-command-palette", "fzf", "rows", "all"]);

        assert!(kind.is_err());
        assert!(category.is_err());
    }
}
