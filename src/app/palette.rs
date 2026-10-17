//! `palette`: the popup. Checks fzf, formats every row
//! once, runs fzf, then hands the selection to a detached `run` so it
//! happens after the popup closes (Herdr shows one popup at a time).
//!
//! The settings decide which rows show, the category it opens on, its
//! colors, and which rows ask before running: for those, a detached `ask`
//! opens the `confirm` popup once this one has closed.
//!
//! Problems stay on screen until a key press, on the panel background,
//! instead of the popup closing at once: a missing or old fzf, a broken
//! `config.toml` or `settings.toml`, an fzf failure. Esc or no match
//! closes silently.
//!
//! With Herdr's `[theme] auto_switch`, the terminal's appearance is asked
//! once per open, before fzf starts, and handed to the fzf callbacks.

use std::fmt::Write as _;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::app::fzf_callbacks::{CATEGORY_FILE, lines, rows_file, rows_json, rows_json_file};
use crate::app::{ask, recent, run_row};
use crate::domain::appearance::Appearance;
use crate::domain::catalog::panes;
use crate::domain::color::Color;
use crate::domain::config::HerdrConfig;
use crate::domain::env::{HCP_APPEARANCE_ENV, HCP_PROGRAM_ENV, HCP_STATE_ENV};
use crate::domain::herdr::Snapshot;
use crate::domain::layout::{self, FooterMode, LayoutMode};
use crate::domain::palette_state;
use crate::domain::requests::Confirmation;
use crate::domain::row::{Category, Kind, Row};
use crate::domain::settings::Settings;
use crate::domain::style::{PickerColors, TextColors};
use crate::domain::theme::{self, ThemeColors};
use crate::ports::{Detached, Pick, PickRequest, Picker, PickerError, SettingsError};

use super::{CLOSE_HINT, Context, Error, PROGRAM, catalog_rows, must_confirm};

/// Oldest fzf the palette works with (`--gutter`, the newest flag used),
/// as (major, minor).
const MIN_FZF_VERSION: (u32, u32) = (0, 66);

/// Last line of the settings error screen (instead of `CLOSE_HINT`).
const SETTINGS_ERROR_KEYS: &str = "edit e · close any key";

/// Payload of the Edit Command Palette settings row
/// ([`PaletteCommand::EditSettings`](crate::domain::commands::PaletteCommand::EditSettings)).
pub(crate) const EDIT_SETTINGS: &str = "edit_settings";

/// Where the error screen sends people who need fzf.
const FZF_INSTALL_URL: &str = "https://github.com/junegunn/fzf#installation";

/// Lines shown on the error screen.
type Screen = Vec<String>;

/// What a palette open is built from, once loaded and checked.
#[derive(Debug, Clone, Copy)]
struct Setup<'a> {
    config: &'a HerdrConfig,
    settings: &'a Settings,
    colors: PickerColors,
    text_colors: TextColors,
    /// The terminal's appearance, when `auto_switch` asked for it.
    appearance: Option<Appearance>,
}

/// Runs the palette; returns 0 when it closes normally, 1 after an error
/// screen.
///
/// # Errors
///
/// [`Error::Output`] when the screen can't be written; with `HCP_DRY_RUN=1`,
/// whatever [`run_row::run_row`] returns for the selection.
pub fn palette(
    context: &Context<'_>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let config = context.config.load();
    let settings = context.settings.load();
    let (theme, appearance) = config
        .as_ref()
        .map_or((ThemeColors::TERMINAL_DEFAULT, None), |config| {
            theme_colors(context, config)
        });
    // Invalid settings still get a screen in Herdr's theme colors.
    let color_settings = settings
        .as_ref()
        .map(|settings| settings.colors)
        .unwrap_or_default();
    let colors = PickerColors::new(theme, &color_settings);
    let background = colors.background;
    if let Some(screen) = preflight(context.picker) {
        return show(context, background, &screen, out);
    }
    let config = match config {
        Ok(config) => config,
        Err(error) => return show(context, background, &[error.to_string()], out),
    };
    let settings = match settings {
        Ok(settings) => settings,
        Err(error) => {
            let screen = settings_screen(&error, context.env.home.as_deref());
            return show_settings_error(context, background, &screen, out, err);
        }
    };
    let setup = Setup {
        config: &config,
        settings: &settings,
        colors,
        text_colors: TextColors::new(theme, &settings.colors),
        appearance,
    };
    let (row, snapshot) = match pick_in_temp_dir(context, setup, err) {
        Ok(Some(picked)) => picked,
        Ok(None) => return Ok(0),
        Err(screen) => return show(context, background, &screen, out),
    };
    if must_confirm(context, &settings, &config, &row, snapshot.as_ref()) {
        return hand_to_ask(context, &row, background, out, err);
    }
    dispatch(context, row.kind, &row.payload, background, out, err)
}

/// Herdr's theme colors for `config`, and the terminal's appearance they
/// were resolved in. The terminal is asked only with `[theme]
/// auto_switch`, and only here: call it once per process, before fzf
/// takes the terminal.
pub(crate) fn theme_colors(
    context: &Context<'_>,
    config: &HerdrConfig,
) -> (ThemeColors, Option<Appearance>) {
    let appearance = if config.theme.auto_switch {
        context.terminal.appearance()
    } else {
        None
    };
    (theme::theme_colors(&config.theme, appearance), appearance)
}

/// What's wrong with the installed fzf, if anything.
fn preflight(picker: &dyn Picker) -> Option<Screen> {
    let (major, minor) = MIN_FZF_VERSION;
    let needs = format!("{PROGRAM} needs fzf {major}.{minor} or newer.");
    let problem = match picker.version() {
        Ok(None) => "fzf not found.".to_owned(),
        Ok(Some(version)) => match parse_version(&version) {
            Some((number, found)) if found < MIN_FZF_VERSION => {
                format!("fzf {number} is too old.")
            }
            // Allow unparseable versions because their compatibility is unknown.
            _ => return None,
        },
        Err(error) => error.to_string(),
    };
    Some(vec![problem, needs, FZF_INSTALL_URL.to_owned()])
}

/// The error screen for settings that can't be used: the problem with its
/// key and line (a TOML syntax error spans lines), the file (`~` for the
/// home dir, so it fits the popup), then what to do.
fn settings_screen(error: &SettingsError, home: Option<&Path>) -> Screen {
    let mut screen: Screen = match error {
        SettingsError::Invalid { path, problem } => {
            let path = panes::with_tilde(&path.to_string_lossy(), home.and_then(Path::to_str));
            let mut lines: Screen = problem.to_string().lines().map(str::to_owned).collect();
            lines.push(format!("in {path}"));
            lines
        }
        SettingsError::Read { .. } | SettingsError::Write { .. } => vec![error.to_string()],
    };
    screen.push(String::new());
    screen.push("Fix the file, then open the palette again.".to_owned());
    screen
}

/// The leading `major.minor[.patch]` of `fzf --version` output, and its
/// (major, minor).
fn parse_version(text: &str) -> Option<(&str, (u32, u32))> {
    let end = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let number = text.get(..end)?.trim_end_matches('.');
    let mut parts = number.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((number, (major, minor)))
}

/// Runs fzf with its state in a fresh temp dir, removed afterwards (also
/// on errors). Leftovers of killed palettes go first.
fn pick_in_temp_dir(
    context: &Context<'_>,
    setup: Setup<'_>,
    err: &mut dyn Write,
) -> Result<Option<(Row, Option<Snapshot>)>, Screen> {
    let dir = context
        .files
        .create_temp_dir()
        .map_err(|error| vec![error.to_string()])?;
    sweep_stale_dirs(context, &dir);
    let picked = pick(context, setup, &dir, err);
    context.files.remove_dir(&dir);
    picked
}

/// Removes the temp dirs that palettes killed by Herdr (SIGHUP on popup
/// close) left next to `dir`, once they are stale.
fn sweep_stale_dirs(context: &Context<'_>, dir: &Path) {
    let now = context.clock.now();
    for leftover in context.files.sibling_temp_dirs(dir) {
        if palette_state::is_stale(leftover.modified, now) {
            context.files.remove_dir(&leftover.path);
        }
    }
}

/// Writes the state, runs fzf and finds the selected row, with the
/// snapshot the rows came from. Rows the settings hide are left out, the
/// rest are in the settings' order; the palette opens on `open_on`.
fn pick(
    context: &Context<'_>,
    setup: Setup<'_>,
    dir: &Path,
    err: &mut dyn Write,
) -> Result<Option<(Row, Option<Snapshot>)>, Screen> {
    let screen = |error: &dyn std::fmt::Display| vec![error.to_string()];
    let settings = setup.settings;
    let text_colors = setup.text_colors;
    // The full popup width: layout keeps the 1-column gutters itself, so
    // divider lines still reach the border.
    let columns = context
        .terminal
        .columns()
        .unwrap_or(layout::DEFAULT_COLUMNS);
    let mode = LayoutMode::for_width(columns, settings.popup.min_width);
    let (mut rows, snapshot) = catalog_rows(context, setup.config, err);
    rows.retain(|row| !settings.commands.hides(row));
    recent::order(
        context,
        settings,
        setup.config,
        snapshot.as_ref(),
        &mut rows,
        err,
    );
    let start = settings.open_on;
    // The callbacks read these while fzf runs (fzf_callbacks).
    let mut state = vec![(CATEGORY_FILE.to_owned(), start.as_str().to_owned())];
    for category in Category::ALL {
        // As data, laid out again on a resize; formatted, as the fallback
        // (fzf_callbacks::rows).
        state.push((rows_json_file(category), rows_json(&rows, category)));
        state.push((
            rows_file(category),
            lines(&rows, category, columns, mode, text_colors),
        ));
    }
    for (name, contents) in &state {
        context
            .files
            .write(&dir.join(name), contents)
            .map_err(|error| screen(&error))?;
    }

    // The bindings name this binary by `HCP_PROGRAM`, not by its path
    // (fzf could rewrite a path as placeholders). A path that isn't UTF-8
    // is passed lossily.
    let mut env = vec![
        (HCP_STATE_ENV.to_owned(), dir.display().to_string()),
        (
            HCP_PROGRAM_ENV.to_owned(),
            context.program.to_string_lossy().into_owned(),
        ),
    ];
    // The callbacks can't ask the terminal while fzf owns it.
    if let Some(appearance) = setup.appearance {
        env.push((HCP_APPEARANCE_ENV.to_owned(), appearance.to_string()));
    }
    let request = PickRequest {
        input: lines(&rows, start, columns, mode, text_colors),
        header: layout::header(start, text_colors),
        footer: layout::footer(FooterMode::Keys, start, &[], None, columns, mode),
        colors: setup.colors,
        env,
    };
    let picked = context.picker.pick(&request).map_err(|error| match error {
        // The screen sentence ends with a period;
        // error messages don't.
        PickerError::Status(_) => vec![format!("{error}.")],
        PickerError::Start(_) => screen(&error),
    })?;
    match picked {
        Pick::Cancelled => Ok(None),
        // Nothing to run, e.g. Enter on an empty list.
        Pick::Selected(line) if line.is_empty() => Ok(None),
        Pick::Selected(line) => {
            let row = selection(&line).and_then(|(kind, payload)| {
                rows.into_iter()
                    .find(|row| row.kind == kind && row.payload == payload)
            });
            match row {
                Some(row) => Ok(Some((row, snapshot))),
                None => Err(vec![format!(
                    "unexpected selection: {}",
                    line.replace('\x1b', "^[")
                )]),
            }
        }
    }
}

/// Kind and payload of a selected line (`kind \t payload \t ...`); `None`
/// when the line doesn't have that shape.
fn selection(line: &str) -> Option<(Kind, String)> {
    let mut fields = line.split('\t');
    let kind = fields.next()?.parse().ok()?;
    let payload = fields.next()?;
    Some((kind, payload.to_owned()))
}

/// Hands a `confirm` row to `ask`, which opens the `confirm` popup once
/// this one has closed: in-process with `HCP_DRY_RUN=1` (prints
/// the request), else detached like `run`. An `ask` that can't start is
/// shown on `background`.
fn hand_to_ask(
    context: &Context<'_>,
    row: &Row,
    background: Color,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    if context.env.dry_run {
        return ask::ask(context, row, out, err);
    }
    let title = row.full_title();
    let args = ["ask", "--", row.kind.as_str(), &row.payload, &title];
    detach(context, &args, background, out)
}

/// Runs the row `kind:payload` from a popup: in-process with
/// `HCP_DRY_RUN=1` (prints the request), else as a detached `run` that
/// outlives the popup (Herdr shows one popup at a time). A `run` that
/// can't start is shown on `background`.
pub(crate) fn dispatch(
    context: &Context<'_>,
    kind: Kind,
    payload: &str,
    background: Color,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    if context.env.dry_run {
        return run_row::run_row(context, kind, payload, Confirmation::NotAsked, out, err);
    }
    detach(
        context,
        &["run", "--", kind.as_str(), payload],
        background,
        out,
    )
}

/// Starts this binary with `args` as a detached process that outlives
/// the popup; on failure shows why on `background`.
fn detach(
    context: &Context<'_>,
    args: &[&str],
    background: Color,
    out: &mut dyn Write,
) -> Result<u8, Error> {
    match context.spawner.spawn_detached(&detached(context, args)) {
        Ok(()) => Ok(0),
        Err(error) => show(context, background, &[error.to_string()], out),
    }
}

/// This binary with `args`, detached from the popup, errors in the run
/// log. `args` put `--` before row data, which may start with `-`.
pub(crate) fn detached(context: &Context<'_>, args: &[&str]) -> Detached {
    Detached {
        program: context.program.to_owned(),
        args: args.iter().map(|&arg| arg.to_owned()).collect(),
        cwd: None,
        log: Some(run_log(context)),
    }
}

/// Where the detached `run` (and `ask`) writes its errors: the plugin
/// state dir, else `$TMPDIR`, else `/tmp`.
fn run_log(context: &Context<'_>) -> PathBuf {
    let env = context.env;
    let dir = env
        .plugin_state_dir
        .as_deref()
        .or(env.tmp_dir.as_deref())
        .unwrap_or(Path::new("/tmp"));
    dir.join("run.log")
}

/// Shows the settings error `screen` with the keys `edit e · close any
/// key`: `e` opens the file in the editor through the Edit Command Palette
/// settings row (it checks the file again when the editor closes), any
/// other key closes with exit status 1.
fn show_settings_error(
    context: &Context<'_>,
    background: Color,
    screen: &[String],
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let mut text = draw(background, screen);
    let _ = write!(text, "\n {SETTINGS_ERROR_KEYS}");
    out.write_all(text.as_bytes())?;
    out.flush()?;
    if context.terminal.read_key() == Some(b'e') {
        return dispatch(context, Kind::Palette, EDIT_SETTINGS, background, out, err);
    }
    Ok(1)
}

/// Shows `screen` until a key press; exit status 1.
fn show(
    context: &Context<'_>,
    background: Color,
    screen: &[String],
    out: &mut dyn Write,
) -> Result<u8, Error> {
    out.write_all(error_screen(background, screen).as_bytes())?;
    out.flush()?;
    context.terminal.wait_for_key();
    Ok(1)
}

/// The error screen: `lines`, then the close hint.
fn error_screen(background: Color, lines: &[String]) -> String {
    let mut screen = draw(background, lines);
    let _ = write!(screen, "\n {CLOSE_HINT}");
    screen
}

/// A full-screen message: `background` everywhere, cursor hidden, one line
/// per entry with a 1-column gutter.
fn draw(background: Color, lines: &[String]) -> String {
    // The background first, so clearing paints the screen with it.
    let mut screen = background.to_ansi_bg();
    screen.push_str(layout::CLEAR_SCREEN);
    screen.push('\n');
    for line in lines {
        let _ = writeln!(screen, " {line}");
    }
    screen
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use crate::app::fakes::{World, invalid_settings, parse_error, spawn_error};
    use crate::cli::Command;
    use crate::domain::env::PluginEnv;
    use crate::domain::fixtures::herdr_json;
    use crate::domain::row::RowId;
    use crate::domain::settings::{ColorSettings, CommandSettings, SettingsProblem};
    use crate::ports::{Clock, Files, TempDirEntry};

    use super::*;

    fn selected(line: &str) -> Pick {
        Pick::Selected(line.to_owned())
    }

    /// A palette open from `config` and `settings`, in terminal colors.
    fn setup<'a>(config: &'a HerdrConfig, settings: &'a Settings) -> Setup<'a> {
        Setup {
            config,
            settings,
            colors: PickerColors::new(ThemeColors::TERMINAL_DEFAULT, &settings.colors),
            text_colors: TextColors::new(ThemeColors::TERMINAL_DEFAULT, &settings.colors),
            appearance: None,
        }
    }

    #[test]
    fn runs_fzf_on_the_commands_with_state_for_the_callbacks() {
        let world = World::new();

        assert_eq!(world.run(&Command::Palette), Ok(0));

        let request = &world.picker.requests()[0];
        assert!(request.input.starts_with("plugin\tdemo.hello\t"));
        assert!(!request.input.contains("\npane\t"));
        assert_eq!(
            request.header,
            layout::header(Category::Commands, TextColors::default())
        );
        // The first frame: Commands' one-line key hints, toggle hint last.
        assert!(
            request
                .footer
                .starts_with(&format!(" {}", layout::KEY_HINTS))
        );
        assert!(request.footer.ends_with("[^/]"));
        assert!(!request.footer.contains('\n'));
        assert_eq!(
            request.colors,
            PickerColors::new(
                theme::theme_colors(&HerdrConfig::default().theme, None),
                &ColorSettings::default()
            )
        );
        assert_eq!(
            request.env,
            [
                ("HCP_STATE".to_owned(), "/tmp/state-1".to_owned()),
                ("HCP_PROGRAM".to_owned(), "/bin/hcp".to_owned()),
            ]
        );
    }

    #[test]
    fn formats_rows_for_the_full_popup_width_and_cleans_up() {
        let world = World::new();

        world.run(&Command::Palette).unwrap();

        assert_eq!(world.files.removed(), [PathBuf::from("/tmp/state-1")]);
        let input = world.picker.requests()[0].input.clone();
        let zoom = input
            .lines()
            .find(|line| line.starts_with("builtin\tzoom"))
            .unwrap();
        let display = zoom.split('\t').nth(2).unwrap();
        // Gutter, then title and padding up to the 8-column shortcut.
        let left = display.split('\x1b').next().unwrap();
        assert_eq!(layout::width(left), 1 + (76 - 2) - 8);
    }

    #[test]
    fn sweeps_stale_leftovers_before_picking_and_keeps_recent_ones() {
        let world = World::new();
        let now = world.clock.now();
        let leftover = |path: &str, age: Duration| TempDirEntry {
            path: PathBuf::from(path),
            modified: now - age,
        };
        world.files.set_siblings(vec![
            leftover("/tmp/stale", palette_state::STALE_AFTER),
            leftover("/tmp/open-elsewhere", Duration::from_mins(1)),
        ]);

        world.run(&Command::Palette).unwrap();

        assert_eq!(
            world.files.removed(),
            [PathBuf::from("/tmp/stale"), PathBuf::from("/tmp/state-1")]
        );
    }

    #[test]
    fn writes_every_category_and_the_starting_category() {
        // `palette` removes the dir at the end; run the step that fills it.
        let world = World::new();
        let mut err = Vec::new();

        pick(
            &world.context(),
            setup(&HerdrConfig::default(), &Settings::default()),
            Path::new("/d"),
            &mut err,
        )
        .unwrap();

        assert!(
            world
                .files
                .get("/d/rows-jump_to")
                .unwrap()
                .starts_with("pane\tw1:p2\t")
        );
        assert!(
            world
                .files
                .get("/d/rows-commands")
                .unwrap()
                .starts_with("plugin\t")
        );
        assert_eq!(world.files.get("/d/category").as_deref(), Some("commands"));
        let json: serde_json::Value =
            serde_json::from_str(&world.files.get("/d/rows-jump_to.json").unwrap()).unwrap();
        let saved: Vec<Row> = serde_json::from_value(json["rows"].clone()).unwrap();
        assert_eq!(saved[0].payload, "w1:p2");
        assert!(saved.iter().all(|row| row.kind == Kind::Pane));
    }

    #[test]
    fn opens_in_the_minimal_layout_on_a_narrow_screen() {
        // The default `min_width` (76): 53 columns are an outer 56, under 75%.
        let world = World::new();
        world.terminal.set_columns(Some(53));

        world.run(&Command::Palette).unwrap();

        let request = &world.picker.requests()[0];
        let zoom = request
            .input
            .lines()
            .find(|line| line.starts_with("builtin\tzoom"))
            .unwrap();
        // Title only: the shortcut moved to the hidden search tail.
        assert!(zoom.contains("\x1b[2m\x1b[0m "), "{zoom:?}");
        assert!(request.footer.starts_with(" ↑↓/pgup/pgdn"));
        assert!(!request.footer.contains("[^/]"), "{:?}", request.footer);
    }

    #[test]
    fn opens_in_the_full_layout_from_75_percent_of_min_width() {
        let world = World::new();
        world.terminal.set_columns(Some(54));

        world.run(&Command::Palette).unwrap();

        assert!(world.picker.requests()[0].footer.ends_with("[^/]"));
    }

    #[test]
    fn runs_the_selection_detached() {
        let mut world = World::new();
        world.env.plugin_state_dir = Some(PathBuf::from("/state"));
        world
            .picker
            .set_pick(Ok(selected("builtin\tzoom\t Toggle zoom\tdesc")));

        assert_eq!(world.run(&Command::Palette), Ok(0));

        assert_eq!(
            world.spawner.detached(),
            [Detached {
                program: PathBuf::from("/bin/hcp"),
                args: vec!["run".into(), "--".into(), "builtin".into(), "zoom".into()],
                cwd: None,
                log: Some(PathBuf::from("/state/run.log")),
            }]
        );
    }

    #[test]
    fn run_log_falls_back_to_tmpdir() {
        let mut world = World::new();
        world.env.tmp_dir = Some(PathBuf::from("/var/tmp"));

        let log = run_log(&world.context());

        assert_eq!(log, Path::new("/var/tmp/run.log"));
    }

    #[test]
    fn run_log_falls_back_to_tmp_without_tmpdir() {
        let mut world = World::new();
        world.env = PluginEnv::from_lookup(|_| None);

        let log = run_log(&world.context());

        assert_eq!(log, Path::new("/tmp/run.log"));
    }

    #[test]
    fn dry_run_prints_the_request_instead() {
        let mut world = World::new();
        world.env.dry_run = true;
        world.picker.set_pick(Ok(selected("pane\tw1:p2\tx\ty")));

        assert_eq!(world.run(&Command::Palette), Ok(0));

        assert_eq!(world.out(), "pane.focus {\"pane_id\":\"w1:p2\"}\n");
        assert_eq!(world.spawner.detached(), []);
    }

    #[test]
    fn missing_fzf_shows_how_to_install_it() {
        let world = World::new();
        world.picker.set_version(Ok(None));

        assert_eq!(world.run(&Command::Palette), Ok(1));

        assert_eq!(
            world.out(),
            "\x1b[48;2;24;24;37m\x1b[?25l\x1b[2J\x1b[H\n fzf not found.\n \
             herdr-command-palette needs fzf 0.66 or newer.\n \
             https://github.com/junegunn/fzf#installation\n\n press any key to close"
        );
        assert_eq!(world.terminal.key_waits(), 1);
        assert_eq!(world.picker.requests(), []);
    }

    /// Herdr's config with `[theme] auto_switch = true`.
    fn auto_switch(world: &World) {
        world.config.set(Ok(
            HerdrConfig::from_toml("[theme]\nauto_switch = true\n").unwrap()
        ));
    }

    #[test]
    fn does_not_ask_the_terminal_without_auto_switch() {
        let world = World::new();
        world.terminal.set_appearance(Some(Appearance::Light));

        world.run(&Command::Palette).unwrap();

        assert_eq!(world.terminal.appearance_queries(), 0);
        // Catppuccin, whatever the terminal says.
        let request = &world.picker.requests()[0];
        assert_eq!(request.colors.background, Color::Rgb(24, 24, 37));
    }

    #[test]
    fn auto_switch_asks_once_and_hands_the_appearance_to_the_callbacks() {
        let world = World::new();
        auto_switch(&world);
        world.terminal.set_appearance(Some(Appearance::Light));

        world.run(&Command::Palette).unwrap();

        assert_eq!(world.terminal.appearance_queries(), 1);
        let request = &world.picker.requests()[0];
        // Catppuccin's light sibling, latte.
        assert_eq!(request.colors.background, Color::Rgb(0xef, 0xf1, 0xf5));
        assert_eq!(
            request.env,
            [
                ("HCP_STATE".to_owned(), "/tmp/state-1".to_owned()),
                ("HCP_PROGRAM".to_owned(), "/bin/hcp".to_owned()),
                ("HCP_APPEARANCE".to_owned(), "light".to_owned()),
            ]
        );
    }

    #[test]
    fn auto_switch_without_an_answer_is_dark_and_tells_the_callbacks_nothing() {
        let world = World::new();
        auto_switch(&world);

        world.run(&Command::Palette).unwrap();

        assert_eq!(world.terminal.appearance_queries(), 1);
        let request = &world.picker.requests()[0];
        assert_eq!(request.colors.background, Color::Rgb(24, 24, 37));
        assert_eq!(
            request.env,
            [
                ("HCP_STATE".to_owned(), "/tmp/state-1".to_owned()),
                ("HCP_PROGRAM".to_owned(), "/bin/hcp".to_owned()),
            ]
        );
    }

    #[test]
    fn error_screens_use_the_auto_switch_appearance() {
        let world = World::new();
        auto_switch(&world);
        world.terminal.set_appearance(Some(Appearance::Light));
        world.picker.set_version(Ok(None));

        assert_eq!(world.run(&Command::Palette), Ok(1));

        assert!(world.out().starts_with("\x1b[48;2;239;241;245m\x1b[?25l"));
        assert_eq!(world.terminal.appearance_queries(), 1);
    }

    #[test]
    fn old_fzf_is_rejected_on_the_theme_background() {
        let world = World::new();
        world.config.set(Ok(
            HerdrConfig::from_toml("[theme]\nname = \"nord\"\n").unwrap()
        ));
        world.picker.set_version(Ok(Some("0.65.2 (brew)".into())));

        assert_eq!(world.run(&Command::Palette), Ok(1));

        assert!(world.out().starts_with("\x1b[48;2;46;52;64m\x1b[?25l"));
        assert!(world.out().contains(" fzf 0.65.2 is too old.\n"));
    }

    #[test]
    fn parses_major_and_minor_from_the_version_line() {
        assert_eq!(parse_version("0.66.0 (abc)"), Some(("0.66.0", (0, 66))));
        assert_eq!(parse_version("1.2"), Some(("1.2", (1, 2))));
        assert_eq!(parse_version("0.70."), Some(("0.70", (0, 70))));
    }

    #[test]
    fn versions_without_major_and_minor_do_not_parse() {
        assert_eq!(parse_version("devel"), None);
        assert_eq!(parse_version("3"), None);
        assert_eq!(parse_version("x.1"), None);
    }

    #[test]
    fn opens_when_the_fzf_version_does_not_parse() {
        let world = World::new();
        world.picker.set_version(Ok(Some("devel".into())));

        assert_eq!(world.run(&Command::Palette), Ok(0));
    }

    #[test]
    fn shows_why_the_fzf_version_is_unknown() {
        let world = World::new();
        world
            .picker
            .set_version(Err(PickerError::Start("denied".into())));

        assert_eq!(world.run(&Command::Palette), Ok(1));

        assert!(world.out().contains(" cannot run fzf: denied\n"));
    }

    #[test]
    fn shows_an_fzf_failure_status() {
        let world = World::new();
        world.picker.set_pick(Err(PickerError::Status(2)));

        assert_eq!(world.run(&Command::Palette), Ok(1));

        assert!(world.out().contains(" fzf exited with status 2.\n"));
    }

    #[test]
    fn shows_an_fzf_that_cannot_run() {
        let world = World::new();
        world
            .picker
            .set_pick(Err(PickerError::Start("killed".into())));

        assert_eq!(world.run(&Command::Palette), Ok(1));

        assert!(world.out().contains(" cannot run fzf: killed\n"));
    }

    #[test]
    fn shows_a_broken_config() {
        let world = World::new();
        world.config.set(Err(parse_error()));

        assert_eq!(world.run(&Command::Palette), Ok(1));

        assert!(world.out().contains(" invalid /c.toml: line 1\n"));
    }

    #[test]
    fn shows_a_state_dir_that_cannot_be_created() {
        let world = World::new();
        world.files.fail_with("disk full");

        assert_eq!(world.run(&Command::Palette), Ok(1));

        assert!(
            world
                .out()
                .contains(" cannot create /tmp/state-1: disk full\n")
        );
    }

    #[test]
    fn shows_state_files_that_cannot_be_written() {
        let world = World::new();
        let dir = world.files.create_temp_dir().unwrap();
        world.files.fail_with("disk full");
        let mut err = Vec::new();

        let screen = pick(
            &world.context(),
            setup(&HerdrConfig::default(), &Settings::default()),
            &dir,
            &mut err,
        )
        .unwrap_err();

        assert_eq!(screen, ["cannot write /tmp/state-1/category: disk full"]);
    }

    #[test]
    fn shows_a_run_that_cannot_start() {
        let world = World::new();
        world.picker.set_pick(Ok(selected("pane\tw1:p2\tx\ty")));
        world.spawner.set_detach(Err(spawn_error("/bin/hcp")));

        assert_eq!(world.run(&Command::Palette), Ok(1));

        assert!(world.out().contains(" cannot start /bin/hcp: not found\n"));
    }

    #[test]
    fn a_selection_is_kind_and_payload() {
        assert_eq!(
            selection("pane\tw1:p2\tx"),
            Some((Kind::Pane, "w1:p2".into()))
        );
    }

    #[test]
    fn a_selection_needs_a_known_kind_and_a_payload() {
        assert_eq!(selection("nope\tx"), None);
        assert_eq!(selection("pane"), None);
    }

    #[test]
    fn shows_an_unexpected_selection_escaped() {
        let world = World::new();
        world.picker.set_pick(Ok(selected("bogus\x1b")));

        assert_eq!(world.run(&Command::Palette), Ok(1));

        assert!(world.out().contains(" unexpected selection: bogus^[\n"));
    }

    #[test]
    fn an_empty_selection_closes_silently() {
        let world = World::new();
        world.picker.set_pick(Ok(selected("")));

        assert_eq!(world.run(&Command::Palette), Ok(0));

        assert_eq!(world.out(), "");
        assert_eq!(world.spawner.detached(), []);
    }

    /// The world with `commands` as the settings' `[commands]`.
    fn with_commands(world: &World, commands: CommandSettings) {
        world.settings.set(Ok(Settings {
            commands,
            ..Settings::default()
        }));
    }

    #[test]
    fn leaves_out_the_rows_the_settings_hide() {
        let world = World::new();
        with_commands(
            &world,
            CommandSettings {
                hide_kinds: vec![Kind::Plugin],
                hide_ids: vec![RowId::new(Kind::Builtin, "zoom")],
                ..CommandSettings::default()
            },
        );

        world.run(&Command::Palette).unwrap();

        let input = &world.picker.requests()[0].input;
        assert!(input.starts_with("builtin\tsplit_vertical\t"), "{input}");
        assert!(!input.contains("builtin\tzoom\t"));
    }

    #[test]
    fn opens_on_the_settings_category() {
        let world = World::new();
        let settings = Settings {
            open_on: Category::JumpTo,
            ..Settings::default()
        };
        let mut err = Vec::new();

        pick(
            &world.context(),
            setup(&HerdrConfig::default(), &settings),
            Path::new("/d"),
            &mut err,
        )
        .unwrap();

        let request = &world.picker.requests()[0];
        // The current pane (w1:p1) is last in the recent order.
        assert!(request.input.starts_with("pane\tw1:p2\t"));
        assert_eq!(
            request.header,
            layout::header(Category::JumpTo, TextColors::default())
        );
        assert_eq!(world.files.get("/d/category").as_deref(), Some("jump_to"));
        // The first frame has Jump to's two footer lines.
        assert_eq!(request.footer.lines().count(), 2);
    }

    #[test]
    fn colors_come_from_the_settings_over_the_theme() {
        let world = World::new();
        let colors = ColorSettings {
            background: Some(Color::Ansi(17)),
            muted: Some(Color::Ansi(8)),
            ..ColorSettings::default()
        };
        world.settings.set(Ok(Settings {
            colors,
            ..Settings::default()
        }));

        world.run(&Command::Palette).unwrap();

        let request = &world.picker.requests()[0];
        assert_eq!(request.colors.background, Color::Ansi(17));
        assert_eq!(request.colors.footer, Color::Ansi(8));
        assert!(request.input.contains("\x1b[38;5;8mprefix+z"));
    }

    #[test]
    fn shows_invalid_settings_on_the_theme_background() {
        let world = World::new();
        world.settings.set(Err(invalid_settings()));

        assert_eq!(world.run(&Command::Palette), Ok(1));

        assert_eq!(
            world.out(),
            "\x1b[48;2;24;24;37m\x1b[?25l\x1b[2J\x1b[H\n \
             invalid `popup.min_width` (line 2): 100 is larger than `popup.max_width` (76)\n \
             in /cfg/settings.toml\n \n \
             Fix the file, then open the palette again.\n\n edit e · close any key"
        );
        assert_eq!(world.picker.requests(), []);
    }

    #[test]
    fn e_on_the_settings_error_opens_the_settings_in_the_editor() {
        let world = World::new();
        world.settings.set(Err(invalid_settings()));
        world.terminal.set_key(Some(b'e'));

        assert_eq!(world.run(&Command::Palette), Ok(0));

        let detached = world.spawner.detached();
        assert_eq!(detached.len(), 1);
        assert_eq!(detached[0].args, ["run", "--", "palette", EDIT_SETTINGS]);
    }

    #[test]
    fn other_keys_close_the_settings_error() {
        let world = World::new();
        world.settings.set(Err(invalid_settings()));
        world.terminal.set_key(Some(b'q'));

        assert_eq!(world.run(&Command::Palette), Ok(1));

        assert_eq!(world.spawner.detached(), []);
    }

    #[test]
    fn edit_settings_is_a_palette_command() {
        assert_eq!(
            EDIT_SETTINGS.parse::<crate::domain::commands::PaletteCommand>(),
            Ok(crate::domain::commands::PaletteCommand::EditSettings)
        );
    }

    #[test]
    fn shows_a_settings_syntax_error_on_several_lines() {
        let error = SettingsError::Invalid {
            path: PathBuf::from("/home/me/.config/s.toml"),
            problem: SettingsProblem {
                key: None,
                line: Some(1),
                message: "invalid table header\nexpected `]`".into(),
            },
        };

        let screen = settings_screen(&error, Some(Path::new("/home/me")));

        assert_eq!(
            screen,
            [
                "invalid line 1: invalid table header",
                "expected `]`",
                "in ~/.config/s.toml",
                "",
                "Fix the file, then open the palette again."
            ]
        );
    }

    #[test]
    fn shows_settings_that_cannot_be_read_as_one_line() {
        let error = SettingsError::Read {
            path: PathBuf::from("/s.toml"),
            message: "denied".into(),
        };

        let screen = settings_screen(&error, None);

        assert_eq!(screen[0], "cannot read /s.toml: denied");
        assert_eq!(screen.len(), 3);
    }

    #[test]
    fn hands_a_confirm_row_to_a_detached_ask_without_asking_here() {
        let mut world = World::new();
        world.env.plugin_state_dir = Some(PathBuf::from("/state"));
        world
            .picker
            .set_pick(Ok(selected("builtin\tclose_workspace\tx\ty")));
        world.terminal.set_key(Some(b'\r'));

        assert_eq!(world.run(&Command::Palette), Ok(0));

        assert_eq!(
            world.spawner.detached(),
            [Detached {
                program: PathBuf::from("/bin/hcp"),
                args: vec![
                    "ask".into(),
                    "--".into(),
                    "builtin".into(),
                    "close_workspace".into(),
                    "Close workspace".into()
                ],
                cwd: None,
                log: Some(PathBuf::from("/state/run.log")),
            }]
        );
        assert_eq!(world.out(), "");
        assert_eq!(world.terminal.key_waits(), 0);
        assert!(world.herdr.last("plugin.pane.open").is_none());
    }

    #[test]
    fn hands_the_rows_the_settings_list_to_ask() {
        let world = World::new();
        with_commands(
            &world,
            CommandSettings {
                confirm_ids: vec![RowId::new(Kind::Builtin, "zoom")],
                ..CommandSettings::default()
            },
        );
        world.picker.set_pick(Ok(selected("builtin\tzoom\tx\ty")));

        world.run(&Command::Palette).unwrap();

        let detached = world.spawner.detached();
        assert_eq!(
            detached[0].args,
            ["ask", "--", "builtin", "zoom", "Toggle zoom"]
        );
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
                .picker
                .set_pick(Ok(selected("builtin\tclose_tab\tx\ty")));

            world.run(&Command::Palette).unwrap();

            assert_eq!(
                world.spawner.detached()[0].args,
                ["ask", "--", "builtin", "close_tab", "Close tab"]
            );
        }
    }

    #[test]
    fn close_pane_on_a_worktree_groups_only_pane_always_asks_with_confirm_close() {
        let world = World::new();
        world.herdr.reply(
            "session.snapshot",
            Ok(herdr_json::worktree_group_snapshot()),
        );
        world
            .picker
            .set_pick(Ok(selected("builtin\tclose_pane\tx\ty")));

        world.run(&Command::Palette).unwrap();

        assert_eq!(
            world.spawner.detached()[0].args,
            ["ask", "--", "builtin", "close_pane", "Close pane"]
        );
    }

    #[test]
    fn close_tab_runs_unasked_without_confirm_close_or_on_another_tab() {
        let off = World::new();
        off.config.set(Ok(
            HerdrConfig::from_toml("[ui]\nconfirm_close = false").unwrap()
        ));
        let other_tab = World::new();
        let mut snapshot = herdr_json::snapshot();
        snapshot["snapshot"]["tabs"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"tab_id": "w1:t2", "workspace_id": "w1", "label": "2"}));
        other_tab.herdr.reply("session.snapshot", Ok(snapshot));

        for world in [off, other_tab] {
            world
                .picker
                .set_pick(Ok(selected("builtin\tclose_tab\tx\ty")));

            world.run(&Command::Palette).unwrap();

            assert_eq!(
                world.spawner.detached()[0].args,
                ["run", "--", "builtin", "close_tab"]
            );
        }
    }

    #[test]
    fn a_dry_run_prints_the_confirm_popup_for_a_confirm_row() {
        let mut world = World::new();
        world.env.dry_run = true;
        world
            .picker
            .set_pick(Ok(selected("builtin\tclose_workspace\tx\ty")));

        assert_eq!(world.run(&Command::Palette), Ok(0));

        let out = world.out();
        assert!(out.starts_with("plugin.pane.open {\"entrypoint\":\"confirm\""));
        assert_eq!(world.spawner.detached(), []);
    }

    #[test]
    fn shows_an_ask_that_cannot_start() {
        let world = World::new();
        world
            .picker
            .set_pick(Ok(selected("builtin\tclose_workspace\tx\ty")));
        world.spawner.set_detach(Err(spawn_error("/bin/hcp")));

        assert_eq!(world.run(&Command::Palette), Ok(1));

        assert!(world.out().contains(" cannot start /bin/hcp: not found\n"));
    }

    #[test]
    fn runs_the_row_whose_payload_spells_a_tag() {
        let world = World::new();
        world.herdr.reply(
            "plugin.action.list",
            Ok(serde_json::json!({"actions": [
                {"plugin_id": "a", "action_id": "x", "title": "Plain"},
                {"plugin_id": "a", "action_id": "x{hcp:x}", "title": "Tagged"},
            ]})),
        );
        world.run(&Command::Palette).unwrap();
        let input = world.picker.requests()[0].input.clone();
        let line = input
            .lines()
            .find(|line| line.starts_with("plugin\ta.x{hcp:x}\t"))
            .unwrap();
        world.picker.set_pick(Ok(selected(line)));

        assert_eq!(world.run(&Command::Palette), Ok(0));

        let args = &world.spawner.detached()[0].args;
        assert_eq!(args[2..], ["plugin", "a.x{hcp:x}"]);
    }

    #[test]
    fn a_selection_that_is_not_listed_is_unexpected() {
        let world = World::new();
        world
            .picker
            .set_pick(Ok(selected("builtin\tteleport\tx\ty")));

        assert_eq!(world.run(&Command::Palette), Ok(1));

        assert!(
            world
                .out()
                .contains(" unexpected selection: builtin\tteleport")
        );
    }

    #[test]
    fn ansi_backgrounds_use_the_256_color_form() {
        assert!(error_screen(Color::Ansi(7), &[]).starts_with("\x1b[48;5;7m"));
    }
}
