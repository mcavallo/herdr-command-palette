//! `scripts/install.sh`, the manifest's `[[build]]` command: picks the
//! release archive for the platform, checks it, falls back to cargo.
//!
//! Runs the script in a temporary checkout with stub `uname`, `git`,
//! `curl` and `cargo` first on `PATH`; `curl` serves files from a
//! directory standing in for the GitHub release.

#![cfg(unix)]

#[path = "support/scripts.rs"]
mod scripts;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use scripts::script;
use tempfile::TempDir;

const VERSION: &str = "1.2.3";
const REPO_URL: &str = "https://github.com/owner/repo.git";
const RELEASE_URL: &str = "https://github.com/owner/repo/releases/download/v1.2.3";
const BINARY: &[u8] = b"prebuilt binary";

/// A checkout with `Cargo.toml` and the script, plus the stubs.
struct Checkout {
    dir: TempDir,
}

impl Checkout {
    /// A machine reporting `os` / `arch` from `uname`, whose checkout's
    /// `origin` is `remote`, with cargo installed.
    fn new(os: &str, arch: &str, remote: &str) -> Self {
        let checkout = Self::without_cargo(os, arch, remote);
        checkout.add_cargo(&checkout.stubs());
        checkout
    }

    fn without_cargo(os: &str, arch: &str, remote: &str) -> Self {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("checkout");
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            format!(
                "[package]\nname = \"herdr-command-palette\"\nversion = \"{VERSION}\"\n\
                 rust-version = \"1.99\"\n\n[dependencies]\nversion = \"0.0.0\"\n"
            ),
        )
        .unwrap();
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/install.sh"),
            root.join("scripts/install.sh"),
        )
        .unwrap();
        for sub in ["stubs", "release", "home"] {
            fs::create_dir_all(dir.path().join(sub)).unwrap();
        }
        let checkout = Self { dir };
        let stubs = checkout.stubs();
        script(
            &stubs,
            "uname",
            &format!("case \"$1\" in -s) echo {os} ;; -m) echo {arch} ;; esac"),
        );
        script(&stubs, "git", &format!("echo {remote}"));
        // `curl ... -o OUT URL`: logs URL, copies release/<file name>
        // to OUT, or fails like curl -f on a 404.
        script(
            &stubs,
            "curl",
            &format!(
                "out=; while [ $# -gt 1 ]; do [ \"$1\" = -o ] && out=$2; shift; done\n\
                 echo \"$1\" >> {log}\n\
                 file={release}/$(basename \"$1\")\n\
                 [ -f \"$file\" ] || exit 22\n\
                 cp \"$file\" \"$out\"",
                log = checkout.path("curl.log").display(),
                release = checkout.path("release").display(),
            ),
        );
        checkout
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn stubs(&self) -> PathBuf {
        self.path("stubs")
    }

    fn binary(&self) -> PathBuf {
        self.path("checkout/target/release/herdr-command-palette")
    }

    /// A `cargo` in `dir` that logs its arguments and writes a binary.
    fn add_cargo(&self, dir: &Path) {
        script(
            dir,
            "cargo",
            &format!(
                "echo \"$@\" >> {log}\nmkdir -p target/release\n\
                 echo built > target/release/herdr-command-palette",
                log = self.path("cargo.log").display(),
            ),
        );
    }

    /// Publishes the release archive for `triple`, with a checksum
    /// file holding `checksum` (the real one when `None`).
    fn publish(&self, triple: &str, checksum: Option<&str>) {
        let staging = self.path("staging");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("herdr-command-palette"), BINARY).unwrap();
        let archive = self
            .path("release")
            .join(format!("herdr-command-palette-{triple}.tar.gz"));
        let tar = Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&staging)
            .arg("herdr-command-palette")
            .status()
            .unwrap();
        assert!(tar.success());
        let sum = checksum.map_or_else(|| sha256(&archive), str::to_owned);
        let mut sidecar = archive.into_os_string();
        sidecar.push(".sha256");
        fs::write(
            sidecar,
            format!("{sum}  herdr-command-palette-{triple}.tar.gz\n"),
        )
        .unwrap();
    }

    /// Runs the script with the stubs first on `PATH`, and no real
    /// cargo anywhere on it.
    fn run(&self, envs: &[(&str, &str)]) -> Output {
        Command::new("sh")
            .arg(self.path("checkout/scripts/install.sh"))
            .env_clear()
            .env("PATH", path_with(&self.stubs()))
            .env("HOME", self.path("home"))
            .envs(envs.iter().copied())
            .output()
            .unwrap()
    }

    fn log(&self, name: &str) -> String {
        fs::read_to_string(self.path(name)).unwrap_or_default()
    }
}

/// `first`, then the test's own `PATH` without the directories that
/// hold a real `cargo` (the stubs in `first` shadow the other tools).
fn path_with(first: &Path) -> String {
    let real = env::var_os("PATH").unwrap();
    let kept = env::split_paths(&real)
        .filter(|dir| !dir.join("cargo").exists())
        .collect::<Vec<_>>();
    env::join_paths(std::iter::once(first.to_path_buf()).chain(kept))
        .unwrap()
        .into_string()
        .unwrap()
}

fn sha256(file: &Path) -> String {
    let output = Command::new("sh")
        .arg("-c")
        .arg("if command -v sha256sum >/dev/null; then sha256sum \"$1\"; else shasum -a 256 \"$1\"; fi")
        .arg("sh")
        .arg(file)
        .output()
        .unwrap();
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn installs_the_release_binary_when_the_checksum_matches() {
    let checkout = Checkout::new("Linux", "x86_64", REPO_URL);
    checkout.publish("x86_64-unknown-linux-musl", None);

    let output = checkout.run(&[]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read(checkout.binary()).unwrap(), BINARY);
    assert_eq!(checkout.log("cargo.log"), "");
    assert_eq!(
        checkout.log("curl.log"),
        format!(
            "{RELEASE_URL}/herdr-command-palette-x86_64-unknown-linux-musl.tar.gz\n\
             {RELEASE_URL}/herdr-command-palette-x86_64-unknown-linux-musl.tar.gz.sha256\n"
        )
    );
}

#[test]
fn downloads_the_archive_for_each_release_target() {
    let cases = [
        ("Linux", "x86_64", "x86_64-unknown-linux-musl"),
        ("Linux", "amd64", "x86_64-unknown-linux-musl"),
        ("Linux", "aarch64", "aarch64-unknown-linux-musl"),
        ("Linux", "arm64", "aarch64-unknown-linux-musl"),
        ("Darwin", "arm64", "aarch64-apple-darwin"),
        ("Darwin", "x86_64", "x86_64-apple-darwin"),
    ];
    for (os, arch, triple) in cases {
        let checkout = Checkout::new(os, arch, REPO_URL);
        checkout.publish(triple, None);

        let output = checkout.run(&[]);

        assert!(output.status.success(), "{os} {arch}: {output:?}");
        assert_eq!(fs::read(checkout.binary()).unwrap(), BINARY, "{os} {arch}");
    }
}

#[test]
fn builds_with_cargo_when_the_checksum_differs() {
    let checkout = Checkout::new("Linux", "x86_64", REPO_URL);
    checkout.publish("x86_64-unknown-linux-musl", Some("0".repeat(64).as_str()));

    let output = checkout.run(&[]);

    assert!(output.status.success(), "{output:?}");
    assert!(stdout(&output).contains("checksum mismatch"));
    assert_eq!(checkout.log("cargo.log"), "build --release --locked\n");
    assert_eq!(fs::read_to_string(checkout.binary()).unwrap(), "built\n");
}

#[test]
fn builds_with_cargo_when_the_release_has_no_archive() {
    let checkout = Checkout::new("Linux", "x86_64", REPO_URL);

    let output = checkout.run(&[]);

    assert!(output.status.success(), "{output:?}");
    assert!(stdout(&output).contains("download failed"));
    assert_eq!(checkout.log("cargo.log"), "build --release --locked\n");
}

#[test]
fn builds_with_cargo_without_downloading_on_an_unknown_platform() {
    let checkout = Checkout::new("FreeBSD", "amd64", REPO_URL);

    let output = checkout.run(&[]);

    assert!(output.status.success(), "{output:?}");
    assert!(stdout(&output).contains("no prebuilt binary for FreeBSD amd64"));
    assert_eq!(checkout.log("curl.log"), "");
    assert_eq!(checkout.log("cargo.log"), "build --release --locked\n");
}

#[test]
fn fails_with_the_rust_version_to_install_when_cargo_is_missing() {
    let checkout = Checkout::without_cargo("Linux", "x86_64", REPO_URL);

    let output = checkout.run(&[]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stdout(&output).contains("cargo not found: install Rust 1.99 or newer"),
        "{output:?}"
    );
    assert!(!checkout.binary().exists());
}

#[test]
fn finds_cargo_through_rustups_env_file() {
    let checkout = Checkout::without_cargo("Linux", "x86_64", REPO_URL);
    let cargo_bin = checkout.path("home/.cargo/bin");
    fs::create_dir_all(&cargo_bin).unwrap();
    checkout.add_cargo(&cargo_bin);
    fs::write(
        checkout.path("home/.cargo/env"),
        format!("PATH=\"{}:$PATH\"\n", cargo_bin.display()),
    )
    .unwrap();

    let output = checkout.run(&[]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(checkout.log("cargo.log"), "build --release --locked\n");
}

#[test]
fn reads_the_repository_from_an_ssh_remote() {
    let checkout = Checkout::new("Linux", "x86_64", "git@github.com:owner/repo.git");
    checkout.publish("x86_64-unknown-linux-musl", None);

    let output = checkout.run(&[]);

    assert!(output.status.success(), "{output:?}");
    assert!(checkout.log("curl.log").starts_with(RELEASE_URL));
    assert_eq!(fs::read(checkout.binary()).unwrap(), BINARY);
}

#[test]
fn takes_the_repository_from_the_environment_first() {
    let checkout = Checkout::new("Linux", "x86_64", REPO_URL);

    checkout.run(&[("HCP_INSTALL_REPO", "other/fork")]);

    assert!(
        checkout
            .log("curl.log")
            .starts_with("https://github.com/other/fork/releases/download/v1.2.3/")
    );
}

#[test]
fn builds_with_cargo_when_origin_is_not_on_github() {
    let checkout = Checkout::new("Linux", "x86_64", "https://example.com/owner/repo.git");

    let output = checkout.run(&[]);

    assert!(output.status.success(), "{output:?}");
    assert!(stdout(&output).contains("can't tell the GitHub repository"));
    assert_eq!(checkout.log("curl.log"), "");
    assert_eq!(checkout.log("cargo.log"), "build --release --locked\n");
}
