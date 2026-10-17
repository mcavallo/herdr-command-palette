//! In-memory fakes for every port (`Fake<Port>`), a [`World`] bundling
//! them for the use case tests, and the port errors those tests share.
//!
//! Each fake has canned answers, set through methods, and records its
//! calls.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

use crate::cli::Command;
use crate::domain::appearance::Appearance;
use crate::domain::config::HerdrConfig;
use crate::domain::env::PluginEnv;
use crate::domain::fixtures::herdr_json;
use crate::domain::git::Head;
use crate::domain::herdr::Request;
use crate::domain::history::Session;
use crate::domain::row::{Category, RowId};
use crate::domain::settings::{Settings, SettingsProblem};
use crate::ports::{
    BranchLookup, Clock, ConfigError, ConfigSource, Detached, Files, FilesError, HerdrApi,
    HerdrError, HistoryError, HistoryStore, Pick, PickRequest, Picker, PickerError, Prune,
    SettingsError, SettingsSource, SpawnError, Spawner, TempDirEntry, Terminal,
};

use super::{Context, Error, run};

/// A Herdr API error answer (`code: message`).
pub(crate) fn api_error(code: &str, message: &str) -> HerdrError {
    HerdrError::Api {
        code: code.to_owned(),
        message: message.to_owned(),
    }
}

/// A `config.toml` that isn't valid TOML: `invalid /c.toml: line 1`.
pub(crate) fn parse_error() -> ConfigError {
    ConfigError::Parse {
        path: PathBuf::from("/c.toml"),
        message: "line 1".to_owned(),
    }
}

/// Settings with a popup min width above the max:
/// ``invalid `popup.min_width` (line 2) in /cfg/settings.toml: 100 is larger than `popup.max_width` (76)``.
pub(crate) fn invalid_settings() -> SettingsError {
    SettingsError::Invalid {
        path: PathBuf::from(SETTINGS_PATH),
        problem: SettingsProblem {
            key: Some("popup.min_width".to_owned()),
            line: Some(2),
            message: "100 is larger than `popup.max_width` (76)".to_owned(),
        },
    }
}

/// Where [`FakeSettings`] says the settings file is.
pub(crate) const SETTINGS_PATH: &str = "/cfg/settings.toml";

/// `program` couldn't be started: `cannot start <program>: not found`.
pub(crate) fn spawn_error(program: &str) -> SpawnError {
    SpawnError {
        program: PathBuf::from(program),
        message: "not found".to_owned(),
    }
}

/// Herdr answering per method; unknown methods answer `{}`.
#[derive(Debug, Default)]
pub(crate) struct FakeHerdr {
    replies: RefCell<HashMap<String, Result<Value, HerdrError>>>,
    calls: RefCell<Vec<Request>>,
}

impl FakeHerdr {
    /// Answers `method` with `reply` from now on.
    pub fn reply(&self, method: &str, reply: Result<Value, HerdrError>) {
        self.replies.borrow_mut().insert(method.to_owned(), reply);
    }

    /// The methods called, in order.
    pub fn methods(&self) -> Vec<String> {
        self.calls
            .borrow()
            .iter()
            .map(|request| request.method.clone())
            .collect()
    }

    /// The bodies of the notifications shown, in order.
    pub fn toasts(&self) -> Vec<String> {
        self.calls
            .borrow()
            .iter()
            .filter(|request| request.method == "notification.show")
            .map(|request| {
                request.params["body"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned()
            })
            .collect()
    }

    /// The last request sent with `method`.
    pub fn last(&self, method: &str) -> Option<Request> {
        self.calls
            .borrow()
            .iter()
            .rev()
            .find(|request| request.method == method)
            .cloned()
    }
}

impl HerdrApi for FakeHerdr {
    fn call(&self, request: &Request) -> Result<Value, HerdrError> {
        self.calls.borrow_mut().push(request.clone());
        self.replies
            .borrow()
            .get(&request.method)
            .cloned()
            .unwrap_or_else(|| Ok(json!({})))
    }
}

/// Herdr's config, or an error; empty by default.
#[derive(Debug)]
pub(crate) struct FakeConfig {
    load: RefCell<Result<HerdrConfig, ConfigError>>,
}

impl FakeConfig {
    /// Answers [`ConfigSource::load`] with `load` from now on.
    pub fn set(&self, load: Result<HerdrConfig, ConfigError>) {
        *self.load.borrow_mut() = load;
    }
}

impl Default for FakeConfig {
    fn default() -> Self {
        Self {
            load: RefCell::new(Ok(HerdrConfig::default())),
        }
    }
}

impl ConfigSource for FakeConfig {
    fn load(&self) -> Result<HerdrConfig, ConfigError> {
        self.load.borrow().clone()
    }
}

/// The plugin's settings, or an error; defaults by default. Records the
/// contents of every file it is asked to create.
#[derive(Debug)]
pub(crate) struct FakeSettings {
    load: RefCell<Result<Settings, SettingsError>>,
    create: RefCell<Result<bool, SettingsError>>,
    created: RefCell<Vec<String>>,
}

impl FakeSettings {
    /// Answers [`SettingsSource::load`] with `load` from now on.
    pub fn set(&self, load: Result<Settings, SettingsError>) {
        *self.load.borrow_mut() = load;
    }

    /// Answers [`SettingsSource::create`] with `create` from now on.
    pub fn set_create(&self, create: Result<bool, SettingsError>) {
        *self.create.borrow_mut() = create;
    }

    /// The contents of every create call, in order.
    pub fn created(&self) -> Vec<String> {
        self.created.borrow().clone()
    }
}

impl Default for FakeSettings {
    fn default() -> Self {
        Self {
            load: RefCell::new(Ok(Settings::default())),
            create: RefCell::new(Ok(true)),
            created: RefCell::new(Vec::new()),
        }
    }
}

impl SettingsSource for FakeSettings {
    fn load(&self) -> Result<Settings, SettingsError> {
        self.load.borrow().clone()
    }

    fn create(&self, contents: &str) -> Result<bool, SettingsError> {
        self.created.borrow_mut().push(contents.to_owned());
        self.create.borrow().clone()
    }
}

/// fzf with a canned version and pick; records each pick request. By
/// default fzf 0.66.0, cancelled.
#[derive(Debug)]
pub(crate) struct FakePicker {
    version: RefCell<Result<Option<String>, PickerError>>,
    pick: RefCell<Result<Pick, PickerError>>,
    requests: RefCell<Vec<PickRequest>>,
}

impl FakePicker {
    /// Answers [`Picker::version`] with `version` from now on.
    pub fn set_version(&self, version: Result<Option<String>, PickerError>) {
        *self.version.borrow_mut() = version;
    }

    /// Answers [`Picker::pick`] with `pick` from now on.
    pub fn set_pick(&self, pick: Result<Pick, PickerError>) {
        *self.pick.borrow_mut() = pick;
    }

    /// Every pick request, in order.
    pub fn requests(&self) -> Vec<PickRequest> {
        self.requests.borrow().clone()
    }
}

impl Default for FakePicker {
    fn default() -> Self {
        Self {
            version: RefCell::new(Ok(Some("0.66.0 (devel)".to_owned()))),
            pick: RefCell::new(Ok(Pick::Cancelled)),
            requests: RefCell::new(Vec::new()),
        }
    }
}

impl Picker for FakePicker {
    fn version(&self) -> Result<Option<String>, PickerError> {
        self.version.borrow().clone()
    }

    fn pick(&self, request: &PickRequest) -> Result<Pick, PickerError> {
        self.requests.borrow_mut().push(request.clone());
        self.pick.borrow().clone()
    }
}

/// Records started processes. Detached starts succeed and shell commands
/// exit 0 unless set otherwise.
#[derive(Debug)]
pub(crate) struct FakeSpawner {
    /// Outcome of both kinds of detached start.
    detach: RefCell<Result<(), SpawnError>>,
    run_shell: RefCell<Result<i32, SpawnError>>,
    detached: RefCell<Vec<Detached>>,
    shell_detached: RefCell<Vec<(String, Option<PathBuf>)>>,
    shell: RefCell<Vec<String>>,
}

impl FakeSpawner {
    /// Answers [`Spawner::spawn_detached`] and
    /// [`Spawner::spawn_shell_detached`] with `detach` from now on.
    pub fn set_detach(&self, detach: Result<(), SpawnError>) {
        *self.detach.borrow_mut() = detach;
    }

    /// Answers [`Spawner::run_shell`] with `run_shell` from now on.
    pub fn set_run_shell(&self, run_shell: Result<i32, SpawnError>) {
        *self.run_shell.borrow_mut() = run_shell;
    }

    /// Every detached process started, in order.
    pub fn detached(&self) -> Vec<Detached> {
        self.detached.borrow().clone()
    }

    /// Every detached shell command started, as (command, cwd), in order.
    pub fn shell_detached(&self) -> Vec<(String, Option<PathBuf>)> {
        self.shell_detached.borrow().clone()
    }

    /// Every shell command run in the foreground, in order.
    pub fn shell(&self) -> Vec<String> {
        self.shell.borrow().clone()
    }
}

impl Default for FakeSpawner {
    fn default() -> Self {
        Self {
            detach: RefCell::new(Ok(())),
            run_shell: RefCell::new(Ok(0)),
            detached: RefCell::new(Vec::new()),
            shell_detached: RefCell::new(Vec::new()),
            shell: RefCell::new(Vec::new()),
        }
    }
}

impl Spawner for FakeSpawner {
    fn spawn_detached(&self, command: &Detached) -> Result<(), SpawnError> {
        self.detached.borrow_mut().push(command.clone());
        self.detach.borrow().clone()
    }

    fn spawn_shell_detached(&self, command: &str, cwd: Option<&Path>) -> Result<(), SpawnError> {
        self.shell_detached
            .borrow_mut()
            .push((command.to_owned(), cwd.map(Path::to_owned)));
        self.detach.borrow().clone()
    }

    fn run_shell(&self, command: &str) -> Result<i32, SpawnError> {
        self.shell.borrow_mut().push(command.to_owned());
        self.run_shell.borrow().clone()
    }
}

/// A terminal 76 columns wide that counts key reads and appearance
/// queries; every key is Esc and the appearance unknown unless set
/// otherwise.
#[derive(Debug)]
pub(crate) struct FakeTerminal {
    columns: RefCell<Option<usize>>,
    key: RefCell<Option<u8>>,
    key_waits: RefCell<usize>,
    appearance: RefCell<Option<Appearance>>,
    appearance_queries: RefCell<usize>,
}

impl FakeTerminal {
    /// Answers [`Terminal::columns`] with `columns` from now on.
    pub fn set_columns(&self, columns: Option<usize>) {
        *self.columns.borrow_mut() = columns;
    }

    /// Answers [`Terminal::appearance`] with `appearance` from now on.
    pub fn set_appearance(&self, appearance: Option<Appearance>) {
        *self.appearance.borrow_mut() = appearance;
    }

    /// How many times the appearance was asked for.
    pub fn appearance_queries(&self) -> usize {
        *self.appearance_queries.borrow()
    }

    /// Answers [`Terminal::read_key`] with `key` from now on.
    pub fn set_key(&self, key: Option<u8>) {
        *self.key.borrow_mut() = key;
    }

    /// How many keys were read (or waited for).
    pub fn key_waits(&self) -> usize {
        *self.key_waits.borrow()
    }
}

impl Default for FakeTerminal {
    fn default() -> Self {
        Self {
            columns: RefCell::new(Some(76)),
            key: RefCell::new(Some(0x1b)),
            key_waits: RefCell::new(0),
            appearance: RefCell::new(None),
            appearance_queries: RefCell::new(0),
        }
    }
}

impl Terminal for FakeTerminal {
    fn columns(&self) -> Option<usize> {
        *self.columns.borrow()
    }

    fn read_key(&self) -> Option<u8> {
        *self.key_waits.borrow_mut() += 1;
        *self.key.borrow()
    }

    fn appearance(&self) -> Option<Appearance> {
        *self.appearance_queries.borrow_mut() += 1;
        *self.appearance.borrow()
    }
}

/// Files in a map; temp dirs are `/tmp/state-<n>`.
#[derive(Debug, Default)]
pub(crate) struct FakeFiles {
    files: RefCell<HashMap<PathBuf, String>>,
    dirs_created: RefCell<usize>,
    removed: RefCell<Vec<PathBuf>>,
    /// What [`Files::sibling_temp_dirs`] finds (leftovers of other palettes).
    siblings: RefCell<Vec<TempDirEntry>>,
    /// Create, read and write fail with this message when set.
    failure: RefCell<Option<String>>,
    /// Only write fails with this message when set.
    write_failure: RefCell<Option<String>>,
}

impl FakeFiles {
    /// Makes every later create, read and write fail with `message`.
    pub fn fail_with(&self, message: &str) {
        *self.failure.borrow_mut() = Some(message.to_owned());
    }

    /// Makes every later write (only) fail with `message`.
    pub fn fail_writes_with(&self, message: &str) {
        *self.write_failure.borrow_mut() = Some(message.to_owned());
    }

    /// Answers [`Files::sibling_temp_dirs`] with `siblings` from now on.
    pub fn set_siblings(&self, siblings: Vec<TempDirEntry>) {
        *self.siblings.borrow_mut() = siblings;
    }

    /// The dirs removed, in order.
    pub fn removed(&self) -> Vec<PathBuf> {
        self.removed.borrow().clone()
    }

    /// The contents of `path`, if written.
    pub fn get(&self, path: &str) -> Option<String> {
        self.files.borrow().get(Path::new(path)).cloned()
    }

    /// Writes `path` directly (test setup).
    pub fn put(&self, path: &str, contents: &str) {
        self.files
            .borrow_mut()
            .insert(PathBuf::from(path), contents.to_owned());
    }

    /// `Err(variant(path, message))` when a failure is set.
    fn check(
        &self,
        path: &Path,
        variant: fn(PathBuf, String) -> FilesError,
    ) -> Result<(), FilesError> {
        match self.failure.borrow().clone() {
            Some(message) => Err(variant(path.to_owned(), message)),
            None => Ok(()),
        }
    }
}

impl Files for FakeFiles {
    fn create_temp_dir(&self) -> Result<PathBuf, FilesError> {
        let mut count = self.dirs_created.borrow_mut();
        *count += 1;
        let dir = PathBuf::from(format!("/tmp/state-{count}"));
        self.check(&dir, |path, message| FilesError::Create { path, message })?;
        Ok(dir)
    }

    fn read(&self, path: &Path) -> Result<Option<String>, FilesError> {
        self.check(path, |path, message| FilesError::Read { path, message })?;
        Ok(self.files.borrow().get(path).cloned())
    }

    fn write(&self, path: &Path, contents: &str) -> Result<(), FilesError> {
        self.check(path, |path, message| FilesError::Write { path, message })?;
        if let Some(message) = self.write_failure.borrow().clone() {
            return Err(FilesError::Write {
                path: path.to_owned(),
                message,
            });
        }
        self.files
            .borrow_mut()
            .insert(path.to_owned(), contents.to_owned());
        Ok(())
    }

    fn remove_dir(&self, path: &Path) {
        self.removed.borrow_mut().push(path.to_owned());
        self.files
            .borrow_mut()
            .retain(|file, _| !file.starts_with(path));
    }

    fn sibling_temp_dirs(&self, _dir: &Path) -> Vec<TempDirEntry> {
        self.siblings.borrow().clone()
    }
}

/// A clock stopped 30 days after the epoch.
#[derive(Debug)]
pub(crate) struct FakeClock {
    now: SystemTime,
}

impl Default for FakeClock {
    fn default() -> Self {
        Self {
            now: SystemTime::UNIX_EPOCH + Duration::from_hours(30 * 24),
        }
    }
}

impl Clock for FakeClock {
    fn now(&self) -> SystemTime {
        self.now
    }
}

/// Git answering per directory; no repository anywhere by default.
#[derive(Debug, Default)]
pub(crate) struct FakeBranches {
    heads: RefCell<HashMap<PathBuf, Head>>,
    lookups: RefCell<Vec<PathBuf>>,
}

impl FakeBranches {
    /// Answers `cwd` with `head` from now on.
    pub fn set(&self, cwd: &str, head: Head) {
        self.heads.borrow_mut().insert(PathBuf::from(cwd), head);
    }

    /// The directories looked up, in order.
    pub fn lookups(&self) -> Vec<PathBuf> {
        self.lookups.borrow().clone()
    }
}

impl BranchLookup for FakeBranches {
    fn head(&self, cwd: &Path) -> Option<Head> {
        self.lookups.borrow_mut().push(cwd.to_owned());
        self.heads.borrow().get(cwd).cloned()
    }
}

/// One write to [`FakeHistoryStore`], with the session as a string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HistoryWrite {
    Focus {
        session: String,
        pane: String,
        at: SystemTime,
    },
    Rename {
        session: String,
        from: String,
        to: String,
    },
    Usage {
        session: String,
        row: RowId,
        at: SystemTime,
    },
    Forget {
        session: String,
        pane: String,
        before: SystemTime,
    },
}

/// A history store with canned reads that records writes and prunes.
/// Empty by default; never pruned.
#[derive(Debug, Default)]
pub(crate) struct FakeHistoryStore {
    focus_order: RefCell<Vec<String>>,
    usage_order: RefCell<Vec<RowId>>,
    last_pruned_at: RefCell<Option<SystemTime>>,
    writes: RefCell<Vec<HistoryWrite>>,
    /// (live panes, max age) of each prune.
    pruned: RefCell<Vec<(Vec<String>, Option<Duration>)>>,
    /// Live panes of each closed-panes-only prune.
    pruned_closed: RefCell<Vec<Vec<String>>>,
    /// Every call fails with this error when set.
    failure: RefCell<Option<HistoryError>>,
}

impl FakeHistoryStore {
    /// The error every call returns after [`fail_with(message)`](Self::fail_with).
    pub fn error(message: &str) -> HistoryError {
        HistoryError::Write {
            path: PathBuf::from("/state/history.db"),
            message: message.to_owned(),
        }
    }

    /// Makes every later call fail with [`error(message)`](Self::error).
    pub fn fail_with(&self, message: &str) {
        *self.failure.borrow_mut() = Some(Self::error(message));
    }

    /// Makes every later call fail as without a plugin state dir.
    pub fn fail_without_store(&self) {
        *self.failure.borrow_mut() = Some(HistoryError::NoStateDir);
    }

    /// Answers [`HistoryStore::focus_order`] with `panes` from now on.
    pub fn set_focus_order(&self, panes: &[&str]) {
        *self.focus_order.borrow_mut() = panes.iter().map(|&pane| pane.to_owned()).collect();
    }

    /// Answers [`HistoryStore::usage_order`] (any category) with `rows`.
    pub fn set_usage_order(&self, rows: Vec<RowId>) {
        *self.usage_order.borrow_mut() = rows;
    }

    /// Answers [`HistoryStore::last_pruned_at`] with `at` from now on.
    pub fn set_last_pruned_at(&self, at: Option<SystemTime>) {
        *self.last_pruned_at.borrow_mut() = at;
    }

    /// Every successful write, in order.
    pub fn writes(&self) -> Vec<HistoryWrite> {
        self.writes.borrow().clone()
    }

    /// The rows of every usage write, in order.
    pub fn used(&self) -> Vec<RowId> {
        self.writes
            .borrow()
            .iter()
            .filter_map(|write| match write {
                HistoryWrite::Usage { row, .. } => Some(row.clone()),
                _ => None,
            })
            .collect()
    }

    /// (live panes, max age) of every prune, in order.
    pub fn pruned(&self) -> Vec<(Vec<String>, Option<Duration>)> {
        self.pruned.borrow().clone()
    }

    /// Live panes of every [`HistoryStore::prune_closed`], in order.
    pub fn pruned_closed(&self) -> Vec<Vec<String>> {
        self.pruned_closed.borrow().clone()
    }

    fn check(&self) -> Result<(), HistoryError> {
        match self.failure.borrow().clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn push(&self, write: HistoryWrite) -> Result<(), HistoryError> {
        self.check()?;
        self.writes.borrow_mut().push(write);
        Ok(())
    }
}

impl HistoryStore for FakeHistoryStore {
    fn record_focus(
        &self,
        session: &Session,
        pane: &str,
        at: SystemTime,
    ) -> Result<(), HistoryError> {
        self.push(HistoryWrite::Focus {
            session: session.as_str().to_owned(),
            pane: pane.to_owned(),
            at,
        })
    }

    fn rename_pane(&self, session: &Session, from: &str, to: &str) -> Result<(), HistoryError> {
        self.push(HistoryWrite::Rename {
            session: session.as_str().to_owned(),
            from: from.to_owned(),
            to: to.to_owned(),
        })
    }

    fn record_usage(
        &self,
        session: &Session,
        row: &RowId,
        at: SystemTime,
    ) -> Result<(), HistoryError> {
        self.push(HistoryWrite::Usage {
            session: session.as_str().to_owned(),
            row: row.clone(),
            at,
        })
    }

    fn forget_pane(
        &self,
        session: &Session,
        pane: &str,
        before: SystemTime,
    ) -> Result<(), HistoryError> {
        self.push(HistoryWrite::Forget {
            session: session.as_str().to_owned(),
            pane: pane.to_owned(),
            before,
        })
    }

    fn focus_order(&self, _session: &Session) -> Result<Vec<String>, HistoryError> {
        self.check()?;
        Ok(self.focus_order.borrow().clone())
    }

    fn usage_order(
        &self,
        _session: &Session,
        _category: Category,
    ) -> Result<Vec<RowId>, HistoryError> {
        self.check()?;
        Ok(self.usage_order.borrow().clone())
    }

    fn prune(
        &self,
        _session: &Session,
        live_panes: &[&str],
        max_age: Option<Duration>,
        now: SystemTime,
    ) -> Result<Prune, HistoryError> {
        self.check()?;
        let live = live_panes.iter().map(|&pane| pane.to_owned()).collect();
        self.pruned.borrow_mut().push((live, max_age));
        *self.last_pruned_at.borrow_mut() = Some(now);
        Ok(Prune::Done {
            focus_rows: 0,
            usage_rows: 0,
        })
    }

    fn prune_closed(&self, _session: &Session, live_panes: &[&str]) -> Result<Prune, HistoryError> {
        self.check()?;
        let live = live_panes.iter().map(|&pane| pane.to_owned()).collect();
        self.pruned_closed.borrow_mut().push(live);
        Ok(Prune::Done {
            focus_rows: 0,
            usage_rows: 0,
        })
    }

    fn last_pruned_at(&self, _session: &Session) -> Result<Option<SystemTime>, HistoryError> {
        self.check()?;
        Ok(*self.last_pruned_at.borrow())
    }
}

/// Every fake, plus captured stdout and stderr.
pub(crate) struct World {
    pub env: PluginEnv,
    /// The binary's path (`/bin/hcp`).
    pub program: PathBuf,
    pub herdr: FakeHerdr,
    pub config: FakeConfig,
    pub settings: FakeSettings,
    pub picker: FakePicker,
    pub spawner: FakeSpawner,
    pub terminal: FakeTerminal,
    pub files: FakeFiles,
    pub clock: FakeClock,
    pub history: FakeHistoryStore,
    pub branches: FakeBranches,
    out: RefCell<Vec<u8>>,
    err: RefCell<Vec<u8>>,
}

impl World {
    /// Herdr answering with [`herdr_json`]'s snapshot and plugin actions,
    /// every other fake as its `Default` (empty config, default settings, fzf 0.66.0 that is
    /// cancelled, a 76-column terminal, `sh` exiting 0, a clock 30 days
    /// after the epoch, an empty history, no git repository).
    pub fn new() -> Self {
        let herdr = FakeHerdr::default();
        herdr.reply("session.snapshot", Ok(herdr_json::snapshot()));
        herdr.reply("plugin.action.list", Ok(herdr_json::plugin_actions()));
        Self {
            env: PluginEnv {
                home: Some(PathBuf::from("/home/me")),
                settings_path: PathBuf::from(SETTINGS_PATH),
                ..PluginEnv::from_lookup(|_| None)
            },
            program: PathBuf::from("/bin/hcp"),
            herdr,
            config: FakeConfig::default(),
            settings: FakeSettings::default(),
            picker: FakePicker::default(),
            spawner: FakeSpawner::default(),
            terminal: FakeTerminal::default(),
            files: FakeFiles::default(),
            clock: FakeClock::default(),
            history: FakeHistoryStore::default(),
            branches: FakeBranches::default(),
            out: RefCell::new(Vec::new()),
            err: RefCell::new(Vec::new()),
        }
    }

    /// The fakes as a use case [`Context`]: the binary is
    /// [`program`](Self::program), the working directory `/work`.
    pub fn context(&self) -> Context<'_> {
        Context {
            env: &self.env,
            program: &self.program,
            cwd: Some(Path::new("/work")),
            herdr: &self.herdr,
            config: &self.config,
            settings: &self.settings,
            picker: &self.picker,
            spawner: &self.spawner,
            terminal: &self.terminal,
            files: &self.files,
            clock: &self.clock,
            history: &self.history,
            branches: &self.branches,
        }
    }

    /// Runs `command` against the fakes.
    pub fn run(&self, command: &Command) -> Result<u8, Error> {
        let mut out = self.out.borrow_mut();
        let mut err = self.err.borrow_mut();
        run(command, &self.context(), &mut *out, &mut *err)
    }

    /// Everything written to stdout so far.
    pub fn out(&self) -> String {
        String::from_utf8(self.out.borrow().clone()).unwrap()
    }

    /// Everything written to stderr so far.
    pub fn err(&self) -> String {
        String::from_utf8(self.err.borrow().clone()).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::row::Kind;

    use super::*;

    #[test]
    fn history_fake_answers_focus_order_and_records_usage() {
        let history = FakeHistoryStore::default();
        let session = Session::from_socket_path(Path::new("/s.sock"));
        let row = RowId::new(Kind::Builtin, "zoom");
        history.set_focus_order(&["w1:p2", "w1:p1"]);

        history
            .record_usage(&session, &row, SystemTime::UNIX_EPOCH)
            .unwrap();

        assert_eq!(history.focus_order(&session).unwrap(), ["w1:p2", "w1:p1"]);
        assert_eq!(
            history.writes(),
            [HistoryWrite::Usage {
                session: "/s.sock".into(),
                row,
                at: SystemTime::UNIX_EPOCH
            }]
        );
    }

    #[test]
    fn history_fake_lists_the_rows_used_only() {
        let history = FakeHistoryStore::default();
        let session = Session::from_socket_path(Path::new("/s.sock"));
        let row = RowId::new(Kind::Builtin, "zoom");

        history
            .record_focus(&session, "w1:p1", SystemTime::UNIX_EPOCH)
            .unwrap();
        history
            .record_usage(&session, &row, SystemTime::UNIX_EPOCH)
            .unwrap();

        assert_eq!(history.used(), [row]);
    }
}
