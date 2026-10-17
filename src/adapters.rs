//! Implementations of the [`ports`](crate::ports) traits: Herdr Unix
//! socket, Herdr's config file, the plugin's settings file, fzf picker,
//! processes, the terminal, temp files, the clock, the history
//! database (SQLite) and git.

pub mod clock;
pub mod files;
pub mod fzf;
pub mod git;
pub mod herdr_config;
pub mod herdr_socket;
pub mod history_db;
pub mod process;
pub mod settings_file;
pub mod terminal;

/// Test helpers: executable shell scripts standing in for fzf, stty, ...
#[cfg(test)]
#[path = "../tests/support/scripts.rs"]
pub(crate) mod scripts;
