//! Executable shell scripts standing in for fzf, stty, ..., and waiting
//! for what a detached process does.
//!
//! Include from an integration test with
//! `#[path = "support/scripts.rs"] mod scripts;`
//! (`src/adapters.rs` uses `#[path = "../tests/support/scripts.rs"]`).

// Reason: each test crate that includes this file uses only part of it.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use tempfile::TempDir;

/// The first line of every `fzf` stub: `--version` prints 0.66.0 (enough
/// for the palette's preflight) and exits.
const FZF_VERSION: &str = "[ \"$1\" = --version ] && { echo '0.66.0 (stub)'; exit 0; }";

/// Writes `#!/bin/sh` + `body` to `dir/name`, executable.
///
/// A child that another test thread forks while a file is open for
/// writing here keeps that write handle until it `exec`s, and running
/// the file meanwhile fails with "Text file busy" (ETXTBSY). So this
/// process writes a draft, which never runs, and `install` copies it to
/// `name` from its own process, where no test thread can fork.
pub fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    let draft = dir.join(format!(".{name}.draft"));
    fs::write(&draft, format!("#!/bin/sh\n{body}\n")).unwrap();
    let installed = Command::new("install")
        .arg("-m")
        .arg("755")
        .arg(&draft)
        .arg(&path)
        .status()
        .unwrap();
    assert!(installed.success(), "install: {installed}");
    fs::remove_file(&draft).unwrap();
    path
}

/// Writes an `fzf` stub to `dir`: [`FZF_VERSION`], then `body`.
pub fn fzf_script(dir: &Path, body: &str) -> PathBuf {
    script(dir, "fzf", &format!("{FZF_VERSION}\n{body}"))
}

/// A dir holding an `fzf` stub that saves its stdin, args and
/// `HCP_STATE` listing next to itself (`input`, `args`, `state`), then
/// prints the first input line matching `pick`.
pub fn fzf_stub(pick: &str) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().display();
    let body = format!(
        "cat > '{out}/input'\n\
         printf '%s\\n' \"$@\" > '{out}/args'\n\
         ls \"$HCP_STATE\" > '{out}/state'\n\
         grep -m1 '{pick}' '{out}/input'"
    );
    fzf_script(dir.path(), &body);
    dir
}

/// Polls `done` every 10 ms until it holds or `timeout` passes; returns
/// whether it held.
pub fn wait_until(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if done() {
            return true;
        }
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
