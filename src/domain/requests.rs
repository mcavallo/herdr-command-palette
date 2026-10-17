//! What running a selected row does: a Herdr socket request, a local shell
//! command, or nothing.
//!
//! Built-ins mirror Herdr's client ([`builtin`]); palette commands, custom
//! commands, plugin actions and pane jumps are mapped here. Targets come
//! from the [`InvocationContext`] when set, else the [`Snapshot`]'s focused
//! ids.
//!
//! Nothing here talks to Herdr or starts processes: the app layer sends the
//! [`Effect`] (with the `ui_busy` retry) or runs the shell command.

pub mod builtin;

use std::path::Path;
use std::str::FromStr;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::domain::catalog::NamedCommand;
use crate::domain::catalog::palette::PALETTE_COMMANDS;
use crate::domain::commands::{Builtin, PaletteCommand};
use crate::domain::config::{CommandType, CustomCommand, UiConfig};
use crate::domain::env::{HCP_CONFIRM_ENV, HCP_EXEC_ENV, PluginEnv};
use crate::domain::herdr::{
    CONFIRM_ENTRYPOINT, EXEC_ENTRYPOINT, InvocationContext, PALETTE_ENTRYPOINT, Request, Snapshot,
    Workspace,
};
use crate::domain::non_empty;
use crate::domain::popup::PopupSize;
use crate::domain::row::{Kind, Row};
use crate::domain::shell;

/// What running a row does.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Send this request to Herdr's socket.
    Herdr(Request),
    /// Run `sh -c command` detached, in `cwd` when known (custom `shell`
    /// commands).
    Shell {
        /// The command line, as written in `config.toml`.
        command: String,
        /// Working directory: the focused pane's, if known.
        cwd: Option<String>,
    },
    /// Nothing to do (e.g. move tab with a single tab, next agent with none).
    Nothing,
}

impl Effect {
    /// The line a dry run prints (`HCP_DRY_RUN=1`): `method {params}`, or
    /// `shell {"command", "cwd"}`; `None` when there is nothing to do.
    #[must_use]
    pub fn dry_run_line(&self) -> Option<String> {
        match self {
            Self::Herdr(request) => Some(request.to_string()),
            Self::Shell { command, cwd } => {
                Some(format!("shell {}", json!({"command": command, "cwd": cwd})))
            }
            Self::Nothing => None,
        }
    }
}

/// Why a row can't be turned into an [`Effect`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RequestError {
    /// The built-in name isn't one the palette knows.
    #[error("unknown built-in `{0}`")]
    UnknownBuiltin(String),
    /// The palette command name isn't one the palette knows.
    #[error("unknown palette command `{0}`")]
    UnknownPaletteCommand(String),
    /// The custom command payload isn't an index into `[[keys.command]]`
    /// (not a number, or the config changed since the list was built).
    #[error("no custom command `{0}` in config.toml")]
    UnknownCustomCommand(String),
    /// The custom command has no `command` to run (missing or empty).
    #[error("custom command {index} has no `command`")]
    MissingCommand {
        /// Index in `[[keys.command]]`.
        index: usize,
    },
    /// The custom command's type isn't run by the palette
    /// (`plugin_action` rows are plugin rows; unknown types are skipped).
    #[error("custom command {index} has type `{kind}`, which the palette does not run")]
    UnsupportedCommandType {
        /// Index in `[[keys.command]]`.
        index: usize,
        /// The command's type.
        kind: &'static str,
    },
}

/// What the edit rows (Edit Herdr config, Edit Command Palette settings)
/// need beyond the environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Editing<'a> {
    /// The settings' `editor`: a command line, arguments allowed; empty
    /// for `$EDITOR`, then `vi`.
    pub editor: &'a str,
    /// This binary, which checks the settings after the editor closes.
    pub program: &'a Path,
}

/// Workspace, tab and pane an action applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Targets<'a> {
    /// Workspace id, if known.
    pub workspace_id: Option<&'a str>,
    /// Tab id, if known.
    pub tab_id: Option<&'a str>,
    /// Pane id, if known.
    pub pane_id: Option<&'a str>,
}

/// Targets for an action: each id from the invocation context when set
/// (popups get no `HERDR_PANE_ID`), else the snapshot's focus.
#[must_use]
pub fn targets<'a>(context: &'a InvocationContext, snapshot: &'a Snapshot) -> Targets<'a> {
    // Empty strings count as unset so snapshot values remain available as fallbacks.
    let pick = |from_context: &'a Option<String>, from_snapshot: &'a Option<String>| {
        non_empty(from_context.as_deref()).or(non_empty(from_snapshot.as_deref()))
    };
    Targets {
        workspace_id: pick(&context.workspace_id, &snapshot.focused_workspace_id),
        tab_id: pick(&context.tab_id, &snapshot.focused_tab_id),
        pane_id: pick(&context.focused_pane_id, &snapshot.focused_pane_id),
    }
}

/// Invokes a plugin action, letting Herdr build its context.
#[must_use]
pub fn plugin_effect(action_id: &str) -> Effect {
    Effect::Herdr(Request::new(
        "plugin.action.invoke",
        json!({"action_id": action_id}),
    ))
}

/// Focuses a pane (Jump to rows).
#[must_use]
pub fn pane_effect(pane_id: &str) -> Effect {
    Effect::Herdr(Request::focus_pane(pane_id))
}

/// What a built-in does, by its `[keys]` name (see [`builtin`]).
///
/// # Errors
///
/// [`RequestError::UnknownBuiltin`] for an unknown name. A missing
/// target is [`Effect::Nothing`], not an error (see [`Builtin::to_effect`]).
pub fn builtin_effect(
    name: &str,
    snapshot: &Snapshot,
    context: &InvocationContext,
) -> Result<Effect, RequestError> {
    let builtin: Builtin = name.parse()?;
    Ok(builtin.to_effect(snapshot, targets(context, snapshot)))
}

/// Whether the user said yes in the confirmation dialog before this run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Confirmation {
    /// No dialog: the row wasn't asked about.
    #[default]
    NotAsked,
    /// Enter in the dialog.
    Confirmed,
}

/// What a built-in confirmed in the dialog does: as [`builtin_effect`],
/// except when it would close a worktree group Herdr refuses to close
/// unasked ([`group_closed_by`]): `close_tab` on the group's last tab,
/// `close_pane` on its only pane. Herdr's own dialog then closes the
/// group, so this does too: `workspace.close {workspace_id, close_group:
/// true}`.
///
/// # Errors
///
/// As [`builtin_effect`].
pub fn confirmed_builtin_effect(
    name: &str,
    snapshot: &Snapshot,
    context: &InvocationContext,
    ui: UiConfig,
) -> Result<Effect, RequestError> {
    let builtin: Builtin = name.parse()?;
    let targets = targets(context, snapshot);
    if let Some(workspace) = group_closed_by(builtin, snapshot, targets, ui) {
        return Ok(Effect::Herdr(Request::new(
            "workspace.close",
            json!({"workspace_id": workspace.workspace_id, "close_group": true}),
        )));
    }
    Ok(builtin.to_effect(snapshot, targets))
}

/// The workspace whose worktree group `builtin` on `targets` would close
/// and Herdr refuses to close unasked (`confirmation_required`, Herdr
/// `src/app/api/tabs.rs` and `panes.rs`, v0.9.3): `close_tab` on the
/// group's last tab, or `close_pane` on its only pane, with
/// `confirm_close` on. `None` otherwise: with `confirm_close` off, the
/// plain request closes the group without asking (the dialog already
/// said so; [`dialog`](crate::domain::dialog)).
#[must_use]
pub fn group_closed_by<'a>(
    builtin: Builtin,
    snapshot: &'a Snapshot,
    targets: Targets<'_>,
    ui: UiConfig,
) -> Option<&'a Workspace> {
    if !ui.confirm_close() {
        return None;
    }
    match builtin {
        Builtin::CloseTab => snapshot.group_closed_by_tab(targets.tab_id?),
        Builtin::ClosePane => snapshot.group_closed_by_pane(targets.pane_id?),
        _ => None,
    }
}

/// Parses a palette command name (`move_pane_new_tab`, ...), as listed in
/// `PALETTE_COMMANDS`.
impl FromStr for PaletteCommand {
    type Err = RequestError;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        NamedCommand::find(&PALETTE_COMMANDS, name)
            .map(|command| command.action)
            .ok_or_else(|| RequestError::UnknownPaletteCommand(name.to_owned()))
    }
}

impl PaletteCommand {
    /// What running this command does. [`Effect::Nothing`] when there is no
    /// pane (or workspace) to move, as for built-ins.
    #[must_use]
    pub fn to_effect(self, snapshot: &Snapshot, env: &PluginEnv, editing: Editing<'_>) -> Effect {
        match self.request(snapshot, env, editing) {
            Some(request) => Effect::Herdr(request),
            None => Effect::Nothing,
        }
    }

    /// The request this command sends; `None` when a target is missing.
    fn request(
        self,
        snapshot: &Snapshot,
        env: &PluginEnv,
        editing: Editing<'_>,
    ) -> Option<Request> {
        let targets = targets(&env.context, snapshot);
        let request = match self {
            Self::MovePaneNewTab => Request::new(
                "pane.move",
                json!({
                    "pane_id": targets.pane_id?,
                    "focus": true,
                    "destination": {"type": "new_tab", "workspace_id": targets.workspace_id?},
                }),
            ),
            Self::MovePaneNewWorkspace => Request::new(
                "pane.move",
                json!({
                    "pane_id": targets.pane_id?,
                    "focus": true,
                    "destination": {"type": "new_workspace"},
                }),
            ),
            Self::EditHerdrConfig => edit_popup(env, &edit_config_command(env, editing.editor)),
            Self::EditSettings => edit_popup(env, &edit_settings_command(env, editing)),
        };
        Some(request)
    }
}

/// `plugin.pane.open` of the `exec` entrypoint running an edit `command`
/// in a large popup.
fn edit_popup(env: &PluginEnv, command: &str) -> Request {
    let mut params = exec_params(env, command, None);
    params.insert("placement".into(), "popup".into());
    params.insert("width".into(), "90%".into());
    params.insert("height".into(), "90%".into());
    Request::plugin_pane_open(params)
}

/// What a palette command does, by name.
///
/// # Errors
///
/// [`RequestError::UnknownPaletteCommand`] for an unknown name.
pub fn palette_effect(
    name: &str,
    snapshot: &Snapshot,
    env: &PluginEnv,
    editing: Editing<'_>,
) -> Result<Effect, RequestError> {
    let command: PaletteCommand = name.parse()?;
    Ok(command.to_effect(snapshot, env, editing))
}

/// The editor's command line: the settings' `editor`, else
/// `${EDITOR:-vi}`. Unquoted either way, so arguments (`code -w`) work
/// as it is the user's own setting.
fn editor_command(editor: &str) -> &str {
    if editor.is_empty() {
        "${EDITOR:-vi}"
    } else {
        editor
    }
}

/// `<editor> <config> && <herdr> config check && <herdr> server
/// reload-config`. A failed check keeps the popup open with Herdr's
/// message (the `exec` entrypoint).
fn edit_config_command(env: &PluginEnv, editor: &str) -> String {
    // A non-UTF-8 path is shown lossily; Herdr itself only takes UTF-8 paths.
    let config = env.config_path.to_string_lossy();
    let herdr = shell::quote(&env.herdr_bin);
    format!(
        "{} {} && {herdr} config check && {herdr} server reload-config",
        editor_command(editor),
        shell::quote(&config)
    )
}

/// `<editor> <settings> && <program> settings check --file <settings>`:
/// like Edit Herdr config, a failed check keeps the popup open with the
/// problem. The check gets the edited path, so it reads the same file
/// whatever the popup pane's environment says.
fn edit_settings_command(env: &PluginEnv, editing: Editing<'_>) -> String {
    let path = env.settings_path.to_string_lossy();
    let settings = shell::quote(&path);
    let program = editing.program.to_string_lossy();
    format!(
        "{} {settings} && {} settings check --file {settings}",
        editor_command(editing.editor),
        shell::quote(&program)
    )
}

/// What a custom command (`[[keys.command]]` entry `index`) does.
///
/// `shell` runs detached in the focused pane's cwd; `popup` and `pane` open
/// the plugin's `exec` entrypoint as a popup (with the entry's
/// `width`/`height`) or as a focused split to the right (Herdr has no API to
/// open an arbitrary popup).
///
/// # Errors
///
/// [`RequestError::UnknownCustomCommand`] when `index` isn't a valid index,
/// [`RequestError::MissingCommand`] when the entry's `command` is missing
/// or empty,
/// [`RequestError::UnsupportedCommandType`] for `plugin_action` and unknown
/// types.
pub fn custom_effect(
    index: &str,
    commands: &[CustomCommand],
    snapshot: &Snapshot,
    env: &PluginEnv,
) -> Result<Effect, RequestError> {
    let (position, entry) = index
        .parse::<usize>()
        .ok()
        .and_then(|position| Some((position, commands.get(position)?)))
        .ok_or_else(|| RequestError::UnknownCustomCommand(index.to_owned()))?;
    let placement = match entry.kind {
        CommandType::Shell => None,
        CommandType::Popup => Some("popup"),
        CommandType::Pane => Some("split"),
        CommandType::PluginAction | CommandType::Other => {
            return Err(RequestError::UnsupportedCommandType {
                index: position,
                kind: entry.kind.as_str(),
            });
        }
    };
    // An empty command would run `sh -c ""`: nothing, silently.
    let command = non_empty(entry.command.as_deref())
        .map(str::to_owned)
        .ok_or(RequestError::MissingCommand { index: position })?;
    let cwd = focused_cwd(&env.context, snapshot);
    let Some(placement) = placement else {
        return Ok(Effect::Shell { command, cwd });
    };

    let mut params = exec_params(env, &command, cwd);
    params.insert("placement".into(), placement.into());
    if entry.kind == CommandType::Popup {
        for (key, dimension) in [("width", &entry.width), ("height", &entry.height)] {
            if let Some(dimension) = dimension {
                params.insert(key.into(), json!(dimension));
            }
        }
    } else {
        // Pane: a temporary split that closes when the command exits.
        params.insert("direction".into(), "right".into());
        params.insert("focus".into(), true.into());
    }
    Ok(Effect::Herdr(Request::plugin_pane_open(params)))
}

/// `plugin.pane.open` for the palette popup (`[[panes]] palette`), sized
/// `size` when known; without one the manifest's fixed size applies.
#[must_use]
pub fn palette_popup(plugin_id: &str, size: Option<PopupSize>) -> Request {
    let mut params = plugin_pane_params(plugin_id, PALETTE_ENTRYPOINT);
    if let Some(size) = size {
        params.insert("width".into(), size.width.into());
        params.insert("height".into(), size.height.into());
    }
    Request::plugin_pane_open(params)
}

/// `plugin.pane.open` for the `confirm` popup (`[[panes]] confirm`) asking
/// about `row`, which travels in `HCP_CONFIRM` as a JSON object
/// `{"kind", "payload", "title"}` (read back by [`confirm_row`]).
#[must_use]
pub fn confirm_popup(plugin_id: &str, row: &Row, size: PopupSize) -> Request {
    let mut params = plugin_pane_params(plugin_id, CONFIRM_ENTRYPOINT);
    // JSON, not separators: a payload (a plugin or pane id) may hold any
    // character, and every one must come back unchanged.
    let value = json!({"kind": row.kind, "payload": row.payload, "title": row.full_title()});
    params.insert("env".into(), json!({ HCP_CONFIRM_ENV: value.to_string() }));
    params.insert("width".into(), size.width.into());
    params.insert("height".into(), size.height.into());
    Request::plugin_pane_open(params)
}

/// A row as [`confirm_popup`] writes it to `HCP_CONFIRM`.
#[derive(Debug, Deserialize)]
struct ConfirmValue {
    kind: Kind,
    payload: String,
    title: String,
}

/// The row a [`confirm_popup`] asks about, from its `HCP_CONFIRM`
/// value: kind, payload and title. `None` when the value isn't that JSON
/// object or the payload is empty.
#[must_use]
pub fn confirm_row(value: &str) -> Option<Row> {
    let value: ConfirmValue = serde_json::from_str(value).ok()?;
    if value.payload.is_empty() {
        return None;
    }
    Some(Row::new(value.kind, value.payload, value.title))
}

/// The `plugin.pane.open` params every pane of this plugin starts with.
fn plugin_pane_params(plugin_id: &str, entrypoint: &str) -> Map<String, Value> {
    let mut params = Map::new();
    params.insert("plugin_id".into(), plugin_id.into());
    params.insert("entrypoint".into(), entrypoint.into());
    params
}

/// `plugin.pane.open` params for the `exec` entrypoint running `command`.
fn exec_params(env: &PluginEnv, command: &str, cwd: Option<String>) -> Map<String, Value> {
    let mut params = plugin_pane_params(&env.plugin_id, EXEC_ENTRYPOINT);
    params.insert("env".into(), json!({ HCP_EXEC_ENV: command }));
    if let Some(cwd) = cwd {
        params.insert("cwd".into(), cwd.into());
    }
    params
}

/// Working directory of the target pane: its foreground cwd, else its cwd,
/// else the context's `focused_pane_cwd` when the target is the context's
/// pane (a target from the snapshot's focus may be another pane).
fn focused_cwd(context: &InvocationContext, snapshot: &Snapshot) -> Option<String> {
    let pane_id = targets(context, snapshot).pane_id;
    let from_context = context
        .focused_pane_cwd
        .as_deref()
        .filter(|_| pane_id.is_some() && pane_id == context.focused_pane_id.as_deref());
    snapshot
        .panes
        .iter()
        .find(|pane| Some(pane.pane_id.as_str()) == pane_id)
        .and_then(|pane| pane.working_dir())
        .or(from_context)
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::domain::config::HerdrConfig;
    use crate::domain::fixtures::{
        group_with_one_pane, herdr_effect, in_tab, on_pane, with_second_tab, worktree_snapshot,
    };

    use super::*;

    fn snapshot() -> Snapshot {
        serde_json::from_value(json!({
            "focused_workspace_id": "w1",
            "focused_tab_id": "w1:t2",
            "focused_pane_id": "w1:p2",
            "workspaces": [{"workspace_id": "w1", "label": "a"}],
            "tabs": [{"tab_id": "w1:t2", "workspace_id": "w1", "label": "2"}],
            "panes": [
                {"pane_id": "w1:p1", "workspace_id": "w1", "tab_id": "w1:t2", "cwd": "/a"},
                {"pane_id": "w1:p2", "workspace_id": "w1", "tab_id": "w1:t2", "cwd": "/b",
                 "foreground_cwd": "/b/src"},
            ],
        }))
        .unwrap()
    }

    #[test]
    fn confirmed_close_tab_closes_the_worktree_group_like_herdrs_dialog() {
        let effect = confirmed_builtin_effect(
            "close_tab",
            &worktree_snapshot(),
            &in_tab("w1:t1"),
            UiConfig::default(),
        );

        assert_eq!(
            effect,
            Ok(herdr_effect(
                "workspace.close",
                json!({"workspace_id": "w1", "close_group": true})
            ))
        );
    }

    #[test]
    fn confirmed_close_tab_closes_the_tab_otherwise() {
        let snapshot = with_second_tab(worktree_snapshot(), "w2");
        let tab_close = |tab_id: &str| herdr_effect("tab.close", json!({"tab_id": tab_id}));
        let effect = |tab_id: &str, ui| {
            confirmed_builtin_effect("close_tab", &snapshot, &in_tab(tab_id), ui).unwrap()
        };

        // Herdr's confirm_close off: tab.close closes the group by itself.
        assert_eq!(
            effect("w1:t1", UiConfig::with_confirm_close(false)),
            tab_close("w1:t1")
        );
        // Not the workspace's last tab.
        assert_eq!(effect("w2:t1", UiConfig::default()), tab_close("w2:t1"));
    }

    #[test]
    fn confirmed_close_tab_on_a_plain_workspaces_last_tab_closes_the_tab() {
        let snapshot = worktree_snapshot();

        let effect = confirmed_builtin_effect(
            "close_tab",
            &snapshot,
            &in_tab("w2:t1"),
            UiConfig::default(),
        );

        // Herdr's `accept_close_confirmation` sends `tab.close` too.
        assert_eq!(
            effect,
            Ok(herdr_effect("tab.close", json!({"tab_id": "w2:t1"})))
        );
    }

    #[test]
    fn confirmed_close_pane_on_a_worktree_groups_only_pane_closes_the_group() {
        let snapshot = group_with_one_pane();
        let effect = |pane_id: &str, ui| {
            confirmed_builtin_effect("close_pane", &snapshot, &on_pane(pane_id), ui)
        };

        assert_eq!(
            effect("w1:p1", UiConfig::default()),
            Ok(herdr_effect(
                "workspace.close",
                json!({"workspace_id": "w1", "close_group": true})
            ))
        );
        assert_eq!(
            effect("w1:p1", UiConfig::with_confirm_close(false)),
            Ok(herdr_effect("pane.close", json!({"pane_id": "w1:p1"})))
        );
        // A linked worktree's only pane.
        assert_eq!(
            effect("w2:p1", UiConfig::default()),
            Ok(herdr_effect("pane.close", json!({"pane_id": "w2:p1"})))
        );
    }

    #[test]
    fn confirmed_builtins_other_than_closes_are_unchanged() {
        let snapshot = worktree_snapshot();
        let context = in_tab("w1:t1");

        for name in ["close_workspace", "zoom"] {
            assert_eq!(
                confirmed_builtin_effect(name, &snapshot, &context, UiConfig::default()),
                builtin_effect(name, &snapshot, &context)
            );
        }
        assert_eq!(
            confirmed_builtin_effect("teleport", &snapshot, &context, UiConfig::default()),
            Err(RequestError::UnknownBuiltin("teleport".into()))
        );
    }

    fn env() -> PluginEnv {
        PluginEnv {
            plugin_id: "mmc.command-palette".into(),
            herdr_bin: "herdr".into(),
            config_path: PathBuf::from("/x/my config.toml"),
            settings_path: PathBuf::from("/cfg/settings.toml"),
            ..PluginEnv::from_lookup(|_| None)
        }
    }

    /// No `editor` setting; this binary is `/bin/hcp`.
    fn editing() -> Editing<'static> {
        Editing {
            editor: "",
            program: Path::new("/bin/hcp"),
        }
    }

    /// The `exec` popup the edit rows open, running `command`.
    fn edit_popup_effect(command: &str) -> Effect {
        herdr_effect(
            "plugin.pane.open",
            json!({"plugin_id": "mmc.command-palette", "entrypoint": "exec",
                   "placement": "popup", "width": "90%", "height": "90%",
                   "env": {"HCP_EXEC": command}}),
        )
    }

    const CONFIG: &str = r#"
        [[keys.command]]
        command = "notify-send hi"

        [[keys.command]]
        type = "popup"
        command = "lazygit"
        width = "90%"
        height = 30

        [[keys.command]]
        type = "pane"
        command = "htop"

        [[keys.command]]
        type = "plugin_action"
        command = "demo.hello"

        [[keys.command]]
        type = "popup"

        [[keys.command]]
        type = "popup"
        command = "btop"

        [[keys.command]]
        command = ""
    "#;

    fn custom(index: &str, context: InvocationContext) -> Result<Effect, RequestError> {
        let config = HerdrConfig::from_toml(CONFIG).unwrap();
        let env = PluginEnv { context, ..env() };
        custom_effect(index, &config.keys.command, &snapshot(), &env)
    }

    #[test]
    fn targets_prefer_the_context_and_skip_empty_ids() {
        let context = InvocationContext {
            workspace_id: Some("w9".into()),
            tab_id: Some(String::new()),
            ..InvocationContext::default()
        };
        let snapshot = snapshot();

        assert_eq!(
            targets(&context, &snapshot),
            Targets {
                workspace_id: Some("w9"),
                tab_id: Some("w1:t2"),
                pane_id: Some("w1:p2"),
            }
        );
    }

    #[test]
    fn targets_are_empty_without_focus() {
        let context = InvocationContext::default();
        let snapshot = Snapshot::default();

        assert_eq!(targets(&context, &snapshot), Targets::default());
    }

    #[test]
    fn plugin_rows_invoke_the_action() {
        assert_eq!(
            plugin_effect("demo.hello"),
            herdr_effect("plugin.action.invoke", json!({"action_id": "demo.hello"}))
        );
    }

    #[test]
    fn jump_to_focuses_the_pane() {
        assert_eq!(
            pane_effect("w2:p1"),
            herdr_effect("pane.focus", json!({"pane_id": "w2:p1"}))
        );
    }

    #[test]
    fn move_pane_to_new_tab_stays_in_the_workspace() {
        let effect = palette_effect("move_pane_new_tab", &snapshot(), &env(), editing()).unwrap();

        assert_eq!(
            effect,
            herdr_effect(
                "pane.move",
                json!({"pane_id": "w1:p2", "focus": true,
                       "destination": {"type": "new_tab", "workspace_id": "w1"}})
            )
        );
    }

    #[test]
    fn move_pane_to_new_workspace() {
        let effect =
            palette_effect("move_pane_new_workspace", &snapshot(), &env(), editing()).unwrap();

        assert_eq!(
            effect,
            herdr_effect(
                "pane.move",
                json!({"pane_id": "w1:p2", "focus": true,
                       "destination": {"type": "new_workspace"}})
            )
        );
    }

    #[test]
    fn edit_config_opens_an_exec_popup() {
        let env = PluginEnv {
            herdr_bin: "/opt/my herdr".into(),
            ..env()
        };

        let effect =
            palette_effect("edit_herdr_config", &Snapshot::default(), &env, editing()).unwrap();

        let command = "${EDITOR:-vi} '/x/my config.toml' \
            && '/opt/my herdr' config check && '/opt/my herdr' server reload-config";
        assert_eq!(effect, edit_popup_effect(command));
    }

    #[test]
    fn edit_config_uses_the_settings_editor() {
        let editing = Editing {
            editor: "code -w",
            ..editing()
        };

        let effect =
            palette_effect("edit_herdr_config", &Snapshot::default(), &env(), editing).unwrap();

        let command = "code -w '/x/my config.toml' \
            && herdr config check && herdr server reload-config";
        assert_eq!(effect, edit_popup_effect(command));
    }

    #[test]
    fn edit_settings_checks_them_after_the_editor() {
        let effect =
            palette_effect("edit_settings", &Snapshot::default(), &env(), editing()).unwrap();

        let command = "${EDITOR:-vi} /cfg/settings.toml \
                       && /bin/hcp settings check --file /cfg/settings.toml";
        assert_eq!(effect, edit_popup_effect(command));
    }

    #[test]
    fn unknown_palette_command_is_an_error() {
        let error = palette_effect("teleport", &snapshot(), &env(), editing()).unwrap_err();

        assert_eq!(
            error,
            RequestError::UnknownPaletteCommand("teleport".into())
        );
    }

    #[test]
    fn move_pane_without_focus_does_nothing() {
        let effect = palette_effect(
            "move_pane_new_workspace",
            &Snapshot::default(),
            &env(),
            editing(),
        )
        .unwrap();

        assert_eq!(effect, Effect::Nothing);
    }

    #[test]
    fn every_listed_palette_command_parses_back() {
        for command in PALETTE_COMMANDS {
            assert_eq!(command.name.parse::<PaletteCommand>(), Ok(command.action));
        }
    }

    #[test]
    fn shell_command_runs_in_the_focused_pane_foreground_cwd() {
        let effect = custom("0", InvocationContext::default()).unwrap();

        assert_eq!(
            effect,
            Effect::Shell {
                command: "notify-send hi".into(),
                cwd: Some("/b/src".into()),
            }
        );
    }

    #[test]
    fn shell_cwd_follows_the_context_pane() {
        let context = InvocationContext::from_json(r#"{"focused_pane_id":"w1:p1"}"#);

        let effect = custom("0", context).unwrap();

        assert_eq!(
            effect.dry_run_line().unwrap(),
            r#"shell {"command":"notify-send hi","cwd":"/a"}"#
        );
    }

    #[test]
    fn shell_cwd_falls_back_to_the_context_cwd() {
        let context = InvocationContext {
            focused_pane_id: Some("w7:p7".into()),
            focused_pane_cwd: Some("/ctx".into()),
            ..InvocationContext::default()
        };

        let effect = custom("0", context).unwrap();

        assert_eq!(
            effect,
            Effect::Shell {
                command: "notify-send hi".into(),
                cwd: Some("/ctx".into()),
            }
        );
    }

    #[test]
    fn shell_cwd_ignores_the_context_cwd_when_the_pane_comes_from_the_snapshot() {
        // The snapshot's focused pane has no cwd; the context's cwd may be
        // another pane's.
        let snapshot: Snapshot = serde_json::from_value(json!({
            "focused_pane_id": "w1:p3",
            "workspaces": [],
            "tabs": [],
            "panes": [{"pane_id": "w1:p3", "workspace_id": "w1", "tab_id": "w1:t1"}],
        }))
        .unwrap();
        let context = |pane_id: Option<&str>| InvocationContext {
            focused_pane_id: pane_id.map(str::to_owned),
            focused_pane_cwd: Some("/ctx".into()),
            ..InvocationContext::default()
        };

        assert_eq!(focused_cwd(&context(None), &snapshot), None);
        assert_eq!(focused_cwd(&context(Some("")), &snapshot), None);
    }

    #[test]
    fn popup_command_opens_exec_with_its_size() {
        let effect = custom("1", InvocationContext::default()).unwrap();

        assert_eq!(
            effect,
            herdr_effect(
                "plugin.pane.open",
                json!({"plugin_id": "mmc.command-palette", "entrypoint": "exec",
                       "env": {"HCP_EXEC": "lazygit"}, "cwd": "/b/src",
                       "placement": "popup", "width": "90%", "height": 30})
            )
        );
    }

    #[test]
    fn popup_without_size_lets_herdr_pick() {
        let effect = custom("5", InvocationContext::default()).unwrap();

        assert_eq!(
            effect,
            herdr_effect(
                "plugin.pane.open",
                json!({"plugin_id": "mmc.command-palette", "entrypoint": "exec",
                       "env": {"HCP_EXEC": "btop"}, "cwd": "/b/src",
                       "placement": "popup"})
            )
        );
    }

    #[test]
    fn pane_command_opens_a_focused_split_right() {
        let context = InvocationContext {
            focused_pane_id: Some("w7:p7".into()),
            ..InvocationContext::default()
        };

        let effect = custom("2", context).unwrap();

        assert_eq!(
            effect,
            herdr_effect(
                "plugin.pane.open",
                json!({"plugin_id": "mmc.command-palette", "entrypoint": "exec",
                       "env": {"HCP_EXEC": "htop"},
                       "placement": "split", "direction": "right", "focus": true})
            )
        );
    }

    #[test]
    fn palette_popup_is_sized_when_the_size_is_known() {
        let request = palette_popup(
            "p",
            Some(PopupSize {
                width: 76,
                height: 22,
            }),
        );

        assert_eq!(
            request,
            Request::new(
                "plugin.pane.open",
                json!({"plugin_id": "p", "entrypoint": "palette", "width": 76, "height": 22})
            )
        );
    }

    #[test]
    fn palette_popup_without_a_size_uses_the_manifest_size() {
        let request = palette_popup("p", None);

        assert_eq!(
            request,
            Request::new(
                "plugin.pane.open",
                json!({"plugin_id": "p", "entrypoint": "palette"})
            )
        );
    }

    #[test]
    fn confirm_popup_carries_the_row_in_its_env() {
        let row = Row::new(Kind::Builtin, "close_workspace", "Close workspace");

        let request = confirm_popup(
            "p",
            &row,
            PopupSize {
                width: 76,
                height: 6,
            },
        );

        assert_eq!(
            request,
            Request::new(
                "plugin.pane.open",
                json!({"plugin_id": "p", "entrypoint": "confirm",
                       "env": {"HCP_CONFIRM": r#"{"kind":"builtin","payload":"close_workspace","title":"Close workspace"}"#},
                       "width": 76, "height": 6})
            )
        );
    }

    #[test]
    fn confirm_rows_round_trip_tabs_in_the_payload_and_title() {
        let row = Row::new(Kind::Plugin, "a\tb", "x\ty");
        let request = confirm_popup(
            "p",
            &row,
            PopupSize {
                width: 76,
                height: 6,
            },
        );

        let value = request.params["env"][HCP_CONFIRM_ENV].as_str().unwrap();

        assert_eq!(confirm_row(value), Some(row));
    }

    #[test]
    fn confirm_rows_read_back_kind_payload_and_title() {
        let row = confirm_row(r#"{"kind":"pane","payload":"w1:p2","title":"logs · 2"}"#);

        assert_eq!(row, Some(Row::new(Kind::Pane, "w1:p2", "logs · 2")));
    }

    #[test]
    fn confirm_rows_need_a_kind_a_payload_and_a_title() {
        assert_eq!(
            confirm_row(r#"{"kind":"nope","payload":"x","title":"X"}"#),
            None
        );
        assert_eq!(
            confirm_row(r#"{"kind":"builtin","payload":"","title":"X"}"#),
            None
        );
        assert_eq!(confirm_row(r#"{"kind":"builtin","payload":"zoom"}"#), None);
        assert_eq!(confirm_row("builtin\tzoom\tZoom"), None);
        assert_eq!(confirm_row(""), None);
    }

    #[test]
    fn unknown_custom_indexes_are_errors() {
        assert_eq!(
            custom("9", InvocationContext::default()),
            Err(RequestError::UnknownCustomCommand("9".into()))
        );
        assert_eq!(
            custom("x", InvocationContext::default()),
            Err(RequestError::UnknownCustomCommand("x".into()))
        );
    }

    #[test]
    fn plugin_action_custom_commands_are_not_run() {
        let result = custom("3", InvocationContext::default());

        assert_eq!(
            result,
            Err(RequestError::UnsupportedCommandType {
                index: 3,
                kind: "plugin_action"
            })
        );
    }

    #[test]
    fn custom_commands_without_a_command_are_errors() {
        let result = custom("4", InvocationContext::default());

        assert_eq!(result, Err(RequestError::MissingCommand { index: 4 }));
    }

    #[test]
    fn custom_commands_with_an_empty_command_are_errors() {
        let result = custom("6", InvocationContext::default());

        assert_eq!(result, Err(RequestError::MissingCommand { index: 6 }));
    }

    #[test]
    fn dry_run_line_is_the_method_then_the_params() {
        let line = pane_effect("w1:p1").dry_run_line();

        assert_eq!(line.as_deref(), Some(r#"pane.focus {"pane_id":"w1:p1"}"#));
    }

    #[test]
    fn nothing_has_no_dry_run_line() {
        assert_eq!(Effect::Nothing.dry_run_line(), None);
    }

    #[test]
    fn unsupported_type_error_names_the_index_and_type() {
        assert_eq!(
            RequestError::UnsupportedCommandType {
                index: 3,
                kind: "plugin_action"
            }
            .to_string(),
            "custom command 3 has type `plugin_action`, which the palette does not run"
        );
    }
}
