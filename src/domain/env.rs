//! The plugin's environment, resolved once at start-up from the variables
//! Herdr sets: plugin id, Herdr binary, config, settings and socket paths,
//! and the invocation context.
//!
//! [`PluginEnv::from_lookup`] takes a lookup function instead of reading
//! the process environment, so it stays pure and testable; `main.rs` passes
//! `std::env::var`.

use std::path::{Path, PathBuf};

use crate::domain::appearance::Appearance;
use crate::domain::herdr::InvocationContext;

/// Plugin id used when Herdr doesn't set `HERDR_PLUGIN_ID` (e.g. `list`
/// run by hand). Matches `herdr-plugin.toml`.
const DEFAULT_PLUGIN_ID: &str = "mmc.command-palette";

/// Env var naming the palette state dir: the per-open temp dir the palette
/// shares with its fzf callbacks (current category, footer mode, rows).
/// Not the plugin state dir ([`PluginEnv::plugin_state_dir`]).
pub(crate) const HCP_STATE_ENV: &str = "HCP_STATE";

/// Env var carrying this binary's absolute path to fzf and its bindings,
/// set by the palette when it starts fzf.
pub(crate) const HCP_PROGRAM_ENV: &str = "HCP_PROGRAM";

/// [`HCP_PROGRAM_ENV`] as POSIX `sh` expands it: how fzf bindings and the
/// actions the callbacks print name this binary. The path itself never
/// enters fzf's action text, so fzf can't rewrite placeholders (`{q}`) in
/// it or end a delimited argument inside it.
pub(crate) const HCP_PROGRAM_SH: &str = "\"$HCP_PROGRAM\"";

/// Env var that, set to `1`, makes the plugin print the Herdr requests it
/// would send instead of sending them.
const HCP_DRY_RUN_ENV: &str = "HCP_DRY_RUN";

/// The settings file's name in the plugin config dir.
const SETTINGS_FILE: &str = "settings.toml";

/// The history database's file name, in the plugin state dir.
const HISTORY_DB_FILE: &str = "history.db";

/// Env var carrying the command line for the `exec` entrypoint
/// ([`EXEC_ENTRYPOINT`](crate::domain::herdr::EXEC_ENTRYPOINT)).
pub(crate) const HCP_EXEC_ENV: &str = "HCP_EXEC";

/// Env var carrying the row the `confirm` entrypoint asks about
/// ([`CONFIRM_ENTRYPOINT`](crate::domain::herdr::CONFIRM_ENTRYPOINT)), as
/// `<kind>\t<payload>\t<title>`.
pub(crate) const HCP_CONFIRM_ENV: &str = "HCP_CONFIRM";

/// Env var carrying the appearance the palette detected (`dark` or
/// `light`) to its fzf callbacks, which can't ask the terminal while fzf
/// owns it. Set only when `auto_switch` is on and the terminal
/// answered.
pub(crate) const HCP_APPEARANCE_ENV: &str = "HCP_APPEARANCE";

/// Everything the palette needs from its environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginEnv {
    /// `HERDR_PLUGIN_ID`, else `DEFAULT_PLUGIN_ID`.
    pub plugin_id: String,
    /// Herdr executable: `HERDR_BIN_PATH`, else `herdr` (looked up in `PATH`).
    pub herdr_bin: String,
    /// Herdr's `config.toml`: `HERDR_CONFIG_PATH`, else
    /// `$XDG_CONFIG_HOME/herdr/config.toml`, else `~/.config/herdr/config.toml`.
    pub config_path: PathBuf,
    /// Herdr's socket: `HERDR_SOCKET_PATH`, else `herdr.sock` next to
    /// [`config_path`](Self::config_path).
    pub socket_path: PathBuf,
    /// The plugin's `settings.toml`, in its config dir:
    /// `HERDR_PLUGIN_CONFIG_DIR`, else where Herdr puts it
    /// (`$XDG_CONFIG_HOME/herdr/plugins/config/<plugin_id>`, else under
    /// `~/.config`; `herdr plugin config-dir`, v0.9.3, which ignores
    /// `HERDR_CONFIG_PATH`).
    pub settings_path: PathBuf,
    /// `HOME`, used to shorten paths to `~`.
    pub home: Option<PathBuf>,
    /// Herdr's persistent plugin state dir (`HERDR_PLUGIN_STATE_DIR`), if
    /// set. Not the per-open [`palette_state`](Self::palette_state).
    pub plugin_state_dir: Option<PathBuf>,
    /// `HERDR_PLUGIN_CONTEXT_JSON`, parsed; empty when unset or invalid.
    pub context: InvocationContext,
    /// `HERDR_PANE_ID`: the pane an event hook is about (the focused pane
    /// for `pane.focused`).
    pub pane_id: Option<String>,
    /// `HERDR_PLUGIN_EVENT_JSON`, as sent: the event an `[[events]]` hook
    /// runs for.
    pub event_json: Option<String>,
    /// `HCP_DRY_RUN=1`: print requests instead of sending them.
    pub dry_run: bool,
    /// The command line the `exec` pane runs (`HCP_EXEC_ENV`).
    pub exec_command: Option<String>,
    /// The row the `confirm` pane asks about (`HCP_CONFIRM_ENV`).
    pub confirm_row: Option<String>,
    /// The palette state dir (`HCP_STATE_ENV`), a per-open temp dir;
    /// set for the fzf callbacks.
    pub palette_state: Option<PathBuf>,
    /// The appearance the palette detected (`HCP_APPEARANCE_ENV`), set
    /// for the fzf callbacks; `None` when unset or invalid.
    pub appearance: Option<Appearance>,
    /// fzf's width (`FZF_COLUMNS`), set for the fzf callbacks.
    pub fzf_columns: Option<usize>,
    /// `TMPDIR`, where the detached `run` logs when there is no plugin state
    /// dir.
    pub tmp_dir: Option<PathBuf>,
    /// `PATH`, shown when an `exec` command fails, to help tell why.
    pub path: Option<String>,
}

impl PluginEnv {
    /// Resolves the environment through `lookup` (variable name -> value).
    /// Empty values count as unset.
    #[must_use]
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        // Treat `VAR=` like an unset variable so empty paths do not override defaults.
        let var = |name: &str| lookup(name).filter(|value| !value.is_empty());

        let home = var("HOME").map(PathBuf::from);
        let herdr_dir = var("XDG_CONFIG_HOME")
            .map_or_else(
                || home.as_deref().unwrap_or(Path::new("")).join(".config"),
                PathBuf::from,
            )
            .join("herdr");
        let config_path =
            var("HERDR_CONFIG_PATH").map_or_else(|| herdr_dir.join("config.toml"), PathBuf::from);
        let socket_path = var("HERDR_SOCKET_PATH").map_or_else(
            || {
                config_path
                    .parent()
                    .unwrap_or(Path::new(""))
                    .join("herdr.sock")
            },
            PathBuf::from,
        );

        let plugin_id = var("HERDR_PLUGIN_ID").unwrap_or_else(|| DEFAULT_PLUGIN_ID.to_owned());
        let settings_path = var("HERDR_PLUGIN_CONFIG_DIR")
            .map_or_else(
                || herdr_dir.join("plugins").join("config").join(&plugin_id),
                PathBuf::from,
            )
            .join(SETTINGS_FILE);

        Self {
            plugin_id,
            herdr_bin: var("HERDR_BIN_PATH").unwrap_or_else(|| "herdr".to_owned()),
            config_path,
            socket_path,
            settings_path,
            home,
            plugin_state_dir: var("HERDR_PLUGIN_STATE_DIR").map(PathBuf::from),
            context: var("HERDR_PLUGIN_CONTEXT_JSON")
                .map(|json| InvocationContext::from_json(&json))
                .unwrap_or_default(),
            pane_id: var("HERDR_PANE_ID"),
            event_json: var("HERDR_PLUGIN_EVENT_JSON"),
            dry_run: var(HCP_DRY_RUN_ENV).as_deref() == Some("1"),
            exec_command: var(HCP_EXEC_ENV),
            confirm_row: var(HCP_CONFIRM_ENV),
            palette_state: var(HCP_STATE_ENV).map(PathBuf::from),
            appearance: var(HCP_APPEARANCE_ENV).and_then(|text| text.parse().ok()),
            fzf_columns: var("FZF_COLUMNS").and_then(|columns| columns.parse().ok()),
            tmp_dir: var("TMPDIR").map(PathBuf::from),
            path: var("PATH"),
        }
    }

    /// The history database, `history.db` in the plugin state dir; `None`
    /// without one.
    #[must_use]
    pub fn history_db(&self) -> Option<PathBuf> {
        self.plugin_state_dir
            .as_ref()
            .map(|dir| dir.join(HISTORY_DB_FILE))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn env(vars: &[(&str, &str)]) -> PluginEnv {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        PluginEnv::from_lookup(|name| map.get(name).cloned())
    }

    #[test]
    fn defaults_follow_home() {
        let env = env(&[("HOME", "/home/u"), ("XDG_CONFIG_HOME", "")]);
        assert_eq!(env.plugin_id, DEFAULT_PLUGIN_ID);
        assert_eq!(env.herdr_bin, "herdr");
        assert_eq!(
            env.config_path,
            Path::new("/home/u/.config/herdr/config.toml")
        );
        assert_eq!(
            env.socket_path,
            Path::new("/home/u/.config/herdr/herdr.sock")
        );
        assert_eq!(
            env.settings_path,
            Path::new("/home/u/.config/herdr/plugins/config/mmc.command-palette/settings.toml")
        );
        assert_eq!(env.home.as_deref(), Some(Path::new("/home/u")));
        assert_eq!(env.plugin_state_dir, None);
        assert_eq!(env.context, InvocationContext::default());
        assert!(!env.dry_run);
        assert_eq!(env.exec_command, None);
        assert_eq!(env.confirm_row, None);
        assert_eq!(env.palette_state, None);
        assert_eq!(env.fzf_columns, None);
        assert_eq!(env.appearance, None);
        assert_eq!(env.pane_id, None);
        assert_eq!(env.event_json, None);
        assert_eq!(env.history_db(), None);
    }

    #[test]
    fn reads_the_event_hook_variables() {
        let env = env(&[
            ("HERDR_PANE_ID", "w1:p2"),
            ("HERDR_PLUGIN_EVENT_JSON", r#"{"event": "pane_focused"}"#),
        ]);

        assert_eq!(env.pane_id.as_deref(), Some("w1:p2"));
        assert_eq!(
            env.event_json.as_deref(),
            Some(r#"{"event": "pane_focused"}"#)
        );
    }

    #[test]
    fn the_history_db_lives_in_the_plugin_state_dir() {
        let env = env(&[("HERDR_PLUGIN_STATE_DIR", "/state")]);

        assert_eq!(env.history_db(), Some(PathBuf::from("/state/history.db")));
    }

    #[test]
    fn reads_the_palette_variables() {
        let env = env(&[
            ("HCP_DRY_RUN", "1"),
            ("HCP_EXEC", "lazygit"),
            (
                "HCP_CONFIRM",
                r#"{"kind":"builtin","payload":"zoom","title":"Zoom"}"#,
            ),
            ("HCP_STATE", "/tmp/p"),
            ("FZF_COLUMNS", "74"),
            ("HCP_APPEARANCE", "light"),
            ("TMPDIR", "/var/tmp"),
            ("PATH", "/bin"),
        ]);

        assert!(env.dry_run);
        assert_eq!(env.exec_command.as_deref(), Some("lazygit"));
        assert_eq!(
            env.confirm_row.as_deref(),
            Some(r#"{"kind":"builtin","payload":"zoom","title":"Zoom"}"#)
        );
        assert_eq!(env.palette_state.as_deref(), Some(Path::new("/tmp/p")));
        assert_eq!(env.fzf_columns, Some(74));
        assert_eq!(env.appearance, Some(Appearance::Light));
        assert_eq!(env.tmp_dir.as_deref(), Some(Path::new("/var/tmp")));
        assert_eq!(env.path.as_deref(), Some("/bin"));
    }

    #[test]
    fn the_program_reference_expands_the_program_variable() {
        assert_eq!(HCP_PROGRAM_SH, format!("\"${HCP_PROGRAM_ENV}\""));
    }

    #[test]
    fn only_dry_run_1_is_a_dry_run() {
        assert!(!env(&[("HCP_DRY_RUN", "yes")]).dry_run);
        assert_eq!(env(&[("FZF_COLUMNS", "wide")]).fzf_columns, None);
        assert_eq!(env(&[("HCP_APPEARANCE", "dusk")]).appearance, None);
    }

    #[test]
    fn xdg_then_explicit_paths_win() {
        let xdg = env(&[("HOME", "/home/u"), ("XDG_CONFIG_HOME", "/xdg")]);
        assert_eq!(xdg.config_path, Path::new("/xdg/herdr/config.toml"));
        assert_eq!(xdg.socket_path, Path::new("/xdg/herdr/herdr.sock"));
        assert_eq!(
            xdg.settings_path,
            Path::new("/xdg/herdr/plugins/config/mmc.command-palette/settings.toml")
        );

        let explicit = env(&[
            ("HERDR_CONFIG_PATH", "/c/my.toml"),
            ("HERDR_SOCKET_PATH", "/s/h.sock"),
            ("HERDR_PLUGIN_ID", "x.y"),
            ("HERDR_BIN_PATH", "/bin/herdr"),
            ("HERDR_PLUGIN_STATE_DIR", "/state"),
            ("HERDR_PLUGIN_CONTEXT_JSON", r#"{"tab_id": "w1:t1"}"#),
        ]);
        assert_eq!(explicit.config_path, Path::new("/c/my.toml"));
        assert_eq!(explicit.socket_path, Path::new("/s/h.sock"));
        assert_eq!(explicit.plugin_id, "x.y");
        assert_eq!(explicit.herdr_bin, "/bin/herdr");
        assert_eq!(
            explicit.plugin_state_dir.as_deref(),
            Some(Path::new("/state"))
        );
        assert_eq!(explicit.context.tab_id.as_deref(), Some("w1:t1"));
        // Herdr's plugin config dir doesn't follow `HERDR_CONFIG_PATH`.
        assert_eq!(
            explicit.settings_path,
            Path::new(".config/herdr/plugins/config/x.y/settings.toml")
        );
    }

    #[test]
    fn settings_live_in_the_plugin_config_dir() {
        let env = env(&[("HERDR_PLUGIN_CONFIG_DIR", "/cfg/plugin")]);

        assert_eq!(env.settings_path, Path::new("/cfg/plugin/settings.toml"));
    }

    #[test]
    fn no_home_gives_relative_paths() {
        let env = env(&[]);
        assert_eq!(env.config_path, Path::new(".config/herdr/config.toml"));
        assert_eq!(env.home, None);
    }
}
