//! End-to-end checks of history use: `run` recording rows, the palette's
//! "recent" orders, the repeat actions and `open` pruning. The real binary
//! with a real SQLite file in a temp state dir, a fake Herdr socket and an
//! fzf stub.

#[path = "support/fake_herdr_server.rs"]
mod fake_herdr_server;
#[path = "support/herdr_json.rs"]
mod herdr_json;
#[path = "support/hermetic.rs"]
mod hermetic;
#[path = "support/scripts.rs"]
mod scripts;

use std::fs;
use std::path::Path;

use fake_herdr_server::{FakeHerdrServer, Reply};
use herdr_json::{plugin_actions, snapshot};
use hermetic::{bin, home_with_settings, path_with};
use rusqlite::Connection;
use serde_json::{Value, json};

fn herdr() -> FakeHerdrServer {
    FakeHerdrServer::by_method([
        ("session.snapshot", Reply::ok(snapshot())),
        ("plugin.action.list", Reply::ok(plugin_actions())),
        ("plugin.pane.open", Reply::ok(json!({}))),
        ("pane.focus", Reply::ok(json!({}))),
        ("pane.zoom", Reply::ok(json!({}))),
        ("server.reload_config", Reply::ok(json!({}))),
        ("plugin.action.invoke", Reply::ok(json!({}))),
        ("notification.show", Reply::ok(json!({}))),
    ])
}

/// The binary for `herdr`'s session, with history in `home`.
fn plugin(herdr: &FakeHerdrServer, home: &Path) -> assert_cmd::Command {
    let mut command = bin(herdr.socket_path(), home);
    command.env("HERDR_PLUGIN_STATE_DIR", home);
    command
}

fn run(herdr: &FakeHerdrServer, home: &Path, kind: &str, payload: &str) {
    plugin(herdr, home)
        .args(["run", "--", kind, payload])
        .assert()
        .success();
}

fn focus(herdr: &FakeHerdrServer, home: &Path, pane: &str) {
    plugin(herdr, home)
        .env("HERDR_PANE_ID", pane)
        .args(["history", "focus"])
        .assert()
        .success()
        .stderr("");
}

/// The rows the palette hands fzf, opening on `category`.
fn palette_input(herdr: &FakeHerdrServer, home: &Path) -> String {
    let stub = tempfile::tempdir().unwrap();
    let out = stub.path().display();
    scripts::fzf_script(stub.path(), &format!("cat > '{out}/input'"));
    plugin(herdr, home)
        .arg("palette")
        .env("PATH", path_with(stub.path()))
        .assert()
        .success();
    fs::read_to_string(stub.path().join("input")).unwrap()
}

/// `kind:payload` of each fzf line.
fn ids(input: &str) -> Vec<String> {
    input
        .lines()
        .map(|line| {
            let mut fields = line.split('\t');
            format!("{}:{}", fields.next().unwrap(), fields.next().unwrap())
        })
        .collect()
}

fn requests(herdr: &FakeHerdrServer, method: &str) -> Vec<Value> {
    herdr
        .requests()
        .into_iter()
        .filter(|request| request["method"] == method)
        .collect()
}

fn toasts(herdr: &FakeHerdrServer) -> Vec<String> {
    requests(herdr, "notification.show")
        .iter()
        .map(|request| request["params"]["body"].as_str().unwrap().to_owned())
        .collect()
}

fn db(home: &Path) -> Connection {
    Connection::open(home.join("history.db")).unwrap()
}

fn count(home: &Path, table: &str) -> i64 {
    db(home)
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
fn commands_run_last_come_first_in_the_palette() {
    let herdr = herdr();
    let home = home_with_settings("");

    run(&herdr, home.path(), "builtin", "zoom");
    run(&herdr, home.path(), "plugin", "demo.bye");

    let ids = ids(&palette_input(&herdr, home.path()));
    assert_eq!(
        ids[..3],
        ["plugin:demo.bye", "builtin:zoom", "plugin:demo.hello"]
    );
}

#[test]
fn custom_commands_keep_their_history_when_the_config_moves_them() {
    let herdr = herdr();
    let home = home_with_settings("");
    let config = home.path().join("config.toml");
    fs::write(
        &config,
        "[[keys.command]]\ntype = \"popup\"\ncommand = \"htop\"\n",
    )
    .unwrap();
    plugin(&herdr, home.path())
        .env("HERDR_CONFIG_PATH", &config)
        .args(["run", "custom", "0"])
        .assert()
        .success();
    fs::write(
        &config,
        "[[keys.command]]\ncommand = \"ls\"\n\
         [[keys.command]]\ntype = \"popup\"\ncommand = \"htop\"\n",
    )
    .unwrap();

    plugin(&herdr, home.path())
        .env("HERDR_CONFIG_PATH", &config)
        .arg("repeat-command")
        .assert()
        .success();

    // Index 1 now: the popup opened again, nothing was notified.
    assert_eq!(toasts(&herdr), Vec::<String>::new());
    assert_eq!(requests(&herdr, "plugin.pane.open").len(), 2);
    let key: String = db(home.path())
        .query_row("SELECT row_key FROM usage", [], |row| row.get(0))
        .unwrap();
    assert!(key.starts_with("custom:"), "{key}");
    let uses: i64 = db(home.path())
        .query_row("SELECT count FROM usage", [], |row| row.get(0))
        .unwrap();
    assert_eq!(uses, 2);
}

#[test]
fn jump_to_lists_recent_panes_first_and_the_current_one_last() {
    let herdr = herdr();
    let home = home_with_settings("open_on = \"jump_to\"\n");
    focus(&herdr, home.path(), "w1:p2");
    focus(&herdr, home.path(), "w1:p1");

    let ids = ids(&palette_input(&herdr, home.path()));

    assert_eq!(ids, ["pane:w1:p2", "pane:w1:p1"]);
}

#[test]
fn herdr_order_ignores_the_history() {
    let herdr = herdr();
    let home = home_with_settings("open_on = \"jump_to\"\n[jump_to]\norder = \"herdr\"\n");
    focus(&herdr, home.path(), "w1:p2");

    let ids = ids(&palette_input(&herdr, home.path()));

    assert_eq!(ids, ["pane:w1:p1", "pane:w1:p2"]);
}

#[test]
fn repeat_jump_focuses_the_newest_jump_target_still_open() {
    let herdr = herdr();
    let home = home_with_settings("");
    run(&herdr, home.path(), "pane", "w1:p2");
    run(&herdr, home.path(), "pane", "w9:p9");

    plugin(&herdr, home.path())
        .arg("repeat-jump")
        .assert()
        .success();

    let focused = requests(&herdr, "pane.focus");
    assert_eq!(
        focused.last().unwrap()["params"],
        json!({"pane_id": "w1:p2"})
    );
}

#[test]
fn repeat_jump_skips_the_current_pane() {
    let herdr = herdr();
    let home = home_with_settings("");
    run(&herdr, home.path(), "pane", "w1:p2");
    run(&herdr, home.path(), "pane", "w1:p1");

    plugin(&herdr, home.path())
        .arg("repeat-jump")
        .assert()
        .success();

    let focused = requests(&herdr, "pane.focus");
    assert_eq!(
        focused.last().unwrap()["params"],
        json!({"pane_id": "w1:p2"})
    );
}

#[test]
fn repeat_command_skips_hidden_rows() {
    let herdr = herdr();
    let home = home_with_settings("[commands]\nhide_ids = [\"builtin:zoom\"]\n");
    run(&herdr, home.path(), "builtin", "zoom");

    plugin(&herdr, home.path())
        .arg("repeat-command")
        .assert()
        .success();

    assert_eq!(requests(&herdr, "pane.zoom").len(), 1);
    assert_eq!(toasts(&herdr), ["no command to repeat yet"]);
}

#[test]
fn skip_recent_rows_are_recorded_and_repeated_but_not_put_first() {
    let herdr = herdr();
    let home = home_with_settings("");
    run(&herdr, home.path(), "builtin", "zoom");
    run(&herdr, home.path(), "builtin", "reload_config");

    let ids = ids(&palette_input(&herdr, home.path()));
    plugin(&herdr, home.path())
        .arg("repeat-command")
        .assert()
        .success();

    // `[commands] skip_recent_ids` defaults to `builtin:reload_config`.
    assert_eq!(ids[..2], ["builtin:zoom", "plugin:demo.hello"]);
    let used: i64 = db(home.path())
        .query_row(
            "SELECT COUNT(*) FROM usage WHERE row_key = 'builtin:reload_config'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(used, 1);
    assert_eq!(requests(&herdr, "server.reload_config").len(), 2);
}

#[test]
fn repeat_command_reruns_and_records_the_newest_command() {
    let herdr = herdr();
    let home = home_with_settings("");
    run(&herdr, home.path(), "builtin", "zoom");

    plugin(&herdr, home.path())
        .arg("repeat-command")
        .assert()
        .success();

    assert_eq!(requests(&herdr, "pane.zoom").len(), 2);
    let uses: i64 = db(home.path())
        .query_row("SELECT count FROM usage", [], |row| row.get(0))
        .unwrap();
    assert_eq!(uses, 2);
}

#[test]
fn repeat_command_of_a_confirm_row_opens_the_confirm_popup() {
    let herdr = herdr();
    let home = home_with_settings("[commands]\nconfirm_ids = [\"builtin:zoom\"]\n");
    run(&herdr, home.path(), "builtin", "zoom");

    plugin(&herdr, home.path())
        .arg("repeat-command")
        .assert()
        .success();

    assert_eq!(requests(&herdr, "pane.zoom").len(), 1);
    let open = requests(&herdr, "plugin.pane.open");
    let params = &open.last().unwrap()["params"];
    assert_eq!(params["entrypoint"], "confirm");
    assert_eq!(
        params["env"]["HCP_CONFIRM"],
        r#"{"kind":"builtin","payload":"zoom","title":"Toggle zoom"}"#
    );
    // Without detail text, the confirmation popup is 64 columns by 6 rows.
    assert_eq!(
        (&params["width"], &params["height"]),
        (&json!(64), &json!(6))
    );
}

#[test]
fn repeat_jump_with_an_empty_history_says_so() {
    let herdr = herdr();
    let home = home_with_settings("");

    plugin(&herdr, home.path())
        .arg("repeat-jump")
        .assert()
        .success();

    assert_eq!(toasts(&herdr), ["no jump to repeat yet"]);
}

#[test]
fn history_off_records_nothing_and_repeat_says_so() {
    let herdr = herdr();
    let home = home_with_settings("[history]\nenabled = false\n");

    focus(&herdr, home.path(), "w1:p2");
    run(&herdr, home.path(), "builtin", "zoom");
    plugin(&herdr, home.path())
        .arg("repeat-command")
        .assert()
        .success();

    assert!(!home.path().join("history.db").exists());
    assert_eq!(toasts(&herdr), ["history is off"]);
}

#[test]
fn open_prunes_closed_panes_after_opening_the_popup() {
    let herdr = herdr();
    let home = home_with_settings("");
    focus(&herdr, home.path(), "w1:p2");
    focus(&herdr, home.path(), "w7:p7");

    plugin(&herdr, home.path()).arg("open").assert().success();

    assert_eq!(herdr.methods(), ["session.snapshot", "plugin.pane.open"]);
    assert_eq!(count(home.path(), "focus"), 1);
}

#[test]
fn an_open_while_a_pane_is_closed_drops_its_history() {
    let herdr = herdr();
    let home = home_with_settings("");
    plugin(&herdr, home.path()).arg("open").assert().success();
    // Within the hour of the full prune: a closed pane (not in the fake
    // snapshot). Once its id is live again, `open` no longer drops it;
    // `history created` does.
    focus(&herdr, home.path(), "w6:p1");

    plugin(&herdr, home.path()).arg("open").assert().success();

    assert_eq!(count(home.path(), "focus"), 0);
}

#[test]
fn an_open_with_no_panes_in_the_snapshot_keeps_the_history() {
    let mut empty = snapshot();
    empty["snapshot"]["panes"] = json!([]);
    let herdr = FakeHerdrServer::by_method([
        ("session.snapshot", Reply::ok(empty)),
        ("plugin.pane.open", Reply::ok(json!({}))),
    ]);
    let home = home_with_settings("");
    focus(&herdr, home.path(), "w1:p1");

    plugin(&herdr, home.path()).arg("open").assert().success();

    assert_eq!(count(home.path(), "focus"), 1);
}

/// The `confirm` pane reads a key from `/dev/tty`; `setsid -w` runs it
/// without a controlling terminal, so the read fails at once (a cancel,
/// as Esc). Typing keys in a pty is `tests/confirm.rs`.
/// util-linux only, hence Linux only (docs/development.md, "Testing").
#[cfg(target_os = "linux")]
mod confirm_pane {
    use crate::hermetic::without_terminal;

    use super::*;

    /// The `confirm` pane for `row`, run without a terminal.
    fn confirm_pane(herdr: &FakeHerdrServer, home: &Path, row: &str) -> assert_cmd::Command {
        let mut command = without_terminal(&plugin(herdr, home));
        command.arg("confirm").env("HCP_CONFIRM", row);
        command
    }

    #[test]
    fn draws_herdrs_close_dialog_and_cancels_without_a_key() {
        let herdr = herdr();
        let home = home_with_settings("");

        confirm_pane(
            &herdr,
            home.path(),
            r#"{"kind":"builtin","payload":"close_workspace","title":"Close workspace"}"#,
        )
        .assert()
        .success()
        .stdout(predicates::str::contains(" Close workspace?"))
        .stdout(predicates::str::contains(" dotfiles "))
        .stdout(predicates::str::contains(" 2 panes "))
        .stdout(predicates::str::contains("  ↵ confirm  "))
        .stdout(predicates::str::contains(" esc cancel "));

        assert_eq!(herdr.methods(), ["session.snapshot"]);
    }

    #[test]
    fn asks_about_other_rows_by_title() {
        let herdr = herdr();
        let home = home_with_settings("");

        confirm_pane(
            &herdr,
            home.path(),
            r#"{"kind":"builtin","payload":"zoom","title":"Toggle zoom"}"#,
        )
        .assert()
        .success()
        .stdout(predicates::str::contains(" Toggle zoom?"));

        assert_eq!(herdr.methods(), ["session.snapshot"]);
    }
}
