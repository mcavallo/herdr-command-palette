# Development

How to build the plugin from source, try your changes in Herdr, and run
the checks a change needs. For how the code is organized and why, see
[architecture.md](architecture.md).

## Build and install from source

You need Rust (stable, at least the `rust-version` in `Cargo.toml`, via
[rustup](https://rustup.rs)), plus the plugin's own
[requirements](../README.md#requirements): Herdr and fzf. The mise
tasks ([Development tools](#development-tools)) are optional here; each
step shows the plain command too.

1. Clone the repository and build the release binary:

   ```sh
   cargo build --release      # or: mise run build
   ```

   The manifest (`herdr-plugin.toml`) runs the binary from
   `target/release/`, so the plugin always uses your latest release
   build.

2. Register the checkout with Herdr (for your user, all sessions):

   ```sh
   herdr plugin link "$PWD"   # or: mise run link (builds first)
   ```

3. Bind the palette to a key, as in the README's
   [Open the palette](../README.md#open-the-palette).

### After a code change:

`cargo build --release` again; the next time the palette opens, it runs the new binary.

### After a change to `herdr-plugin.toml`:

`mise run unlink && mise run link`, so Herdr reads the manifest again.

### To uninstall:

`mise run unlink` (or `herdr plugin unlink mmc.command-palette`).

### Keep a stable build while you work

Linking your working checkout means every build you make is the palette
you use. To keep using a known build, link a separate worktree instead
and update it only when you want to:

```sh
git worktree add --detach ../hcp-try master   # once; then link ../hcp-try
cd ../hcp-try && git checkout --detach master && cargo build --release
```

## Development tools

Rust comes from rustup (above). The other tools are pinned in
`mise.toml`, with download URLs and checksums for Linux and macOS in
`mise.lock`; CI installs the same versions. [mise](https://mise.jdx.dev)
also runs the [tasks](#tasks). With mise:

```sh
mise trust && mise install   # once per clone, again after a version bump
rustup component add llvm-tools-preview
```

| Tool             | For                                                                                 |
| ---------------- | ----------------------------------------------------------------------------------- |
| `cargo-llvm-cov` | Coverage (needs `llvm-tools-preview`)                                               |
| `cargo-deny`     | License and advisory checks                                                         |
| `dprint`         | Formatting JSON, TOML and Markdown (Rust uses rustfmt)                              |
| `shellcheck`     | Linting `scripts/`                                                                  |
| `actionlint`     | Linting `.github/workflows/`                                                        |
| `pinact`         | Pinning GitHub Actions                                                              |
| `lefthook`       | Formatting staged files before each commit (optional; then `lefthook install` once) |

Not managed by mise: git ≥ 2.28 (tests create temporary repositories;
`theme-drift`), the [GitHub CLI](https://cli.github.com) logged in (only
when changing `.github/`), and podman (only for the `e2e-install` task).
dprint downloads its plugins on first run (needs network).

**Bumping a tool:** set the version in `mise.toml`, then
`mise lock --platform linux-x64,linux-arm64,macos-x64,macos-arm64` and
`mise install`. Take a release at least 7 days old (pinact's cooldown
for actions); `minimum_release_age` in `mise.toml` makes
`mise upgrade` and `@latest` skip younger ones. Commit both files.

## Tasks

The build, test and check commands are mise tasks, defined in
`mise.toml` (`[tasks]`, each with a description and, where it takes
arguments, their help). `mise tasks` lists them; `mise run <task>`
runs one, `mise run <task> --help` shows its arguments. CI runs the
same tasks ([GitHub Actions](#github-actions)), and every cargo task
passes `--locked`, as CI does.

**Before you commit:** `mise run fmt`, then `mise run ci`. With lefthook
installed (`lefthook install` once per clone), `lefthook.yml` runs the
formatting part on every commit: rustfmt and dprint on the staged
files, re-staged.

## Code layout

```
src/
  main.rs       # entry point only: parse, run, exit code
  lib.rs        # crate docs, module list
  cli.rs        # subcommands (clap)
  app.rs        # use cases, one file each under app/
  domain.rs     # pure logic, under domain/
  ports.rs      # traits for the outside world (Herdr, fzf, files, history)
  adapters.rs   # implementations of the ports, under adapters/
tests/          # integration tests, one file per area; support/ holds the fakes
```

Logic goes in `domain/`, I/O only behind a port. A module grows into a
folder (`domain.rs` + `domain/row.rs`) when it fills up; there are no
`mod.rs` files. The layers: [architecture.md](architecture.md). Naming,
errors, docs and lints: [code-style.md](code-style.md).

## Testing

- **Unit tests** sit next to the code (`#[cfg(test)] mod tests`).
- **Use case tests** (`src/app/`) use in-memory fakes of every port
  (`src/app/fakes.rs`).
- **Integration tests** (`tests/`) run the compiled binary in a cleared
  environment against a fake Herdr socket and an fzf stub. They never
  reach your real Herdr.
- **Snapshot tests** use `insta`: review changes with
  `cargo insta review`. **Property tests** use `proptest`.
- **Rows JSON snapshot** (`rows_json_keeps_its_shape`): if you change it,
  also bump `ROWS_JSON_VERSION`, so a palette that's open during an
  upgrade doesn't read the old format.
- **Linux only:** tests of screens that wait for a key need `setsid`
  (util-linux) or a pty; on macOS they are skipped, and unit tests cover
  the same logic.

### Trying it live without touching your Herdr

For checks no test can do (popup sizes, resizing, colors), run a
separate headless Herdr: its own `XDG_CONFIG_HOME` / `XDG_STATE_HOME`
under a short `/tmp` dir, every `HERDR_*` variable unset (`env -i`), the
plugin linked there, and a Herdr client in a private tmux server
(`tmux -L <name>`) that you drive with `send-keys` and read with
`capture-pane`. Remove the dir when done.

## Coverage

`mise run cov` fails below 100% line coverage of the plugin's code (`src/`).
`src/main.rs` is excluded (it only maps the result to an exit code), and
test code isn't measured. Code you can't cover usually needs a port, so a
test can stand in for the real thing.

## Upgrading Herdr

Do this whenever the plugin moves to a new Herdr version (a new
`min_herdr_version`, or before claiming support for a release).

### Theme tables

Herdr's theme colors are copied into `src/domain/theme.rs`. Check them
against Herdr's source:

```sh
mise run theme-drift          # against min_herdr_version in herdr-plugin.toml
mise run theme-drift v0.9.4   # against another tag
```

It needs git and network, and lists every difference. On drift, update the
tables, the version cited in their doc comments and the tests, in one
commit. The default tag follows `min_herdr_version`.

### Herdr behaviors to re-check on upgrades

The palette relies on Herdr behaviors no test can see. Check them live
(see [Trying it live](#trying-it-live-without-touching-your-herdr)):

| Behavior (Herdr 0.9.3)                                                  | Used by                                                                                                     |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| A popup's terminal is 3 columns narrower than the popup                 | popup sizing, the compact layout ([Resize](architecture.md#resize))                                         |
| Popups resize with the terminal, never past the size they opened with   | resize handling ([Resize](architecture.md#resize))                                                          |
| A command started from the popup keeps running after the popup closes   | `run`, `ask` ([Callback state and process lifecycle](architecture.md#callback-state-and-process-lifecycle)) |
| Pane ids come back after a restart; popups don't fire pane focus events | history ([History store](architecture.md#history-store))                                                    |

Quick resize check: open the palette in a 160-column client, then
`tmux resize-window` to just above and just below the compact layout
threshold (with the default `min_width` and a 26-column sidebar: 83 and
82 columns) and back, in both lists; then open it at 82 columns.

## End-to-end install check

`mise run e2e-install` runs `herdr plugin install` the way a user does, in a
throwaway container (podman; `HCP_E2E_RUNTIME=docker` for Docker): it
downloads a Herdr release binary, installs this plugin from GitHub
(`origin`, or `HCP_E2E_REPO=owner/repo`), and checks that the plugin is
listed, the installed binary's `--version` matches the manifest, and the
expected build path ran. Your own Herdr and home directory never enter
the container.

| Mode                 | Image                                 | Checks                                                          |
| -------------------- | ------------------------------------- | --------------------------------------------------------------- |
| `download` (default) | Debian, no Rust                       | the release binary was downloaded (public repository, released) |
| `cargo`              | `rust:<rust-version>-slim` (the MSRV) | the cargo fallback built it                                     |

The second argument picks the Herdr version (default: Herdr's latest
release): `mise run e2e-install cargo 0.9.3` checks `min_herdr_version`.

It tests what is pushed and released, not your working tree: Herdr
installs from GitHub. The images stay cached between runs (the Rust
one is about 1 GB); `mise run e2e-clean` removes them.

Run it after changing `scripts/install.sh`, `build.yml`'s packaging or
the manifest, on Herdr upgrades, and after a release
([releasing.md](releasing.md#verify-a-release)).

## GitHub Actions

Pull requests go against `develop`, the work branch
(`gh pr create --base develop`); `master` only receives releases.
Releases, the branches and the release workflows:
[releasing.md](releasing.md).

`ci.yml` runs the tasks of `mise run ci`, plus `ci-audit` and `msrv`,
on pull requests and `develop` pushes, one task per step, as five
parallel jobs (`test-macos` runs cargo directly: no mise there). Every
job sets up through two local composite actions: `setup-rust`
(toolchain, components, target, the cache rules below) and
`setup-mise` (pinned mise, only the tools the job needs, and
`MISE_TASK_RUN_AUTO_INSTALL=false` so `mise run` doesn't install the
rest). Each documents its inputs in its `action.yml` under
`.github/actions/`:

| Job          | Runs                                                                                 |
| ------------ | ------------------------------------------------------------------------------------ |
| `lint`       | fmt, dprint, clippy, `cargo doc`, shellcheck, actionlint, version sync, pinact check |
| `deny`       | `cargo deny check`                                                                   |
| `coverage`   | all tests on Linux with the 100% line gate, doc tests                                |
| `test-macos` | all tests on macOS                                                                   |
| `msrv`       | `cargo check` with the `rust-version` in `Cargo.toml` (`mise run msrv`; not in `ci`) |

Cargo caches are saved from `develop` only, by jobs that pass; pull
requests restore them. A cache key is saved once and never updated (it
changes with `Cargo.lock` or the Rust version), so a bad cache stays
until deleted: if clippy or the tests compile every dependency on each
run, check its size with `gh cache list` and remove it with
`gh cache delete <key>`; the next `develop` run saves a new one.
A new push to a pull request cancels its running checks.
`pr-title.yml` checks the PR title is a Conventional Commit: it becomes
the squash commit and the changelog entry. `pr-base.yml` moves pull
requests opened against `master` to `develop` and reopens them, which
starts CI (it only runs on pull requests against `develop`).

### Pinning actions

Use [pinact](https://github.com/suzuki-shunsuke/pinact) to pin GitHub Actions
to full commit SHAs and maintain their version comments. Run `mise run ci-audit`
after changing workflows to check the pins, verify that version comments
match the referenced commits, and enforce the 7-day release cooldown.
The audit checks files without modifying them; `mise run ci-pin` and
`mise run ci-update` apply changes:

- **Adding an action:** write it with a tag (`uses: owner/action@v1.2.3`),
  then `mise run ci-pin`. Branch refs (`@main`) can't be pinned.
- **Cooldown:** releases younger than 7 days are refused. `mise run ci-update`
  picks the newest one past it but never downgrades: if a major tag points
  to a release from this week, pin an older tag by hand and run
  `mise run ci-pin`.
- **Before pushing changes to `.github/`:** `mise run ci-audit` (uses your `gh`
  login; `PINACT_GITHUB_TOKEN` overrides it).

CI's dev tools come from `mise.toml` through `jdx/mise-action`, pinned
like any action, inside `.github/actions/setup-mise/action.yml`, with
the mise version and the SHA-256 of its (Linux x64) binary. pinact
checks and updates the pins there too (`.github/pinact.yaml` lists
`.github/actions/`). actionlint checks the workflows' calls to the
local actions (input names) but not the `action.yml` files themselves.
To bump mise itself: take a release at least 7
days old, download `mise-v<version>-linux-x64.tar.gz`, check it against
the release's `SHASUMS256.txt`, and use the SHA-256 of the extracted
`mise/bin/mise`.

The same 7-day rule applies to dprint plugins: `dprint config update`
doesn't know it, so check a new plugin release's date before taking it.
