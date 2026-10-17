//! The palette's "recent" orders (settings `[commands] order`,
//! `[jump_to] order`): reads the history and
//! hands it to [`domain::recent`](crate::domain::recent).
//!
//! Rows in `[commands] skip_recent_ids` keep their fixed place;
//! they are still recorded, so `repeat-command` repeats them.
//!
//! History is a nicety: when it is off, or the store can't be read, the
//! rows keep their fixed (Herdr) order. A store error is one line on
//! `err`; a missing plugin state dir (a palette started by hand) is not
//! worth one.

use std::io::Write;

use crate::domain::config::HerdrConfig;
use crate::domain::herdr::Snapshot;
use crate::domain::history;
use crate::domain::recent;
use crate::domain::requests;
use crate::domain::row::{Category, Row};
use crate::domain::settings::{CommandOrder, JumpToOrder, Settings};

use super::{Context, session, warn_unless_no_store};

/// Orders `rows` as the settings say. Jump to puts the current pane (the
/// one the palette was opened from) last.
pub(crate) fn order(
    context: &Context<'_>,
    settings: &Settings,
    config: &HerdrConfig,
    snapshot: Option<&Snapshot>,
    rows: &mut [Row],
    err: &mut dyn Write,
) {
    if !settings.history.enabled {
        return;
    }
    let session = session(context);
    if settings.commands.order == CommandOrder::Recent {
        match context.history.usage_order(&session, Category::Commands) {
            Ok(used) => recent::order_commands(rows, &used, |row| {
                // Matched on the settings id (`custom:<description>`), not
                // the usage key, which for custom rows is a hash.
                if settings.commands.skips_recent(row) {
                    return None;
                }
                history::usage_key(row.kind, &row.payload, &config.keys.command)
            }),
            Err(error) => warn_unless_no_store(err, &error),
        }
    }
    if settings.jump_to.order == JumpToOrder::Recent {
        match context.history.focus_order(&session) {
            Ok(focused) => {
                let current = snapshot
                    .and_then(|snapshot| requests::targets(&context.env.context, snapshot).pane_id);
                recent::order_panes(rows, &focused, current);
            }
            Err(error) => warn_unless_no_store(err, &error),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::catalog_rows;
    use crate::app::fakes::{FakeHistoryStore, World};
    use crate::domain::row::{Kind, RowId};
    use crate::domain::settings::{CommandSettings, HistorySettings, JumpToSettings};

    use super::*;

    const CONFIG: &str = "[[keys.command]]\ntype = \"popup\"\ncommand = \"lazygit\"\n";

    /// The palette's rows for `world` in `settings`' order, as `kind:payload`,
    /// and what went to `err`.
    fn ordered(world: &World, settings: &Settings) -> (Vec<String>, String) {
        let config = HerdrConfig::from_toml(CONFIG).unwrap();
        let mut err = Vec::new();
        let context = world.context();
        let (mut rows, snapshot) = catalog_rows(&context, &config, &mut err);
        order(
            &context,
            settings,
            &config,
            snapshot.as_ref(),
            &mut rows,
            &mut err,
        );
        let ids = rows
            .iter()
            .map(|row| format!("{}:{}", row.kind, row.payload))
            .collect();
        (ids, String::from_utf8(err).unwrap())
    }

    fn first_command(ids: &[String]) -> &str {
        &ids[0]
    }

    fn panes(ids: &[String]) -> Vec<&str> {
        ids.iter()
            .filter(|id| id.starts_with("pane:"))
            .map(String::as_str)
            .collect()
    }

    fn custom_key() -> RowId {
        let config = HerdrConfig::from_toml(CONFIG).unwrap();
        history::usage_key(Kind::Custom, "0", &config.keys.command).unwrap()
    }

    #[test]
    fn recently_run_commands_come_first() {
        let world = World::new();
        world
            .history
            .set_usage_order(vec![RowId::new(Kind::Builtin, "zoom")]);

        let (ids, err) = ordered(&world, &Settings::default());

        assert_eq!(first_command(&ids), "builtin:zoom");
        assert_eq!(err, "");
    }

    #[test]
    fn custom_commands_match_their_history_by_command() {
        let world = World::new();
        world.history.set_usage_order(vec![custom_key()]);

        let (ids, _) = ordered(&world, &Settings::default());

        assert_eq!(first_command(&ids), "custom:0");
    }

    /// The rows' fixed order: `ids` with history off.
    fn fixed_order(world: &World) -> Vec<String> {
        let settings = Settings {
            history: HistorySettings {
                enabled: false,
                ..HistorySettings::default()
            },
            ..Settings::default()
        };
        ordered(world, &settings).0
    }

    fn commands(ids: &[String]) -> Vec<&str> {
        ids.iter()
            .filter(|id| !id.starts_with("pane:"))
            .map(String::as_str)
            .collect()
    }

    /// The Commands rows: `first`, then the fixed order without it.
    fn first_then_fixed(world: &World, first: &str) -> Vec<String> {
        let mut ids = vec![first.to_owned()];
        let fixed = fixed_order(world);
        ids.extend(
            commands(&fixed)
                .into_iter()
                .filter(|id| *id != first)
                .map(str::to_owned),
        );
        ids
    }

    #[test]
    fn skipped_rows_keep_their_fixed_place_after_newer_use() {
        let world = World::new();
        world.history.set_usage_order(vec![
            RowId::new(Kind::Builtin, "reload_config"),
            RowId::new(Kind::Builtin, "zoom"),
        ]);

        let (ids, _) = ordered(&world, &Settings::default());

        assert_eq!(commands(&ids), first_then_fixed(&world, "builtin:zoom"));
    }

    #[test]
    fn custom_commands_are_skipped_by_their_settings_id() {
        let world = World::new();
        world
            .history
            .set_usage_order(vec![custom_key(), RowId::new(Kind::Builtin, "zoom")]);
        let settings = Settings {
            commands: CommandSettings {
                skip_recent_ids: vec![RowId::new(Kind::Custom, "lazygit")],
                ..CommandSettings::default()
            },
            ..Settings::default()
        };

        let (ids, _) = ordered(&world, &settings);

        assert_eq!(commands(&ids), first_then_fixed(&world, "builtin:zoom"));
    }

    #[test]
    fn the_fixed_order_ignores_history() {
        let world = World::new();
        world
            .history
            .set_usage_order(vec![RowId::new(Kind::Builtin, "zoom")]);
        let settings = Settings {
            commands: CommandSettings {
                order: CommandOrder::Fixed,
                ..CommandSettings::default()
            },
            ..Settings::default()
        };

        let (ids, _) = ordered(&world, &settings);

        assert_eq!(first_command(&ids), "plugin:demo.hello");
    }

    #[test]
    fn recently_focused_panes_come_first_and_the_current_one_last() {
        let world = World::new();
        world.history.set_focus_order(&["w1:p1", "w1:p2"]);

        let (ids, _) = ordered(&world, &Settings::default());

        assert_eq!(panes(&ids), ["pane:w1:p2", "pane:w1:p1"]);
    }

    #[test]
    fn the_herdr_order_ignores_focus_history() {
        let world = World::new();
        world.history.set_focus_order(&["w1:p2"]);
        let settings = Settings {
            jump_to: JumpToSettings {
                order: JumpToOrder::Herdr,
            },
            ..Settings::default()
        };

        let (ids, _) = ordered(&world, &settings);

        assert_eq!(panes(&ids), ["pane:w1:p1", "pane:w1:p2"]);
    }

    #[test]
    fn history_off_keeps_the_fixed_orders_without_reading_it() {
        let world = World::new();
        world.history.fail_with("must not be read");
        let settings = Settings {
            history: HistorySettings {
                enabled: false,
                ..HistorySettings::default()
            },
            ..Settings::default()
        };

        let (ids, err) = ordered(&world, &settings);

        assert_eq!(first_command(&ids), "plugin:demo.hello");
        assert_eq!(panes(&ids), ["pane:w1:p1", "pane:w1:p2"]);
        assert_eq!(err, "");
    }

    #[test]
    fn store_errors_keep_the_fixed_orders_with_a_warning() {
        let world = World::new();
        world.history.fail_with("disk on fire");

        let (ids, err) = ordered(&world, &Settings::default());

        assert_eq!(panes(&ids), ["pane:w1:p1", "pane:w1:p2"]);
        let warning = format!(
            "herdr-command-palette: history unavailable: {}\n",
            FakeHistoryStore::error("disk on fire")
        );
        assert_eq!(err, warning.repeat(2));
    }

    #[test]
    fn no_store_is_not_worth_a_warning() {
        let world = World::new();
        world.history.fail_without_store();

        let (_, err) = ordered(&world, &Settings::default());

        assert_eq!(err, "");
    }
}
