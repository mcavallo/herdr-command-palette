//! [`Picker`] running fzf with the palette's flags, bindings and colors,
//! including the version preflight.
//!
//! fzf reads the rows from stdin and draws on `/dev/tty`; the selected line
//! comes back on stdout. Its exit status says how it ended: 0 selected,
//! 1 no match, 130 esc; anything else is a failure.

use std::io::{ErrorKind, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::domain::color::Color;
use crate::domain::env::HCP_PROGRAM_SH;
use crate::ports::{Pick, PickRequest, Picker, PickerError};

/// fzf's key for the footer toggle (shown as `^/`).
const TOGGLE_KEY: &str = "ctrl-/";

/// fzf, found in `PATH` unless told otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fzf {
    program: PathBuf,
}

impl Fzf {
    /// `fzf` from `PATH` (`PATH` only, no guessing).
    #[must_use]
    pub fn new() -> Self {
        Self::with_program("fzf")
    }

    /// Another executable (tests use a stub script).
    #[must_use]
    pub fn with_program(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }
}

impl Default for Fzf {
    fn default() -> Self {
        Self::new()
    }
}

impl Picker for Fzf {
    fn version(&self) -> Result<Option<String>, PickerError> {
        let output = match Command::new(&self.program)
            .arg("--version")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
        {
            Ok(output) => output,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(PickerError::Start(error.to_string())),
        };
        let text = String::from_utf8_lossy(&output.stdout);
        Ok(Some(text.trim().to_owned()))
    }

    fn pick(&self, request: &PickRequest) -> Result<Pick, PickerError> {
        let start = |error: std::io::Error| PickerError::Start(error.to_string());
        let mut child = Command::new(&self.program)
            .args(args(request))
            .envs(request.env.iter().map(|(name, value)| (name, value)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(start)?;
        if let Some(mut stdin) = child.stdin.take() {
            // fzf reads while it starts; if it quits early the pipe is
            // closed, which its exit status explains better.
            let _ = stdin.write_all(request.input.as_bytes());
        }
        let output = child.wait_with_output().map_err(start)?;
        match output.status.code() {
            Some(0) => {
                let line = String::from_utf8_lossy(&output.stdout);
                Ok(Pick::Selected(line.trim_end_matches('\n').to_owned()))
            }
            Some(1 | 130) => Ok(Pick::Cancelled),
            // Killed by a signal: no code.
            code => Err(PickerError::Status(code.unwrap_or(-1))),
        }
    }
}

/// fzf's command line for `request`.
///
/// Rows are `kind \t payload \t display \t footer` (`layout::row_line`):
/// only the display column is shown and searched (`--with-nth=3`), and the
/// footer callback gets the last field (its lines, tagged). `--no-hscroll`
/// and an empty `--ellipsis` keep the hidden search tail out of view.
/// `--tiebreak=index` keeps equal matches in list order (recent first, then
/// the catalog), the same at every width, so a resize can't reorder rows or
/// move the cursor to another row.
#[must_use]
fn args(request: &PickRequest) -> Vec<String> {
    let program = HCP_PROGRAM_SH;
    let colors = &request.colors;
    let bg = colors.background.to_fzf();
    // The selected row stays bold, in `selected` (Herdr's `text` by
    // default), not fzf's own `fg+`, which differs per terminal. Its
    // background stays the panel's: fzf paints `bg+` on its own last
    // column too, which looked broken.
    let selected = format!("{}:bold", colors.selected.to_fzf());
    let footer = colors.footer.to_fzf();
    // Rows, query and categories in Herdr's theme colors, not fzf's own
    // defaults, which differ per terminal. The `Search` ghost inherits the
    // query's color, dimmed by fzf.
    let rows = colors.rows.to_fzf();
    let query = colors.query.to_fzf();
    let header = colors.header.to_fzf();
    // Matches are underlined; bold only in the selected row, which is
    // bold itself (elsewhere bold clashed with it). Without `[colors]
    // matches` they keep their row's color.
    let matches = colors.matches.to_fzf();
    let selected_matches = colors.selected_matches.to_fzf();
    let dividers = colors
        .dividers
        .map_or_else(|| "dim".to_owned(), Color::to_fzf);
    [
        // Minimal chrome: Herdr already draws the border.
        "--ansi",
        "--layout=reverse",
        "--info=hidden",
        "--no-hscroll",
        "--ellipsis=",
        "--no-separator",
        "--no-scrollbar",
        "--border=none",
        "--margin=0",
        "--padding=0",
        "--prompt= ",
        "--input-border=line",
        "--ghost=Search",
        "--pointer=",
        "--marker=",
        "--gutter= ",
        "--delimiter=\t",
        "--with-nth=3",
        "--tiebreak=index",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain([
        format!("--header={}", request.header),
        "--header-first".to_owned(),
        "--header-border=line".to_owned(),
        format!("--footer={}", request.footer),
        "--footer-border=line".to_owned(),
        // Bindings run with POSIX `sh`, not the user's `$SHELL` (fish, nu),
        // which expands `"$HCP_PROGRAM"` (set by the palette, passed on by
        // fzf) to this binary. Its path stays out of the binding text, where
        // fzf would expand placeholders (`{q}`) or end an argument at `)`.
        // Each binding's action uses fzf's `action:arg` form, which takes the
        // rest of the text.
        "--with-shell=sh -c".to_owned(),
        // `--`: a footer may start with `-` (a workspace named `-scratch`);
        // clap must not read it as an option. The kind (`{1}`) says which
        // footer tag to color. `{4}` is the row's footer, its lines and parts
        // joined with `{hcp:…}` tags.
        format!(
            "--bind=focus,result:transform-footer:{program} fzf footer focus --kind {{1}} -- {{4}}"
        ),
        format!(
            "--bind={TOGGLE_KEY}:transform-footer:{program} fzf footer toggle --kind {{1}} -- {{4}}"
        ),
        format!("--bind=tab:transform:{program} fzf switch 1"),
        format!("--bind=shift-tab:transform:{program} fzf switch -1"),
        // Herdr resizes the popup with its terminal: the rows are laid out
        // again for the new width, and the reload's `result` redraws the footer.
        format!("--bind=resize:transform:{program} fzf resize"),
        // Herdr's panel background hides its reserved column; the footer
        // uses the keybinds modal's color. The settings' `[colors]` replace
        // either.
        format!("--color=bg:{bg},bg+:{bg},gutter:{bg},footer:{footer}"),
        format!("--color=fg:{rows},query:{query},header:{header}"),
        format!(
            "--color=hl:{matches}:underline,hl+:{selected_matches}:bold:underline,fg+:{selected},\
             input-border:{dividers},header-border:{dividers},footer-border:{dividers}"
        ),
    ])
    .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use crate::adapters::scripts;
    use crate::domain::style::PickerColors;

    use super::*;

    /// A plugin path with fzf placeholders, a delimiter end and a space.
    const PROGRAM: &str = "/opt/{q} {} a)+b/hcp";

    fn request() -> PickRequest {
        PickRequest {
            input: "builtin\tzoom\t Toggle zoom\tZoom\npane\tw1:p1\t x\ty\n".into(),
            header: " HEADER".into(),
            footer: " FOOTER".into(),
            colors: PickerColors {
                background: Color::Rgb(0x18, 0x18, 0x25),
                footer: Color::Ansi(7),
                header: Color::Rgb(0xcd, 0xd6, 0xf4),
                query: Color::Rgb(0xa6, 0xad, 0xc8),
                rows: Color::Rgb(0xa6, 0xad, 0xc8),
                matches: Color::Rgb(0xa6, 0xad, 0xc8),
                selected_matches: Color::Rgb(0xcd, 0xd6, 0xf4),
                selected: Color::Rgb(0xcd, 0xd6, 0xf4),
                dividers: None,
            },
            env: vec![
                ("HCP_STATE".into(), "/tmp/s".into()),
                ("HCP_PROGRAM".into(), PROGRAM.into()),
            ],
        }
    }

    /// A stub fzf running `body`.
    fn script(dir: &Path, body: &str) -> PathBuf {
        scripts::script(dir, "fzf", body)
    }

    /// An fzf that exists but can't be run, in `dir`.
    fn unrunnable(dir: &Path) -> Fzf {
        let path = dir.join("fzf");
        fs::write(&path, "not executable").unwrap();
        Fzf::with_program(&path)
    }

    #[test]
    fn builds_the_fzf_command_line() {
        insta::assert_snapshot!(args(&request()).join("\n"));
    }

    #[test]
    fn bindings_name_the_program_by_its_variable_not_its_path() {
        let args = args(&request());

        let bindings: Vec<&String> = args
            .iter()
            .filter(|arg| arg.starts_with("--bind="))
            .collect();
        assert_eq!(bindings.len(), 5);
        for binding in bindings {
            assert!(binding.contains(":\"$HCP_PROGRAM\" fzf "), "{binding}");
        }
        assert!(args.iter().all(|arg| !arg.contains("/opt")));
    }

    #[test]
    fn colors_from_the_settings_replace_the_defaults() {
        let mut request = request();
        request.colors.matches = Color::Ansi(1);
        request.colors.selected_matches = Color::Ansi(1);
        request.colors.selected = Color::Rgb(0, 0, 0xff);
        request.colors.dividers = Some(Color::Reset);

        let args = args(&request);

        let colors: Vec<&String> = args
            .iter()
            .filter(|arg| arg.starts_with("--color="))
            .collect();
        assert_eq!(
            colors,
            [
                "--color=bg:#181825,bg+:#181825,gutter:#181825,footer:7",
                "--color=fg:#a6adc8,query:#a6adc8,header:#cdd6f4",
                "--color=hl:1:underline,hl+:1:bold:underline,fg+:#0000ff:bold,\
                 input-border:-1,header-border:-1,footer-border:-1"
            ]
        );
    }

    /// The `--color=hl:…` argument for `request`.
    fn match_colors(request: &PickRequest) -> String {
        let args = args(request);
        let colors = args.iter().find(|arg| arg.starts_with("--color=hl:"));
        let colors = colors.unwrap().trim_start_matches("--color=");
        colors.split(',').take(2).collect::<Vec<_>>().join(",")
    }

    #[test]
    fn matches_are_underlined_and_bold_only_in_the_selected_row() {
        assert_eq!(
            match_colors(&request()),
            "hl:#a6adc8:underline,hl+:#cdd6f4:bold:underline"
        );
    }

    #[test]
    fn matches_take_their_colors() {
        let mut request = request();
        request.colors.matches = Color::Ansi(1);
        request.colors.selected_matches = Color::Ansi(4);

        assert_eq!(
            match_colors(&request),
            "hl:1:underline,hl+:4:bold:underline"
        );
    }

    #[test]
    fn reset_matches_keep_the_terminal_color() {
        let mut request = request();
        request.colors.matches = Color::Reset;
        request.colors.selected_matches = Color::Reset;

        assert_eq!(
            match_colors(&request),
            "hl:-1:underline,hl+:-1:bold:underline"
        );
    }

    /// The `fg+:…` slot for `request`.
    fn selected_color(request: &PickRequest) -> String {
        let args = args(request);
        let colors = args.iter().find(|arg| arg.starts_with("--color=hl:"));
        let slot = colors
            .unwrap()
            .split(',')
            .find(|slot| slot.starts_with("fg+:"));
        slot.unwrap().to_owned()
    }

    #[test]
    fn the_selected_row_is_bold_in_its_color() {
        assert_eq!(selected_color(&request()), "fg+:#cdd6f4:bold");
    }

    #[test]
    fn the_selected_row_keeps_the_terminal_text_color_when_reset() {
        let mut request = request();
        request.colors.selected = Color::Reset;

        assert_eq!(selected_color(&request), "fg+:-1:bold");
    }

    /// The `--color=fg:…` argument (rows, query, header) for `request`.
    fn text_colors(request: &PickRequest) -> String {
        let args = args(request);
        let colors = args.iter().find(|arg| arg.starts_with("--color=fg:"));
        colors.unwrap().clone()
    }

    #[test]
    fn rows_query_and_categories_take_their_colors() {
        let mut request = request();
        request.colors.rows = Color::Ansi(1);
        request.colors.query = Color::Ansi(2);
        request.colors.header = Color::Ansi(3);

        assert_eq!(text_colors(&request), "--color=fg:1,query:2,header:3");
    }

    #[test]
    fn reset_text_colors_keep_the_terminal_color() {
        let mut request = request();
        request.colors.rows = Color::Reset;
        request.colors.query = Color::Reset;
        request.colors.header = Color::Reset;

        assert_eq!(text_colors(&request), "--color=fg:-1,query:-1,header:-1");
    }

    #[test]
    fn reads_the_version() {
        let dir = tempfile::tempdir().unwrap();
        let fzf = Fzf::with_program(script(dir.path(), "echo '0.66.0 (brew)'"));

        assert_eq!(fzf.version(), Ok(Some("0.66.0 (brew)".into())));
    }

    #[test]
    fn missing_fzf_has_no_version() {
        let fzf = Fzf::with_program("/nonexistent/fzf");

        assert_eq!(fzf.version(), Ok(None));
    }

    #[test]
    fn default_is_new() {
        assert_eq!(Fzf::default(), Fzf::new());
    }

    #[test]
    fn an_unrunnable_fzf_has_a_start_error_for_a_version() {
        let dir = tempfile::tempdir().unwrap();

        let version = unrunnable(dir.path()).version();

        assert!(matches!(version, Err(PickerError::Start(_))));
    }

    #[test]
    fn an_unrunnable_fzf_cannot_pick() {
        let dir = tempfile::tempdir().unwrap();

        let pick = unrunnable(dir.path()).pick(&request());

        assert!(matches!(pick, Err(PickerError::Start(_))));
    }

    #[test]
    fn returns_the_selected_line_with_input_and_env_passed() {
        let dir = tempfile::tempdir().unwrap();
        let fzf = Fzf::with_program(script(dir.path(), "grep zoom | sed \"s|Zoom|$HCP_STATE|\""));

        let pick = fzf.pick(&request()).unwrap();

        assert_eq!(
            pick,
            Pick::Selected("builtin\tzoom\t Toggle zoom\t/tmp/s".into())
        );
    }

    #[test]
    fn no_match_and_esc_are_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        for status in [1, 130] {
            let fzf = Fzf::with_program(script(
                dir.path(),
                &format!("cat >/dev/null; exit {status}"),
            ));
            assert_eq!(fzf.pick(&request()), Ok(Pick::Cancelled));
        }
    }

    #[test]
    fn other_statuses_are_failures() {
        let dir = tempfile::tempdir().unwrap();
        let fzf = Fzf::with_program(script(dir.path(), "exit 2"));

        assert_eq!(fzf.pick(&request()), Err(PickerError::Status(2)));
    }

    #[test]
    fn a_killed_fzf_is_a_failure_with_status_minus_1() {
        let dir = tempfile::tempdir().unwrap();
        let fzf = Fzf::with_program(script(dir.path(), "kill -9 $$"));

        assert_eq!(fzf.pick(&request()), Err(PickerError::Status(-1)));
    }
}
