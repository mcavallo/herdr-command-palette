//! `history focus`, `history move` and `history created`: the
//! `pane.focused`, `pane.moved` and `pane.created` event hooks.
//!
//! Hooks run on every pane switch, so they are fast and silent: one store
//! write, no output, exit 0. They also exit 0 when something goes wrong,
//! with one line on stderr (Herdr's plugin log) and no notification: a lost
//! focus timestamp is harmless, a toast on every pane switch is not.
//!
//! With `[history] enabled = false` they exit at once, before opening the
//! store. Invalid settings count as the defaults (history on), silently:
//! a broken file must not log a line on every pane switch; the palette
//! shows the problem.
//!
//! Reading history (recent orders, repeat actions) and pruning are not
//! here.

use std::io::Write;

use crate::domain::herdr::{PaneCreated, PaneMoved};
use crate::domain::history;

use super::{Context, session, warn};

/// Whether the settings turn history off. Invalid settings don't
/// (see the module docs).
fn is_off(context: &Context<'_>) -> bool {
    context
        .settings
        .load()
        .is_ok_and(|settings| !settings.history.enabled)
}

/// Records the focused pane (`HERDR_PANE_ID`) for this session. Returns
/// the exit status: always 0, failures go to `err` (see the module docs).
pub fn focus(context: &Context<'_>, err: &mut dyn Write) -> u8 {
    if is_off(context) {
        return 0;
    }
    let Some(pane) = context.env.pane_id.as_deref() else {
        warn(err, "history focus: HERDR_PANE_ID is not set");
        return 0;
    };
    let session = session(context);
    if let Err(error) = context
        .history
        .record_focus(&session, pane, context.clock.now())
    {
        warn(err, &format!("history focus: {error}"));
    }
    0
}

/// Carries a moved pane's history over to its new id
/// (`HERDR_PLUGIN_EVENT_JSON`); nothing to do when the id didn't change.
/// Returns the exit status: always 0, failures go to `err` (see the module
/// docs).
pub fn moved(context: &Context<'_>, err: &mut dyn Write) -> u8 {
    if is_off(context) {
        return 0;
    }
    let event = context.env.event_json.as_deref().unwrap_or_default();
    let Some(moved) = PaneMoved::from_event_json(event) else {
        warn(
            err,
            "history move: HERDR_PLUGIN_EVENT_JSON is not a pane.moved event",
        );
        return 0;
    };
    if !moved.has_new_id() {
        return 0;
    }
    let session = session(context);
    if let Err(error) =
        context
            .history
            .rename_pane(&session, &moved.previous_pane_id, &moved.pane_id)
    {
        warn(err, &format!("history move: {error}"));
    }
    0
}

/// Forgets history an earlier pane left under a new pane's id
/// (`HERDR_PLUGIN_EVENT_JSON`): Herdr reuses ids after a restart. Rows
/// from the last [`CREATED_GRACE`](history::CREATED_GRACE) stay: the new
/// pane's own `pane.focused` hook may have run first. Returns the exit
/// status: always 0, failures go to `err` (see the module docs).
pub fn created(context: &Context<'_>, err: &mut dyn Write) -> u8 {
    if is_off(context) {
        return 0;
    }
    let event = context.env.event_json.as_deref().unwrap_or_default();
    let Some(created) = PaneCreated::from_event_json(event) else {
        warn(
            err,
            "history created: HERDR_PLUGIN_EVENT_JSON is not a pane.created event",
        );
        return 0;
    };
    let session = session(context);
    let before = history::created_cutoff(context.clock.now());
    if let Err(error) = context
        .history
        .forget_pane(&session, &created.pane_id, before)
    {
        warn(err, &format!("history created: {error}"));
    }
    0
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::app::fakes::{FakeHistoryStore, HistoryWrite, World, invalid_settings};
    use crate::cli::{Command, HistoryCommand};
    use crate::domain::history::CREATED_GRACE;
    use crate::domain::settings::{HistorySettings, Settings};
    use crate::ports::Clock;

    const FOCUS: Command = Command::History(HistoryCommand::Focus);
    const MOVE: Command = Command::History(HistoryCommand::Move);
    const CREATED: Command = Command::History(HistoryCommand::Created);

    fn world() -> World {
        let mut world = World::new();
        world.env.socket_path = PathBuf::from("/c/herdr.sock");
        world
    }

    fn moved_event(from: &str, to: &str) -> String {
        format!(
            r#"{{"event": "pane_moved", "data": {{"type": "pane_moved",
                "previous_pane_id": "{from}", "pane": {{"pane_id": "{to}"}}}}}}"#
        )
    }

    fn created_event(pane: &str) -> String {
        format!(
            r#"{{"event": "pane_created", "data": {{"type": "pane_created",
                "pane": {{"pane_id": "{pane}"}}}}}}"#
        )
    }

    #[test]
    fn focus_records_the_pane_for_the_session() {
        let mut world = world();
        world.env.pane_id = Some("w1:p2".into());

        let status = world.run(&FOCUS);

        assert_eq!(status, Ok(0));
        assert_eq!(
            world.history.writes(),
            [HistoryWrite::Focus {
                session: "/c/herdr.sock".into(),
                pane: "w1:p2".into(),
                at: world.clock.now(),
            }]
        );
        assert_eq!(world.out(), "");
        assert_eq!(world.err(), "");
    }

    #[test]
    fn focus_without_a_pane_id_warns_and_exits_0() {
        let world = world();

        let status = world.run(&FOCUS);

        assert_eq!(status, Ok(0));
        assert_eq!(world.history.writes(), Vec::<HistoryWrite>::new());
        assert_eq!(
            world.err(),
            "herdr-command-palette: history focus: HERDR_PANE_ID is not set\n"
        );
    }

    #[test]
    fn focus_store_errors_warn_without_notifying() {
        let mut world = world();
        world.env.pane_id = Some("w1:p2".into());
        world.history.fail_with("database is locked");

        let status = world.run(&FOCUS);

        assert_eq!(status, Ok(0));
        assert_eq!(
            world.err(),
            format!(
                "herdr-command-palette: history focus: {}\n",
                FakeHistoryStore::error("database is locked")
            )
        );
        assert_eq!(world.herdr.methods(), Vec::<String>::new());
    }

    fn history_off(world: &World) {
        world.settings.set(Ok(Settings {
            history: HistorySettings {
                enabled: false,
                ..HistorySettings::default()
            },
            ..Settings::default()
        }));
    }

    #[test]
    fn focus_records_nothing_when_history_is_off() {
        let mut world = world();
        world.env.pane_id = Some("w1:p2".into());
        history_off(&world);
        world.history.fail_with("must not be opened");

        let status = world.run(&FOCUS);

        assert_eq!(status, Ok(0));
        assert_eq!(world.err(), "");
    }

    #[test]
    fn move_renames_nothing_when_history_is_off() {
        let mut world = world();
        world.env.event_json = Some(moved_event("w1:p2", "w2:p1"));
        history_off(&world);

        let status = world.run(&MOVE);

        assert_eq!(status, Ok(0));
        assert_eq!(world.history.writes(), Vec::<HistoryWrite>::new());
    }

    #[test]
    fn focus_records_with_invalid_settings_silently() {
        let mut world = world();
        world.env.pane_id = Some("w1:p2".into());
        world.settings.set(Err(invalid_settings()));

        world.run(&FOCUS).unwrap();

        assert_eq!(world.history.writes().len(), 1);
        assert_eq!(world.err(), "");
        assert_eq!(world.herdr.methods(), Vec::<String>::new());
    }

    #[test]
    fn move_to_another_workspace_renames_the_pane() {
        let mut world = world();
        world.env.event_json = Some(moved_event("w1:p2", "w2:p1"));

        let status = world.run(&MOVE);

        assert_eq!(status, Ok(0));
        assert_eq!(
            world.history.writes(),
            [HistoryWrite::Rename {
                session: "/c/herdr.sock".into(),
                from: "w1:p2".into(),
                to: "w2:p1".into(),
            }]
        );
        assert_eq!(world.err(), "");
    }

    #[test]
    fn move_within_the_workspace_does_nothing() {
        let mut world = world();
        world.env.event_json = Some(moved_event("w1:p2", "w1:p2"));

        let status = world.run(&MOVE);

        assert_eq!(status, Ok(0));
        assert_eq!(world.history.writes(), Vec::<HistoryWrite>::new());
        assert_eq!(world.err(), "");
    }

    #[test]
    fn move_without_a_move_event_warns_and_exits_0() {
        let world = world();

        let status = world.run(&MOVE);

        assert_eq!(status, Ok(0));
        assert_eq!(
            world.err(),
            "herdr-command-palette: history move: HERDR_PLUGIN_EVENT_JSON is not a pane.moved event\n"
        );
    }

    #[test]
    fn move_store_errors_warn_without_notifying() {
        let mut world = world();
        world.env.event_json = Some(moved_event("w1:p2", "w2:p1"));
        world.history.fail_with("disk full");

        let status = world.run(&MOVE);

        assert_eq!(status, Ok(0));
        assert_eq!(
            world.err(),
            format!(
                "herdr-command-palette: history move: {}\n",
                FakeHistoryStore::error("disk full")
            )
        );
        assert_eq!(world.herdr.methods(), Vec::<String>::new());
    }

    #[test]
    fn created_forgets_the_new_panes_older_history() {
        let mut world = world();
        world.env.event_json = Some(created_event("w3:p1"));

        let status = world.run(&CREATED);

        assert_eq!(status, Ok(0));
        assert_eq!(
            world.history.writes(),
            [HistoryWrite::Forget {
                session: "/c/herdr.sock".into(),
                pane: "w3:p1".into(),
                before: world.clock.now() - CREATED_GRACE,
            }]
        );
        assert_eq!(world.out(), "");
        assert_eq!(world.err(), "");
    }

    #[test]
    fn created_forgets_nothing_when_history_is_off() {
        let mut world = world();
        world.env.event_json = Some(created_event("w3:p1"));
        history_off(&world);
        world.history.fail_with("must not be opened");

        let status = world.run(&CREATED);

        assert_eq!(status, Ok(0));
        assert_eq!(world.err(), "");
    }

    #[test]
    fn created_without_a_created_event_warns_and_exits_0() {
        let mut world = world();
        world.env.event_json = Some(moved_event("w1:p2", "w2:p1"));

        let status = world.run(&CREATED);

        assert_eq!(status, Ok(0));
        assert_eq!(world.history.writes(), Vec::<HistoryWrite>::new());
        assert_eq!(
            world.err(),
            "herdr-command-palette: history created: HERDR_PLUGIN_EVENT_JSON is not a pane.created event\n"
        );
    }

    #[test]
    fn created_store_errors_warn_without_notifying() {
        let mut world = world();
        world.env.event_json = Some(created_event("w3:p1"));
        world.history.fail_with("disk full");

        let status = world.run(&CREATED);

        assert_eq!(status, Ok(0));
        assert_eq!(
            world.err(),
            format!(
                "herdr-command-palette: history created: {}\n",
                FakeHistoryStore::error("disk full")
            )
        );
        assert_eq!(world.herdr.methods(), Vec::<String>::new());
    }
}
