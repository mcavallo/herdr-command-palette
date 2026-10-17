//! `ask`: opens the `confirm` popup for a row the settings' `confirm_ids`
//! lists. The palette starts it detached when such a row is
//! picked, then exits: Herdr shows one popup at a time, so this process
//! waits out the palette's closing popup (`ui_busy`, retried like `run`)
//! and opens the dialog in its place.
//!
//! `open_confirm` is the one way to open that popup; the repeat actions
//! use it too. Errors are notified: nothing is on screen any more.

use std::io::Write;

use crate::app::open::tab_area;
use crate::domain::dialog;
use crate::domain::herdr::Snapshot;
use crate::domain::popup::{self, Bounds, PopupSize};
use crate::domain::requests;
use crate::domain::row::Row;

use super::{Context, Error, notify_failure, snapshot_or_warn};

/// The `confirm` popup's size for a dialog with a detail: Herdr's close
/// dialog, 64 by 6 (Herdr `src/client/shell/overlays.rs`, v0.9.3), one row
/// taller for the detail's second line. Matches `[[panes]]
/// confirm` in the manifest, the default the request overrides.
pub(crate) const CONFIRM_SIZE: PopupSize = PopupSize {
    width: 64,
    height: 7,
};

/// The `confirm` popup's size for a dialog without detail (title and
/// buttons): Herdr's 64 by 6.
pub(crate) const TITLE_ONLY_CONFIRM_SIZE: PopupSize = PopupSize {
    width: 64,
    height: 6,
};

/// Opens the `confirm` popup for `row`, or prints the request in a dry
/// run; returns the exit status.
///
/// # Errors
///
/// [`Error::Herdr`] when the popup can't open (Herdr is notified first,
/// except in a dry run), [`Error::Output`] when the dry run's request
/// can't be printed.
pub fn ask(
    context: &Context<'_>,
    row: &Row,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    // Only sizes the popup, which has a fallback.
    let snapshot = snapshot_or_warn(context, err);
    open_confirm(context, row, snapshot.as_ref(), out)
        .inspect_err(|error| notify_failure(context, error))
}

/// Opens the `confirm` popup for `row` (it runs the row on Enter), or
/// prints the request in a dry run. [`CONFIRM_SIZE`] when the dialog
/// has a detail, else [`TITLE_ONLY_CONFIRM_SIZE`] (the text from
/// `snapshot`, as the `confirm` pane will draw it), clamped to the
/// target tab's layout area in `snapshot` when known.
///
/// # Errors
///
/// [`Error::Herdr`] when `plugin.pane.open` fails, [`Error::Output`] when
/// the dry run's request can't be printed.
pub(crate) fn open_confirm(
    context: &Context<'_>,
    row: &Row,
    snapshot: Option<&Snapshot>,
    out: &mut dyn Write,
) -> Result<u8, Error> {
    let fixed = |size: u16| Bounds {
        min: size,
        max: size,
    };
    let wanted = match dialog::text(row, &context.env.context, snapshot).detail {
        Some(_) => CONFIRM_SIZE,
        None => TITLE_ONLY_CONFIRM_SIZE,
    };
    let size = snapshot
        .and_then(|snapshot| tab_area(context, snapshot))
        .map_or(wanted, |area| {
            popup::popup_size(area, fixed(wanted.width), fixed(wanted.height))
        });
    let request = requests::confirm_popup(&context.env.plugin_id, row, size);
    if context.env.dry_run {
        writeln!(out, "{request}")?;
    } else {
        context.herdr.call(&request)?;
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::app::fakes::{World, api_error};
    use crate::cli::Command;
    use crate::domain::row::Kind;
    use crate::ports::HerdrError;

    use super::*;

    fn ask_command() -> Command {
        Command::Ask {
            kind: Kind::Builtin,
            payload: "close_workspace".into(),
            title: "Close workspace".into(),
        }
    }

    #[test]
    fn opens_the_confirm_popup_at_herdrs_dialog_size() {
        let world = World::new();

        let status = world.run(&ask_command());

        assert_eq!(status, Ok(0));
        assert_eq!(
            world.herdr.last("plugin.pane.open").unwrap().params,
            json!({"plugin_id": "mmc.command-palette", "entrypoint": "confirm",
                   "env": {"HCP_CONFIRM": r#"{"kind":"builtin","payload":"close_workspace","title":"Close workspace"}"#},
                   "width": 64, "height": 7})
        );
        assert_eq!(world.spawner.detached(), []);
    }

    #[test]
    fn opens_herdrs_64_by_6_popup_for_a_dialog_without_detail() {
        let world = World::new();

        world
            .run(&Command::Ask {
                kind: Kind::Builtin,
                payload: "zoom".into(),
                title: "Toggle zoom".into(),
            })
            .unwrap();

        let params = world.herdr.last("plugin.pane.open").unwrap().params;
        assert_eq!(
            (&params["width"], &params["height"]),
            (&json!(64), &json!(6))
        );
    }

    #[test]
    fn clamps_the_popup_to_a_small_tab() {
        let world = World::new();
        world.herdr.reply(
            "session.snapshot",
            Ok(json!({"snapshot": {
                "focused_tab_id": "w1:t1",
                "workspaces": [], "tabs": [], "panes": [],
                "layouts": [{"tab_id": "w1:t1",
                             "area": {"x": 0, "y": 0, "width": 40, "height": 4}}]
            }})),
        );

        world.run(&ask_command()).unwrap();

        let params = world.herdr.last("plugin.pane.open").unwrap().params;
        assert_eq!(
            (&params["width"], &params["height"]),
            (&json!(40), &json!(4))
        );
    }

    #[test]
    fn opens_at_the_fixed_size_with_a_warning_without_a_snapshot() {
        let world = World::new();
        world.herdr.reply(
            "session.snapshot",
            Err(HerdrError::Transport("down".into())),
        );

        world.run(&ask_command()).unwrap();

        // No snapshot, no detail: the title-only size.
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
    fn a_popup_that_cannot_open_is_notified() {
        let world = World::new();
        let busy = api_error("ui_busy", "popup open");
        world.herdr.reply("plugin.pane.open", Err(busy.clone()));

        let error = world.run(&ask_command()).unwrap_err();

        assert_eq!(error, Error::Herdr(busy));
        assert_eq!(world.herdr.toasts(), ["ui_busy: popup open"]);
    }

    #[test]
    fn a_dry_run_prints_the_request() {
        let mut world = World::new();
        world.env.dry_run = true;

        world.run(&ask_command()).unwrap();

        assert_eq!(world.out().split(' ').next(), Some("plugin.pane.open"));
        assert!(world.herdr.last("plugin.pane.open").is_none());
    }
}
