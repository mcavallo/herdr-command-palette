//! `open`: size the popup to the focused tab, within the settings'
//! `[popup]` bounds, and open the palette pane. The
//! manifest only accepts dimensions in cells or as a percentage.
//!
//! Errors go to stderr and a Herdr notification: `open` runs from a key
//! binding, so the user would otherwise see nothing. Invalid
//! settings are the exception: the popup opens at the default size and
//! the palette in it shows the problem, and nothing is pruned.
//!
//! Once the popup is showing, `open` prunes the history: closed panes
//! every time, rows past `max_age_days` at most hourly, never waiting on
//! busy hooks, and never failing the open. A snapshot without panes
//! prunes nothing.

use std::io::Write;
use std::time::Duration;

use crate::domain::herdr::{Area, Snapshot};
use crate::domain::history;
use crate::domain::popup;
use crate::domain::requests;
use crate::domain::settings::{HistorySettings, PopupSettings};

use super::{Context, Error, notify_failure, session, warn_unless_no_store};

/// Seconds in a day, for `max_age_days`.
const DAY_SECONDS: u64 = 24 * 60 * 60;

/// Opens the palette popup, or prints the request with `HCP_DRY_RUN=1`.
///
/// # Errors
///
/// [`Error::Herdr`] when the snapshot or `plugin.pane.open` fails (after a
/// notification, unless dry-running).
pub fn open(context: &Context<'_>, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Error> {
    // Invalid settings: the default size, silently (the palette shows the
    // problem), and no pruning: the defaults could delete history the file
    // asks to keep.
    let loaded = context.settings.load();
    let popup = loaded
        .as_ref()
        .map(|settings| settings.popup)
        .unwrap_or_default();
    let result = context
        .herdr
        .snapshot()
        .map_err(Error::from)
        .and_then(|snapshot| {
            let size = popup_size(context, &snapshot, popup);
            let request = requests::palette_popup(&context.env.plugin_id, size);
            if context.env.dry_run {
                writeln!(out, "{request}")?;
            } else {
                context.herdr.call(&request)?;
                if let Ok(settings) = &loaded {
                    prune(context, settings.history, &snapshot, err);
                }
            }
            Ok(())
        });
    if let Err(error) = &result {
        notify_failure(context, error);
    }
    result.map(|()| 0)
}

/// Prunes the history: a full prune (closed panes and old rows) if the
/// last one is at least [`PRUNE_INTERVAL`](history::PRUNE_INTERVAL) old,
/// else only the closed panes (storage hygiene; a reused pane id is
/// cleared by `history created`). Nothing when the snapshot has
/// no panes. The store waits ~20 ms for the write lock, then skips. Best
/// effort: errors are one line on `err`.
fn prune(
    context: &Context<'_>,
    history: HistorySettings,
    snapshot: &Snapshot,
    err: &mut dyn Write,
) {
    // No panes is never a real live set in a `workspace` context: pruning
    // against it would delete the session's whole history.
    if !history.enabled || snapshot.panes.is_empty() {
        return;
    }
    let live: Vec<&str> = snapshot
        .panes
        .iter()
        .map(|pane| pane.pane_id.as_str())
        .collect();
    // 0 keeps everything.
    let max_age = (history.max_age_days > 0)
        .then(|| Duration::from_secs(u64::from(history.max_age_days) * DAY_SECONDS));
    let now = context.clock.now();
    let session = session(context);
    // The age step at most hourly per session; closed panes on every open.
    let due = context
        .history
        .last_pruned_at(&session)
        .map(|last| history::is_prune_due(last, now));
    let pruned = match due {
        Ok(true) => context.history.prune(&session, &live, max_age, now),
        Ok(false) => context.history.prune_closed(&session, &live),
        Err(error) => Err(error),
    };
    if let Err(error) = pruned {
        warn_unless_no_store(err, &error);
    }
}

/// The popup size for the target tab's layout area, if Herdr reports one.
fn popup_size(
    context: &Context<'_>,
    snapshot: &Snapshot,
    bounds: PopupSettings,
) -> Option<popup::PopupSize> {
    tab_area(context, snapshot).map(|area| popup::popup_size(area, bounds.width(), bounds.height()))
}

/// The layout area of the tab a popup opens over (the invocation
/// context's, else the focused one), if Herdr reports it.
pub(crate) fn tab_area(context: &Context<'_>, snapshot: &Snapshot) -> Option<Area> {
    let tab = requests::targets(&context.env.context, snapshot).tab_id?;
    snapshot
        .layouts
        .iter()
        .find(|layout| layout.tab_id == tab)
        .map(|layout| layout.area)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use std::time::Duration;

    use crate::app::fakes::{FakeHistoryStore, World, api_error, invalid_settings};
    use crate::cli::Command;
    use crate::domain::fixtures::herdr_json;
    use crate::domain::herdr::InvocationContext;
    use crate::domain::settings::HistorySettings;
    use crate::domain::settings::Settings;
    use crate::ports::Clock;
    use crate::ports::HerdrError;

    use super::*;

    #[test]
    fn opens_the_palette_sized_to_the_focused_tab() {
        let world = World::new();

        let status = world.run(&Command::Open).unwrap();

        assert_eq!(status, 0);
        assert_eq!(
            world.herdr.last("plugin.pane.open").unwrap().params,
            json!({"plugin_id": "mmc.command-palette", "entrypoint": "palette",
                   "width": 76, "height": 22})
        );
    }

    #[test]
    fn sizes_the_popup_within_the_settings_bounds() {
        let world = World::new();
        world.settings.set(Ok(Settings {
            popup: PopupSettings {
                min_width: 40,
                max_width: 100,
                min_height: 10,
                max_height: 30,
            },
            ..Settings::default()
        }));

        world.run(&Command::Open).unwrap();

        let params = world.herdr.last("plugin.pane.open").unwrap().params;
        // The fixture's tab is 120 x 40: both maxes fit with the margins.
        assert_eq!(
            (&params["width"], &params["height"]),
            (&json!(100), &json!(30))
        );
    }

    #[test]
    fn invalid_settings_open_at_the_default_size_silently() {
        let world = World::new();
        world.settings.set(Err(invalid_settings()));

        world.run(&Command::Open).unwrap();

        assert_eq!(
            world.herdr.methods(),
            ["session.snapshot", "plugin.pane.open"]
        );
        assert_eq!(
            world.herdr.last("plugin.pane.open").unwrap().params["width"],
            76
        );
    }

    #[test]
    fn invalid_settings_do_not_prune() {
        let world = World::new();
        world.settings.set(Err(invalid_settings()));

        world.run(&Command::Open).unwrap();

        assert_eq!(world.history.pruned(), []);
        assert_eq!(world.history.pruned_closed(), Vec::<Vec<String>>::new());
    }

    #[test]
    fn uses_the_context_tab_and_falls_back_to_the_manifest_size() {
        let mut world = World::new();
        world.env.context = InvocationContext::from_json(r#"{"tab_id": "w9:t9"}"#);

        world.run(&Command::Open).unwrap();

        assert_eq!(
            world.herdr.last("plugin.pane.open").unwrap().params,
            json!({"plugin_id": "mmc.command-palette", "entrypoint": "palette"})
        );
    }

    #[test]
    fn dry_run_prints_the_request() {
        let mut world = World::new();
        world.env.dry_run = true;

        world.run(&Command::Open).unwrap();

        assert_eq!(
            world.out(),
            "plugin.pane.open {\"entrypoint\":\"palette\",\"height\":22,\
             \"plugin_id\":\"mmc.command-palette\",\"width\":76}\n"
        );
        assert_eq!(world.herdr.methods(), ["session.snapshot"]);
    }

    #[test]
    fn failures_are_notified() {
        let world = World::new();
        let busy = api_error("ui_busy", "popup open");
        world.herdr.reply("plugin.pane.open", Err(busy.clone()));

        let error = world.run(&Command::Open).unwrap_err();

        assert_eq!(error, Error::Herdr(busy));
        assert_eq!(
            world.herdr.last("notification.show").unwrap().params,
            json!({"title": "command-palette", "body": "ui_busy: popup open"})
        );
    }

    #[test]
    fn dry_run_failures_are_not_notified() {
        let mut world = World::new();
        world.env.dry_run = true;
        world.herdr.reply(
            "session.snapshot",
            Err(HerdrError::Transport("down".into())),
        );

        assert!(world.run(&Command::Open).is_err());
        assert_eq!(world.herdr.methods(), ["session.snapshot"]);
    }

    fn history(world: &World, history: HistorySettings) {
        world.settings.set(Ok(Settings {
            history,
            ..Settings::default()
        }));
    }

    #[test]
    fn prunes_closed_panes_and_old_rows_after_opening() {
        let world = World::new();

        world.run(&Command::Open).unwrap();

        assert_eq!(
            world.history.pruned(),
            [(
                vec!["w1:p1".to_owned(), "w1:p2".to_owned()],
                Some(Duration::from_secs(90 * DAY_SECONDS))
            )]
        );
    }

    #[test]
    fn max_age_0_keeps_old_rows() {
        let world = World::new();
        history(
            &world,
            HistorySettings {
                max_age_days: 0,
                ..HistorySettings::default()
            },
        );

        world.run(&Command::Open).unwrap();

        assert_eq!(world.history.pruned()[0].1, None);
    }

    #[test]
    fn prunes_old_rows_at_most_hourly() {
        let world = World::new();
        let recently = world.clock.now() - Duration::from_secs(60);
        world.history.set_last_pruned_at(Some(recently));

        world.run(&Command::Open).unwrap();

        assert_eq!(world.history.pruned(), []);
    }

    #[test]
    fn prunes_closed_panes_on_every_open() {
        let world = World::new();
        let recently = world.clock.now() - Duration::from_secs(60);
        world.history.set_last_pruned_at(Some(recently));

        world.run(&Command::Open).unwrap();

        assert_eq!(
            world.history.pruned_closed(),
            [vec!["w1:p1".to_owned(), "w1:p2".to_owned()]]
        );
    }

    #[test]
    fn a_full_prune_replaces_the_closed_panes_prune() {
        let world = World::new();

        world.run(&Command::Open).unwrap();

        assert_eq!(world.history.pruned().len(), 1);
        assert_eq!(world.history.pruned_closed(), Vec::<Vec<String>>::new());
    }

    #[test]
    fn prunes_again_once_an_hour_has_passed() {
        let world = World::new();
        let an_hour_ago = world.clock.now() - history::PRUNE_INTERVAL;
        world.history.set_last_pruned_at(Some(an_hour_ago));

        world.run(&Command::Open).unwrap();

        assert_eq!(world.history.pruned().len(), 1);
    }

    #[test]
    fn does_not_prune_when_history_is_off() {
        let world = World::new();
        history(
            &world,
            HistorySettings {
                enabled: false,
                ..HistorySettings::default()
            },
        );

        world.run(&Command::Open).unwrap();

        assert_eq!(world.history.pruned(), []);
        assert_eq!(world.history.pruned_closed(), Vec::<Vec<String>>::new());
    }

    #[test]
    fn does_not_prune_when_the_snapshot_has_no_panes() {
        let world = World::new();
        let mut snapshot = herdr_json::snapshot();
        snapshot["snapshot"]["panes"] = json!([]);
        world.herdr.reply("session.snapshot", Ok(snapshot));

        world.run(&Command::Open).unwrap();

        assert_eq!(world.history.pruned(), []);
        assert_eq!(world.history.pruned_closed(), Vec::<Vec<String>>::new());
        assert_eq!(world.err(), "");
    }

    #[test]
    fn does_not_prune_when_the_popup_did_not_open_or_in_a_dry_run() {
        let mut world = World::new();
        world
            .herdr
            .reply("plugin.pane.open", Err(api_error("ui_busy", "popup open")));
        world.run(&Command::Open).unwrap_err();
        world.env.dry_run = true;
        world.herdr.reply("plugin.pane.open", Ok(json!({})));

        world.run(&Command::Open).unwrap();

        assert_eq!(world.history.pruned(), []);
        assert_eq!(world.history.pruned_closed(), Vec::<Vec<String>>::new());
    }

    #[test]
    fn prune_errors_are_a_warning_only() {
        let world = World::new();
        world.history.fail_with("locked");

        let status = world.run(&Command::Open);

        assert_eq!(status, Ok(0));
        assert_eq!(
            world.err(),
            format!(
                "herdr-command-palette: history unavailable: {}\n",
                FakeHistoryStore::error("locked")
            )
        );
        assert_eq!(
            world.herdr.methods(),
            ["session.snapshot", "plugin.pane.open"]
        );
    }
}
