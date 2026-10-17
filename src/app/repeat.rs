//! `repeat-command` and `repeat-jump`: run the newest row of a category
//! from the history again (plugin actions meant for key bindings).
//!
//! The row must still exist and be visible: a closed pane, a removed
//! plugin, an edited custom command, a row `hide_kinds` / `hide_ids` hide
//! and the current pane (so repeat-jump toggles between the last two
//! panes) are skipped for the next newest. When Herdr can't list
//! the rows (plugin actions for commands, the snapshot for jumps), the
//! repeat fails instead: unavailable is not removed. A row the settings'
//! `confirm_ids` lists opens the small `confirm` popup first (Enter runs it),
//! and so do, with Herdr's `confirm_close` on, `close_tab` on a
//! workspace's last tab and `close_pane` on a worktree group's only pane,
//! as Herdr's own keys do.
//! Running goes through `run_row::run_with`, so the repeat is recorded
//! too.
//!
//! These run from a key binding with nothing on screen, so every outcome
//! but a plain run is a Herdr toast: history off, nothing to
//! repeat, errors.

use std::io::Write;

use crate::app::{ask, run_row};
use crate::domain::catalog::{self, Sources};
use crate::domain::config::HerdrConfig;
use crate::domain::herdr::Snapshot;
use crate::domain::history;
use crate::domain::requests::{self, Confirmation};
use crate::domain::row::{Category, Kind, Row};
use crate::domain::settings::Settings;

use super::{
    Context, Error, must_confirm, notify_failure, session, settings_or_default, snapshot_or_warn,
    tell,
};

/// What the repeat actions say with `[history] enabled = false`.
const HISTORY_OFF: &str = "history is off";

/// Runs the newest still existing row of `category` again; returns the
/// exit status.
///
/// # Errors
///
/// [`Error::Config`], [`Error::History`] or [`Error::Herdr`] when the
/// row can't be found or the `confirm` popup can't open, and whatever
/// [`run_row::run_row`] returns for the row; Herdr is notified first,
/// except in a dry run.
pub fn repeat(
    context: &Context<'_>,
    category: Category,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let settings = settings_or_default(context, err);
    if !settings.history.enabled {
        tell(context, err, HISTORY_OFF);
        return Ok(0);
    }
    let found = context
        .config
        .load()
        .map_err(Error::from)
        .and_then(|config| {
            let found = latest(context, &settings, &config, category, err)?;
            Ok(found.map(|found| (found, config)))
        });
    let ((row, snapshot), config) = match found {
        Ok(Some(found)) => found,
        Ok(None) => {
            tell(context, err, nothing_to_repeat(category));
            return Ok(0);
        }
        Err(error) => {
            notify_failure(context, &error);
            return Err(error);
        }
    };
    if must_confirm(context, &settings, &config, &row, snapshot.as_ref()) {
        return ask::open_confirm(context, &row, snapshot.as_ref(), out)
            .inspect_err(|error| notify_failure(context, error));
    }
    run_row::run_with(
        context,
        &settings,
        row.kind,
        &row.payload,
        Confirmation::NotAsked,
        out,
        err,
    )
}

/// What the toast says when the history has nothing (left) to repeat.
fn nothing_to_repeat(category: Category) -> &'static str {
    match category {
        Category::Commands => "no command to repeat yet",
        Category::JumpTo => "no jump to repeat yet",
    }
}

/// The newest row of `category` in the history that is still in the
/// catalog and visible, and the session snapshot (for the `confirm`
/// popup's size and whether the row must ask, `dialog::is_required`).
///
/// Visible: not hidden by `hide_kinds` / `hide_ids` (a hidden row doesn't
/// come back through a key), and not the current pane (jumping to it does
/// nothing; skipping it makes repeat-jump toggle between the last two
/// panes).
///
/// The catalog is built from what this category needs, fetched strictly:
/// a row Herdr can't list right now is unavailable, not removed, so a
/// failed fetch fails the repeat instead of falling back to an older row.
fn latest(
    context: &Context<'_>,
    settings: &Settings,
    config: &HerdrConfig,
    category: Category,
    err: &mut dyn Write,
) -> Result<Option<(Row, Option<Snapshot>)>, Error> {
    let (actions, snapshot) = match category {
        Category::Commands => {
            let actions = context.herdr.plugin_actions()?;
            // Only sizes the `confirm` popup and tells whether a close_tab
            // or close_pane must ask; both have a fallback.
            (actions, snapshot_or_warn(context, err))
        }
        Category::JumpTo => (Vec::new(), Some(context.herdr.snapshot()?)),
    };
    let rows = catalog::rows(&Sources {
        plugin_id: &context.env.plugin_id,
        plugin_actions: &actions,
        config,
        snapshot: snapshot.as_ref(),
        home: context.env.home.as_deref(),
    });
    let current = snapshot
        .as_ref()
        .and_then(|snapshot| requests::targets(&context.env.context, snapshot).pane_id);
    let mut keyed: Vec<_> = rows
        .into_iter()
        .filter(|row| row.category() == category)
        .filter(|row| !settings.commands.hides(row))
        .filter(|row| !(row.kind == Kind::Pane && current == Some(row.payload.as_str())))
        .filter_map(|row| {
            history::usage_key(row.kind, &row.payload, &config.keys.command).map(|key| (key, row))
        })
        .collect();
    let used = context.history.usage_order(&session(context), category)?;
    // Newest first: the first key still in the catalog wins; older keys of
    // rows that are gone are skipped.
    let found = used
        .iter()
        .find_map(|id| keyed.iter().position(|(key, _)| key == id));
    Ok(found.map(|index| (keyed.swap_remove(index).1, snapshot)))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::app::fakes::{
        FakeHistoryStore, HistoryWrite, World, api_error, invalid_settings, parse_error,
    };
    use crate::cli::Command;
    use crate::domain::config::HerdrConfig;
    use crate::domain::fixtures::herdr_json;
    use crate::domain::row::RowId;
    use crate::domain::settings::{CommandSettings, HistorySettings};
    use crate::ports::HerdrError;

    use super::*;

    fn toasts(world: &World) -> Vec<String> {
        world.herdr.toasts()
    }

    fn confirming(world: &World, id: RowId) {
        world.settings.set(Ok(Settings {
            commands: CommandSettings {
                confirm_ids: vec![id],
                ..CommandSettings::default()
            },
            ..Settings::default()
        }));
    }

    #[test]
    fn says_history_is_off_without_reading_it() {
        let world = World::new();
        world.settings.set(Ok(Settings {
            history: HistorySettings {
                enabled: false,
                ..HistorySettings::default()
            },
            ..Settings::default()
        }));
        world.history.fail_with("must not be read");

        let status = world.run(&Command::RepeatCommand);

        assert_eq!(status, Ok(0));
        assert_eq!(toasts(&world), ["history is off"]);
        assert_eq!(world.err(), "herdr-command-palette: history is off\n");
    }

    #[test]
    fn says_when_there_is_nothing_to_repeat() {
        let world = World::new();

        world.run(&Command::RepeatCommand).unwrap();
        world.run(&Command::RepeatJump).unwrap();

        assert_eq!(
            toasts(&world),
            ["no command to repeat yet", "no jump to repeat yet"]
        );
    }

    #[test]
    fn reruns_the_newest_command_that_still_exists_and_records_it() {
        let world = World::new();
        world.history.set_usage_order(vec![
            RowId::new(Kind::Builtin, "teleport"),
            RowId::new(Kind::Pane, "w1:p2"),
            RowId::new(Kind::Builtin, "zoom"),
            RowId::new(Kind::Builtin, "split_vertical"),
        ]);

        let status = world.run(&Command::RepeatCommand);

        assert_eq!(status, Ok(0));
        assert!(world.herdr.last("pane.zoom").is_some());
        let usage = world.history.writes();
        assert!(
            matches!(&usage[..], [HistoryWrite::Usage { row, .. }] if *row == RowId::new(Kind::Builtin, "zoom")),
            "{usage:?}"
        );
    }

    #[test]
    fn repeats_a_row_kept_out_of_the_recent_order() {
        let world = World::new();
        world.history.set_usage_order(vec![
            RowId::new(Kind::Builtin, "reload_config"),
            RowId::new(Kind::Builtin, "zoom"),
        ]);

        world.run(&Command::RepeatCommand).unwrap();

        assert!(world.herdr.last("server.reload_config").is_some());
        assert!(world.herdr.last("pane.zoom").is_none());
    }

    #[test]
    fn jumps_to_the_newest_pane_still_open() {
        let world = World::new();
        world.history.set_usage_order(vec![
            RowId::new(Kind::Builtin, "zoom"),
            RowId::new(Kind::Pane, "w1:p9"),
            RowId::new(Kind::Pane, "w1:p2"),
        ]);

        world.run(&Command::RepeatJump).unwrap();

        assert_eq!(
            world.herdr.last("pane.focus").unwrap().params,
            json!({"pane_id": "w1:p2"})
        );
    }

    fn hiding(world: &World, hide_kinds: Vec<Kind>, hide_ids: Vec<RowId>) {
        world.settings.set(Ok(Settings {
            commands: CommandSettings {
                hide_kinds,
                hide_ids,
                ..CommandSettings::default()
            },
            ..Settings::default()
        }));
    }

    #[test]
    fn skips_commands_hidden_by_id_or_kind() {
        let world = World::new();
        hiding(
            &world,
            vec![Kind::Plugin],
            vec![RowId::new(Kind::Builtin, "zoom")],
        );
        world.history.set_usage_order(vec![
            RowId::new(Kind::Builtin, "zoom"),
            RowId::new(Kind::Plugin, "demo.hello"),
            RowId::new(Kind::Builtin, "split_vertical"),
        ]);

        world.run(&Command::RepeatCommand).unwrap();

        assert!(world.herdr.last("pane.zoom").is_none());
        assert!(world.herdr.last("plugin.action.invoke").is_none());
        assert!(world.herdr.last("pane.split").is_some());
    }

    #[test]
    fn says_nothing_to_repeat_when_every_used_command_is_hidden() {
        let world = World::new();
        hiding(&world, Vec::new(), vec![RowId::new(Kind::Builtin, "zoom")]);
        world
            .history
            .set_usage_order(vec![RowId::new(Kind::Builtin, "zoom")]);

        world.run(&Command::RepeatCommand).unwrap();

        assert!(world.herdr.last("pane.zoom").is_none());
        assert_eq!(toasts(&world), ["no command to repeat yet"]);
    }

    #[test]
    fn repeat_jump_skips_the_focused_pane_of_the_snapshot() {
        let world = World::new();
        world.history.set_usage_order(vec![
            RowId::new(Kind::Pane, "w1:p1"),
            RowId::new(Kind::Pane, "w1:p2"),
        ]);

        world.run(&Command::RepeatJump).unwrap();

        assert_eq!(
            world.herdr.last("pane.focus").unwrap().params,
            json!({"pane_id": "w1:p2"})
        );
    }

    #[test]
    fn repeat_jump_skips_the_pane_it_was_invoked_from() {
        let mut world = World::new();
        world.env.context.focused_pane_id = Some("w1:p2".into());
        world.history.set_usage_order(vec![
            RowId::new(Kind::Pane, "w1:p2"),
            RowId::new(Kind::Pane, "w1:p1"),
        ]);

        world.run(&Command::RepeatJump).unwrap();

        assert_eq!(
            world.herdr.last("pane.focus").unwrap().params,
            json!({"pane_id": "w1:p1"})
        );
    }

    #[test]
    fn repeat_jump_says_nothing_to_repeat_when_only_the_current_pane_was_used() {
        let world = World::new();
        world
            .history
            .set_usage_order(vec![RowId::new(Kind::Pane, "w1:p1")]);

        world.run(&Command::RepeatJump).unwrap();

        assert!(world.herdr.last("pane.focus").is_none());
        assert_eq!(toasts(&world), ["no jump to repeat yet"]);
    }

    #[test]
    fn repeats_custom_commands_found_by_their_command() {
        let world = World::new();
        let config = HerdrConfig::from_toml(
            "[[keys.command]]\ncommand = \"ls\"\n[[keys.command]]\ncommand = \"make\"\n",
        )
        .unwrap();
        let make = history::usage_key(Kind::Custom, "1", &config.keys.command).unwrap();
        world.config.set(Ok(config));
        world.history.set_usage_order(vec![make]);

        world.run(&Command::RepeatCommand).unwrap();

        assert_eq!(world.spawner.shell_detached()[0].0, "make");
    }

    #[test]
    fn a_confirm_row_opens_the_confirm_popup_instead_of_running() {
        let world = World::new();
        confirming(&world, RowId::new(Kind::Builtin, "zoom"));
        world
            .history
            .set_usage_order(vec![RowId::new(Kind::Builtin, "zoom")]);

        world.run(&Command::RepeatCommand).unwrap();

        assert!(world.herdr.last("pane.zoom").is_none());
        assert_eq!(
            world.herdr.last("plugin.pane.open").unwrap().params,
            json!({"plugin_id": "mmc.command-palette", "entrypoint": "confirm",
                   "env": {"HCP_CONFIRM": r#"{"kind":"builtin","payload":"zoom","title":"Toggle zoom"}"#},
                   "width": 64, "height": 6})
        );
        assert_eq!(world.history.writes(), []);
    }

    #[test]
    fn close_tab_on_a_workspaces_last_tab_always_asks_with_confirm_close() {
        let plain = World::new();
        let group = World::new();
        group.herdr.reply(
            "session.snapshot",
            Ok(herdr_json::worktree_group_snapshot()),
        );

        for world in [plain, group] {
            world
                .history
                .set_usage_order(vec![RowId::new(Kind::Builtin, "close_tab")]);

            world.run(&Command::RepeatCommand).unwrap();

            assert!(world.herdr.last("tab.close").is_none());
            assert_eq!(
                world.herdr.last("plugin.pane.open").unwrap().params["env"],
                json!({"HCP_CONFIRM": r#"{"kind":"builtin","payload":"close_tab","title":"Close tab"}"#})
            );
        }
    }

    #[test]
    fn enter_on_the_asked_close_tab_of_a_plain_workspace_closes_the_tab() {
        let world = World::new();
        world
            .history
            .set_usage_order(vec![RowId::new(Kind::Builtin, "close_tab")]);
        world.run(&Command::RepeatCommand).unwrap();

        // Enter in the `confirm` popup runs the row with `--confirmed`.
        world
            .run(&Command::Run {
                kind: Kind::Builtin,
                payload: "close_tab".into(),
                confirmed: true,
            })
            .unwrap();

        assert_eq!(
            world.herdr.last("tab.close").unwrap().params,
            json!({"tab_id": "w1:t1"})
        );
        assert!(world.herdr.last("workspace.close").is_none());
    }

    #[test]
    fn close_pane_on_a_worktree_groups_only_pane_always_asks_with_confirm_close() {
        let world = World::new();
        world.herdr.reply(
            "session.snapshot",
            Ok(herdr_json::worktree_group_snapshot()),
        );
        world
            .history
            .set_usage_order(vec![RowId::new(Kind::Builtin, "close_pane")]);

        world.run(&Command::RepeatCommand).unwrap();

        assert!(world.herdr.last("pane.close").is_none());
        assert_eq!(
            world.herdr.last("plugin.pane.open").unwrap().params["env"],
            json!({"HCP_CONFIRM": r#"{"kind":"builtin","payload":"close_pane","title":"Close pane"}"#})
        );
    }

    #[test]
    fn close_tab_repeats_unasked_without_confirm_close() {
        let world = World::new();
        world.herdr.reply(
            "session.snapshot",
            Ok(herdr_json::worktree_group_snapshot()),
        );
        world.config.set(Ok(
            HerdrConfig::from_toml("[ui]\nconfirm_close = false").unwrap()
        ));
        world
            .history
            .set_usage_order(vec![RowId::new(Kind::Builtin, "close_tab")]);

        world.run(&Command::RepeatCommand).unwrap();

        assert_eq!(
            world.herdr.last("tab.close").unwrap().params,
            json!({"tab_id": "w1:t1"})
        );
    }

    #[test]
    fn a_confirm_row_with_a_detail_opens_the_taller_popup() {
        let world = World::new();
        confirming(&world, RowId::new(Kind::Builtin, "close_workspace"));
        world
            .history
            .set_usage_order(vec![RowId::new(Kind::Builtin, "close_workspace")]);

        world.run(&Command::RepeatCommand).unwrap();

        let params = world.herdr.last("plugin.pane.open").unwrap().params;
        assert_eq!(
            (&params["width"], &params["height"]),
            (&json!(64), &json!(7))
        );
    }

    #[test]
    fn the_confirm_popup_falls_back_to_the_dialog_size_without_a_layout() {
        let world = World::new();
        confirming(&world, RowId::new(Kind::Builtin, "zoom"));
        world
            .history
            .set_usage_order(vec![RowId::new(Kind::Builtin, "zoom")]);
        world.herdr.reply(
            "session.snapshot",
            Ok(json!({"snapshot": {"workspaces": [], "tabs": [], "panes": []}})),
        );

        world.run(&Command::RepeatCommand).unwrap();

        let params = world.herdr.last("plugin.pane.open").unwrap().params;
        assert_eq!(
            (&params["width"], &params["height"]),
            (&json!(64), &json!(6))
        );
    }

    #[test]
    fn a_dry_run_prints_the_confirm_popup() {
        let mut world = World::new();
        world.env.dry_run = true;
        confirming(&world, RowId::new(Kind::Builtin, "zoom"));
        world
            .history
            .set_usage_order(vec![RowId::new(Kind::Builtin, "zoom")]);

        world.run(&Command::RepeatCommand).unwrap();

        assert_eq!(world.out().split(' ').next(), Some("plugin.pane.open"));
        assert!(world.herdr.last("plugin.pane.open").is_none());
    }

    #[test]
    fn a_confirm_popup_that_cannot_open_is_notified() {
        let world = World::new();
        confirming(&world, RowId::new(Kind::Builtin, "zoom"));
        world
            .history
            .set_usage_order(vec![RowId::new(Kind::Builtin, "zoom")]);
        let busy = api_error("ui_busy", "popup open");
        world.herdr.reply("plugin.pane.open", Err(busy.clone()));

        let error = world.run(&Command::RepeatCommand).unwrap_err();

        assert_eq!(error, Error::Herdr(busy));
        assert_eq!(toasts(&world), ["ui_busy: popup open"]);
    }

    #[test]
    fn store_and_config_errors_are_notified() {
        let world = World::new();
        world.history.fail_with("locked");
        let store = world.run(&Command::RepeatJump).unwrap_err();
        world.config.set(Err(parse_error()));
        let config = world.run(&Command::RepeatCommand).unwrap_err();

        assert_eq!(store, Error::History(FakeHistoryStore::error("locked")));
        assert_eq!(config, Error::Config(parse_error()));
        assert_eq!(toasts(&world).len(), 2);
    }

    #[test]
    fn a_failed_plugin_list_fails_instead_of_repeating_an_older_command() {
        let world = World::new();
        world.history.set_usage_order(vec![
            RowId::new(Kind::Plugin, "demo.hello"),
            RowId::new(Kind::Builtin, "zoom"),
        ]);
        let down = HerdrError::Transport("down".into());
        world.herdr.reply("plugin.action.list", Err(down.clone()));

        let error = world.run(&Command::RepeatCommand).unwrap_err();

        assert_eq!(error, Error::Herdr(down));
        assert!(world.herdr.last("pane.zoom").is_none());
        assert_eq!(toasts(&world), ["cannot talk to herdr: down"]);
    }

    #[test]
    fn a_failed_snapshot_fails_repeat_jump_instead_of_finding_nothing() {
        let world = World::new();
        world
            .history
            .set_usage_order(vec![RowId::new(Kind::Pane, "w1:p2")]);
        let down = HerdrError::Transport("down".into());
        world.herdr.reply("session.snapshot", Err(down.clone()));

        let error = world.run(&Command::RepeatJump).unwrap_err();

        assert_eq!(error, Error::Herdr(down));
        assert_eq!(
            world.herdr.methods(),
            ["session.snapshot", "notification.show"]
        );
    }

    #[test]
    fn a_failed_snapshot_still_opens_the_confirm_popup_with_a_warning() {
        let world = World::new();
        confirming(&world, RowId::new(Kind::Builtin, "zoom"));
        world
            .history
            .set_usage_order(vec![RowId::new(Kind::Builtin, "zoom")]);
        world.herdr.reply(
            "session.snapshot",
            Err(HerdrError::Transport("down".into())),
        );

        world.run(&Command::RepeatCommand).unwrap();

        let params = world.herdr.last("plugin.pane.open").unwrap().params;
        assert_eq!(
            (&params["width"], &params["height"]),
            (&json!(64), &json!(6))
        );
        assert_eq!(
            world.err(),
            "herdr-command-palette: session unavailable: cannot talk to herdr: down\n"
        );
    }

    #[test]
    fn invalid_settings_repeat_with_the_defaults_after_one_toast() {
        let world = World::new();
        world.settings.set(Err(invalid_settings()));
        world
            .history
            .set_usage_order(vec![RowId::new(Kind::Builtin, "zoom")]);

        world.run(&Command::RepeatCommand).unwrap();

        assert!(world.herdr.last("pane.zoom").is_some());
        assert_eq!(toasts(&world).len(), 1);
    }
}
