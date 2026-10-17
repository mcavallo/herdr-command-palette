//! The `confirm` pane end to end in a pseudo-terminal: the real binary
//! draws the dialog on a pty that is its controlling terminal (`/dev/tty`),
//! the test types the key, and the detached `run` reaches a fake Herdr
//! socket. Linux only, like the other tests of screens that read
//! `/dev/tty` (docs/development.md, "Testing").

#![cfg(target_os = "linux")]

#[path = "support/fake_herdr_server.rs"]
mod fake_herdr_server;
#[path = "support/herdr_json.rs"]
mod herdr_json;
#[path = "support/hermetic.rs"]
mod hermetic;
#[path = "support/scripts.rs"]
mod scripts;

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::path::Path;
use std::process::Child;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use fake_herdr_server::{FakeHerdrServer, Reply};
use hermetic::bin;
use pty_process::Size;
use pty_process::blocking::{Command, Pty};
use serde_json::{Value, json};

/// How long any step may take before the test fails.
const TIMEOUT: Duration = Duration::from_secs(10);

fn herdr(snapshot: Value) -> FakeHerdrServer {
    FakeHerdrServer::by_method([
        ("session.snapshot", Reply::ok(snapshot)),
        ("workspace.close", Reply::ok(json!({}))),
        ("tab.close", Reply::ok(json!({}))),
    ])
}

/// Interior row count for a 64-column by 7-row popup with detail text.
const DETAIL_ROWS: u16 = 5;
/// Interior row count for a 64-column by 6-row popup without detail text.
const TITLE_ONLY_ROWS: u16 = 4;

/// The `confirm` pane for `row` on a pty 61 columns wide (the inside of
/// the 64-column popup) and `rows` high, with the hermetic environment of
/// `bin`; its state dir (run log) is `home`.
fn confirm_pane(herdr: &FakeHerdrServer, home: &Path, row: &str, rows: u16) -> (Pty, Child) {
    let (pty, pts) = pty_process::blocking::open().unwrap();
    pty.resize(Size::new(rows, 61)).unwrap();
    let inner = bin(herdr.socket_path(), home);
    let mut command = Command::new(inner.get_program()).env_clear();
    for (name, value) in inner.get_envs() {
        if let Some(value) = value {
            command = command.env(name, value);
        }
    }
    let child = command
        .env("HERDR_PLUGIN_STATE_DIR", home)
        .env("HCP_CONFIRM", row)
        .arg("confirm")
        .spawn(pts)
        .unwrap();
    (pty, child)
}

/// Everything the pane writes, read on a thread so a test can time out.
fn output(pty: &Pty) -> Receiver<Vec<u8>> {
    let mut reader = File::from(pty.as_fd().try_clone_to_owned().unwrap());
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        // Ends with the pane: reading a pty without a writer fails (EIO).
        while let Ok(count @ 1..) = reader.read(&mut buffer) {
            if sender.send(buffer[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    receiver
}

/// Waits until the screen contains every one of `texts`; returns it.
fn wait_for_screen(output: &Receiver<Vec<u8>>, texts: &[&str]) -> String {
    let deadline = Instant::now() + TIMEOUT;
    let mut screen = Vec::new();
    loop {
        let text = String::from_utf8_lossy(&screen).into_owned();
        if texts.iter().all(|wanted| text.contains(wanted)) {
            return text;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        match output.recv_timeout(left) {
            Ok(bytes) => screen.extend(bytes),
            Err(error) => panic!("{error}: {texts:?} not in {text:?}"),
        }
    }
}

/// The row the buttons are drawn at: the last cursor move (`ESC [ row ;
/// 1 H`) before ` esc cancel `.
fn buttons_row(screen: &str) -> Option<u16> {
    let before = &screen[..screen.find(" esc cancel ")?];
    let end = before.rfind(";1H")?;
    let start = before[..end].rfind("\x1b[")? + 2;
    before[start..end].parse().ok()
}

/// Waits for `child` to exit; its exit code.
fn wait_for_exit(child: &mut Child) -> Option<i32> {
    let mut status = None;
    let exited = scripts::wait_until(TIMEOUT, || {
        status = child.try_wait().unwrap();
        status.is_some()
    });
    assert!(exited, "the pane did not exit");
    status.unwrap().code()
}

/// Waits until Herdr got `method`; its params.
fn wait_for_request(herdr: &FakeHerdrServer, method: &str) -> Value {
    let mut request = None;
    let arrived = scripts::wait_until(TIMEOUT, || {
        request = herdr.requests().into_iter().find(|r| r["method"] == method);
        request.is_some()
    });
    assert!(arrived, "no {method}, got {:?}", herdr.methods());
    request.unwrap()["params"].clone()
}

#[test]
fn enter_runs_the_row_detached_after_the_pane_exits() {
    let herdr = herdr(herdr_json::snapshot());
    let home = tempfile::tempdir().unwrap();
    let (mut pty, mut child) = confirm_pane(
        &herdr,
        home.path(),
        r#"{"kind":"builtin","payload":"close_workspace","title":"Close workspace"}"#,
        DETAIL_ROWS,
    );
    let output = output(&pty);

    let screen = wait_for_screen(
        &output,
        &[
            " Close workspace?",
            " dotfiles ",
            " 2 panes ",
            " esc cancel ",
        ],
    );
    assert_eq!(buttons_row(&screen), Some(DETAIL_ROWS));
    pty.write_all(b"\r").unwrap();

    assert_eq!(wait_for_exit(&mut child), Some(0));
    assert_eq!(
        wait_for_request(&herdr, "workspace.close"),
        json!({"workspace_id": "w1", "close_group": true})
    );
}

#[test]
fn enter_on_close_tab_closes_the_worktree_group() {
    let herdr = herdr(herdr_json::worktree_group_snapshot());
    let home = tempfile::tempdir().unwrap();
    let (mut pty, mut child) = confirm_pane(
        &herdr,
        home.path(),
        r#"{"kind":"builtin","payload":"close_tab","title":"Close tab"}"#,
        DETAIL_ROWS,
    );
    let output = output(&pty);

    wait_for_screen(
        &output,
        &[" Close worktree group?", " repo ", " 2 workspaces, 2 panes"],
    );
    pty.write_all(b"\r").unwrap();

    assert_eq!(wait_for_exit(&mut child), Some(0));
    assert_eq!(
        wait_for_request(&herdr, "workspace.close"),
        json!({"workspace_id": "w1", "close_group": true})
    );
    assert!(!herdr.methods().contains(&"tab.close".to_owned()));
}

#[test]
fn another_key_cancels() {
    let herdr = herdr(herdr_json::snapshot());
    let home = tempfile::tempdir().unwrap();
    let (mut pty, mut child) = confirm_pane(
        &herdr,
        home.path(),
        r#"{"kind":"builtin","payload":"zoom","title":"Toggle zoom"}"#,
        TITLE_ONLY_ROWS,
    );
    let output = output(&pty);

    let screen = wait_for_screen(&output, &[" Toggle zoom?", " esc cancel "]);
    assert_eq!(buttons_row(&screen), Some(TITLE_ONLY_ROWS));
    pty.write_all(b"n").unwrap();

    assert_eq!(wait_for_exit(&mut child), Some(0));
    // The pane opens the run log before it starts a detached run, so
    // once it has exited, no log means no run, without waiting for one.
    assert!(!home.path().join("run.log").exists());
    assert_eq!(herdr.methods(), ["session.snapshot"]);
}
