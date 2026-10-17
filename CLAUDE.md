# Instructions for AI coding agents

This project is a Herdr command palette plugin written in Rust. Write
clear, idiomatic code that follows Rust community conventions.

## Before writing code

- Read [docs/code-style.md](docs/code-style.md) and follow it.
- Read [docs/architecture.md](docs/architecture.md).

## Rules

- Run `mise run ci` before every commit. All checks must pass: formatting,
  Clippy with pedantic lints, cargo-deny, tests, 100% line coverage and
  documentation.
- Use Conventional Commits (`feat:`, `fix:`, `docs:`, etc.).
- Do not push. The owner handles pushes.
- Do not register the plugin with or send requests to the user's Herdr
  instance. This includes the `link`, `unlink`, `open` and `logs` mise
  tasks, `herdr plugin` commands and direct socket access. For live
  checks, run an isolated headless Herdr with its own `XDG_CONFIG_HOME`
  and `XDG_STATE_HOME` under a short `/tmp` directory.
  Unset inherited `HERDR_*` variables. Keep socket paths within the
  108-byte limit and remove the temporary environment when finished.
- Explain architecture, dependency and behavior changes in the commit or
  pull request description. Update the relevant project documentation.
