//! Use cases: what each subcommand does, written against the [`ports`]
//! traits so it can be tested without Herdr, fzf or a terminal.
//!
//! [`run`] dispatches a [`Command`] to its use case. Everything a use case
//! needs from the outside world arrives in a [`Context`]; output goes to
//! the `out` / `err` writers (stdout is protocol for the fzf callbacks, so
//! diagnostics always go to `err`).
//!
//! [`ports`]: crate::ports

pub mod ask;
pub mod confirm;
pub mod exec;
pub mod fzf_callbacks;
pub mod history;
pub mod list;
pub mod open;
pub mod palette;
pub mod recent;
pub mod repeat;
pub mod run_row;
pub mod settings;

use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

use crate::cli::{Command, FzfCommand, HistoryCommand, SettingsCommand};
use crate::domain::catalog::{self, Sources};
use crate::domain::config::HerdrConfig;
use crate::domain::dialog;
use crate::domain::env::{HCP_STATE_ENV, PluginEnv};
use crate::domain::fzf_action::ActionError;
use crate::domain::herdr::Snapshot;
use crate::domain::history::Session;
use crate::domain::requests::{Confirmation, RequestError};
use crate::domain::retry::RetryPolicy;
use crate::domain::row::{Category, Row};
use crate::domain::settings::Settings;
use crate::domain::tags;
use crate::ports::{
    BranchLookup, Clock, ConfigError, ConfigSource, Files, FilesError, HerdrApi, HerdrError,
    HistoryError, HistoryStore, Picker, SettingsError, SettingsSource, SpawnError, Spawner,
    Terminal,
};

/// The program name: prefix of every diagnostic line (`main.rs` adds it to
/// returned errors).
pub const PROGRAM: &str = env!("CARGO_PKG_NAME");

/// Last line of a screen that stays open until a key press.
const CLOSE_HINT: &str = "press any key to close";

/// Exit status of a pane that received nothing to do (`exec` without a
/// command, `confirm` without a row).
const NOTHING_RECEIVED: u8 = 2;

/// Everything a use case may use: the environment and one adapter per port.
///
/// (No `Debug`: the ports are trait objects, which don't implement it.)
#[derive(Clone, Copy)]
pub struct Context<'a> {
    /// The plugin's environment, resolved once in `main`.
    pub env: &'a PluginEnv,
    /// This binary, for the fzf callbacks and the detached `run`.
    pub program: &'a Path,
    /// The working directory, shown when an `exec` command fails.
    pub cwd: Option<&'a Path>,
    /// The Herdr server.
    pub herdr: &'a dyn HerdrApi,
    /// Herdr's `config.toml`.
    pub config: &'a dyn ConfigSource,
    /// The plugin's `settings.toml`.
    pub settings: &'a dyn SettingsSource,
    /// fzf.
    pub picker: &'a dyn Picker,
    /// Child processes.
    pub spawner: &'a dyn Spawner,
    /// The popup's terminal.
    pub terminal: &'a dyn Terminal,
    /// Per-open state files.
    pub files: &'a dyn Files,
    /// The current time.
    pub clock: &'a dyn Clock,
    /// Pane focus and palette use (opened on first use).
    pub history: &'a dyn HistoryStore,
    /// The git branch of a directory (Jump to's footer).
    pub branches: &'a dyn BranchLookup,
}

/// Errors a use case returns to the binary, which prints them to stderr
/// and exits with status 1.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// Herdr couldn't be reached or refused a request.
    #[error(transparent)]
    Herdr(#[from] HerdrError),
    /// Herdr's `config.toml` couldn't be loaded.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The plugin's `settings.toml` couldn't be loaded or created.
    #[error(transparent)]
    Settings(#[from] SettingsError),
    /// A row's payload doesn't name something the palette can run.
    #[error(transparent)]
    Request(#[from] RequestError),
    /// A process (a custom `shell` command) couldn't be started.
    #[error(transparent)]
    Spawn(#[from] SpawnError),
    /// The palette's state files couldn't be used.
    #[error(transparent)]
    Files(#[from] FilesError),
    /// The history store couldn't be used.
    #[error(transparent)]
    History(#[from] HistoryError),
    /// An fzf callback's action can't carry its argument (the plugin's
    /// path has every closing delimiter fzf accepts).
    #[error(transparent)]
    FzfAction(#[from] ActionError),
    /// An fzf callback ran outside the palette (no palette state dir).
    #[error("not inside the palette: {HCP_STATE_ENV} is not set")]
    NoPaletteState,
    /// Writing the output failed (e.g. a closed pipe).
    #[error("cannot write output: {0}")]
    Output(String),
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Output(error.to_string())
    }
}

/// Runs one subcommand and returns the process exit status.
///
/// # Errors
///
/// Whatever the use case returns.
pub fn run(
    command: &Command,
    context: &Context<'_>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    match command {
        Command::Open => open::open(context, out, err),
        Command::Palette => palette::palette(context, out, err),
        Command::Fzf(FzfCommand::Rows { category }) => fzf_callbacks::rows(context, *category, out),
        Command::Fzf(FzfCommand::Switch { offset }) => fzf_callbacks::switch(context, *offset, out),
        Command::Fzf(FzfCommand::Resize) => fzf_callbacks::resize(context, out),
        Command::Fzf(FzfCommand::Footer { event, kind, lines }) => {
            // fzf's `{1}`: empty without a focused row.
            let kind = kind.as_deref().and_then(|kind| kind.parse().ok());
            let lines = tags::parse(lines.as_deref().unwrap_or_default());
            fzf_callbacks::footer(context, *event, kind, &lines, out)
        }
        Command::Run {
            kind,
            payload,
            confirmed,
        } => {
            let confirmation = if *confirmed {
                Confirmation::Confirmed
            } else {
                Confirmation::NotAsked
            };
            run_row::run_row(context, *kind, payload, confirmation, out, err)
        }
        Command::Ask {
            kind,
            payload,
            title,
        } => ask::ask(context, &Row::new(*kind, payload, title), out, err),
        Command::Confirm => confirm::confirm(context, out, err),
        Command::Exec => exec::exec(context, out),
        Command::List { ids } => list::list(context, *ids, out, err),
        Command::Settings(SettingsCommand::Check { .. }) => settings::check(context),
        Command::History(HistoryCommand::Focus) => Ok(history::focus(context, err)),
        Command::History(HistoryCommand::Move) => Ok(history::moved(context, err)),
        Command::History(HistoryCommand::Created) => Ok(history::created(context, err)),
        Command::RepeatCommand => repeat::repeat(context, Category::Commands, out, err),
        Command::RepeatJump => repeat::repeat(context, Category::JumpTo, out, err),
    }
}

/// The settings file `command` reads: `settings check --file <path>`'s
/// path, else the one Herdr's environment names. The edit settings flow
/// passes the file it just edited, so the check reads that file even if a
/// popup pane's environment differed.
#[must_use]
pub fn settings_path<'a>(command: &'a Command, env: &'a PluginEnv) -> &'a Path {
    match command {
        Command::Settings(SettingsCommand::Check { file: Some(file) }) => file,
        _ => &env.settings_path,
    }
}

/// How hard each use case retries a Herdr call answered with `ui_busy`.
///
/// - `run` waits up to 2 s (40 * 50 ms): it starts while the palette popup
///   is still closing, and its request may open another popup (Herdr
///   shows one popup at a time).
///   The repeat actions run a row too, so they wait the same, and so does
///   `ask`, which opens the `confirm` popup while the palette's closes.
/// - `open` retries briefly (10 * 50 ms): a popup that is just closing
///   shouldn't make the key press fail, but one that stays open shouldn't
///   keep the user waiting.
/// - Everything else never retries: reads (`session.snapshot`, plugin
///   actions) don't return `ui_busy`.
#[must_use]
pub fn retry_policy(command: &Command) -> RetryPolicy {
    match command {
        Command::Run { .. }
        | Command::Ask { .. }
        | Command::RepeatCommand
        | Command::RepeatJump => RetryPolicy::default(),
        Command::Open => RetryPolicy {
            attempts: 10,
            delay: Duration::from_millis(50),
        },
        _ => RetryPolicy::NONE,
    }
}

/// Shows `error` as a Herdr toast unless dry-running, for use cases the
/// user starts from a key binding and would otherwise see nothing of
/// it.
fn notify_failure(context: &Context<'_>, error: &Error) {
    notify(context, &error.to_string());
}

/// The settings, for a use case that can't draw an error screen: on
/// errors the defaults, with the error on `err` and as a Herdr toast
/// (unless dry-running). Each process loads its settings once, so the
/// toast shows once per run.
fn settings_or_default(context: &Context<'_>, err: &mut dyn Write) -> Settings {
    match context.settings.load() {
        Ok(settings) => settings,
        Err(error) => {
            tell(
                context,
                err,
                &format!("{error} (using the default settings)"),
            );
            Settings::default()
        }
    }
}

/// Shows `message` as a Herdr toast unless dry-running, and writes it to
/// `err`: for the repeat actions, which run from a key binding and have
/// nothing to show otherwise.
fn tell(context: &Context<'_>, err: &mut dyn Write, message: &str) {
    warn(err, message);
    notify(context, message);
}

/// Shows `message` as a Herdr toast unless dry-running. Best effort: if
/// Herdr is down, the notification fails too.
fn notify(context: &Context<'_>, message: &str) {
    if !context.env.dry_run {
        let _ = context.herdr.notify(message);
    }
}

/// The Herdr session this process runs for: history rows belong to it.
fn session(context: &Context<'_>) -> Session {
    Session::from_socket_path(&context.env.socket_path)
}

/// Every palette row, from fresh Herdr data, and the session snapshot they
/// were built from. When Herdr can't be asked for plugin actions or the
/// session, those rows are left out (and the snapshot is `None`) and a
/// warning goes to `err`.
fn catalog_rows(
    context: &Context<'_>,
    config: &HerdrConfig,
    err: &mut dyn Write,
) -> (Vec<Row>, Option<Snapshot>) {
    let actions = context
        .herdr
        .plugin_actions()
        .inspect_err(|error| warn(err, &format!("plugin actions unavailable: {error}")))
        .unwrap_or_default();
    let snapshot = snapshot_or_warn(context, err);
    let rows = catalog::rows(&Sources {
        plugin_id: &context.env.plugin_id,
        plugin_actions: &actions,
        config,
        snapshot: snapshot.as_ref(),
        home: context.env.home.as_deref(),
    });
    (rows, snapshot)
}

/// Whether `row` opens the `confirm` popup instead of running: listed in
/// `confirm_ids`, or a close that Herdr's `[ui] confirm_close` would ask
/// about (`dialog::is_required`, from `snapshot`). The palette and the
/// repeat actions share this rule.
fn must_confirm(
    context: &Context<'_>,
    settings: &Settings,
    config: &HerdrConfig,
    row: &Row,
    snapshot: Option<&Snapshot>,
) -> bool {
    settings.commands.confirms(row)
        || dialog::is_required(row, &context.env.context, snapshot, config.ui)
}

/// The session snapshot, or `None` with a warning on `err` when Herdr
/// can't be asked: for callers that have a fallback without it.
fn snapshot_or_warn(context: &Context<'_>, err: &mut dyn Write) -> Option<Snapshot> {
    context
        .herdr
        .snapshot()
        .inspect_err(|error| warn(err, &format!("session unavailable: {error}")))
        .ok()
}

/// Writes `<PROGRAM>: message` to `err`. A warning that can't be written
/// isn't worth failing for, so write errors are ignored.
fn warn(err: &mut dyn Write, message: &str) {
    let _ = writeln!(err, "{PROGRAM}: {message}");
}

/// Warns about a history store `error` on `err`, except
/// [`HistoryError::NoStateDir`] (a palette started by hand has no store,
/// which isn't worth a line). History is best effort in every use case.
fn warn_unless_no_store(err: &mut dyn Write, error: &HistoryError) {
    if *error != HistoryError::NoStateDir {
        warn(err, &format!("history unavailable: {error}"));
    }
}

/// In-memory fakes for every port, shared by the use case tests.
#[cfg(test)]
pub(crate) mod fakes;

#[cfg(test)]
mod tests {
    use crate::app::fakes::{FakeHistoryStore, World, invalid_settings};
    use crate::domain::row::Kind;

    use super::*;

    #[test]
    fn history_errors_pass_through_unchanged() {
        let error = Error::from(FakeHistoryStore::error("disk full"));

        assert_eq!(
            error.to_string(),
            "cannot write /state/history.db: disk full"
        );
    }

    #[test]
    fn only_run_ask_open_and_repeat_retry_ui_busy() {
        let run = Command::Run {
            kind: Kind::Pane,
            payload: "w1:p1".into(),
            confirmed: false,
        };
        let ask = Command::Ask {
            kind: Kind::Builtin,
            payload: "close_workspace".into(),
            title: "Close workspace".into(),
        };

        assert_eq!(retry_policy(&run), RetryPolicy::default());
        assert_eq!(retry_policy(&ask), RetryPolicy::default());
        assert_eq!(retry_policy(&Command::RepeatJump), RetryPolicy::default());
        assert_eq!(retry_policy(&Command::Open).attempts, 10);
        assert_eq!(retry_policy(&Command::Palette), RetryPolicy::NONE);
    }

    #[test]
    fn settings_check_reads_the_file_it_is_given() {
        let world = World::new();
        let check = Command::Settings(SettingsCommand::Check {
            file: Some("/edited/settings.toml".into()),
        });

        assert_eq!(
            settings_path(&check, &world.env),
            Path::new("/edited/settings.toml")
        );
    }

    #[test]
    fn settings_come_from_the_environment_without_a_file() {
        let world = World::new();
        let check = Command::Settings(SettingsCommand::Check { file: None });

        assert_eq!(settings_path(&check, &world.env), world.env.settings_path);
        assert_eq!(
            settings_path(&Command::Open, &world.env),
            world.env.settings_path
        );
    }

    #[test]
    fn io_errors_become_output_errors() {
        let error = Error::from(io::Error::new(io::ErrorKind::BrokenPipe, "closed"));

        assert_eq!(error, Error::Output("closed".into()));
    }

    #[test]
    fn invalid_settings_fall_back_to_defaults_with_a_toast() {
        let world = World::new();
        world.settings.set(Err(invalid_settings()));
        let mut err = Vec::new();

        let settings = settings_or_default(&world.context(), &mut err);

        assert_eq!(settings, Settings::default());
        let message = "invalid `popup.min_width` (line 2) in /cfg/settings.toml: \
                       100 is larger than `popup.max_width` (76) (using the default settings)";
        assert_eq!(
            String::from_utf8(err).unwrap(),
            format!("herdr-command-palette: {message}\n")
        );
        assert_eq!(
            world.herdr.last("notification.show").unwrap().params["body"],
            message
        );
    }

    #[test]
    fn invalid_settings_are_not_notified_in_a_dry_run() {
        let mut world = World::new();
        world.env.dry_run = true;
        world.settings.set(Err(invalid_settings()));

        let settings = settings_or_default(&world.context(), &mut Vec::new());

        assert_eq!(settings, Settings::default());
        assert_eq!(world.herdr.methods(), Vec::<String>::new());
    }

    #[test]
    fn valid_settings_are_used_as_they_are() {
        let world = World::new();
        let settings = Settings {
            editor: "nano".into(),
            ..Settings::default()
        };
        world.settings.set(Ok(settings.clone()));

        assert_eq!(
            settings_or_default(&world.context(), &mut Vec::new()),
            settings
        );
    }

    #[test]
    fn no_palette_state_names_the_variable() {
        let message = Error::NoPaletteState.to_string();

        assert_eq!(message, "not inside the palette: HCP_STATE is not set");
    }
}
