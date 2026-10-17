//! Pure domain logic: palette rows, categories, text layout and the mapping
//! from rows to Herdr requests. No I/O lives here.
//!
//! Shared data types, used by several modules:
//!
//! - [`row`]: [`Row`](row::Row), [`Kind`](row::Kind),
//!   [`Category`](row::Category), [`RowId`](row::RowId).
//! - [`herdr`]: what Herdr sends and receives: session snapshot, plugin
//!   actions, invocation context, hook events, [`Request`](herdr::Request).
//! - [`commands`]: the actions behind built-in and palette-command rows
//!   ([`Builtin`](commands::Builtin), [`PaletteCommand`](commands::PaletteCommand)).
//! - [`config`]: the parts of Herdr's `config.toml` the palette reads.
//! - [`env`](mod@env): the plugin's environment (ids, paths, context), resolved once.
//! - [`settings`]: the plugin's own `settings.toml`, typed, with defaults.
//!
//! Logic, one concern per module:
//!
//! - [`appearance`]: the terminal's light/dark appearance, as Herdr reads it.
//! - [`catalog`]: row sources (plugin actions, built-ins, palette and custom
//!   commands, panes).
//! - [`color`]: terminal colors, rendered for fzf and ANSI escapes.
//! - [`dialog`]: what the confirmation dialog says for a row.
//! - [`fzf_action`]: fzf actions with an argument, delimited so fzf reads it whole.
//! - [`git`]: what is checked out in a directory, from git's output.
//! - [`history`]: history keys (session, row use), timestamps, when to prune.
//! - [`jump_to`]: names panes the way Herdr's "Go to" does.
//! - [`layout`]: text width, truncation and the fzf line of a row.
//! - [`palette_state`]: naming and staleness of the palette state dirs.
//! - [`requests`]: which Herdr request a selected row sends.
//! - [`popup`]: popup size for the screen.
//! - [`recent`]: the "recent" orders of Commands and Jump to.
//! - [`retry`]: how often to retry a call while Herdr is busy.
//! - [`shell`]: POSIX shell quoting for command lines we build.
//! - [`style`]: the palette's colors, settings over Herdr's theme.
//! - [`tags`]: the `{hcp:<name>}` tags of the footer field, and stripping them.
//! - [`theme`]: colors borrowed from Herdr's theme.

pub mod appearance;
pub mod catalog;
pub mod color;
pub mod commands;
pub mod config;
pub mod dialog;
pub mod env;
pub mod fzf_action;
pub mod git;
pub mod herdr;
pub mod history;
pub mod jump_to;
pub mod layout;
pub mod palette_state;
pub mod popup;
pub mod recent;
pub mod requests;
pub mod retry;
pub mod row;
pub mod settings;
pub mod shell;
pub mod style;
pub mod tags;
pub mod theme;

/// Test data shared by the domain tests.
#[cfg(test)]
pub(crate) mod fixtures;

/// `text` unless it is missing or empty; empty values count as unset.
pub(crate) fn non_empty(text: Option<&str>) -> Option<&str> {
    text.filter(|text| !text.is_empty())
}
