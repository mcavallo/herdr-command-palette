//! End-to-end checks of the plugin settings (`settings.toml`):
//! the real binary with the file in a temp plugin config dir, an fzf stub
//! and a fake Herdr socket.

#[path = "support/fake_herdr_server.rs"]
mod fake_herdr_server;
#[path = "support/herdr_json.rs"]
mod herdr_json;
#[path = "support/hermetic.rs"]
mod hermetic;
#[path = "support/scripts.rs"]
mod scripts;

use std::fs;

use fake_herdr_server::{FakeHerdrServer, Reply};
use herdr_json::{plugin_actions, snapshot};
use hermetic::{bin, bin_without_herdr, home_with_settings, path_with};
use predicates::prelude::*;
use serde_json::json;

fn herdr() -> FakeHerdrServer {
    FakeHerdrServer::by_method([
        ("session.snapshot", Reply::ok(snapshot())),
        ("plugin.action.list", Reply::ok(plugin_actions())),
        ("plugin.pane.open", Reply::ok(json!({}))),
        ("notification.show", Reply::ok(json!({}))),
    ])
}

#[test]
fn check_passes_without_a_file() {
    let home = tempfile::tempdir().unwrap();

    bin_without_herdr(home.path())
        .args(["settings", "check"])
        .assert()
        .success()
        .stdout("")
        .stderr("");
}

#[test]
fn check_names_the_file_key_and_line() {
    let home = home_with_settings("[popup]\nmin_width = 100\n");
    let path = home.path().join("settings.toml");

    bin_without_herdr(home.path())
        .args(["settings", "check"])
        .assert()
        .code(1)
        .stdout("")
        .stderr(format!(
            "herdr-command-palette: invalid `popup.min_width` (line 2) in {}: 100 is larger than `popup.max_width` (76)\n",
            path.display()
        ));
}

#[test]
fn check_file_reads_that_file_not_the_environments() {
    let home = home_with_settings("");
    let edited = home.path().join("edited.toml");
    fs::write(&edited, "bogus = 1\n").unwrap();

    bin_without_herdr(home.path())
        .args(["settings", "check", "--file"])
        .arg(&edited)
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::starts_with(format!(
            "herdr-command-palette: invalid `bogus` (line 1) in {}:",
            edited.display()
        )));
}

#[test]
fn palette_leaves_out_hidden_rows() {
    let herdr = herdr();
    let home = home_with_settings(
        "[commands]\nhide_kinds = [\"plugin\"]\nhide_ids = [\"builtin:zoom\"]\n",
    );
    let stub = scripts::fzf_stub("no such row");

    bin(herdr.socket_path(), home.path())
        .arg("palette")
        .env("PATH", path_with(stub.path()))
        .assert()
        .success();

    let input = fs::read_to_string(stub.path().join("input")).unwrap();
    assert!(input.starts_with("builtin\tsplit_vertical\t"), "{input}");
    assert!(!input.contains("builtin\tzoom\t"));
}

#[test]
fn palette_opens_on_the_settings_category_in_its_colors() {
    let herdr = herdr();
    let home = home_with_settings("open_on = \"jump_to\"\n[colors]\nselected = 4\n");
    let stub = scripts::fzf_stub("no such row");

    let output = bin(herdr.socket_path(), home.path())
        .arg("palette")
        .env("PATH", path_with(stub.path()))
        .output()
        .unwrap();

    assert!(output.status.success());
    let input = fs::read_to_string(stub.path().join("input")).unwrap();
    assert!(input.starts_with("pane\tw1:p1\t"), "{input}");
    let args = fs::read_to_string(stub.path().join("args")).unwrap();
    assert!(args.contains("--color=bg:#181825,bg+:#181825,"), "{args}");
    assert!(args.contains(",fg+:4:bold,"), "{args}");
}

#[test]
fn open_sizes_the_popup_from_the_settings() {
    let herdr = herdr();
    let home = home_with_settings("[popup]\nmin_width = 50\nmax_width = 100\nmax_height = 30\n");

    bin(herdr.socket_path(), home.path())
        .arg("open")
        .assert()
        .success();

    assert_eq!(
        herdr.requests()[1]["params"],
        json!({"plugin_id": "mmc.command-palette", "entrypoint": "palette",
               "width": 100, "height": 30})
    );
}

#[test]
fn open_raises_popup_sizes_below_the_defaults_silently() {
    let herdr = herdr();
    let home = home_with_settings("[popup]\nmin_width = 0\nmax_width = 40\nmax_height = 10\n");

    bin(herdr.socket_path(), home.path())
        .arg("open")
        .assert()
        .success()
        .stderr("");

    assert_eq!(
        herdr.requests()[1]["params"],
        json!({"plugin_id": "mmc.command-palette", "entrypoint": "palette",
               "width": 76, "height": 22})
    );
}

#[test]
fn check_accepts_popup_sizes_below_the_defaults() {
    let home = home_with_settings("[popup]\nmin_width = 0\nmax_width = 40\n");

    bin_without_herdr(home.path())
        .args(["settings", "check"])
        .assert()
        .success()
        .stdout("")
        .stderr("");
}

#[test]
fn edit_settings_writes_the_commented_file_and_opens_it() {
    let herdr = herdr();
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("settings.toml");

    bin(herdr.socket_path(), home.path())
        .args(["run", "palette", "edit_settings"])
        .assert()
        .success();

    let text = fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("# Command Palette settings"), "{text}");
    let command = herdr.requests()[1]["params"]["env"]["HCP_EXEC"].clone();
    let command = command.as_str().unwrap();
    assert!(
        command.starts_with(&format!("${{EDITOR:-vi}} {}", path.display())),
        "{command}"
    );
    assert!(
        command.ends_with(&format!(" settings check --file {}", path.display())),
        "{command}"
    );
}

#[test]
fn run_falls_back_to_defaults_with_a_toast() {
    let herdr = herdr();
    let home = home_with_settings("editor = 3\n");

    bin(herdr.socket_path(), home.path())
        .args(["run", "palette", "edit_herdr_config"])
        .assert()
        .success()
        .stderr(predicate::str::contains("(using the default settings)"));

    assert_eq!(
        herdr.methods(),
        ["notification.show", "session.snapshot", "plugin.pane.open"]
    );
    let body = herdr.requests()[0]["params"]["body"].clone();
    assert!(
        body.as_str()
            .unwrap()
            .starts_with("invalid `editor` (line 1) in "),
        "{body}"
    );
}

#[test]
fn list_ids_name_custom_commands_by_description() {
    let herdr = herdr();
    let home = tempfile::tempdir().unwrap();

    bin(herdr.socket_path(), home.path())
        .args(["list", "--ids"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\ncustom:Popup: lazygit\tPopup: lazygit\n",
        ));
}

/// Screens that wait for a key read `/dev/tty`. `setsid -w` runs the
/// binary without a controlling terminal, so the read fails at once (Esc,
/// in effect) instead of waiting on the developer's terminal. util-linux
/// only, hence Linux only; the unit tests cover the logic everywhere.
#[cfg(target_os = "linux")]
mod without_a_terminal {
    use crate::hermetic::without_terminal;

    use super::*;

    #[test]
    fn invalid_settings_show_the_error_screen() {
        let herdr = herdr();
        let home = home_with_settings("[colours]\n");
        let stub = scripts::fzf_stub("no such row");

        without_terminal(&bin(herdr.socket_path(), home.path()))
            .arg("palette")
            .env("PATH", path_with(stub.path()))
            .assert()
            .code(1)
            .stdout(predicate::str::contains(
                " invalid `colours` (line 1): unknown field",
            ))
            .stdout(predicate::str::contains(" in ~/settings.toml\n"))
            .stdout(predicate::str::contains("edit e · close any key"));

        assert!(!stub.path().join("input").exists(), "fzf never started");
    }
}
