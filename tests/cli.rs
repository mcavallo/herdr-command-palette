//! End-to-end checks of the binary's command-line surface.

#[path = "support/hermetic.rs"]
mod hermetic;

use hermetic::bare_bin;
use predicates::prelude::*;

#[test]
fn version_prints_name_and_version() {
    bare_bin()
        .arg("--version")
        .assert()
        .success()
        .stdout(format!(
            "herdr-command-palette {}\n",
            env!("CARGO_PKG_VERSION")
        ));
}

#[test]
fn help_lists_roles_but_hides_internal_ones() {
    bare_bin().arg("--help").assert().success().stdout(
        predicate::str::contains("repeat-command")
            .and(predicate::str::contains("fzf").not())
            .and(predicate::str::contains("confirm").not())
            .and(predicate::str::contains("  ask ").not()),
    );
}

#[test]
fn unknown_role_is_a_usage_error() {
    bare_bin().arg("nope").assert().code(2);
}
