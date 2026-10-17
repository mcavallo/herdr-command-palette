//! End-to-end checks of the event hooks `history focus`, `history move`
//! and `history created`:
//! the real binary writing a real SQLite file in a temp state dir.

#[path = "support/hermetic.rs"]
mod hermetic;

use std::path::Path;
use std::process::{Command, Stdio};

use hermetic::{SYSTEM_PATH, bare_bin};
use rusqlite::Connection;

const SOCKET: &str = "/c/herdr/herdr.sock";

/// The binary as Herdr starts a hook: state dir, socket and pane id set.
/// `HOME` and the plugin config dir are the state dir too, so the
/// settings the hook reads are never the real ones (or a path relative to
/// the working directory).
fn hook(state: &Path, pane: &str) -> assert_cmd::Command {
    let mut command = bare_bin();
    command
        .env("PATH", SYSTEM_PATH)
        .env("HOME", state)
        .env("HERDR_PLUGIN_CONFIG_DIR", state)
        .env("HERDR_PLUGIN_STATE_DIR", state)
        .env("HERDR_SOCKET_PATH", SOCKET)
        .env("HERDR_PANE_ID", pane);
    command
}

fn moved_event(from: &str, to: &str) -> String {
    format!(
        r#"{{"event": "pane_moved", "data": {{"type": "pane_moved",
            "previous_pane_id": "{from}", "pane": {{"pane_id": "{to}"}}}}}}"#
    )
}

fn created_event(pane: &str) -> String {
    format!(
        r#"{{"event": "pane_created", "data": {{"type": "pane_created",
            "pane": {{"pane_id": "{pane}"}}}}}}"#
    )
}

/// Moves every row of the store a minute back in time, as if written by
/// a pane that closed before a restart.
fn age_rows(state: &Path) {
    let db = Connection::open(state.join("history.db")).unwrap();
    db.execute_batch(
        "UPDATE focus SET last_at = last_at - 60000; UPDATE usage SET last_at = last_at - 60000;",
    )
    .unwrap();
}

/// (session, pane) of every focus row, by pane.
fn focus_rows(state: &Path) -> Vec<(String, String)> {
    let db = Connection::open(state.join("history.db")).unwrap();
    let mut statement = db
        .prepare("SELECT session, pane_key FROM focus ORDER BY pane_key")
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn focus_records_the_pane_silently() {
    let state = tempfile::tempdir().unwrap();

    hook(state.path(), "w1:p2")
        .args(["history", "focus"])
        .assert()
        .success()
        .stdout("")
        .stderr("");

    assert_eq!(
        focus_rows(state.path()),
        [(SOCKET.to_owned(), "w1:p2".to_owned())]
    );
}

#[test]
fn focus_without_a_state_dir_exits_0_with_one_line() {
    let home = tempfile::tempdir().unwrap();
    let mut command = bare_bin();
    command
        .env("HOME", home.path())
        .env("HERDR_PLUGIN_CONFIG_DIR", home.path())
        .env("HERDR_PANE_ID", "w1:p2");

    command
        .args(["history", "focus"])
        .assert()
        .success()
        .stdout("")
        .stderr(
            "herdr-command-palette: history focus: no history store: \
         HERDR_PLUGIN_STATE_DIR is not set\n",
        );
}

#[test]
fn move_to_another_workspace_carries_the_focus_over() {
    let state = tempfile::tempdir().unwrap();
    hook(state.path(), "w1:p2")
        .args(["history", "focus"])
        .assert()
        .success();

    hook(state.path(), "w2:p1")
        .env("HERDR_PLUGIN_EVENT_JSON", moved_event("w1:p2", "w2:p1"))
        .args(["history", "move"])
        .assert()
        .success()
        .stdout("")
        .stderr("");

    assert_eq!(
        focus_rows(state.path()),
        [(SOCKET.to_owned(), "w2:p1".to_owned())]
    );
}

#[test]
fn concurrent_hook_processes_lose_no_focus() {
    let state = tempfile::tempdir().unwrap();
    let writers = 32;

    // Start every process before waiting for any, like a burst of hooks.
    let children: Vec<_> = (0..writers)
        .map(|n| {
            Command::new(assert_cmd::cargo::cargo_bin!("herdr-command-palette"))
                .env_clear()
                .envs(std::env::var_os("LLVM_PROFILE_FILE").map(|path| ("LLVM_PROFILE_FILE", path)))
                .env("HERDR_PLUGIN_STATE_DIR", state.path())
                .env("HERDR_SOCKET_PATH", SOCKET)
                .env("HERDR_PANE_ID", format!("w1:p{n}"))
                .args(["history", "focus"])
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    let outputs: Vec<_> = children
        .into_iter()
        .map(|child| child.wait_with_output().unwrap())
        .collect();

    for output in &outputs {
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stderr), "");
    }
    assert_eq!(focus_rows(state.path()).len(), writers);
}

#[test]
fn created_drops_the_history_of_an_earlier_pane_with_the_same_id() {
    let state = tempfile::tempdir().unwrap();
    hook(state.path(), "w3:p1")
        .args(["history", "focus"])
        .assert()
        .success();
    let db = Connection::open(state.path().join("history.db")).unwrap();
    db.execute(
        "INSERT INTO usage (session, row_key, kind, last_at) VALUES (?1, 'pane:w3:p1', 'jump_to', 0)",
        [SOCKET],
    )
    .unwrap();
    age_rows(state.path());

    hook(state.path(), "w3:p1")
        .env("HERDR_PLUGIN_EVENT_JSON", created_event("w3:p1"))
        .args(["history", "created"])
        .assert()
        .success()
        .stdout("")
        .stderr("");

    assert_eq!(focus_rows(state.path()), []);
    let usage: i64 = db
        .query_row("SELECT count(*) FROM usage", [], |row| row.get(0))
        .unwrap();
    assert_eq!(usage, 0);
}

#[test]
fn created_keeps_the_focus_the_new_pane_just_recorded() {
    let state = tempfile::tempdir().unwrap();
    // Herdr starts both hooks at once, in either order.
    hook(state.path(), "w3:p1")
        .args(["history", "focus"])
        .assert()
        .success();

    hook(state.path(), "w3:p1")
        .env("HERDR_PLUGIN_EVENT_JSON", created_event("w3:p1"))
        .args(["history", "created"])
        .assert()
        .success();

    assert_eq!(
        focus_rows(state.path()),
        [(SOCKET.to_owned(), "w3:p1".to_owned())]
    );
}
