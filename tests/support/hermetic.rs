//! The binary under test, in a hermetic environment: every variable
//! cleared (so no `HERDR_*` from a dev shell can reach the user's real
//! Herdr), then only what the test needs.
//!
//! Include with `#[path = "support/hermetic.rs"] mod hermetic;`.

// Reason: each test crate that includes this file uses only part of it.
#![allow(dead_code)]

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use tempfile::TempDir;

/// Herdr configuration fixture shared with the catalog tests.
pub const FIXTURE_CONFIG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/config.toml");

/// A config with Herdr's `[keys.indexed]` table: the catalog
/// must accept `[keys]` entries that aren't bindings.
pub const INDEXED_CONFIG: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/config-indexed.toml"
);

/// The system dirs of `PATH`: enough for `sh` and coreutils, never a
/// user's `herdr` or `fzf`.
pub const SYSTEM_PATH: &str = "/usr/local/bin:/usr/bin:/bin";

/// The binary with a cleared environment: `PATH` (system dirs), `HOME`,
/// `TMPDIR` and the plugin config dir (also `home`, so the palette's temp
/// dirs, its sweep of stale ones and `settings.toml` never touch the real
/// ones), the fake Herdr socket and the fixture config.
pub fn bin(socket: &Path, home: &Path) -> Command {
    let mut command = bare_bin();
    command
        .env("PATH", SYSTEM_PATH)
        .env("HOME", home)
        .env("TMPDIR", home)
        .env("HERDR_PLUGIN_CONFIG_DIR", home)
        .env("HERDR_SOCKET_PATH", socket)
        .env("HERDR_CONFIG_PATH", FIXTURE_CONFIG);
    command
}

/// [`bin`] with a socket nothing listens on: for commands that don't
/// need Herdr, or that must cope without it.
pub fn bin_without_herdr(home: &Path) -> Command {
    bin(&home.join("none.sock"), home)
}

/// `command` (a program and its environment, no arguments yet) as
/// `setsid -w <program>`: run without a controlling terminal, a read of
/// `/dev/tty` fails at once instead of waiting on the developer's
/// terminal. util-linux, hence Linux only.
pub fn without_terminal(command: &Command) -> Command {
    let mut wrapped = Command::new("setsid");
    wrapped.env_clear().arg("-w").arg(command.get_program());
    for (name, value) in command.get_envs() {
        if let Some(value) = value {
            wrapped.env(name, value);
        }
    }
    wrapped
}

/// `PATH` with `dir` (a stub's) before the [`SYSTEM_PATH`] dirs.
pub fn path_with(dir: &Path) -> String {
    format!("{}:{SYSTEM_PATH}", dir.display())
}

/// A home (also the plugin config dir) whose `settings.toml` is `text`.
pub fn home_with_settings(text: &str) -> TempDir {
    let home = tempfile::tempdir().unwrap();
    fs::write(home.path().join("settings.toml"), text).unwrap();
    home
}

/// The binary with no inherited environment at all (no `HERDR_*` from a
/// dev shell), except `cargo llvm-cov`'s profile path: without it,
/// instrumented runs would drop `default_*.profraw` files in the working
/// directory.
pub fn bare_bin() -> Command {
    let mut command = Command::cargo_bin("herdr-command-palette").expect("binary built");
    command.env_clear();
    if let Some(path) = std::env::var_os("LLVM_PROFILE_FILE") {
        command.env("LLVM_PROFILE_FILE", path);
    }
    command
}
