//! End-to-end checks of `palette`: the real binary with an fzf stub (a
//! shell script that records what it got and picks a line) and a fake
//! Herdr socket.

#[path = "support/fake_herdr_server.rs"]
mod fake_herdr_server;
#[path = "support/herdr_json.rs"]
mod herdr_json;
#[path = "support/hermetic.rs"]
mod hermetic;
#[path = "support/scripts.rs"]
mod scripts;

use std::fs::{self, File, FileTimes};
use std::time::{Duration, SystemTime};

use fake_herdr_server::{FakeHerdrServer, Reply};
use herdr_json::{plugin_actions, snapshot};
use hermetic::{bin, path_with};
use serde_json::json;
use tempfile::TempDir;

fn herdr() -> FakeHerdrServer {
    FakeHerdrServer::by_method([
        ("session.snapshot", Reply::ok(snapshot())),
        ("plugin.action.list", Reply::ok(plugin_actions())),
        ("pane.zoom", Reply::ok(json!({}))),
        ("plugin.pane.open", Reply::ok(json!({}))),
    ])
}

/// A dir holding an `fzf` stub that runs bindings the way fzf does: with
/// `--with-shell` when given, else `$SHELL -c`. It runs the `tab` binding
/// into `switch` (Commands to Jump to), then takes the
/// `focus,result:transform-footer` binding, puts the first Jump to row's
/// footer field in for `{4}` (single-quoted, as fzf does),
/// saves the output to `footer` and exits like Esc.
fn fzf_binding_stub() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().display();
    let body = format!(
        "cat > /dev/null\n\
         for arg; do case \"$arg\" in \
           --with-shell=*) with_shell=${{arg#--with-shell=}};; \
           --bind=focus,result:transform-footer:*) footer=${{arg#*transform-footer:}};; \
           --bind=tab:transform:*) switch=${{arg#*transform:}};; \
         esac; done\n\
         run() {{ if [ -n \"$with_shell\" ]; then $with_shell \"$1\"; \
           else \"${{SHELL:-sh}}\" -c \"$1\"; fi; }}\n\
         run \"$switch\" > '{out}/switch' 2>&1\n\
         lines=$(head -n1 \"$HCP_STATE/rows-jump_to\" | cut -f4)\n\
         run \"${{footer%\\{{4\\}}}}'$lines'\" > '{out}/footer' 2>&1\n\
         exit 1"
    );
    scripts::fzf_script(dir.path(), &body);
    dir
}

/// A dir holding an `fzf` stub that resizes like fzf does: it runs the
/// `resize` binding with `FZF_COLUMNS` set, saves its action to `resize`,
/// runs the `reload-sync` it holds and then, as fzf's `result` event
/// does, the `focus,result:transform-footer` binding, at 60 columns
/// (`rows-60`, `footer-60`, full) and 40 (`rows-40`, `footer-40`,
/// minimal), then the reload again without the JSON rows
/// (`rows-fallback`, next to a copy of the formatted rows,
/// `rows-formatted`), and exits like Esc.
fn fzf_resize_stub() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().display();
    let body = format!(
        "cat > /dev/null\n\
         for arg; do case \"$arg\" in \
           --bind=resize:transform:*) resize=${{arg#*transform:}};; \
           --bind=focus,result:transform-footer:*) footer=${{arg#*transform-footer:}};; \
         esac; done\n\
         action=$(FZF_COLUMNS=40 sh -c \"$resize\")\n\
         printf '%s' \"$action\" > '{out}/resize'\n\
         reload=${{action#reload-sync(}}; reload=${{reload%)}}\n\
         footer=$(printf '%s' \"$footer\" | sed \"s/{{1}}/builtin/; s/{{4}}/'Zoom the focused pane'/\")\n\
         for columns in 60 40; do \
           FZF_COLUMNS=$columns sh -c \"$reload\" > '{out}/rows-'$columns; \
           FZF_COLUMNS=$columns sh -c \"$footer\" > '{out}/footer-'$columns; \
         done\n\
         cp \"$HCP_STATE/rows-commands\" '{out}/rows-formatted'\n\
         rm \"$HCP_STATE/rows-commands.json\"\n\
         FZF_COLUMNS=40 sh -c \"$reload\" > '{out}/rows-fallback'\n\
         exit 1"
    );
    scripts::fzf_script(dir.path(), &body);
    dir
}

/// A dir holding a `fish` stand-in: a shell that can't run POSIX `sh -c`
/// commands (it fails on every `-c`).
fn non_posix_shell() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    scripts::script(
        dir.path(),
        "fish",
        "echo \"fish: unsupported: $*\"\nexit 127",
    );
    dir
}

#[test]
fn footer_binding_shows_descriptions_that_start_with_a_dash() {
    let mut session = snapshot();
    session["snapshot"]["workspaces"][0]["label"] = json!("-scratch");
    let herdr = FakeHerdrServer::by_method([
        ("session.snapshot", Reply::ok(session)),
        ("plugin.action.list", Reply::ok(plugin_actions())),
    ]);
    let home = tempfile::tempdir().unwrap();
    let stub = fzf_binding_stub();

    bin(herdr.socket_path(), home.path())
        .arg("palette")
        .env("PATH", path_with(stub.path()))
        .assert()
        .success();

    let footer = fs::read_to_string(stub.path().join("footer")).unwrap();
    assert!(footer.starts_with(" -scratch / "), "{footer}");
}

#[test]
fn bindings_work_when_the_user_shell_is_not_posix() {
    let herdr = herdr();
    let home = tempfile::tempdir().unwrap();
    let stub = fzf_binding_stub();
    let fish = non_posix_shell();

    bin(herdr.socket_path(), home.path())
        .arg("palette")
        .env("PATH", path_with(stub.path()))
        .env("SHELL", fish.path().join("fish"))
        .assert()
        .success();

    let footer = fs::read_to_string(stub.path().join("footer")).unwrap();
    let switch = fs::read_to_string(stub.path().join("switch")).unwrap();
    // Jump to's two lines: workspace / tab / id, then cwd and `[^/]`.
    let lines: Vec<&str> = footer.lines().collect();
    assert_eq!(lines.len(), 2, "{footer}");
    assert!(lines[0].starts_with(" dotfiles / "), "{footer}");
    assert!(lines[1].starts_with(" /"), "{footer}");
    assert!(lines[1].ends_with("[^/]"), "{footer}");
    assert!(switch.starts_with("reload("), "{switch}");
    assert!(switch.contains(" fzf rows jump_to)"), "{switch}");
}

/// The display field (fzf's `{3}`) of the `builtin zoom` row in `rows`.
fn zoom_display(rows: &str) -> String {
    let line = rows
        .lines()
        .find(|line| line.starts_with("builtin\tzoom\t"))
        .unwrap();
    line.split('\t').nth(2).unwrap().to_owned()
}

#[test]
fn resize_lays_the_rows_and_the_footer_out_for_the_new_width() {
    let herdr = herdr();
    let home = tempfile::tempdir().unwrap();
    let stub = fzf_resize_stub();
    let read = |name: &str| fs::read_to_string(stub.path().join(name)).unwrap();

    bin(herdr.socket_path(), home.path())
        .arg("palette")
        .env("PATH", path_with(stub.path()))
        .assert()
        .success();

    // The footer follows on the reload's `result` (focus,result binding).
    let resize = read("resize");
    assert!(resize.starts_with("reload-sync("), "{resize}");
    assert!(resize.ends_with(" fzf rows commands)"), "{resize}");
    // 60 columns (outer 63, the default `min_width` 76): full, the
    // shortcut right-aligned against the right gutter.
    let full = zoom_display(&read("rows-60"));
    assert!(full.starts_with(" Toggle zoom "), "{full:?}");
    assert!(full.contains("prefix+z, prefix+f\x1b[0m "), "{full:?}");
    assert_eq!(
        full.split('\x1b').next().unwrap().chars().count(),
        1 + 58 - 18
    );
    assert!(read("footer-60").ends_with("[^/]"));
    // 40 columns (outer 43): minimal, the shortcut only searchable.
    let minimal = zoom_display(&read("rows-40"));
    assert!(
        minimal.ends_with("\x1b[0m prefix+z, prefix+f"),
        "{minimal:?}"
    );
    assert_eq!(read("footer-40"), " Zoom the focused pane");
}

/// The payloads (fzf's `{2}`) of `rows`, in order.
fn payloads(rows: &str) -> Vec<String> {
    rows.lines()
        .map(|line| line.split('\t').nth(1).unwrap().to_owned())
        .collect()
}

#[test]
fn resize_keeps_the_row_order_in_both_layouts() {
    // With `--tiebreak=index`, fzf ranks equal matches in this order, so
    // a resize can't move the cursor to another row.
    let herdr = herdr();
    let home = tempfile::tempdir().unwrap();
    let stub = fzf_resize_stub();
    let read = |name: &str| fs::read_to_string(stub.path().join(name)).unwrap();

    bin(herdr.socket_path(), home.path())
        .arg("palette")
        .env("PATH", path_with(stub.path()))
        .assert()
        .success();

    assert_eq!(payloads(&read("rows-40")), payloads(&read("rows-60")));
    assert_eq!(
        payloads(&read("rows-60")),
        payloads(&read("rows-formatted"))
    );
}

#[test]
fn resize_falls_back_to_the_formatted_rows_without_the_json() {
    let herdr = herdr();
    let home = tempfile::tempdir().unwrap();
    let stub = fzf_resize_stub();
    let read = |name: &str| fs::read_to_string(stub.path().join(name)).unwrap();

    bin(herdr.socket_path(), home.path())
        .arg("palette")
        .env("PATH", path_with(stub.path()))
        .assert()
        .success();

    assert!(read("rows-formatted").starts_with("plugin\tdemo.hello\t"));
    assert_eq!(read("rows-fallback"), read("rows-formatted"));
}

#[test]
fn dry_run_prints_the_request_for_the_selection() {
    let herdr = herdr();
    let home = tempfile::tempdir().unwrap();
    let stub = scripts::fzf_stub("^builtin\tzoom\t");

    bin(herdr.socket_path(), home.path())
        .arg("palette")
        .env("PATH", path_with(stub.path()))
        .env("HCP_DRY_RUN", "1")
        .assert()
        .success()
        .stdout("pane.zoom {\"mode\":\"toggle\",\"pane_id\":\"w1:p1\"}\n");

    let input = fs::read_to_string(stub.path().join("input")).unwrap();
    assert!(input.starts_with("plugin\tdemo.hello\t"));
    assert!(!input.contains("\npane\t"), "Jump to rows load on switch");
    let args = fs::read_to_string(stub.path().join("args")).unwrap();
    assert!(args.contains("--with-nth=3\n"));
    assert!(args.contains("--tiebreak=index\n"));
    assert!(args.contains(" fzf switch 1\n"));
    assert_eq!(
        fs::read_to_string(stub.path().join("state")).unwrap(),
        "category\nrows-commands\nrows-commands.json\nrows-jump_to\nrows-jump_to.json\n"
    );
}

#[test]
fn runs_the_selection_detached_after_exiting() {
    let herdr = herdr();
    let home = tempfile::tempdir().unwrap();
    let stub = scripts::fzf_stub("^builtin\tzoom\t");

    bin(herdr.socket_path(), home.path())
        .arg("palette")
        .env("PATH", path_with(stub.path()))
        .env("HERDR_PLUGIN_STATE_DIR", home.path())
        .assert()
        .success()
        .stdout("");

    let zoomed = scripts::wait_until(Duration::from_secs(10), || {
        herdr.methods().contains(&"pane.zoom".to_owned())
    });
    assert!(zoomed, "no pane.zoom, got {:?}", herdr.methods());
    assert_eq!(
        herdr.methods(),
        [
            "plugin.action.list",
            "session.snapshot",
            "session.snapshot",
            "pane.zoom"
        ]
    );
}

#[test]
fn a_confirm_row_opens_the_confirm_popup_from_a_detached_ask() {
    let herdr = herdr();
    let home = tempfile::tempdir().unwrap();
    let stub = scripts::fzf_stub("^builtin\tclose_workspace\t");

    bin(herdr.socket_path(), home.path())
        .arg("palette")
        .env("PATH", path_with(stub.path()))
        .env("HERDR_PLUGIN_STATE_DIR", home.path())
        .assert()
        .success()
        .stdout("");

    let opened = scripts::wait_until(Duration::from_secs(10), || {
        herdr.methods().contains(&"plugin.pane.open".to_owned())
    });
    assert!(opened, "no plugin.pane.open, got {:?}", herdr.methods());
    let open = herdr.requests().pop().unwrap();
    assert_eq!(
        open["params"],
        json!({"plugin_id": "mmc.command-palette", "entrypoint": "confirm",
               "env": {"HCP_CONFIRM": r#"{"kind":"builtin","payload":"close_workspace","title":"Close workspace"}"#},
               "width": 64, "height": 7})
    );
    assert_eq!(
        herdr.methods(),
        [
            "plugin.action.list",
            "session.snapshot",
            "session.snapshot",
            "plugin.pane.open"
        ]
    );
}

#[test]
fn removes_stale_leftovers_of_killed_palettes() {
    let herdr = herdr();
    let home = tempfile::tempdir().unwrap();
    let stub = scripts::fzf_stub("no such row");
    let tmp = tempfile::tempdir().unwrap();
    let two_days_ago = SystemTime::now() - Duration::from_hours(48);
    for (name, is_old) in [
        ("herdr-command-palette-1-0", true),
        ("herdr-command-palette-2-0", false),
        ("herdr-command-palette-notes", true),
        ("unrelated", true),
    ] {
        let dir = tmp.path().join(name);
        fs::create_dir(&dir).unwrap();
        if is_old {
            let times = FileTimes::new().set_modified(two_days_ago);
            File::open(&dir).unwrap().set_times(times).unwrap();
        }
    }

    bin(herdr.socket_path(), home.path())
        .arg("palette")
        .env("PATH", path_with(stub.path()))
        .env("TMPDIR", tmp.path())
        .assert()
        .success();

    let mut left: Vec<String> = fs::read_dir(tmp.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    left.sort();
    assert_eq!(
        left,
        [
            "herdr-command-palette-2-0",
            "herdr-command-palette-notes",
            "unrelated"
        ]
    );
}

#[test]
fn esc_closes_silently_and_cleans_up() {
    let herdr = herdr();
    let home = tempfile::tempdir().unwrap();
    let stub = scripts::fzf_stub("no such row");
    let tmp = tempfile::tempdir().unwrap();

    bin(herdr.socket_path(), home.path())
        .arg("palette")
        .env("PATH", path_with(stub.path()))
        .env("TMPDIR", tmp.path())
        .assert()
        .success()
        .stdout("");

    assert_eq!(
        fs::read_dir(tmp.path()).unwrap().count(),
        0,
        "palette state dir removed"
    );
}

/// `/dev/tty` is the developer's terminal under `cargo test`; `setsid -w`
/// runs the binary without one (util-linux, hence Linux only).
#[cfg(target_os = "linux")]
mod without_a_terminal {
    use crate::hermetic::without_terminal;

    use super::*;

    #[test]
    fn auto_switch_without_a_terminal_opens_in_the_dark_theme() {
        let herdr = herdr();
        let home = tempfile::tempdir().unwrap();
        let config = home.path().join("config.toml");
        fs::write(
            &config,
            "[theme]\nauto_switch = true\nname = \"tokyo-night-day\"\n",
        )
        .unwrap();
        let stub = scripts::fzf_stub("no such row");

        without_terminal(&bin(herdr.socket_path(), home.path()))
            .arg("palette")
            .env("PATH", path_with(stub.path()))
            .env("HERDR_CONFIG_PATH", &config)
            .timeout(Duration::from_secs(10))
            .assert()
            .success();

        // Unknown appearance is dark: tokyo-night's panel_bg.
        let args = fs::read_to_string(stub.path().join("args")).unwrap();
        assert!(args.contains("bg:#1a1b26"), "{args}");
    }
}
