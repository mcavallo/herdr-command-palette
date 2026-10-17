//! `run <kind> <payload>`: map a row to its [`Effect`] and carry it out.
//! `HCP_DRY_RUN=1` prints the request instead.
//!
//! Started detached by the palette after its popup closes, so failures
//! go to stderr (the run log) and a Herdr notification.
//! Invalid settings mean the defaults, with a notification too, except for
//! Edit Command Palette settings: that is how they get fixed.
//!
//! A row that ran is recorded in the history (unless `[history] enabled =
//! false`), for the "recent" orders and the repeat actions; the repeat
//! actions run rows through `run_with`, so their runs count too.
//! (Module and function are `run_row`, like `open::open`, so they don't
//! clash with [`app::run`](crate::app::run).)

use std::io::Write;
use std::path::Path;

use crate::app::palette::EDIT_SETTINGS;
use crate::domain::commands::PaletteCommand;
use crate::domain::history;
use crate::domain::requests::{self, Confirmation, Editing, Effect};
use crate::domain::row::{Kind, RowId};
use crate::domain::settings::{Settings, template};
use crate::ports::HerdrError;

use super::{Context, Error, notify_failure, session, settings_or_default, warn_unless_no_store};

/// Runs the row `kind:payload`; `confirmation` says whether the user
/// confirmed it in the dialog (for `close_tab` / `close_pane` closing a
/// worktree group, [`requests::confirmed_builtin_effect`]).
///
/// # Errors
///
/// [`Error::Request`], [`Error::Herdr`], [`Error::Config`],
/// [`Error::Settings`] or [`Error::Spawn`] when the row can't be mapped or
/// run; Herdr is notified first, except in a dry run.
pub fn run_row(
    context: &Context<'_>,
    kind: Kind,
    payload: &str,
    confirmation: Confirmation,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let settings = if kind == Kind::Palette && payload == EDIT_SETTINGS {
        context.settings.load().unwrap_or_default()
    } else {
        settings_or_default(context, err)
    };
    run_with(context, &settings, kind, payload, confirmation, out, err)
}

/// [`run_row`] with the settings already loaded (the repeat actions load
/// them first, so a broken file is reported once).
///
/// # Errors
///
/// As [`run_row`].
pub(crate) fn run_with(
    context: &Context<'_>,
    settings: &Settings,
    kind: Kind,
    payload: &str,
    confirmation: Confirmation,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let result =
        effect(context, settings, kind, payload, confirmation).and_then(|(effect, key)| {
            if context.env.dry_run {
                if let Some(line) = effect.dry_run_line() {
                    writeln!(out, "{line}")?;
                }
            } else {
                apply(context, effect)?;
            }
            Ok(key)
        });
    match result {
        Ok(key) => {
            if let Some(key) = key {
                record_usage(context, settings, &key, err);
            }
            Ok(0)
        }
        // A Jump to row whose pane closed since the palette opened: do
        // nothing, silently, like a missing target.
        Err(Error::Herdr(error)) if kind == Kind::Pane && is_gone(&error) => Ok(0),
        Err(error) => {
            notify_failure(context, &error);
            Err(error)
        }
    }
}

/// Herdr says the pane no longer exists.
fn is_gone(error: &HerdrError) -> bool {
    error.code() == Some(PANE_NOT_FOUND)
}

/// Herdr's error code for an unknown pane id (`pane.focus`, v0.9.3).
const PANE_NOT_FOUND: &str = "pane_not_found";

/// Records that the row ran, unless history is off or this is a dry run.
/// Best effort: a store error is one line on `err`, never a failure (the
/// row did run).
fn record_usage(context: &Context<'_>, settings: &Settings, key: &RowId, err: &mut dyn Write) {
    if !settings.history.enabled || context.env.dry_run {
        return;
    }
    let now = context.clock.now();
    if let Err(error) = context.history.record_usage(&session(context), key, now) {
        warn_unless_no_store(err, &error);
    }
}

/// What running the row does, and the row's history key; built-ins and
/// friends need a fresh snapshot so they act on the current focus.
fn effect(
    context: &Context<'_>,
    settings: &Settings,
    kind: Kind,
    payload: &str,
    confirmation: Confirmation,
) -> Result<(Effect, Option<RowId>), Error> {
    let env = context.env;
    let effect = match kind {
        Kind::Plugin => requests::plugin_effect(payload),
        Kind::Pane => requests::pane_effect(payload),
        Kind::Builtin => {
            let snapshot = context.herdr.snapshot()?;
            match confirmation {
                Confirmation::NotAsked => {
                    requests::builtin_effect(payload, &snapshot, &env.context)?
                }
                // Herdr's `confirm_close`; an unreadable config counts as
                // Herdr's default, as Herdr itself does.
                Confirmation::Confirmed => {
                    let ui = context
                        .config
                        .load()
                        .map(|config| config.ui)
                        .unwrap_or_default();
                    requests::confirmed_builtin_effect(payload, &snapshot, &env.context, ui)?
                }
            }
        }
        Kind::Palette => palette_effect(context, settings, payload)?,
        Kind::Custom => {
            let config = context.config.load()?;
            let snapshot = context.herdr.snapshot()?;
            let effect = requests::custom_effect(payload, &config.keys.command, &snapshot, env)?;
            // Custom rows are keyed by their command, from this config.
            let key = history::usage_key(kind, payload, &config.keys.command);
            return Ok((effect, key));
        }
    };
    Ok((effect, history::usage_key(kind, payload, &[])))
}

/// What a palette command does. Edit Command Palette settings first
/// writes the commented default file if there is none (not in a dry run),
/// so the editor opens on every key.
fn palette_effect(
    context: &Context<'_>,
    settings: &Settings,
    payload: &str,
) -> Result<Effect, Error> {
    let command: PaletteCommand = payload.parse()?;
    if command == PaletteCommand::EditSettings && !context.env.dry_run {
        context.settings.create(&template::default_file())?;
    }
    let editing = Editing {
        editor: &settings.editor,
        program: context.program,
    };
    let snapshot = context.herdr.snapshot()?;
    Ok(command.to_effect(&snapshot, context.env, editing))
}

/// Sends the request, or starts the shell command detached in its cwd.
fn apply(context: &Context<'_>, effect: Effect) -> Result<(), Error> {
    match effect {
        Effect::Herdr(request) => {
            context.herdr.call(&request)?;
        }
        Effect::Shell { command, cwd } => {
            let cwd = cwd.as_deref().map(Path::new);
            context.spawner.spawn_shell_detached(&command, cwd)?;
        }
        Effect::Nothing => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use crate::app::fakes::{
        FakeHistoryStore, HistoryWrite, World, api_error, invalid_settings, parse_error,
        spawn_error,
    };
    use crate::cli::Command;
    use crate::domain::config::HerdrConfig;
    use crate::domain::fixtures::herdr_json;
    use crate::domain::row::RowId;
    use crate::domain::settings::{HistorySettings, Settings};
    use crate::ports::{Clock, SettingsError};

    use super::*;

    fn run_row(kind: Kind, payload: &str) -> Command {
        Command::Run {
            kind,
            payload: payload.into(),
            confirmed: false,
        }
    }

    #[test]
    fn sends_builtins_with_a_fresh_snapshot() {
        let world = World::new();

        world.run(&run_row(Kind::Builtin, "zoom")).unwrap();

        assert_eq!(world.herdr.methods(), ["session.snapshot", "pane.zoom"]);
        assert_eq!(
            world.herdr.last("pane.zoom").unwrap().params,
            json!({"pane_id": "w1:p1", "mode": "toggle"})
        );
    }

    fn confirmed(kind: Kind, payload: &str) -> Command {
        Command::Run {
            kind,
            payload: payload.into(),
            confirmed: true,
        }
    }

    fn on_a_worktree_group() -> World {
        let world = World::new();
        world.herdr.reply(
            "session.snapshot",
            Ok(herdr_json::worktree_group_snapshot()),
        );
        world
    }

    #[test]
    fn a_confirmed_close_tab_closes_the_worktree_group_with_confirm_close() {
        let world = on_a_worktree_group();

        world.run(&confirmed(Kind::Builtin, "close_tab")).unwrap();

        assert_eq!(
            world.herdr.methods(),
            ["session.snapshot", "workspace.close"]
        );
        assert_eq!(
            world.herdr.last("workspace.close").unwrap().params,
            json!({"workspace_id": "w1", "close_group": true})
        );
    }

    #[test]
    fn a_confirmed_close_pane_on_the_groups_only_pane_closes_the_worktree_group() {
        let world = on_a_worktree_group();

        world.run(&confirmed(Kind::Builtin, "close_pane")).unwrap();

        assert_eq!(
            world.herdr.methods(),
            ["session.snapshot", "workspace.close"]
        );
        assert_eq!(
            world.herdr.last("workspace.close").unwrap().params,
            json!({"workspace_id": "w1", "close_group": true})
        );
    }

    #[test]
    fn a_confirmed_close_tab_closes_the_tab_without_confirm_close() {
        let world = on_a_worktree_group();
        world.config.set(Ok(
            HerdrConfig::from_toml("[ui]\nconfirm_close = false").unwrap()
        ));

        world.run(&confirmed(Kind::Builtin, "close_tab")).unwrap();

        assert_eq!(
            world.herdr.last("tab.close").unwrap().params,
            json!({"tab_id": "w1:t1"})
        );
    }

    #[test]
    fn an_unreadable_config_counts_as_confirm_close_on() {
        let world = on_a_worktree_group();
        world.config.set(Err(parse_error()));

        world.run(&confirmed(Kind::Builtin, "close_tab")).unwrap();

        assert!(world.herdr.last("workspace.close").is_some());
    }

    #[test]
    fn an_unconfirmed_close_tab_sends_tab_close_and_herdr_decides() {
        let world = on_a_worktree_group();

        world.run(&run_row(Kind::Builtin, "close_tab")).unwrap();

        assert!(world.herdr.last("tab.close").is_some());
        assert!(world.herdr.last("workspace.close").is_none());
    }

    #[test]
    fn plugin_actions_need_no_snapshot() {
        let world = World::new();

        world.run(&run_row(Kind::Plugin, "demo.hello")).unwrap();

        assert_eq!(world.herdr.methods(), ["plugin.action.invoke"]);
    }

    #[test]
    fn jumps_need_no_snapshot() {
        let world = World::new();

        world.run(&run_row(Kind::Pane, "w1:p2")).unwrap();

        assert_eq!(world.herdr.methods(), ["pane.focus"]);
    }

    #[test]
    fn palette_commands_move_the_focused_pane() {
        let world = World::new();

        world
            .run(&run_row(Kind::Palette, "move_pane_new_workspace"))
            .unwrap();

        assert_eq!(
            world.herdr.last("pane.move").unwrap().params,
            json!({"pane_id": "w1:p1", "focus": true, "destination": {"type": "new_workspace"}})
        );
    }

    #[test]
    fn edit_settings_writes_the_commented_file_then_opens_it() {
        let world = World::new();

        world.run(&run_row(Kind::Palette, "edit_settings")).unwrap();

        assert_eq!(world.settings.created(), [template::default_file()]);
        let request = world.herdr.last("plugin.pane.open").unwrap();
        assert_eq!(
            request.params["env"]["HCP_EXEC"],
            "${EDITOR:-vi} /cfg/settings.toml && /bin/hcp settings check --file /cfg/settings.toml"
        );
    }

    #[test]
    fn edit_settings_writes_nothing_in_a_dry_run() {
        let mut world = World::new();
        world.env.dry_run = true;

        world.run(&run_row(Kind::Palette, "edit_settings")).unwrap();

        assert_eq!(world.settings.created(), Vec::<String>::new());
        assert!(world.out().starts_with("plugin.pane.open "));
    }

    #[test]
    fn a_settings_file_that_cannot_be_written_is_notified() {
        let world = World::new();
        let denied = SettingsError::Write {
            path: PathBuf::from("/cfg/settings.toml"),
            message: "denied".into(),
        };
        world.settings.set_create(Err(denied.clone()));

        let error = world
            .run(&run_row(Kind::Palette, "edit_settings"))
            .unwrap_err();

        assert_eq!(error, Error::Settings(denied));
        assert_eq!(world.herdr.methods(), ["notification.show"]);
    }

    #[test]
    fn edit_rows_use_the_settings_editor() {
        let world = World::new();
        world.settings.set(Ok(Settings {
            editor: "hx".into(),
            ..Settings::default()
        }));

        world
            .run(&run_row(Kind::Palette, "edit_herdr_config"))
            .unwrap();

        let request = world.herdr.last("plugin.pane.open").unwrap();
        let command = request.params["env"]["HCP_EXEC"].as_str().unwrap();
        assert!(command.starts_with("hx "), "{command}");
    }

    #[test]
    fn invalid_settings_use_the_default_editor_and_say_so() {
        let world = World::new();
        world.settings.set(Err(invalid_settings()));

        world
            .run(&run_row(Kind::Palette, "edit_herdr_config"))
            .unwrap();

        let command =
            world.herdr.last("plugin.pane.open").unwrap().params["env"]["HCP_EXEC"].clone();
        assert!(command.as_str().unwrap().starts_with("${EDITOR:-vi} "));
        assert_eq!(
            world.herdr.methods(),
            ["notification.show", "session.snapshot", "plugin.pane.open"]
        );
        assert!(world.err().contains("(using the default settings)"));
    }

    #[test]
    fn unknown_palette_commands_are_notified() {
        let world = World::new();

        let error = world.run(&run_row(Kind::Palette, "teleport")).unwrap_err();

        assert_eq!(error.to_string(), "unknown palette command `teleport`");
        assert_eq!(world.herdr.methods(), ["notification.show"]);
    }

    #[test]
    fn custom_shell_commands_start_detached_in_the_pane_cwd() {
        let world = World::new();
        world.config.set(Ok(HerdrConfig::from_toml(
            "[[keys.command]]\ncommand = \"make\"\n",
        )
        .unwrap()));

        world.run(&run_row(Kind::Custom, "0")).unwrap();

        assert_eq!(
            world.spawner.shell_detached(),
            [("make".to_owned(), Some(PathBuf::from("/home/me/src")))]
        );
    }

    #[test]
    fn nothing_to_do_sends_nothing() {
        let world = World::new();
        world.herdr.reply(
            "session.snapshot",
            Ok(json!({"snapshot": {
                "workspaces": [], "tabs": [], "panes": []
            }})),
        );

        assert_eq!(world.run(&run_row(Kind::Builtin, "close_pane")), Ok(0));
        assert_eq!(world.herdr.methods(), ["session.snapshot"]);
    }

    #[test]
    fn dry_run_prints_instead_of_sending() {
        let mut world = World::new();
        world.env.dry_run = true;

        world.run(&run_row(Kind::Pane, "w1:p2")).unwrap();

        assert_eq!(world.out(), "pane.focus {\"pane_id\":\"w1:p2\"}\n");
        assert_eq!(world.herdr.methods(), Vec::<String>::new());
    }

    #[test]
    fn dry_run_prints_nothing_when_there_is_nothing_to_do() {
        let mut world = World::new();
        world.env.dry_run = true;

        world.run(&run_row(Kind::Builtin, "move_tab_next")).unwrap();

        assert_eq!(world.out(), "");
        assert_eq!(world.herdr.methods(), ["session.snapshot"]);
    }

    #[test]
    fn failures_are_notified() {
        let world = World::new();
        let busy = api_error("ui_busy", "a popup is open");
        world.herdr.reply("pane.focus", Err(busy.clone()));

        let error = world.run(&run_row(Kind::Pane, "w1:p2")).unwrap_err();

        assert_eq!(error, Error::Herdr(busy));
        assert_eq!(
            world.herdr.last("notification.show").unwrap().params["body"],
            "ui_busy: a popup is open"
        );
    }

    #[test]
    fn jumping_to_a_closed_pane_does_nothing_silently() {
        let world = World::new();
        world.herdr.reply(
            "pane.focus",
            Err(api_error("pane_not_found", "pane w1:p9 not found")),
        );

        assert_eq!(world.run(&run_row(Kind::Pane, "w1:p9")), Ok(0));
        assert_eq!(world.herdr.methods(), ["pane.focus"]);
    }

    #[test]
    fn pane_not_found_from_other_rows_is_still_notified() {
        let world = World::new();
        let gone = api_error("pane_not_found", "pane w1:p1 not found");
        world.herdr.reply("pane.zoom", Err(gone.clone()));

        let error = world.run(&run_row(Kind::Builtin, "zoom")).unwrap_err();

        assert_eq!(error, Error::Herdr(gone));
        assert_eq!(
            world.herdr.methods(),
            ["session.snapshot", "pane.zoom", "notification.show"]
        );
    }

    #[test]
    fn unknown_payloads_are_notified() {
        let world = World::new();

        let error = world.run(&run_row(Kind::Builtin, "teleport")).unwrap_err();

        assert_eq!(error.to_string(), "unknown built-in `teleport`");
        assert_eq!(
            world.herdr.methods(),
            ["session.snapshot", "notification.show"]
        );
    }

    #[test]
    fn unknown_payloads_are_not_notified_in_a_dry_run() {
        let mut world = World::new();
        world.env.dry_run = true;

        let result = world.run(&run_row(Kind::Builtin, "teleport"));

        assert!(result.is_err());
        assert_eq!(world.herdr.methods(), ["session.snapshot"]);
    }

    #[test]
    fn a_broken_config_is_a_config_error() {
        let world = World::new();
        world.config.set(Err(parse_error()));

        let result = world.run(&run_row(Kind::Custom, "0"));

        assert_eq!(result, Err(Error::Config(parse_error())));
    }

    #[test]
    fn a_shell_that_cannot_start_is_a_spawn_error() {
        let world = World::new();
        world.config.set(Ok(HerdrConfig::from_toml(
            "[[keys.command]]\ncommand = \"make\"\n",
        )
        .unwrap()));
        world.spawner.set_detach(Err(spawn_error("sh")));

        let result = world.run(&run_row(Kind::Custom, "0"));

        assert_eq!(result, Err(Error::Spawn(spawn_error("sh"))));
    }

    fn used(world: &World) -> Vec<RowId> {
        world.history.used()
    }

    #[test]
    fn records_the_row_it_ran_for_the_session() {
        let mut world = World::new();
        world.env.socket_path = PathBuf::from("/c/herdr.sock");

        world.run(&run_row(Kind::Pane, "w1:p2")).unwrap();

        assert_eq!(
            world.history.writes(),
            [HistoryWrite::Usage {
                session: "/c/herdr.sock".into(),
                row: RowId::new(Kind::Pane, "w1:p2"),
                at: world.clock.now(),
            }]
        );
    }

    #[test]
    fn records_custom_commands_by_their_command() {
        let world = World::new();
        let config = HerdrConfig::from_toml("[[keys.command]]\ncommand = \"make\"\n").unwrap();
        let key = history::usage_key(Kind::Custom, "0", &config.keys.command).unwrap();
        world.config.set(Ok(config));

        world.run(&run_row(Kind::Custom, "0")).unwrap();

        assert_eq!(used(&world), [key]);
    }

    #[test]
    fn records_nothing_when_history_is_off() {
        let world = World::new();
        world.settings.set(Ok(Settings {
            history: HistorySettings {
                enabled: false,
                ..HistorySettings::default()
            },
            ..Settings::default()
        }));

        world.run(&run_row(Kind::Builtin, "zoom")).unwrap();

        assert_eq!(used(&world), []);
    }

    #[test]
    fn records_nothing_in_a_dry_run() {
        let mut world = World::new();
        world.env.dry_run = true;

        world.run(&run_row(Kind::Builtin, "zoom")).unwrap();

        assert_eq!(used(&world), []);
    }

    #[test]
    fn records_nothing_when_the_row_failed_or_its_pane_is_gone() {
        let world = World::new();
        world.herdr.reply(
            "pane.focus",
            Err(api_error("pane_not_found", "pane w1:p9 not found")),
        );
        world
            .herdr
            .reply("pane.zoom", Err(api_error("ui_busy", "busy")));

        world.run(&run_row(Kind::Pane, "w1:p9")).unwrap();
        world.run(&run_row(Kind::Builtin, "zoom")).unwrap_err();

        assert_eq!(used(&world), []);
    }

    #[test]
    fn a_store_error_is_a_warning_not_a_failure() {
        let world = World::new();
        world.history.fail_with("disk full");

        let status = world.run(&run_row(Kind::Builtin, "zoom"));

        assert_eq!(status, Ok(0));
        assert_eq!(
            world.err(),
            format!(
                "herdr-command-palette: history unavailable: {}\n",
                FakeHistoryStore::error("disk full")
            )
        );
        assert_eq!(world.herdr.methods(), ["session.snapshot", "pane.zoom"]);
    }

    #[test]
    fn edit_settings_does_not_toast_invalid_settings() {
        let world = World::new();
        world.settings.set(Err(invalid_settings()));

        world.run(&run_row(Kind::Palette, "edit_settings")).unwrap();

        assert_eq!(
            world.herdr.methods(),
            ["session.snapshot", "plugin.pane.open"]
        );
        assert_eq!(world.err(), "");
    }
}
