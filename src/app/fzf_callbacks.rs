//! `fzf rows|switch|resize|footer`: callbacks fzf runs through its
//! bindings while the palette is open.
//!
//! They share the palette state dir (`HCP_STATE_ENV`): the rows of each
//! category, saved once at open as data and formatted (no Herdr calls
//! while switching or resizing), the current category and the sticky
//! footer mode (fzf has no user variables). Rows and footer are laid out
//! for fzf's `$FZF_COLUMNS` on every call, full or minimal, so they
//! follow a resized popup. stdout is fzf protocol: only what fzf expects
//! goes there.
//!
//! The colors and `[popup] min_width` come from the settings, read again
//! on each call; invalid settings give the defaults here (the palette has
//! already shown the problem when it opened).

use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::cli::FooterEvent;
use crate::domain::catalog::panes;
use crate::domain::env::HCP_PROGRAM_SH;
use crate::domain::fzf_action;
use crate::domain::git::Head;
use crate::domain::layout::{self, FooterBranch, FooterMode, LayoutMode};
use crate::domain::row::{Category, FooterLine, Kind, Row};
use crate::domain::settings::Settings;
use crate::domain::style::TextColors;
use crate::domain::theme::{self, ThemeColors};

use super::{Context, Error};

/// File holding the current category (`commands`, `jump_to`).
pub(crate) const CATEGORY_FILE: &str = "category";

/// File holding the footer mode (`desc`, `keys`).
const FOOTER_FILE: &str = "footer";

/// File holding the footer last printed, kept while a category switch
/// lands (see [`footer`]).
const SHOWN_FOOTER_FILE: &str = "footer-shown";

/// File holding the rows of `category` formatted at open: what `fzf rows`
/// prints when it can't lay them out again (see [`rows`]).
pub(crate) fn rows_file(category: Category) -> String {
    format!("rows-{category}")
}

/// File holding the rows of `category` as data ([`rows_json`]), which
/// `fzf rows` lays out for the popup's current width.
pub(crate) fn rows_json_file(category: Category) -> String {
    format!("rows-{category}.json")
}

/// Version of the rows JSON ([`rows_json`]). Bump it whenever [`Row`]'s
/// serialized shape changes: a build installed while a palette is open
/// then falls back to the formatted rows instead of reading the old JSON
/// wrongly (a missing `Option` field parses as `None`).
// When the `rows_json_keeps_its_shape` snapshot changes, bump this.
const ROWS_JSON_VERSION: u32 = 1;

/// The contents of [`rows_json_file`]: the rows tagged with the version
/// of their shape. `R` is `&Row` to write, `Row` to read.
#[derive(Serialize, Deserialize)]
struct SavedRows<R> {
    version: u32,
    rows: Vec<R>,
}

/// The rows of `category` as JSON, in list order: the contents of
/// [`rows_json_file`].
pub(crate) fn rows_json(rows: &[Row], category: Category) -> String {
    let saved = SavedRows {
        version: ROWS_JSON_VERSION,
        rows: rows
            .iter()
            .filter(|row| row.category() == category)
            .collect(),
    };
    // Strings, numbers and lists only: serializing can't fail. If it ever
    // did, the empty file wouldn't parse and `rows` would fall back.
    serde_json::to_string(&saved).unwrap_or_default()
}

/// The fzf lines of every row in `category`, laid out as `mode` says:
/// the contents of [`rows_file`], and what [`rows`] prints.
pub(crate) fn lines(
    rows: &[Row],
    category: Category,
    columns: usize,
    mode: LayoutMode,
    colors: TextColors,
) -> String {
    let mut text = String::new();
    for row in rows.iter().filter(|row| row.category() == category) {
        // Writing to a String can't fail.
        let _ = writeln!(text, "{}", layout::row_line(row, columns, mode, colors));
    }
    text
}

/// Prints the rows of `category` (fzf `reload`), laid out for
/// `$FZF_COLUMNS` in the layout that width gets ([`LayoutMode`], from the
/// settings' `[popup] min_width`).
///
/// Falls back to the rows as formatted at open (`rows_file`) when there
/// is no `$FZF_COLUMNS` or the JSON rows can't be read or parsed, or
/// carry another version (written by an older build while this one was
/// installed).
///
/// # Errors
///
/// [`Error::NoPaletteState`] outside the palette, [`Error::Files`] when
/// the formatted rows can't be read.
pub fn rows(context: &Context<'_>, category: Category, out: &mut dyn Write) -> Result<u8, Error> {
    let dir = palette_state(context)?;
    if let Some(columns) = context.env.fzf_columns
        && let Some(saved) = saved_rows(context, dir, category)
    {
        let settings = settings(context);
        let colors = text_colors(context, &settings);
        let mode = LayoutMode::for_width(columns, settings.popup.min_width);
        // One write: stdout is line buffered, so a write per row would be
        // a system call per row.
        out.write_all(lines(&saved, category, columns, mode, colors).as_bytes())?;
        return Ok(0);
    }
    let rows = context.files.read(&dir.join(rows_file(category)))?;
    out.write_all(rows.unwrap_or_default().as_bytes())?;
    Ok(0)
}

/// The rows of `category` saved at open ([`rows_json_file`]); `None` when
/// they can't be read or parsed, or aren't [`ROWS_JSON_VERSION`].
fn saved_rows(context: &Context<'_>, dir: &Path, category: Category) -> Option<Vec<Row>> {
    let text = context
        .files
        .read(&dir.join(rows_json_file(category)))
        .ok()
        .flatten()?;
    let saved: SavedRows<Row> = serde_json::from_str(&text).ok()?;
    (saved.version == ROWS_JSON_VERSION).then_some(saved.rows)
}

/// Prints the fzf action for a resized popup (fzf's `resize` event): the
/// current category's rows laid out again for the new width.
/// `reload-sync` keeps the old list until the new one is ready (no empty
/// frame). The query stays, and so does the cursor's position, on the
/// same row: equal matches keep list order (`--tiebreak=index`,
/// `adapters::fzf`). The footer follows on fzf's `result` event, which
/// fires after every reload (the `focus,result` binding).
///
/// # Errors
///
/// As [`rows`], when the current category can't be read;
/// [`Error::FzfAction`] when the action fits no fzf delimiter (a safety
/// net: it names this binary by `HCP_PROGRAM`, not its path).
pub fn resize(context: &Context<'_>, out: &mut dyn Write) -> Result<u8, Error> {
    let dir = palette_state(context)?;
    let category = current_category(context, dir)?;
    let action = fzf_action::action("reload-sync", &rows_command(category))?;
    out.write_all(action.as_bytes())?;
    Ok(0)
}

/// The command that prints `category`'s rows, as a `reload` runs it with
/// `sh`: this binary by `HCP_PROGRAM`, not by its path, which fzf could
/// rewrite as placeholders.
fn rows_command(category: Category) -> String {
    format!("{HCP_PROGRAM_SH} fzf rows {category}")
}

/// Moves `offset` categories (wrapping) and prints the fzf actions that
/// show it: `reload(rows)+change-header(header)+first`.
///
/// # Errors
///
/// As [`rows`]; [`Error::FzfAction`] when an action fits no fzf
/// delimiter (a safety net; the category stays).
pub fn switch(context: &Context<'_>, offset: i32, out: &mut dyn Write) -> Result<u8, Error> {
    let dir = palette_state(context)?;
    let next = current_category(context, dir)?.cycle(offset);
    let header = layout::header(next, text_colors(context, &settings(context)));
    // Built before the category is saved: if no fzf delimiter fits, the
    // category stays as fzf shows it.
    let actions = [("reload", rows_command(next)), ("change-header", header)]
        .iter()
        .map(|(name, arg)| fzf_action::action(name, arg))
        .collect::<Result<Vec<_>, _>>()?;
    context
        .files
        .write(&dir.join(CATEGORY_FILE), next.as_str())?;
    out.write_all(format!("{}+first", actions.join("+")).as_bytes())?;
    Ok(0)
}

/// Prints the footer for the focused row's footer `lines` (its `{4}`,
/// parsed back), with as many lines as the current category's footer
/// has, its `kind`'s tag colored as the settings say; a toggle flips the
/// sticky mode first. In Jump to's description mode, the cwd's git
/// branch goes before it, in the settings' `branch` color.
///
/// A focus on a row of the other category prints the last footer again
/// (fzf has no no-op here: an empty `transform-footer` removes the
/// footer). That happens while a switch lands: `switch`'s `first` moves
/// the cursor in the old list before the reload, and the old row would
/// show in the new category's layout for a few frames. The footer for
/// the new list follows on fzf's `result` event.
///
/// # Errors
///
/// As [`rows`].
pub fn footer(
    context: &Context<'_>,
    event: FooterEvent,
    kind: Option<Kind>,
    lines: &[FooterLine],
    out: &mut dyn Write,
) -> Result<u8, Error> {
    let dir = palette_state(context)?;
    let category = current_category(context, dir)?;
    let columns = context.env.fzf_columns.unwrap_or(layout::DEFAULT_COLUMNS);
    let settings = settings(context);
    let layout_mode = LayoutMode::for_width(columns, settings.popup.min_width);
    let shown_path = dir.join(SHOWN_FOOTER_FILE);
    let is_stale = kind.is_some_and(|kind| kind.category() != category);
    if event == FooterEvent::Focus && is_stale {
        // Never printed yet: the palette's first frame, key hints.
        let shown = context.files.read(&shown_path)?.unwrap_or_else(|| {
            layout::footer(FooterMode::Keys, category, &[], None, columns, layout_mode)
        });
        write!(out, "{shown}")?;
        return Ok(0);
    }
    let path = dir.join(FOOTER_FILE);
    let mut mode = context
        .files
        .read(&path)?
        .and_then(|text| text.parse::<FooterMode>().ok())
        .unwrap_or_default();
    if event == FooterEvent::Toggle {
        mode = mode.toggled();
        context.files.write(&path, mode.as_str())?;
    }
    let colors = text_colors(context, &settings);
    let head = match mode {
        FooterMode::Description => branch(context, category, lines, columns),
        FooterMode::Keys => None,
    };
    let branch = head.as_ref().map(|head| FooterBranch {
        label: head.label(),
        color: colors.branch,
    });
    let mut text = layout::footer(mode, category, lines, branch, columns, layout_mode);
    if let Some(kind) = kind {
        text = layout::color_tag(&text, kind, colors);
    }
    // Best effort: the saved copy only bridges a category switch
    // (`footer-shown`); failing to save it mustn't blank this footer.
    let _ = context.files.write(&shown_path, &text);
    write!(out, "{text}")?;
    Ok(0)
}

/// What is checked out in the cwd the footer shows for `lines` (Jump to
/// only): one lookup per footer, none without a cwd or when the line is
/// too narrow to show a branch.
fn branch(
    context: &Context<'_>,
    category: Category,
    lines: &[FooterLine],
    columns: usize,
) -> Option<Head> {
    if !layout::fits_branch(columns) {
        return None;
    }
    let cwd = layout::shown_cwd(category, lines)?;
    let cwd = panes::without_tilde(&cwd, context.env.home.as_deref());
    context.branches.head(&cwd)
}

/// The category shown, as `switch` last saved it (the palette writes
/// `open_on` at open); Commands when unknown.
fn current_category(context: &Context<'_>, dir: &Path) -> Result<Category, Error> {
    let category = context
        .files
        .read(&dir.join(CATEGORY_FILE))?
        .and_then(|text| text.parse::<Category>().ok())
        .unwrap_or(Category::Commands);
    Ok(category)
}

/// The settings, or the defaults when they are invalid (the palette has
/// shown the problem at open).
fn settings(context: &Context<'_>) -> Settings {
    context.settings.load().unwrap_or_default()
}

/// The `settings`' text colors over Herdr's theme, in the appearance the
/// palette detected (never asked here: fzf owns the terminal); the
/// defaults when the config is invalid.
fn text_colors(context: &Context<'_>, settings: &Settings) -> TextColors {
    let theme = context
        .config
        .load()
        .map_or(ThemeColors::TERMINAL_DEFAULT, |config| {
            theme::theme_colors(&config.theme, context.env.appearance)
        });
    TextColors::new(theme, &settings.colors)
}

/// The palette state dir, which only exists while the palette runs.
fn palette_state<'a>(context: &Context<'a>) -> Result<&'a Path, Error> {
    context
        .env
        .palette_state
        .as_deref()
        .ok_or(Error::NoPaletteState)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::app::fakes::{World, invalid_settings};
    use crate::cli::{Command, FzfCommand};
    use crate::domain::appearance::Appearance;
    use crate::domain::color::Color;
    use crate::domain::config::HerdrConfig;
    use crate::domain::settings::{ColorSettings, Settings};
    use crate::ports::FilesError;

    use super::*;

    fn world() -> World {
        let mut world = World::new();
        world.env.palette_state = Some(PathBuf::from("/s"));
        world.env.fzf_columns = Some(30);
        world.settings.set(Ok(narrow_popup()));
        world
    }

    /// Settings whose `[popup] min_width` keeps 30 columns in the full
    /// layout. Below the floor (76), so only reachable in tests: it keeps the 30-column expectations of the full layout short.
    fn narrow_popup() -> Settings {
        let mut settings = Settings::default();
        settings.popup.min_width = 40;
        settings
    }

    fn footer_command(event: FooterEvent, lines: Option<&str>) -> Command {
        Command::Fzf(FzfCommand::Footer {
            event,
            kind: None,
            lines: lines.map(str::to_owned),
        })
    }

    fn tags_colored(world: &World) {
        let colors = ColorSettings {
            muted: Some(Color::Ansi(8)),
            tags: Some(Color::Ansi(5)),
            ..ColorSettings::default()
        };
        world.settings.set(Ok(Settings {
            colors,
            ..narrow_popup()
        }));
    }

    #[test]
    fn rows_prints_the_category_file() {
        let world = world();
        world.files.put("/s/rows-jump_to", "pane\tw1:p1\tx\ty\n");

        world
            .run(&Command::Fzf(FzfCommand::Rows {
                category: Category::JumpTo,
            }))
            .unwrap();

        assert_eq!(world.out(), "pane\tw1:p1\tx\ty\n");
    }

    #[test]
    fn switch_wraps_and_remembers_the_category() {
        let world = world();

        world
            .run(&Command::Fzf(FzfCommand::Switch { offset: -1 }))
            .unwrap();

        assert_eq!(world.files.get("/s/category").as_deref(), Some("jump_to"));
        assert_eq!(
            world.out(),
            "reload(\"$HCP_PROGRAM\" fzf rows jump_to)+change-header( \x1b[2mCommands\x1b[0m  \
             \x1b[1mJump to\x1b[0m)+first"
        );
    }

    #[test]
    fn switch_names_the_program_by_its_variable_not_its_path() {
        let mut world = world();
        world.program = "/opt/{q} {} a)+b/hcp".into();

        world
            .run(&Command::Fzf(FzfCommand::Switch { offset: 1 }))
            .unwrap();

        let out = world.out();
        assert!(out.starts_with("reload(\"$HCP_PROGRAM\" fzf rows jump_to)+"));
        assert!(!out.contains("/opt"));
    }

    #[test]
    fn switch_mutes_the_header_with_the_settings_color() {
        let world = world();
        tags_colored(&world);

        world
            .run(&Command::Fzf(FzfCommand::Switch { offset: 1 }))
            .unwrap();

        assert!(world.out().contains("( \x1b[38;5;8mCommands\x1b[0m  "));
    }

    #[test]
    fn invalid_settings_give_default_colors_silently() {
        let world = world();
        world.settings.set(Err(invalid_settings()));

        world
            .run(&Command::Fzf(FzfCommand::Switch { offset: 1 }))
            .unwrap();

        assert!(
            world
                .out()
                .contains(&layout::header(Category::JumpTo, TextColors::default()))
        );
        assert_eq!(world.herdr.methods(), Vec::<String>::new());
    }

    #[test]
    fn footer_colors_the_kind_tag() {
        let world = world();
        tags_colored(&world);

        world
            .run(&Command::Fzf(FzfCommand::Footer {
                event: FooterEvent::Focus,
                kind: Some("custom".into()),
                lines: Some("custom · make".into()),
            }))
            .unwrap();

        assert!(
            world
                .out()
                .starts_with(" \x1b[38;5;5mcustom\x1b[0m · make ")
        );
    }

    #[test]
    fn footer_tags_default_to_the_theme_subtext0() {
        let world = world();

        world
            .run(&Command::Fzf(FzfCommand::Footer {
                event: FooterEvent::Focus,
                kind: Some("plugin".into()),
                lines: Some("plugin · demo.hello".into()),
            }))
            .unwrap();

        // Catppuccin (Herdr's default theme) subtext0: #a6adc8.
        assert!(
            world
                .out()
                .starts_with(" \x1b[38;2;166;173;200mplugin\x1b[0m · demo.hello ")
        );
    }

    #[test]
    fn footer_tags_use_the_appearance_the_palette_detected() {
        let mut world = world();
        world.env.appearance = Some(Appearance::Light);
        world.config.set(Ok(
            HerdrConfig::from_toml("[theme]\nauto_switch = true").unwrap()
        ));
        world.terminal.set_appearance(Some(Appearance::Dark));

        world
            .run(&Command::Fzf(FzfCommand::Footer {
                event: FooterEvent::Focus,
                kind: Some("plugin".into()),
                lines: Some("plugin · demo.hello".into()),
            }))
            .unwrap();

        // Latte's subtext0 (#6c6f85); fzf owns the terminal, so it isn't asked.
        assert!(
            world
                .out()
                .starts_with(" \x1b[38;2;108;111;133mplugin\x1b[0m · demo.hello ")
        );
        assert_eq!(world.terminal.appearance_queries(), 0);
    }

    #[test]
    fn footer_ignores_an_empty_kind() {
        let world = world();
        tags_colored(&world);

        world
            .run(&Command::Fzf(FzfCommand::Footer {
                event: FooterEvent::Focus,
                kind: Some(String::new()),
                lines: Some("custom · make".into()),
            }))
            .unwrap();

        assert!(world.out().starts_with(" custom · make "));
    }

    #[test]
    fn switch_starts_from_the_saved_category() {
        let world = world();
        world.files.put("/s/category", "jump_to");

        world
            .run(&Command::Fzf(FzfCommand::Switch { offset: 1 }))
            .unwrap();

        assert_eq!(world.files.get("/s/category").as_deref(), Some("commands"));
    }

    #[test]
    fn footer_shows_the_description_until_toggled() {
        let world = world();

        world
            .run(&footer_command(FooterEvent::Focus, Some("Close the pane")))
            .unwrap();
        world
            .run(&footer_command(FooterEvent::Toggle, Some("Close the pane")))
            .unwrap();

        assert_eq!(
            world.out(),
            " Close the pane          [^/] ↑↓/pgup/pgdn · catego…  [^/]"
        );
        assert_eq!(world.files.get("/s/footer").as_deref(), Some("keys"));
    }

    #[test]
    fn footer_shows_even_when_it_cannot_be_saved() {
        let world = world();
        world.files.fail_writes_with("disk full");

        let code = world.run(&footer_command(FooterEvent::Focus, Some("Close the pane")));

        assert_eq!(code, Ok(0));
        assert_eq!(world.out(), " Close the pane          [^/]");
    }

    #[test]
    fn footer_has_two_lines_in_jump_to() {
        let world = world();
        world.files.put("/s/category", "jump_to");

        world
            .run(&Command::Fzf(FzfCommand::Footer {
                event: FooterEvent::Focus,
                kind: Some("pane".into()),
                lines: Some("api{hcp:seg}build{hcp:seg}w1:p2{hcp:line}~/src/api".into()),
            }))
            .unwrap();

        assert_eq!(
            world.out(),
            " api / build / w1:p2\n ~/src/api               [^/]"
        );
    }

    fn focus_on(kind: &str, lines: &str) -> Command {
        Command::Fzf(FzfCommand::Footer {
            event: FooterEvent::Focus,
            kind: Some(kind.into()),
            lines: Some(lines.into()),
        })
    }

    /// A Jump to footer focus on a pane in `~/src/api`.
    fn focus_on_api() -> Command {
        focus_on("pane", "api{hcp:seg}w1:p2{hcp:line}~/src/api")
    }

    fn in_jump_to_with_branch() -> World {
        let world = world();
        world.files.put("/s/category", "jump_to");
        world
            .branches
            .set("/home/me/src/api", Head::Branch("main".into()));
        world
    }

    #[test]
    fn footer_shows_the_branch_before_the_cwd_in_mauve() {
        let world = in_jump_to_with_branch();

        world.run(&focus_on_api()).unwrap();

        // Catppuccin's mauve: #cba6f7.
        assert_eq!(
            world.out(),
            " api / w1:p2\n \x1b[38;2;203;166;247mmain\x1b[0m · ~/src/api        [^/]"
        );
        assert_eq!(
            world.branches.lookups(),
            [PathBuf::from("/home/me/src/api")]
        );
    }

    #[test]
    fn footer_paints_the_branch_with_the_settings_color() {
        let world = in_jump_to_with_branch();
        world.settings.set(Ok(Settings {
            colors: ColorSettings {
                branch: Some(Color::Ansi(3)),
                ..ColorSettings::default()
            },
            ..Settings::default()
        }));

        world.run(&focus_on_api()).unwrap();

        assert!(
            world
                .out()
                .contains("\n \x1b[38;5;3mmain\x1b[0m · ~/src/api")
        );
    }

    #[test]
    fn footer_shows_a_detached_heads_short_hash() {
        let world = in_jump_to_with_branch();
        world
            .branches
            .set("/home/me/src/api", Head::Detached("1a2b3c4".into()));

        world.run(&focus_on_api()).unwrap();

        // 30 columns: a branch cap of 6.
        assert!(world.out().contains("m1a2b3…\x1b[0m · ~/src/api"));
    }

    #[test]
    fn footer_shows_the_cwd_alone_outside_a_repository() {
        let world = world();
        world.files.put("/s/category", "jump_to");

        world.run(&focus_on_api()).unwrap();

        assert_eq!(world.out(), " api / w1:p2\n ~/src/api               [^/]");
        assert_eq!(world.branches.lookups().len(), 1);
    }

    #[test]
    fn footer_looks_nothing_up_without_a_cwd() {
        let world = in_jump_to_with_branch();

        world
            .run(&focus_on("pane", "api{hcp:seg}w1:p2{hcp:line}"))
            .unwrap();

        assert_eq!(world.branches.lookups(), Vec::<PathBuf>::new());
    }

    #[test]
    fn footer_looks_nothing_up_too_narrow_for_a_branch() {
        // 11 inner columns: a branch cap of 0.
        let mut world = in_jump_to_with_branch();
        world.env.fzf_columns = Some(13);

        world.run(&focus_on_api()).unwrap();

        assert_eq!(world.branches.lookups(), Vec::<PathBuf>::new());
    }

    #[test]
    fn footer_looks_nothing_up_in_keys_mode() {
        let world = in_jump_to_with_branch();
        world.files.put("/s/footer", "keys");

        world.run(&focus_on_api()).unwrap();

        assert_eq!(world.branches.lookups(), Vec::<PathBuf>::new());
    }

    #[test]
    fn footer_looks_up_the_branch_when_toggled_back_to_the_description() {
        let world = in_jump_to_with_branch();
        world.files.put("/s/footer", "keys");

        world
            .run(&Command::Fzf(FzfCommand::Footer {
                event: FooterEvent::Toggle,
                kind: Some("pane".into()),
                lines: Some("api{hcp:seg}w1:p2{hcp:line}~/src/api".into()),
            }))
            .unwrap();

        assert!(world.out().contains("main\x1b[0m · ~/src/api"));
    }

    #[test]
    fn footer_looks_nothing_up_in_commands_or_on_a_stale_row() {
        let world = world();
        world
            .branches
            .set("/home/me/src/api", Head::Branch("main".into()));

        world.run(&focus_on("builtin", "Close the pane")).unwrap();
        world.run(&focus_on_api()).unwrap();

        assert_eq!(world.branches.lookups(), Vec::<PathBuf>::new());
    }

    #[test]
    fn footer_keeps_the_last_footer_on_a_pane_row_in_commands() {
        let world = world();
        world.run(&focus_on("builtin", "Close the pane")).unwrap();
        let shown = world.out();

        world
            .run(&focus_on("pane", "api{hcp:seg}1{hcp:seg}w1:p3"))
            .unwrap();

        assert_eq!(world.out(), format!("{shown}{shown}"));
    }

    #[test]
    fn footer_keeps_the_last_footer_on_a_command_row_in_jump_to() {
        let world = world();
        world.files.put("/s/category", "jump_to");
        world
            .run(&focus_on("pane", "api{hcp:seg}1{hcp:seg}w1:p3"))
            .unwrap();
        let shown = world.out();

        world.run(&focus_on("builtin", "Close the pane")).unwrap();

        assert_eq!(world.out(), format!("{shown}{shown}"));
        assert_eq!(shown.lines().count(), 2);
    }

    #[test]
    fn footer_shows_the_first_frame_on_a_stale_row_before_any_footer() {
        let world = world();
        world.files.put("/s/category", "jump_to");

        world.run(&focus_on("builtin", "Close the pane")).unwrap();

        assert_eq!(
            world.out(),
            layout::footer(
                FooterMode::Keys,
                Category::JumpTo,
                &[],
                None,
                30,
                LayoutMode::Full
            )
        );
        assert_eq!(world.files.get("/s/footer-shown"), None);
    }

    #[test]
    fn footer_toggles_on_a_stale_row() {
        let world = world();

        world
            .run(&Command::Fzf(FzfCommand::Footer {
                event: FooterEvent::Toggle,
                kind: Some("pane".into()),
                lines: Some("api{hcp:seg}1{hcp:seg}w1:p3".into()),
            }))
            .unwrap();

        assert_eq!(
            world.out(),
            layout::footer(
                FooterMode::Keys,
                Category::Commands,
                &[],
                None,
                30,
                LayoutMode::Full
            )
        );
    }

    #[test]
    fn footer_keeps_two_lines_in_jump_to_keys_mode() {
        let world = world();
        world.files.put("/s/category", "jump_to");
        world.files.put("/s/footer", "keys");

        world
            .run(&footer_command(
                FooterEvent::Focus,
                Some("api{hcp:seg}build{hcp:seg}w1:p2"),
            ))
            .unwrap();

        assert_eq!(
            world.out(),
            layout::footer(
                FooterMode::Keys,
                Category::JumpTo,
                &[],
                None,
                30,
                LayoutMode::Full
            )
        );
        assert_eq!(world.out().lines().count(), 2);
    }

    #[test]
    fn footer_mode_is_sticky() {
        let world = world();
        world.files.put("/s/footer", "keys");

        world
            .run(&footer_command(FooterEvent::Focus, Some("Close the pane")))
            .unwrap();

        assert_eq!(
            world.out(),
            layout::footer(
                FooterMode::Keys,
                Category::Commands,
                &[FooterLine::from_text("Close the pane")],
                None,
                30,
                LayoutMode::Full
            )
        );
    }

    #[test]
    fn footer_is_80_columns_wide_without_fzf_columns() {
        let mut world = world();
        world.env.fzf_columns = None;

        world
            .run(&footer_command(FooterEvent::Focus, Some("Close the pane")))
            .unwrap();

        assert_eq!(
            world.out(),
            layout::footer(
                FooterMode::Description,
                Category::Commands,
                &[FooterLine::from_text("Close the pane")],
                None,
                80,
                LayoutMode::Full
            )
        );
    }

    #[test]
    fn callbacks_need_the_palette_state() {
        let world = World::new();

        let error = world
            .run(&Command::Fzf(FzfCommand::Switch { offset: 1 }))
            .unwrap_err();

        assert_eq!(error, Error::NoPaletteState);
    }

    #[test]
    fn unreadable_state_is_a_files_error() {
        let world = world();
        world.files.fail_with("denied");

        let error = world
            .run(&Command::Fzf(FzfCommand::Rows {
                category: Category::Commands,
            }))
            .unwrap_err();

        assert_eq!(
            error,
            Error::Files(FilesError::Read {
                path: PathBuf::from("/s/rows-commands"),
                message: "denied".into(),
            })
        );
    }

    /// A Jump to row and a Commands row, as the palette saves them.
    fn saved_rows() -> Vec<Row> {
        vec![
            Row::new(Kind::Builtin, "zoom", "Toggle zoom")
                .with_shortcut("prefix+z")
                .with_footer_text("Zoom"),
            Row::new(Kind::Pane, "w1:p1", "fix · 3")
                .with_title_column("api ")
                .with_shortcut("claude")
                .with_right_width(8)
                .with_footer(vec![FooterLine::from_parts(["api", "w1:p1"])]),
        ]
    }

    fn rows_command(category: Category) -> Command {
        Command::Fzf(FzfCommand::Rows { category })
    }

    #[test]
    fn saves_the_rows_of_one_category_as_json() {
        let rows = saved_rows();

        let json = rows_json(&rows, Category::JumpTo);

        let saved: SavedRows<Row> = serde_json::from_str(&json).unwrap();
        assert_eq!(saved.version, ROWS_JSON_VERSION);
        assert_eq!(saved.rows, rows[1..]);
        assert_eq!(rows_json_file(Category::JumpTo), "rows-jump_to.json");
    }

    /// Every row kind with every field set, both categories.
    fn rows_of_every_kind() -> Vec<Row> {
        vec![
            Row::new(Kind::Plugin, "mmc.git.status", "Git status")
                .with_shortcut("prefix+g")
                .with_right_width(10)
                .with_footer_text("Show git status")
                .with_search_extra("git"),
            Row::new(Kind::Builtin, "zoom", "Toggle zoom")
                .with_shortcut("prefix+z, prefix+f")
                .with_footer_text("Zoom the focused pane"),
            Row::new(Kind::Palette, "reload", "Reload palette").with_footer_text("Reload"),
            Row::new(Kind::Custom, "2", "Popup: lazygit")
                .with_id_name("Popup: lazygit")
                .with_shortcut("prefix+l")
                .with_footer_text("lazygit"),
            Row::new(Kind::Pane, "w1:p1", "fix · 3")
                .with_title_column("api-gat…")
                .with_shortcut("claude")
                .with_right_width(8)
                .with_footer(vec![
                    FooterLine::from_parts(["api-gateway", "logs", "w1:p1"]),
                    FooterLine::from_text("~/code/api"),
                ])
                .with_search_extra("api-gateway"),
        ]
    }

    // When this snapshot changes, bump `ROWS_JSON_VERSION`: a palette
    // opened by an older build must fall back to its formatted rows
    // instead of misreading the new JSON. The version is in the snapshot,
    // so the bump shows in the review diff.
    #[test]
    fn rows_json_keeps_its_shape() {
        let rows = rows_of_every_kind();

        let json = Category::ALL
            .map(|category| {
                let value: serde_json::Value =
                    serde_json::from_str(&rows_json(&rows, category)).unwrap();
                format!("{category}:\n{value:#}")
            })
            .join("\n\n");

        insta::assert_snapshot!(json);
    }

    #[test]
    fn rows_lays_the_saved_rows_out_for_fzf_columns() {
        let world = world();
        world.files.put("/s/rows-commands", "stale\n");
        world.files.put(
            "/s/rows-commands.json",
            &rows_json(&saved_rows(), Category::Commands),
        );

        world.run(&rows_command(Category::Commands)).unwrap();

        assert_eq!(
            world.out(),
            format!(
                "{}\n",
                layout::row_line(
                    &saved_rows()[0],
                    30,
                    LayoutMode::Full,
                    TextColors::default()
                )
            )
        );
    }

    #[test]
    fn rows_uses_the_minimal_layout_below_75_percent_of_min_width() {
        // The default `min_width` (76): 30 columns are an outer 33.
        let world = world();
        world.settings.set(Ok(Settings::default()));
        world.files.put(
            "/s/rows-jump_to.json",
            &rows_json(&saved_rows(), Category::JumpTo),
        );

        world.run(&rows_command(Category::JumpTo)).unwrap();

        assert_eq!(
            world.out(),
            format!(
                "{}\n",
                layout::row_line(
                    &saved_rows()[1],
                    30,
                    LayoutMode::Minimal,
                    TextColors::default()
                )
            )
        );
        assert!(world.out().contains("\t fix · 3 "), "{}", world.out());
    }

    #[test]
    fn rows_mutes_the_shortcuts_with_the_settings_color() {
        let world = world();
        tags_colored(&world);
        world.files.put(
            "/s/rows-commands.json",
            &rows_json(&saved_rows(), Category::Commands),
        );

        world.run(&rows_command(Category::Commands)).unwrap();

        assert!(world.out().contains("\x1b[38;5;8mprefix+z"));
    }

    #[test]
    fn rows_falls_back_to_the_formatted_rows_without_json() {
        let world = world();
        world.files.put("/s/rows-commands", "formatted\n");

        world.run(&rows_command(Category::Commands)).unwrap();

        assert_eq!(world.out(), "formatted\n");
    }

    #[test]
    fn rows_falls_back_to_the_formatted_rows_when_the_json_is_corrupt() {
        let world = world();
        world.files.put("/s/rows-commands", "formatted\n");
        world.files.put("/s/rows-commands.json", "[{\"kind\":");

        world.run(&rows_command(Category::Commands)).unwrap();

        assert_eq!(world.out(), "formatted\n");
    }

    #[test]
    fn rows_falls_back_to_the_formatted_rows_when_the_json_has_no_version() {
        // As written before the version: a bare list of rows.
        let world = world();
        world.files.put("/s/rows-commands", "formatted\n");
        let unversioned = serde_json::to_string(&saved_rows()[..1]).unwrap();
        world.files.put("/s/rows-commands.json", &unversioned);

        world.run(&rows_command(Category::Commands)).unwrap();

        assert_eq!(world.out(), "formatted\n");
    }

    #[test]
    fn rows_falls_back_to_the_formatted_rows_when_the_json_version_differs() {
        let world = world();
        world.files.put("/s/rows-commands", "formatted\n");
        let other = SavedRows {
            version: ROWS_JSON_VERSION + 1,
            rows: saved_rows(),
        };
        world.files.put(
            "/s/rows-commands.json",
            &serde_json::to_string(&other).unwrap(),
        );

        world.run(&rows_command(Category::Commands)).unwrap();

        assert_eq!(world.out(), "formatted\n");
    }

    #[test]
    fn rows_falls_back_to_the_formatted_rows_without_fzf_columns() {
        let mut world = world();
        world.env.fzf_columns = None;
        world.files.put("/s/rows-commands", "formatted\n");
        world.files.put(
            "/s/rows-commands.json",
            &rows_json(&saved_rows(), Category::Commands),
        );

        world.run(&rows_command(Category::Commands)).unwrap();

        assert_eq!(world.out(), "formatted\n");
    }

    #[test]
    fn resize_reloads_the_current_category() {
        let world = world();
        world.files.put("/s/category", "jump_to");

        world.run(&Command::Fzf(FzfCommand::Resize)).unwrap();

        assert_eq!(
            world.out(),
            "reload-sync(\"$HCP_PROGRAM\" fzf rows jump_to)"
        );
    }

    #[test]
    fn resize_names_the_program_by_its_variable_not_its_path() {
        let mut world = world();
        world.program = "/opt/{q} {} a)+b/hcp".into();
        world.files.put("/s/category", "jump_to");

        world.run(&Command::Fzf(FzfCommand::Resize)).unwrap();

        assert_eq!(
            world.out(),
            "reload-sync(\"$HCP_PROGRAM\" fzf rows jump_to)"
        );
    }

    #[test]
    fn resize_needs_the_palette_state() {
        let world = World::new();

        let error = world.run(&Command::Fzf(FzfCommand::Resize)).unwrap_err();

        assert_eq!(error, Error::NoPaletteState);
    }

    #[test]
    fn footer_drops_the_toggle_hint_below_75_percent_of_min_width() {
        let world = world();
        world.settings.set(Ok(Settings::default()));

        world
            .run(&footer_command(FooterEvent::Focus, Some("Close the pane")))
            .unwrap();

        assert_eq!(world.out(), " Close the pane");
    }

    #[test]
    fn footer_shows_a_minimal_first_frame_on_a_stale_row() {
        let world = world();
        world.settings.set(Ok(Settings::default()));
        world.files.put("/s/category", "jump_to");

        world.run(&focus_on("builtin", "Close the pane")).unwrap();

        assert_eq!(
            world.out(),
            layout::footer(
                FooterMode::Keys,
                Category::JumpTo,
                &[],
                None,
                30,
                LayoutMode::Minimal
            )
        );
    }

    #[test]
    fn rows_files_are_named_by_category() {
        assert_eq!(rows_file(Category::Commands), "rows-commands");
    }
}
