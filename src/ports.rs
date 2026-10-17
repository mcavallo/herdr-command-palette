//! Traits the use cases depend on: Herdr API, Herdr's config, the
//! plugin's settings, picker (fzf), processes, the terminal, small state
//! files, the clock, the history store ([`history`]) and git's branch.
//! Adapters implement them; tests use fakes.
//!
//! Each trait is the smallest surface the use cases need. Errors carry
//! strings rather than `std::io::Error` so they can be compared in tests
//! and cloned into fakes.

pub mod history;

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::domain::appearance::Appearance;
use crate::domain::config::HerdrConfig;
use crate::domain::git::Head;
use crate::domain::herdr::{PluginAction, PluginActionsResult, Request, Snapshot, SnapshotResult};
use crate::domain::settings::{Settings, SettingsProblem};
use crate::domain::style::PickerColors;

pub use history::{HistoryError, HistoryStore, Prune};

/// Talks to the running Herdr server.
///
/// Implementors only write [`call`](HerdrApi::call); the typed helpers are
/// built on it.
pub trait HerdrApi {
    /// Sends one request and returns the response's `result` object.
    ///
    /// # Errors
    ///
    /// [`HerdrError::Api`] when Herdr answers with an error body,
    /// [`HerdrError::Transport`] when it can't be reached,
    /// [`HerdrError::Decode`] when the answer isn't valid JSON.
    fn call(&self, request: &Request) -> Result<Value, HerdrError>;

    /// The live session (`session.snapshot`).
    ///
    /// # Errors
    ///
    /// As [`call`](HerdrApi::call), plus [`HerdrError::Decode`] when the
    /// snapshot doesn't have the expected shape.
    fn snapshot(&self) -> Result<Snapshot, HerdrError> {
        let result: SnapshotResult = decode(self.call(&Request::snapshot())?)?;
        Ok(result.snapshot)
    }

    /// Every action of every enabled plugin (`plugin.action.list`).
    ///
    /// # Errors
    ///
    /// As [`snapshot`](HerdrApi::snapshot).
    fn plugin_actions(&self) -> Result<Vec<PluginAction>, HerdrError> {
        let result: PluginActionsResult = decode(self.call(&Request::plugin_actions())?)?;
        Ok(result.actions)
    }

    /// Shows `body` as a Herdr toast ([`Request::notification`]), for
    /// errors the user would otherwise not see. Callers usually
    /// ignore the result.
    ///
    /// # Errors
    ///
    /// As [`call`](HerdrApi::call).
    fn notify(&self, body: &str) -> Result<(), HerdrError> {
        self.call(&Request::notification(body))?;
        Ok(())
    }
}

/// Deserializes a `result` object into its typed shape.
fn decode<T: DeserializeOwned>(result: Value) -> Result<T, HerdrError> {
    serde_json::from_value(result).map_err(|error| HerdrError::Decode(error.to_string()))
}

/// Why a Herdr call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HerdrError {
    /// Herdr answered with an error body (`{"error": {"code", "message"}}`).
    #[error("{code}: {message}")]
    Api {
        /// Machine-readable code, e.g. `ui_busy`, `pane_not_found`.
        code: String,
        /// Human-readable message.
        message: String,
    },
    /// The socket couldn't be reached, written or read.
    #[error("cannot talk to herdr: {0}")]
    Transport(String),
    /// The answer wasn't the JSON we expected.
    #[error("unexpected answer from herdr: {0}")]
    Decode(String),
}

impl HerdrError {
    /// The error code, for [`HerdrError::Api`] errors.
    #[must_use]
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Api { code, .. } => Some(code),
            Self::Transport(_) | Self::Decode(_) => None,
        }
    }

    /// Herdr is busy with UI (e.g. our popup is still closing); worth a retry.
    #[must_use]
    pub fn is_ui_busy(&self) -> bool {
        self.code() == Some("ui_busy")
    }
}

/// Loads Herdr's `config.toml`.
pub trait ConfigSource {
    /// The parsed config; a missing file is [`HerdrConfig::default()`].
    ///
    /// # Errors
    ///
    /// [`ConfigError`] when the file exists but can't be read or parsed.
    fn load(&self) -> Result<HerdrConfig, ConfigError>;
}

/// Why Herdr's config couldn't be loaded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// The file exists but couldn't be read.
    #[error("cannot read {}: {message}", path.display())]
    Read {
        /// The config file.
        path: PathBuf,
        /// The I/O error, as text.
        message: String,
    },
    /// The file isn't valid TOML, or has a wrong type in `[[keys.command]]`
    /// `type`/`command`/`description`/`width`/`height` or in `[theme]`.
    #[error("invalid {}: {message}", path.display())]
    Parse {
        /// The config file.
        path: PathBuf,
        /// The TOML error, as text (includes line and column).
        message: String,
    },
}

/// The plugin's `settings.toml`. Read on every use, never cached.
pub trait SettingsSource {
    /// The parsed settings; a missing file is [`Settings::default()`].
    ///
    /// # Errors
    ///
    /// [`SettingsError::Read`] when the file exists but can't be read,
    /// [`SettingsError::Invalid`] when its contents aren't valid settings.
    fn load(&self) -> Result<Settings, SettingsError>;

    /// Writes `contents` as the file, and its directory, unless the file
    /// exists. Returns whether it wrote.
    ///
    /// # Errors
    ///
    /// [`SettingsError::Write`] when the file can't be created.
    fn create(&self, contents: &str) -> Result<bool, SettingsError>;
}

/// Why the settings couldn't be loaded or created.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    /// The file exists but couldn't be read.
    #[error("cannot read {}: {message}", path.display())]
    Read {
        /// The settings file.
        path: PathBuf,
        /// The I/O error, as text.
        message: String,
    },
    /// The file isn't valid settings: what and where.
    #[error("invalid {} in {}: {}", problem.location(), path.display(), problem.message)]
    Invalid {
        /// The settings file.
        path: PathBuf,
        /// What is wrong, at which key and line.
        problem: SettingsProblem,
    },
    /// The file (or its directory) couldn't be created.
    #[error("cannot write {}: {message}", path.display())]
    Write {
        /// The settings file.
        path: PathBuf,
        /// The I/O error, as text.
        message: String,
    },
}

/// Runs the interactive picker (fzf): the palette's list, header, footer
/// and key bindings that call back into this binary.
pub trait Picker {
    /// The installed picker's version line (e.g. `0.66.1 (brew)`), `None`
    /// when it is not installed.
    ///
    /// # Errors
    ///
    /// [`PickerError::Start`] when it exists but can't be run.
    fn version(&self) -> Result<Option<String>, PickerError>;

    /// Shows `request.input` and waits for the user.
    ///
    /// # Errors
    ///
    /// [`PickerError::Start`] when the picker can't start,
    /// [`PickerError::Status`] when it fails.
    fn pick(&self, request: &PickRequest) -> Result<Pick, PickerError>;
}

/// What the picker shows and how it calls back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickRequest {
    /// Lines to choose from (the default category's rows), newline separated.
    pub input: String,
    /// Header line (categories).
    pub header: String,
    /// Initial footer line.
    pub footer: String,
    /// fzf's colors: the settings over Herdr's theme.
    pub colors: PickerColors,
    /// Extra environment variables for the picker and its callbacks.
    pub env: Vec<(String, String)>,
}

/// How the picker ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// The user chose this line.
    Selected(String),
    /// Esc, or Enter with no match.
    Cancelled,
}

/// Why the picker failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PickerError {
    /// It couldn't be started (or its version read).
    #[error("cannot run fzf: {0}")]
    Start(String),
    /// It exited with an unexpected status (not 0, 1 or 130).
    #[error("fzf exited with status {0}")]
    Status(i32),
}

/// Starts processes: detached ones that outlive this one, and shell
/// commands in the foreground.
pub trait Spawner {
    /// Starts `command` in its own process group and returns without
    /// waiting.
    ///
    /// # Errors
    ///
    /// [`SpawnError`] when the process can't be started.
    fn spawn_detached(&self, command: &Detached) -> Result<(), SpawnError>;

    /// Starts `<shell> -c command` detached, in `cwd` when given (inherited
    /// otherwise), output discarded: custom `shell` commands. The
    /// shell is the same one [`run_shell`](Spawner::run_shell) uses.
    ///
    /// # Errors
    ///
    /// [`SpawnError`] when the shell can't be started.
    fn spawn_shell_detached(&self, command: &str, cwd: Option<&Path>) -> Result<(), SpawnError>;

    /// Runs `<shell> -c command` (`sh` unless the adapter is configured
    /// otherwise) with this process's terminal and waits for it.
    /// Returns its exit status (`128 + n` when killed by signal `n`).
    ///
    /// # Errors
    ///
    /// [`SpawnError`] when `sh` can't be started.
    fn run_shell(&self, command: &str) -> Result<i32, SpawnError>;
}

/// A process to start detached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detached {
    /// Executable.
    pub program: PathBuf,
    /// Arguments.
    pub args: Vec<String>,
    /// Working directory; inherited when `None`.
    pub cwd: Option<PathBuf>,
    /// File that receives stdout and stderr (appended); discarded when `None`.
    pub log: Option<PathBuf>,
}

/// Why a process couldn't be started.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("cannot start {}: {message}", program.display())]
pub struct SpawnError {
    /// The executable.
    pub program: PathBuf,
    /// The I/O error, as text.
    pub message: String,
}

/// The terminal the popup runs in.
pub trait Terminal {
    /// Its width in columns, if it can be measured.
    fn columns(&self) -> Option<usize>;

    /// Waits for one key press, without echo, and returns its first byte
    /// (Enter is `\r` or `\n`, Esc `\x1b`). Returns `None` at once when
    /// there is no terminal to read from.
    fn read_key(&self) -> Option<u8>;

    /// The terminal's light/dark appearance, as Herdr's client decides it:
    /// asks with [`appearance::QUERY`](crate::domain::appearance::QUERY)
    /// and reads the replies with
    /// [`appearance::from_replies`](crate::domain::appearance::from_replies).
    /// Returns within a short timeout; `None` when there is no terminal or
    /// it doesn't say.
    fn appearance(&self) -> Option<Appearance>;

    /// Waits for one key press and ignores it ("press any key").
    fn wait_for_key(&self) {
        let _ = self.read_key();
    }
}

/// Small text files: the palette's per-open state, shared with the fzf
/// callbacks (current category, footer mode, rows).
pub trait Files {
    /// Creates a new private temporary directory.
    ///
    /// # Errors
    ///
    /// [`FilesError`] when it can't be created.
    fn create_temp_dir(&self) -> Result<PathBuf, FilesError>;

    /// The file's contents; `None` when it doesn't exist.
    ///
    /// # Errors
    ///
    /// [`FilesError`] when it exists but can't be read.
    fn read(&self, path: &Path) -> Result<Option<String>, FilesError>;

    /// Replaces the file's contents.
    ///
    /// # Errors
    ///
    /// [`FilesError`] when it can't be written.
    fn write(&self, path: &Path, contents: &str) -> Result<(), FilesError>;

    /// Removes a directory and everything in it; failures are ignored
    /// (it's a temporary directory).
    fn remove_dir(&self, path: &Path);

    /// The other palette temp dirs next to `dir` (made by
    /// [`create_temp_dir`](Files::create_temp_dir)): same parent, a
    /// [`palette_state::is_name`](crate::domain::palette_state::is_name) name,
    /// real directories (not symlinks) with the same owner as `dir`.
    /// Entries that can't be inspected are skipped; an unreadable parent
    /// gives an empty list (the sweep is best effort).
    fn sibling_temp_dirs(&self, dir: &Path) -> Vec<TempDirEntry>;
}

/// A palette temp dir found by [`Files::sibling_temp_dirs`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TempDirEntry {
    /// The directory.
    pub path: PathBuf,
    /// When it was last modified.
    pub modified: SystemTime,
}

/// The current time, behind a port so tests can fix it.
pub trait Clock {
    /// Now.
    fn now(&self) -> SystemTime;
}

/// What is checked out in a directory: Jump to's footer shows it before
/// the cwd.
pub trait BranchLookup {
    /// The branch (or detached commit) checked out in `cwd`. `None` when
    /// git is missing or older than 2.22, `cwd` isn't in a repository, or
    /// git fails or takes too long: the branch is optional, never an
    /// error.
    fn head(&self, cwd: &Path) -> Option<Head>;
}

/// Why a file operation failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FilesError {
    /// A temporary directory couldn't be created.
    #[error("cannot create {}: {message}", path.display())]
    Create {
        /// The directory (or its parent, when no free name was found).
        path: PathBuf,
        /// The I/O error, as text.
        message: String,
    },
    /// A file exists but couldn't be read.
    #[error("cannot read {}: {message}", path.display())]
    Read {
        /// The file.
        path: PathBuf,
        /// The I/O error, as text.
        message: String,
    },
    /// A file couldn't be written.
    #[error("cannot write {}: {message}", path.display())]
    Write {
        /// The file.
        path: PathBuf,
        /// The I/O error, as text.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::app::fakes::{FakeHerdr, api_error, parse_error, spawn_error};

    use super::*;

    #[test]
    fn snapshot_decodes_the_result() {
        let herdr = FakeHerdr::default();
        herdr.reply(
            "session.snapshot",
            Ok(json!({"type": "session_snapshot", "snapshot": {
                "workspaces": [], "tabs": [], "panes": [], "focused_pane_id": "w1:p1"
            }})),
        );

        let snapshot = herdr.snapshot().unwrap();

        assert_eq!(snapshot.focused_pane_id.as_deref(), Some("w1:p1"));
    }

    #[test]
    fn plugin_actions_decode_the_result() {
        let herdr = FakeHerdr::default();
        herdr.reply(
            "plugin.action.list",
            Ok(json!({"actions": [
                {"plugin_id": "demo", "action_id": "hello", "title": "Say hello", "command": []}
            ]})),
        );

        let actions = herdr.plugin_actions().unwrap();

        assert_eq!(actions[0].qualified_id(), "demo.hello");
    }

    #[test]
    fn a_missing_field_is_a_decode_error() {
        let herdr = FakeHerdr::default();

        let error = herdr.snapshot().unwrap_err();

        assert_eq!(error, HerdrError::Decode("missing field `snapshot`".into()));
    }

    #[test]
    fn a_wrong_type_is_a_decode_error() {
        let herdr = FakeHerdr::default();
        herdr.reply("plugin.action.list", Ok(json!({"actions": 3})));

        let error = herdr.plugin_actions().unwrap_err();

        assert!(matches!(error, HerdrError::Decode(ref m) if m.starts_with("invalid type: ")));
    }

    #[test]
    fn api_errors_pass_through_the_typed_helpers() {
        let herdr = FakeHerdr::default();
        herdr.reply("session.snapshot", Err(api_error("ui_busy", "closing")));

        let result = herdr.snapshot();

        assert_eq!(result, Err(api_error("ui_busy", "closing")));
    }

    #[test]
    fn ui_busy_is_an_api_error_code() {
        let busy = api_error("ui_busy", "a popup is closing");

        assert!(busy.is_ui_busy());
        assert_eq!(busy.code(), Some("ui_busy"));
        assert_eq!(busy.to_string(), "ui_busy: a popup is closing");
    }

    #[test]
    fn transport_errors_have_no_code() {
        let down = HerdrError::Transport("connection refused".into());

        assert!(!down.is_ui_busy());
        assert_eq!(down.code(), None);
        assert_eq!(down.to_string(), "cannot talk to herdr: connection refused");
    }

    #[test]
    fn decode_errors_say_the_answer_was_unexpected() {
        let message = HerdrError::Decode("x".into()).to_string();

        assert_eq!(message, "unexpected answer from herdr: x");
    }

    #[test]
    fn config_errors_name_the_file() {
        let read = ConfigError::Read {
            path: "/c.toml".into(),
            message: "denied".into(),
        };

        assert_eq!(read.to_string(), "cannot read /c.toml: denied");
        assert_eq!(parse_error().to_string(), "invalid /c.toml: line 1");
    }

    #[test]
    fn settings_errors_name_the_file_key_and_line() {
        let invalid = SettingsError::Invalid {
            path: "/s.toml".into(),
            problem: SettingsProblem {
                key: Some("popup.min_width".into()),
                line: Some(3),
                message: "100 is larger than `popup.max_width` (76)".into(),
            },
        };
        let read = SettingsError::Read {
            path: "/s.toml".into(),
            message: "denied".into(),
        };
        let write = SettingsError::Write {
            path: "/s.toml".into(),
            message: "read-only".into(),
        };

        assert_eq!(
            invalid.to_string(),
            "invalid `popup.min_width` (line 3) in /s.toml: 100 is larger than `popup.max_width` (76)"
        );
        assert_eq!(read.to_string(), "cannot read /s.toml: denied");
        assert_eq!(write.to_string(), "cannot write /s.toml: read-only");
    }

    #[test]
    fn picker_errors_name_fzf() {
        assert_eq!(
            PickerError::Start("gone".into()).to_string(),
            "cannot run fzf: gone"
        );
        assert_eq!(
            PickerError::Status(2).to_string(),
            "fzf exited with status 2"
        );
    }

    #[test]
    fn files_errors_name_the_path() {
        let files = FilesError::Write {
            path: "/t/x".into(),
            message: "denied".into(),
        };

        assert_eq!(files.to_string(), "cannot write /t/x: denied");
    }

    #[test]
    fn spawn_errors_name_the_program() {
        assert_eq!(spawn_error("sh").to_string(), "cannot start sh: not found");
    }
}
