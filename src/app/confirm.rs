//! `confirm`: the popup that asks before a row the settings' `confirm_ids`
//! lists runs (`[[panes]] confirm`), opened by `ask` (palette) or a repeat
//! action through `ask::open_confirm`.
//! It draws Herdr's close dialog, accent colored, so it looks native:
//! the title, for closes the workspace's label and the counts on two
//! lines (from a snapshot, best effort), the buttons.
//! Enter runs the row as a detached `run --confirmed` once the popup has
//! closed (a `run` that can't start is a notification, not drawn); any
//! other key, or no terminal, closes it.
//!
//! The row arrives in `HCP_CONFIRM` (`requests::confirm_popup`).

use std::fmt::Write as _;
use std::io::Write;

use crate::app::{palette, run_row};
use crate::domain::dialog::{self, DialogText};
use crate::domain::env::HCP_CONFIRM_ENV;
use crate::domain::layout;
use crate::domain::requests::{self, Confirmation};
use crate::domain::row::Row;
use crate::domain::style::DialogColors;
use crate::domain::theme::ThemeColors;

use super::{CLOSE_HINT, Context, Error, NOTHING_RECEIVED, PROGRAM, notify_failure};

/// Columns assumed when the terminal doesn't say: the inside of the
/// 64-column popup, whose frame takes 3 (Herdr `src/popup_size.rs`,
/// v0.9.3).
const FALLBACK_COLUMNS: usize = 61;

/// Asks, then runs the row on Enter; returns the exit status.
///
/// # Errors
///
/// [`Error::Output`] when the screen can't be written; with `HCP_DRY_RUN=1`,
/// whatever [`run_row`](crate::app::run_row::run_row) returns.
pub fn confirm(
    context: &Context<'_>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let Some(row) = context
        .env
        .confirm_row
        .as_deref()
        .and_then(requests::confirm_row)
    else {
        let message = format!("{PROGRAM}: no row received ({HCP_CONFIRM_ENV} unset or invalid)");
        writeln!(out, "{message}\n\n{CLOSE_HINT}")?;
        out.flush()?;
        context.terminal.wait_for_key();
        return Ok(NOTHING_RECEIVED);
    };
    // Broken files only cost the colors here; `run` reports them.
    let (theme, _) = context
        .config
        .load()
        .map_or((ThemeColors::TERMINAL_DEFAULT, None), |config| {
            palette::theme_colors(context, &config)
        });
    let colors = DialogColors::new(theme);
    // Only the detail needs it; without it, the title alone. Nothing
    // on `err`: it would land in the dialog.
    let snapshot = context.herdr.snapshot().ok();
    let text = dialog::text(&row, &context.env.context, snapshot.as_ref());
    let columns = context.terminal.columns().unwrap_or(FALLBACK_COLUMNS);
    out.write_all(screen(&text, columns, colors).as_bytes())?;
    out.flush()?;
    if !matches!(context.terminal.read_key(), Some(b'\r' | b'\n')) {
        return Ok(0);
    }
    run_confirmed(context, &row, out, err)
}

/// Runs `row` confirmed: in-process with `HCP_DRY_RUN=1` (prints the
/// request), else as a detached `run --confirmed` that outlives the popup.
/// A `run` that can't start is not drawn in the 4- or 5-row pane: it is a
/// Herdr notification and the returned error (stderr, exit status 1).
fn run_confirmed(
    context: &Context<'_>,
    row: &Row,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    if context.env.dry_run {
        return run_row::run_row(
            context,
            row.kind,
            &row.payload,
            Confirmation::Confirmed,
            out,
            err,
        );
    }
    let args = ["run", "--confirmed", "--", row.kind.as_str(), &row.payload];
    let detached = palette::detached(context, &args);
    context.spawner.spawn_detached(&detached).map_err(|error| {
        let error = Error::from(error);
        notify_failure(context, &error);
        error
    })?;
    Ok(0)
}

/// The dialog's screen: the background everywhere, cursor hidden, each of
/// [`layout::dialog`]'s lines placed at its row. No newlines: the last
/// line fills the pane's last row, and a newline after it would scroll
/// the dialog up.
fn screen(text: &DialogText, columns: usize, colors: DialogColors) -> String {
    // The background first, so clearing paints the screen with it.
    let mut screen = colors.background.to_ansi_bg();
    screen.push_str(layout::CLEAR_SCREEN);
    for (index, line) in layout::dialog(text, columns, colors).iter().enumerate() {
        // Writing to a String can't fail.
        let _ = write!(screen, "\x1b[{};1H{line}", index + 1);
    }
    screen
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::app::fakes::{World, spawn_error};
    use crate::cli::Command;
    use crate::domain::appearance::Appearance;
    use crate::domain::color::Color;
    use crate::domain::config::HerdrConfig;
    use crate::domain::dialog::{DialogDetail, DialogLine};
    use crate::domain::settings::{ColorSettings, Settings};
    use crate::domain::theme;
    use crate::ports::HerdrError;

    use super::*;

    fn world(row: &str) -> World {
        let mut world = World::new();
        world.env.confirm_row = Some(row.into());
        world
    }

    /// The dialog the default world's theme (catppuccin) draws for `title`
    /// and `detail` (label, counts) in `columns` (wide enough for both: nothing is cut).
    fn expected(title: &str, detail: Option<(&str, &str)>, columns: usize) -> String {
        let theme = theme::theme_colors(&HerdrConfig::default().theme, None);
        let text = DialogText {
            title: DialogLine::new(title),
            detail: detail.map(|(label, counts)| DialogDetail::new(label, counts)),
        };
        screen(&text, columns, DialogColors::new(theme))
    }

    #[test]
    fn draws_herdrs_close_dialog_with_the_workspace_detail() {
        let world =
            world(r#"{"kind":"builtin","payload":"close_workspace","title":"Close workspace"}"#);

        let status = world.run(&Command::Confirm);

        assert_eq!(status, Ok(0));
        assert_eq!(
            world.out(),
            expected("Close workspace?", Some(("dotfiles", "2 panes")), 76)
        );
        assert_eq!(world.spawner.detached(), []);
    }

    #[test]
    fn places_each_line_on_its_row_without_newlines() {
        let world = world(r#"{"kind":"builtin","payload":"zoom","title":"Toggle zoom"}"#);

        world.run(&Command::Confirm).unwrap();

        let screen = world.out();
        assert!(screen.starts_with("\x1b[48;2;24;24;37m\x1b[?25l\x1b[2J\x1b[H\x1b[1;1H"));
        // No detail: four lines, the buttons on the 64 by 6 popup's last row.
        assert!(screen.contains("\x1b[4;1H"));
        assert!(!screen.contains("\x1b[5;1H"));
        assert!(!screen.contains('\n'), "{screen:?}");
        assert!(screen.contains(" Toggle zoom?"));
    }

    #[test]
    fn shows_the_title_alone_without_a_snapshot() {
        let world =
            world(r#"{"kind":"builtin","payload":"close_workspace","title":"Close workspace"}"#);
        world.herdr.reply(
            "session.snapshot",
            Err(HerdrError::Transport("down".into())),
        );

        world.run(&Command::Confirm).unwrap();

        assert_eq!(world.out(), expected("Close workspace?", None, 76));
        assert_eq!(world.err(), "");
    }

    #[test]
    fn fills_the_popup_inside_when_the_terminal_has_no_width() {
        let world = world(r#"{"kind":"builtin","payload":"zoom","title":"Toggle zoom"}"#);
        world.terminal.set_columns(None);

        world.run(&Command::Confirm).unwrap();

        assert_eq!(world.out(), expected("Toggle zoom?", None, 61));
    }

    #[test]
    fn runs_the_row_detached_on_enter() {
        let world =
            world(r#"{"kind":"builtin","payload":"close_workspace","title":"Close workspace"}"#);
        world.terminal.set_key(Some(b'\r'));

        let status = world.run(&Command::Confirm);

        assert_eq!(status, Ok(0));
        let detached = world.spawner.detached();
        assert_eq!(detached.len(), 1);
        assert_eq!(
            detached[0].args,
            ["run", "--confirmed", "--", "builtin", "close_workspace"]
        );
    }

    #[test]
    fn a_run_that_cannot_start_is_notified_not_drawn() {
        let world =
            world(r#"{"kind":"builtin","payload":"close_workspace","title":"Close workspace"}"#);
        world.terminal.set_key(Some(b'\r'));
        world.spawner.set_detach(Err(spawn_error("/bin/hcp")));

        let result = world.run(&Command::Confirm);

        assert_eq!(result, Err(Error::Spawn(spawn_error("/bin/hcp"))));
        assert_eq!(
            world.out(),
            expected("Close workspace?", Some(("dotfiles", "2 panes")), 76)
        );
        assert_eq!(world.herdr.toasts(), ["cannot start /bin/hcp: not found"]);
        assert_eq!(world.terminal.key_waits(), 1);
    }

    #[test]
    fn other_keys_or_no_terminal_cancel() {
        let world =
            world(r#"{"kind":"builtin","payload":"close_workspace","title":"Close workspace"}"#);

        for key in [Some(0x1b), Some(b'y'), None] {
            world.terminal.set_key(key);

            assert_eq!(world.run(&Command::Confirm), Ok(0));
        }

        assert_eq!(world.spawner.detached(), []);
        assert_eq!(world.terminal.key_waits(), 3);
    }

    #[test]
    fn runs_in_process_on_enter_in_a_dry_run() {
        let mut world = world(r#"{"kind":"pane","payload":"w1:p2","title":"logs"}"#);
        world.env.dry_run = true;
        world.terminal.set_key(Some(b'\n'));

        world.run(&Command::Confirm).unwrap();

        assert!(
            world
                .out()
                .ends_with("pane.focus {\"pane_id\":\"w1:p2\"}\n")
        );
    }

    #[test]
    fn asks_on_the_auto_switch_background() {
        let world =
            world(r#"{"kind":"builtin","payload":"close_workspace","title":"Close workspace"}"#);
        world.config.set(Ok(HerdrConfig::from_toml(
            "[theme]\nauto_switch = true\nname = \"gruvbox\"",
        )
        .unwrap()));
        world.terminal.set_appearance(Some(Appearance::Light));

        world.run(&Command::Confirm).unwrap();

        // gruvbox-light's panel_bg.
        assert!(world.out().starts_with("\x1b[48;2;251;241;199m"));
        assert_eq!(world.terminal.appearance_queries(), 1);
    }

    #[test]
    fn ignores_the_settings_background() {
        let world = world(r#"{"kind":"builtin","payload":"zoom","title":"Toggle zoom"}"#);
        world.settings.set(Ok(Settings {
            colors: ColorSettings {
                background: Some(Color::Ansi(17)),
                ..ColorSettings::default()
            },
            ..Settings::default()
        }));

        world.run(&Command::Confirm).unwrap();

        assert_eq!(world.out(), expected("Toggle zoom?", None, 76));
    }

    #[test]
    fn asks_about_a_tab_that_is_not_its_workspaces_last() {
        let world = world(r#"{"kind":"builtin","payload":"close_tab","title":"Close tab"}"#);
        world.herdr.reply(
            "session.snapshot",
            Ok(json!({"snapshot": {
                "focused_workspace_id": "w1",
                "focused_tab_id": "w1:t1",
                "workspaces": [{"workspace_id": "w1", "label": "api"}],
                "tabs": [
                    {"tab_id": "w1:t1", "workspace_id": "w1", "label": "1"},
                    {"tab_id": "w1:t2", "workspace_id": "w1", "label": "2"}
                ],
                "panes": []
            }})),
        );

        world.run(&Command::Confirm).unwrap();

        assert_eq!(world.out(), expected("Close tab?", None, 76));
        assert_eq!(world.herdr.methods(), ["session.snapshot"]);
    }

    #[test]
    fn without_a_row_says_so_until_a_key_press() {
        let world = World::new();

        let status = world.run(&Command::Confirm);

        assert_eq!(status, Ok(2));
        assert_eq!(
            world.out(),
            "herdr-command-palette: no row received (HCP_CONFIRM unset or invalid)\n\n\
             press any key to close\n"
        );
        assert_eq!(world.terminal.key_waits(), 1);
    }
}
