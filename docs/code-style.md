# Code style

This guide defines the Rust conventions used throughout the project.
The following references provide additional guidance on the language and
standard library:

- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/) (naming, docs, type design)
- [The Rust Book](https://doc.rust-lang.org/book/) (language basics)
- [Clippy lint list](https://rust-lang.github.io/rust-clippy/master/) (what each lint means)
- [Rust by Example](https://doc.rust-lang.org/rust-by-example/), [std docs](https://doc.rust-lang.org/std/)

Layers and process model: [architecture.md](architecture.md). Tooling and
tasks: [development.md](development.md).

**Prefer straightforward code.** Use a plain `match` when it makes the
control flow easier to follow than a chain of combinators.

## Project structure

The project uses ports and adapters. Pure domain logic is separate from
use cases and I/O implementations:

| Layer      | Contains                                                                           | May depend on              | Never                                                    |
| ---------- | ---------------------------------------------------------------------------------- | -------------------------- | -------------------------------------------------------- |
| `domain`   | Types and pure functions: rows, ordering, layout, request mapping, theme, settings | std, serde, unicode crates | I/O, env vars, clock, processes                          |
| `ports`    | Traits for the outside world (`HerdrApi`, `Picker`, ...) and their error types     | `domain`                   | Implementations                                          |
| `app`      | One use case per module, written against port traits                               | `domain`, `ports`          | Concrete adapters, `std::fs`, `std::env`, `std::process` |
| `adapters` | Port implementations: Unix socket, fzf, files, processes                           | everything                 | Business rules                                           |
| `main.rs`  | Parse args, build adapters, call `app`, map result to exit code                    | everything                 | Logic of any kind (it's excluded from coverage)          |

Rules:

- **`foo.rs` + `foo/`, no `mod.rs`.** `domain.rs` declares `pub mod row;`,
  the code lives in `domain/row.rs`. (Edition 2018+ style; `mod.rs` makes
  every editor tab say `mod.rs`.)
- **One concept per module.** `layout.rs` does widths and truncation, not
  colors. If a module's doc comment needs "and", consider splitting it.
- **Keep modules focused.** Review modules with more than about 300 lines
  of non-test code for responsibilities that could be separated.
- **Visibility:** private by default; `pub(crate)` before `pub`. Only what
  another layer needs is `pub`.
- **Constants** follow the same rule: private unless another module uses
  them (`pub(crate)`) or they are part of the documented surface (`pub`):
  mirrored Herdr tables and items used by `main.rs`, `tests/` or doctests.
  A constant used only in its own module is private, even if it has a doc
  comment.
- **Imports:** `use` at the top, grouped std / external crates / `crate::`
  (blank line between groups). Import types by name; call functions via
  their module (`layout::truncate(..)`) when the module name adds meaning.
  No glob imports except `use super::*;` in test modules.
- **`super::` only for the parent module's items**, `crate::` for
  everything else (siblings included). `super` imports go in the last
  group. `mod` declarations come before `use`.

  ```rust
  // In src/domain/layout/footer.rs
  // Good
  use crate::domain::theme::Theme;

  use super::{GAP, truncate};

  // Bad: a sibling through `super::`, and `crate` after `super`.
  use super::{GAP, truncate};
  use super::super::theme::Theme;
  ```

  Test modules keep the same groups (std / external / `crate::`), with
  `use super::*;` last; shared fakes come in as `use crate::app::fakes::World;`.

Where new code goes:

| You're adding...                                                        | Put it in                                                                            |
| ----------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| A rule about rows, ordering, widths, ids                                | `domain/<concept>.rs`                                                                |
| Something that reads a file, env var, clock, socket or spawns a process | a method on a port trait (`ports.rs`) + its adapter in `adapters/` + a fake in tests |
| A new subcommand                                                        | `cli.rs` (args) + `app/<use_case>.rs` (logic) + a dispatch arm in `app.rs`           |
| Data copied from Herdr                                                  | its own constant in `domain/`, see [Mirrored Herdr data](#mirrored-herdr-data)       |

## Naming

Follow the [API Guidelines naming chapter](https://rust-lang.github.io/api-guidelines/naming.html).
The ones that come up most:

- `UpperCamelCase` types and traits, `snake_case` functions, modules and
  variables, `SCREAMING_SNAKE_CASE` constants. Acronyms are words: `HerdrApi`,
  `Tsv`, not `HerdrAPI`.
- **No `get_` prefix.** `fn width(&self)`, not `fn get_width(&self)`.
- **Conversions:** `as_` free borrow-to-borrow (`as_str`), `to_` costly or
  owned result (`to_string`, `to_effect`), `into_` consumes `self`
  (`into_inner`). `from_` / `TryFrom` for constructors from another type.
- **Constructors:** `new` for the main one, `Default` when there is an
  obvious empty value. `with_` has two uses, told apart by the receiver:
  - `with_<thing>(thing) -> Self`: an alternate constructor
    (`Fzf::with_program(path)`). `without_<thing>` for the inverse
    (`Request::without_params`).
  - `with_<field>(self, value) -> Self`: a builder setter for an optional
    field (`Row::with_shortcut`, `UnixSocketHerdr::with_retry`).
- **`Self` inside `impl` blocks**, not the type name.

  ```rust
  impl Fzf {
      // Good
      pub fn new() -> Self { Self { program: PathBuf::from("fzf") } }
      // Bad: repeats the type name; renaming the type touches every line.
      pub fn new() -> Fzf { Fzf { program: PathBuf::from("fzf") } }
  }
  ```
- **Booleans** read as questions: `is_hidden`, `has_shortcut`, `can_run`.
  Not `hidden_flag`, not `check_hidden`.
- **Errors bind as `error`:** `Err(error)`, `.map_err(|error| ..)`,
  `if let Err(error) = ..`. `err` is the stderr writer in use cases
  (`out: &mut dyn Write, err: &mut dyn Write`), so it never names an error.
- **Fallible lookups** return `Option`; name them after the thing
  (`fn shortcut(&self) -> Option<&str>`), not `find_` / `try_get_`.
- **Tests** describe behavior: `truncates_at_grapheme_boundary`,
  `hides_builtins_that_need_input`, `does_x_when_y`. Not `test_truncate_1`.

## Errors

```rust
// Good: one enum per module that can fail, context in the variants.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    /// The file exists but couldn't be read.
    #[error("cannot read {}: {message}", path.display())]
    Read { path: PathBuf, message: String },
    /// A key has a value of the wrong type or range.
    #[error("invalid `{key}` in {}: {message}", path.display())]
    Invalid { path: PathBuf, key: String, message: String },
}

// Bad: stringly-typed, no context, can't be matched in tests.
fn load() -> Result<Settings, String> { Err("Error reading file.".into()) }
```

- **`thiserror` enum per module that can fail.** Variants say _what_ went
  wrong and carry the data to explain it (path, key, pane id).
- **Messages:** lowercase, no trailing period, no "error:" prefix
  (`"cannot read {path}: {message}"`). The caller adds context; `main.rs`
  adds the program name.
- **Message wording:** quote user-supplied values in backticks, and list
  the accepted values the same way. I/O failures start with `cannot <verb>`
  and name their subject (path, program, socket).

  ```rust
  // Good
  #[error("unknown category `{0}` (expected `commands` or `jump_to`)")]
  #[error("cannot write {}: {message}", path.display())]

  // Bad: value not quoted, accepted values not quoted, no verb.
  #[error("unknown category: {0} (expected commands or jump_to)")]
  #[error("{}: {message}", path.display())]
  ```

- **One variant per failure cause.** A port error is wrapped once, in
  `app::Error` (`Error::Herdr(#[from] HerdrError)`). Don't wrap it again in
  a per-use-case enum: `Error::Run(RunError::Herdr(..))` next to
  `Error::Herdr(..)` means a `match` on one silently misses the other.
- **Propagate with `?`.** Convert between layers with `#[from]` or an
  explicit `map_err` that adds context; don't stringify early.
- **No `unwrap`, `expect`, `panic!`, `unreachable!`, indexing that can
  panic** outside tests. The one exception is a real invariant the type
  system can't express: use `expect("<why this can't fail>")` with a
  comment, and document it under `# Panics`.
- **`anyhow` only at the binary edge** (`main.rs`), if ever. Library code
  returns typed errors so tests can `assert_eq!` on them.
- Prefer `Option` when "not there" is a normal outcome, not an error.

## Types

- **Newtypes for ids.** A pane id and a row id are both strings; the
  compiler should stop you mixing them up.

  ```rust
  /// A Herdr pane id (`w1-2`), stable across restarts.
  #[derive(Debug, Clone, PartialEq, Eq, Hash)]
  pub struct PaneId(String);

  impl PaneId {
      /// The id as Herdr writes it.
      #[must_use]
      pub fn as_str(&self) -> &str { &self.0 }
  }
  ```

- **Enums over strings and bools for closed sets.**

  ```rust
  // Good
  pub enum Category { Commands, JumpTo }
  fn rows(category: Category) -> Vec<Row>

  // Bad: what does `true` mean at the call site? What about "jumpto"?
  fn rows(jump_to: bool) -> Vec<Row>
  fn rows(category: &str) -> Vec<Row>
  ```

  Parse strings into the enum once, at the edge (CLI, settings, socket),
  with `FromStr` / serde.
- **`#[must_use]`** on pure functions and getters whose result is the whole
  point (clippy pedantic's `must_use_candidate` will ask for it).
- **Borrow in parameters, own in fields.** Take `&str`, `&[T]`, `&Path`;
  store `String`, `Vec<T>`, `PathBuf`. Return borrowed data (`&str`) from
  getters when you can.
- **`impl Into<String>`** only on constructors called with both literals and
  owned strings; not everywhere.
- **No premature generics.** A concrete type or `&dyn Trait` until a second
  real caller needs something else. Use cases take ports as `&impl Trait`
  or `&dyn Trait`; pick one per use case and don't mix.
- **Derive order:** std traits first in this order, then third-party:
  `Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default`, then
  `Serialize, Deserialize`, `thiserror::Error`. Derive `Debug` on every type.
- **JSON values:** `json!` when every key is unconditional; build a
  `Map` only when some keys are optional.
- **Structs with many optional fields** (settings): `Default` + serde
  `#[serde(default, deny_unknown_fields)]`, not a builder.
- **Serde attributes:** container-level `#[serde(default)]` when every
  field may be missing. No `#[serde(default)]` on `Option` fields (serde
  already reads a missing `Option` as `None`); field-level `default` only
  for a non-`Option` field that may be missing. Herdr's own data
  (snapshots, its config) is parsed leniently, without
  `deny_unknown_fields`, so new Herdr fields remain compatible; explain
  this in a comment. The plugin's settings reject unknown fields to catch
  misspelled keys.

  ```rust
  // Good: a missing `label` is already `None`.
  pub struct Pane { pub pane_id: String, pub label: Option<String> }

  // Good: every field may be missing, so say it once.
  #[derive(Debug, Default, Deserialize)]
  #[serde(default)]
  pub struct Theme { pub accent: Option<Color>, pub panel_bg: Option<Color> }

  // Bad: per-field noise that changes nothing.
  pub struct Pane {
      pub pane_id: String,
      #[serde(default)]
      pub label: Option<String>,
  }
  ```

## Runtime rules

- **Synchronous only.** No async runtime: every operation is a short
  request/response, so an async runtime is unnecessary.
- **No `unsafe`** (forbidden in `Cargo.toml`).
- **No global mutable state:** no `static mut`, no `lazy_static`/`OnceLock`
  holding config. Pass values in.
- **Time, env vars, filesystem, processes go behind ports.** `app` and
  `domain` never call `SystemTime::now()`, `std::env::var`, `std::fs` or
  `Command`. A use case receives what it needs (a `Clock`, an `Env`, a
  context struct) so tests are deterministic.

  ```rust
  // Bad: untestable without touching the real environment.
  fn open() -> Result<(), OpenError> {
      let pane = std::env::var("HERDR_PANE_ID")?;
      ...
  }

  // Good: main.rs/adapters read the env; the use case gets a value.
  fn open(herdr: &impl HerdrApi, env: &PluginEnv) -> Result<(), OpenError>
  ```

## Strings and text width

Display widths must match terminal columns. Use the same Unicode crates
as Herdr to keep layout measurements consistent.

- **Never byte-slice text for display.** `&s[..n]` panics inside a
  multi-byte character. `s.len()` is bytes, `s.chars().count()` is code
  points; neither is columns.
- Width: `unicode-width` 0.2 (`UnicodeWidthStr::width`). Cutting:
  `unicode-segmentation` grapheme clusters, so an emoji or accented letter
  is never split.
- **Only `domain/layout.rs` computes display width or truncates.** Anything
  else that needs a width (footer, header) calls into it.
- Byte slicing is fine for ASCII protocol data you've just checked
  (`strip_prefix`, `split_once`), which is what those methods are for.

```rust
// Bad: panics on "日本語", wrong width for emoji.
let cut = &title[..limit];

// Good
let cut = layout::truncate(title, limit);
```

## Docs and comments

- **Every public item has a doc comment** (`missing_docs` warns, CI denies).
  First line: what it is/does, one sentence. Then why, if not obvious.
- **`# Errors`** on every public fn returning `Result` (which variant,
  when). **`# Panics`** if it can panic. **`# Examples`** on domain
  functions where an example teaches something (they run as doc tests).
- **Module docs** (`//!`) at the top of every file: what the module is for
  and what it deliberately doesn't do.
- **Comments say why, not what.** Delete comments that repeat the code.
- **Explain non-obvious Rust idioms briefly.** For example:
  `// take moves the value out, leaving null, without cloning.`
  Keep longer explanations in documentation.
- **Cite Herdr when mirroring it:** file and version,
  `// Herdr src/client/shell/actions.rs, v0.9.3`. Explain behavior and
  constraints directly in the comment so it can be understood on its own.
- `TODO(<who-or-what>): <what>` only; no bare `TODO`/`FIXME`.

## Mirrored Herdr data

Default keys, theme colors and the mapping from built-in actions to socket
requests are copied from Herdr's source. They need updating when Herdr changes.

- **One table/constant per concept** (`THEME_COLORS`, `BUILTINS`,
  `DEFAULT_KEYS`), in its own module, not scattered across `match` arms.
- **Doc comment with source file(s) and Herdr version** it was copied from.
- **Tested:** a test pins a few known entries and checks invariants
  (no duplicate ids, every built-in has a request, colors parse).
- Updating to a new Herdr version = update the table, the cited version,
  and the tests in one commit.

## Lints and formatting

- `clippy::pedantic` is on (`Cargo.toml` `[lints]`); CI runs clippy with
  `-D warnings`.
- **Allow a lint only locally**, on the smallest item, with a reason:

  ```rust
  // Reason: the cast is bounded by the terminal width (u16), so it fits.
  #[allow(clippy::cast_possible_truncation)]
  fn columns(width: usize) -> u16 { ... }
  ```

  Never crate-wide, never in `lib.rs`. Prefer `#[expect(...)]` (warns when
  the lint stops firing) once you know it fires.
- `rustfmt.toml` (stable options only, `max_width = 100`). Don't fight the
  formatter; `mise run fmt`.
- **`mise run ci` green before every commit.**

## Testing

| Kind                                           | Where                                                   | Tool                                                                                            |
| ---------------------------------------------- | ------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| Unit                                           | `#[cfg(test)] mod tests` at the bottom of the same file | std `#[test]`                                                                                   |
| Integration                                    | `tests/*.rs`, shared helpers in `tests/support/`        | `assert_cmd` + `predicates` running the real binary against a fake Herdr socket and an fzf stub |
| Rendered output (rows, header, footer, layout) | unit or integration                                     | `insta` snapshots                                                                               |
| Text width and truncation                      | `domain/layout.rs`                                      | `proptest`: width <= limit, no split grapheme, ellipsis only when cut                           |
| Files                                          | anywhere                                                | `tempfile` dirs, never the real home                                                            |

- **Fakes over mocks.** A fake is a small struct implementing the port
  with canned answers and a record of calls (`FakeHerdr { replies,
  calls: RefCell<Vec<Request>> }`). No mocking crate.
- **Fakes are named `Fake<Port>`** (`FakeHerdr`, `FakeConfig`,
  `FakeSpawner`, `FakeFiles`), live in `src/app/fakes.rs`, and set
  outcomes through methods, not public fields. A fake of something that
  isn't a port says what it is (`FakeHerdrServer`, the socket server in
  `tests/support/`).

  ```rust
  // Good
  world.config.set(Err(parse_error()));
  world.files.fail_with("disk full");

  // Bad: tuple-field access, one setup style per fake.
  *world.config.0.borrow_mut() = Err(parse_error());
  world.files.failure = Some("disk full".into());
  ```

- **Shared test data lives in a fixtures module**
  (`#[cfg(test)] pub(crate) mod fixtures`, or `fakes` for port fakes),
  never in another module's `mod tests`. Files shared by unit and
  integration tests live in `tests/support/` and are included with
  `#[path]`.
- **One behavior per test**, named `does_x_when_y`. Arrange / act / assert,
  separated by blank lines.
- `unwrap`/`expect` are fine in tests. Prefer `assert_eq!` on whole values
  over many field asserts.
- **100% line coverage** (`mise run cov`); only `src/main.rs` is excluded.
  Code you can't cover means a missing port, not a new exclusion.
- Review snapshot changes (`cargo insta review`); never accept blindly.

## Plugin process rules

The binary is started by Herdr (actions, panes, event hooks) and by fzf
(callbacks). Each role has a contract:

- **Hooks are fast and silent.** `history focus` runs on every pane switch:
  no output on success, exit 0, no network beyond the Herdr socket, no
  work that can wait for `open`.
- **stdout is protocol** for fzf callbacks (`fzf rows|switch|footer`):
  only the rows/text fzf expects. Diagnostics go to **stderr**, always.
- **Errors:** stderr (ends up in Herdr's plugin log) plus a Herdr
  notification when the failure would otherwise be invisible.
  Popup errors stay on screen so the user can read them.
- **Tests never touch the user's real Herdr.** No calls to the `herdr`
  binary or socket from `mise run test`. Live checks use an isolated headless
  Herdr (own `XDG_CONFIG_HOME`/`XDG_STATE_HOME` under a short `/tmp` dir,
  `HERDR_*` unset).

## Dependencies

- **Prefer std.** A new crate needs a reason in the PR/commit body: what it
  replaces, why std isn't enough, its size and maintenance.
- Licenses must be on the `deny.toml` allow-list (GPL-3.0-or-later
  compatible); `cargo deny check` enforces it. No git or non-crates.io
  sources, no wildcard versions.
- Minimal features (`default-features = false` when the defaults pull in
  things we don't use).
- Dev-only crates go in `[dev-dependencies]`.

## Commits and pull requests

- [Conventional Commits](https://www.conventionalcommits.org/) (`feat:`,
  `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `ci:`; `!` for breaking).
- Small commits that each pass `mise run ci`. Squash-merged PRs: the PR title
  is the final commit message.
- Explain architecture, dependency and behavior changes in the commit or
  pull request description, and update the relevant documentation.
- Pin GitHub Actions to full commit SHAs using pinact so workflow
  dependencies do not change when upstream tags move. Run `mise run ci-audit`
  when touching `.github/`.
