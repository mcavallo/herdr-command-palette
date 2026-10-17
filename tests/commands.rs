//! End-to-end checks of `list`, `open`, `run`, `exec` and the fzf
//! callbacks: the real binary against a fake Herdr socket.

#[path = "support/fake_herdr_server.rs"]
mod fake_herdr_server;
#[path = "support/herdr_json.rs"]
mod herdr_json;
#[path = "support/hermetic.rs"]
mod hermetic;

use std::fs;

use fake_herdr_server::{FakeHerdrServer, Reply};
use herdr_json::{plugin_actions, snapshot};
use hermetic::{INDEXED_CONFIG, bin, bin_without_herdr};
use predicates::prelude::*;
use serde_json::json;

fn herdr() -> FakeHerdrServer {
    FakeHerdrServer::by_method([
        ("session.snapshot", Reply::ok(snapshot())),
        ("plugin.action.list", Reply::ok(plugin_actions())),
        ("plugin.pane.open", Reply::ok(json!({}))),
        ("pane.zoom", Reply::ok(json!({}))),
        ("notification.show", Reply::ok(json!({}))),
        (
            "pane.focus",
            Reply::error("pane_not_found", "pane w1:p9 not found"),
        ),
    ])
}

#[test]
fn list_prints_every_row() {
    let herdr = herdr();
    let home = tempfile::tempdir().unwrap();

    let output = bin(herdr.socket_path(), home.path())
        .env("HOME", "/home/me")
        .arg("list")
        .output()
        .unwrap();

    assert!(output.status.success());
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap());
    assert_eq!(herdr.methods(), ["plugin.action.list", "session.snapshot"]);
}

#[test]
fn list_works_with_indexed_keys_in_the_config() {
    let herdr = herdr();
    let home = tempfile::tempdir().unwrap();

    bin(herdr.socket_path(), home.path())
        .env("HERDR_CONFIG_PATH", INDEXED_CONFIG)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "builtin\tzoom\tToggle zoom\tprefix+f\t",
        ))
        .stdout(predicate::str::contains("custom\t0\thtop\tprefix+alt+t\t"))
        .stderr("");
}

#[test]
fn run_custom_works_with_indexed_keys_in_the_config() {
    let herdr = herdr();
    let home = tempfile::tempdir().unwrap();

    bin(herdr.socket_path(), home.path())
        .env("HERDR_CONFIG_PATH", INDEXED_CONFIG)
        .env("HCP_DRY_RUN", "1")
        .args(["run", "custom", "0"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"HCP_EXEC\":\"htop\""));
}

#[test]
fn list_warns_and_lists_the_rest_without_herdr() {
    let home = tempfile::tempdir().unwrap();

    bin_without_herdr(home.path())
        .args(["list", "--ids"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "builtin:split_vertical\tSplit pane right\n",
        ))
        .stderr(predicate::str::contains("plugin actions unavailable"))
        .stderr(predicate::str::contains("session unavailable"));
}

#[test]
fn open_sizes_the_popup_to_the_focused_tab() {
    let herdr = herdr();
    let home = tempfile::tempdir().unwrap();

    bin(herdr.socket_path(), home.path())
        .arg("open")
        .assert()
        .success()
        .stdout("");

    let requests = herdr.requests();
    assert_eq!(herdr.methods(), ["session.snapshot", "plugin.pane.open"]);
    assert_eq!(
        requests[1]["params"],
        json!({"plugin_id": "mmc.command-palette", "entrypoint": "palette",
               "width": 76, "height": 22})
    );
}

#[test]
fn open_dry_run_prints_the_request() {
    let herdr = herdr();
    let home = tempfile::tempdir().unwrap();

    bin(herdr.socket_path(), home.path())
        .arg("open")
        .env("HCP_DRY_RUN", "1")
        .env("HERDR_PLUGIN_ID", "mmc.command-palette-dev")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("plugin.pane.open {"))
        .stdout(predicate::str::contains(
            "\"plugin_id\":\"mmc.command-palette-dev\"",
        ));
}

#[test]
fn run_sends_the_builtin_request() {
    let herdr = herdr();
    let home = tempfile::tempdir().unwrap();

    bin(herdr.socket_path(), home.path())
        .args(["run", "builtin", "zoom"])
        .env(
            "HERDR_PLUGIN_CONTEXT_JSON",
            r#"{"focused_pane_id": "w1:p2"}"#,
        )
        .assert()
        .success();

    assert_eq!(herdr.methods(), ["session.snapshot", "pane.zoom"]);
    assert_eq!(
        herdr.requests()[1]["params"],
        json!({"pane_id": "w1:p2", "mode": "toggle"})
    );
}

#[test]
fn a_confirmed_close_tab_closes_the_worktree_group() {
    let herdr = FakeHerdrServer::by_method([
        (
            "session.snapshot",
            Reply::ok(herdr_json::worktree_group_snapshot()),
        ),
        ("workspace.close", Reply::ok(json!({}))),
    ]);
    let home = tempfile::tempdir().unwrap();

    bin(herdr.socket_path(), home.path())
        .args(["run", "--confirmed", "--", "builtin", "close_tab"])
        .assert()
        .success();

    assert_eq!(herdr.methods(), ["session.snapshot", "workspace.close"]);
    assert_eq!(
        herdr.requests()[1]["params"],
        json!({"workspace_id": "w1", "close_group": true})
    );
}

#[test]
fn run_failures_go_to_stderr_and_a_notification() {
    let herdr = herdr();
    let home = tempfile::tempdir().unwrap();

    bin(herdr.socket_path(), home.path())
        .args(["run", "plugin", "demo.hello"])
        .assert()
        .code(1)
        .stderr("herdr-command-palette: unknown_method: plugin.action.invoke\n");

    assert_eq!(
        herdr.methods(),
        ["plugin.action.invoke", "notification.show"]
    );
    assert_eq!(
        herdr.requests()[1]["params"],
        json!({"title": "command-palette", "body": "unknown_method: plugin.action.invoke"})
    );
}

#[test]
fn run_takes_a_payload_that_starts_with_a_dash_after_double_dash() {
    let herdr = herdr();
    let home = tempfile::tempdir().unwrap();

    bin(herdr.socket_path(), home.path())
        .args(["run", "--", "plugin", "-odd.id"])
        .env("HCP_DRY_RUN", "1")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"-odd.id\""));
}

#[test]
fn jumping_to_a_closed_pane_is_silent() {
    let herdr = herdr();
    let home = tempfile::tempdir().unwrap();

    bin(herdr.socket_path(), home.path())
        .args(["run", "pane", "w1:p9"])
        .assert()
        .success()
        .stdout("")
        .stderr("");

    assert_eq!(herdr.methods(), ["pane.focus"]);
}

#[test]
fn run_dry_run_prints_custom_popups() {
    let herdr = herdr();
    let home = tempfile::tempdir().unwrap();

    let output = bin(herdr.socket_path(), home.path())
        .args(["run", "custom", "1"])
        .env("HCP_DRY_RUN", "1")
        .output()
        .unwrap();

    assert!(output.status.success());
    insta::assert_snapshot!(String::from_utf8(output.stdout).unwrap());
    assert_eq!(herdr.methods(), ["session.snapshot"]);
}

#[test]
fn exec_runs_the_command_under_sh() {
    let home = tempfile::tempdir().unwrap();

    bin_without_herdr(home.path())
        .arg("exec")
        .env("HCP_EXEC", "echo hi from $0")
        .assert()
        .success()
        .stdout("hi from sh\n");
}

#[test]
fn fzf_callbacks_share_the_palette_state_dir() {
    let home = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fs::write(state.path().join("rows-jump_to"), "pane\tw1:p1\tx\ty\n").unwrap();
    let callback = |args: &[&str]| {
        let mut command = bin_without_herdr(home.path());
        command
            .arg("fzf")
            .args(args)
            .env("HCP_STATE", state.path())
            .env("FZF_COLUMNS", "60");
        command
    };

    callback(&["switch", "1"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            " fzf rows jump_to)+change-header(",
        ));
    callback(&["rows", "jump_to"])
        .assert()
        .success()
        .stdout("pane\tw1:p1\tx\ty\n");
    // Jump to (after the switch): key hints, then `[^/]` alone.
    let toggle = [
        "footer",
        "toggle",
        "--kind",
        "pane",
        "--",
        "api{hcp:seg}feat / x{hcp:seg}w1:p1{hcp:line}/tmp",
    ];
    callback(&toggle)
        .assert()
        .success()
        .stdout(predicate::str::is_match("^ ↑↓/pgup/pgdn[^\n]*\n +\\[\\^/\\]$").unwrap());
    callback(&toggle)
        .assert()
        .success()
        .stdout(format!(" api / feat / x / w1:p1\n /tmp{:50}[^/]", ""));

    assert_eq!(
        fs::read_to_string(state.path().join("category")).unwrap(),
        "jump_to"
    );
    assert_eq!(
        fs::read_to_string(state.path().join("footer")).unwrap(),
        "desc"
    );
}

#[test]
fn fzf_callbacks_fail_outside_the_palette() {
    let home = tempfile::tempdir().unwrap();

    bin_without_herdr(home.path())
        .args(["fzf", "switch", "1"])
        .assert()
        .code(1)
        .stdout("")
        .stderr("herdr-command-palette: not inside the palette: HCP_STATE is not set\n");
}

#[test]
fn fzf_footer_shows_the_git_branch_before_the_cwd() {
    let home = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let repo = home.path().join("src/api");
    fs::create_dir_all(&repo).unwrap();
    let init = std::process::Command::new("git")
        .args(["init", "-q", "-b", "feat/x"])
        .arg(&repo)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_DIR")
        .status()
        .expect("git ≥ 2.28 on PATH (tests create temp repositories)");
    assert!(init.success());
    fs::write(state.path().join("category"), "jump_to").unwrap();
    let footer = |path: &str| {
        let mut command = bin_without_herdr(home.path());
        command
            .args(["fzf", "footer", "focus", "--kind", "pane", "--"])
            .arg("api{hcp:seg}w1:p1{hcp:line}~/src/api")
            .env("HCP_STATE", state.path())
            .env("FZF_COLUMNS", "60")
            .env("PATH", path)
            // No system git config; the global one is already the temp
            // `HOME`'s (none).
            .env("GIT_CONFIG_NOSYSTEM", "1")
            // Cleared for git: it would point at another repository.
            .env("GIT_DIR", home.path().join("elsewhere"));
        command
    };
    // macOS: /usr/bin/git is an xcrun shim that caches its lookup in
    // TMPDIR, here a fresh temp dir; the first, uncached run can take
    // longer than the footer's 200 ms git limit. Warm the cache with
    // the same environment.
    let warm = std::process::Command::new("git")
        .arg("--version")
        .env_clear()
        .env("PATH", hermetic::SYSTEM_PATH)
        .env("HOME", home.path())
        .env("TMPDIR", home.path())
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(warm.success());

    footer(hermetic::SYSTEM_PATH)
        .assert()
        .success()
        .stdout(predicate::str::ends_with(format!(
            "feat/x\x1b[0m · ~/src/api{:36}[^/]",
            ""
        )));
    // No git on `PATH`: the cwd alone, no error.
    footer(&home.path().join("empty").to_string_lossy())
        .assert()
        .success()
        .stdout(format!(" api / w1:p1\n ~/src/api{:45}[^/]", ""))
        .stderr("");
}
