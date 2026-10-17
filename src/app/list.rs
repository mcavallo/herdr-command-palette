//! `list [--ids]`: print every row, for debugging.
//!
//! One row per line, tab-separated: kind, payload, title,
//! shortcut, description and, when present, the hidden search text.
//! `--ids` prints the row id instead (`<kind>:<name>`, what the settings'
//! `hide_ids` and `confirm_ids` take) and the title. Settings don't apply:
//! hidden rows are listed too, so their ids can be looked up.

use std::io::Write;

use crate::domain::row::{FooterLine, Row};

use super::{Context, Error, catalog_rows};

/// Prints the rows; Herdr being unreachable only drops its rows (with a
/// warning on `err`).
///
/// # Errors
///
/// [`Error::Config`] when `config.toml` can't be loaded, [`Error::Output`]
/// when stdout fails.
pub fn list(
    context: &Context<'_>,
    ids: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let config = context.config.load()?;
    let (rows, _) = catalog_rows(context, &config, err);
    for row in rows {
        let line = if ids {
            format!(
                "{}\t{}",
                field(&row.id.to_string()),
                field(&row.full_title())
            )
        } else {
            tsv(&row)
        };
        writeln!(out, "{line}")?;
    }
    Ok(0)
}

/// The row's fields as one TSV line.
fn tsv(row: &Row) -> String {
    let mut fields = vec![
        row.kind.as_str().to_owned(),
        field(&row.payload),
        field(&row.full_title()),
        field(&row.shortcut),
        field(&description(row)),
    ];
    if !row.search_extra.is_empty() {
        fields.push(field(&row.search_extra));
    }
    fields.join("\t")
}

/// The description column: the footer's lines joined with ` · `, the
/// parts of a line with ` / `, so the column keeps Jump
/// to's cwd. Empty lines are left out (a pane without a cwd ends with its
/// location, not a dangling ` · `).
fn description(row: &Row) -> String {
    let lines: Vec<String> = row
        .footer
        .iter()
        .filter(|line| !line.is_empty())
        .map(FooterLine::to_text)
        .collect();
    lines.join(" · ")
}

/// `text` with tabs and newlines replaced by spaces, so it stays one field.
fn field(text: &str) -> String {
    text.replace(['\t', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use crate::app::fakes::{World, parse_error};
    use crate::cli::Command;
    use crate::domain::config::HerdrConfig;
    use crate::domain::row::Kind;
    use crate::ports::HerdrError;

    use super::*;

    #[test]
    fn prints_every_row_as_tsv() {
        let world = World::new();

        world.run(&Command::List { ids: false }).unwrap();

        let out = world.out();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "plugin\tdemo.hello\tdemo.hello\t\tdemo.hello");
        assert_eq!(lines[1], "plugin\tdemo.bye\tdemo.bye\t\tdemo.bye");
        assert_eq!(
            lines[2],
            "builtin\tsplit_vertical\tSplit pane right\tprefix+v\t\
             Split the focused pane, new pane on the right"
        );
        assert_eq!(
            lines.last().unwrap(),
            &"pane\tw1:p2\tdotfiles  terminal · 2\tterminal\t\
              dotfiles / 1 / w1:p2 · /tmp"
        );
    }

    #[test]
    fn ids_print_the_row_id_and_title() {
        let world = World::new();

        world.run(&Command::List { ids: true }).unwrap();

        assert!(world.out().contains("\nbuiltin:zoom\tToggle zoom\n"));
    }

    #[test]
    fn ids_name_custom_commands_by_description() {
        let world = World::new();
        world.config.set(Ok(HerdrConfig::from_toml(
            "[[keys.command]]\ncommand = \"lazygit\"\ndescription = \"Git\"\n",
        )
        .unwrap()));

        world.run(&Command::List { ids: true }).unwrap();

        assert!(world.out().contains("\ncustom:Git\tGit\n"));
    }

    #[test]
    fn keeps_fields_on_one_line() {
        assert_eq!(field("a\tb\nc"), "a b c");
        let row = Row::new(Kind::Pane, "w1:p1", "t").with_search_extra("x");
        assert_eq!(tsv(&row), "pane\tw1:p1\tt\t\t\tx");
    }

    #[test]
    fn joins_footer_lines_with_dots_and_parts_with_slashes() {
        let row = Row::new(Kind::Pane, "w1:p1", "t").with_footer(vec![
            FooterLine::from_parts(["ws", "feat / x", "w1:p1"]),
            FooterLine::from_text("~/src"),
        ]);
        let no_cwd = Row::new(Kind::Pane, "w1:p1", "t").with_footer(vec![
            FooterLine::from_parts(["ws", "w1:p1"]),
            FooterLine::from_text(""),
        ]);

        assert_eq!(description(&row), "ws / feat / x / w1:p1 · ~/src");
        assert_eq!(description(&no_cwd), "ws / w1:p1");
    }

    #[test]
    fn lists_the_rest_when_herdr_is_down() {
        let world = World::new();
        let down = HerdrError::Transport("refused".into());
        world.herdr.reply("plugin.action.list", Err(down.clone()));
        world.herdr.reply("session.snapshot", Err(down));

        world.run(&Command::List { ids: false }).unwrap();

        assert!(world.out().starts_with("builtin\tsplit_vertical\t"));
        assert_eq!(
            world.err(),
            "herdr-command-palette: plugin actions unavailable: cannot talk to herdr: refused\n\
             herdr-command-palette: session unavailable: cannot talk to herdr: refused\n"
        );
    }

    #[test]
    fn a_broken_config_is_an_error() {
        let world = World::new();
        world.config.set(Err(parse_error()));

        assert_eq!(
            world.run(&Command::List { ids: false }),
            Err(Error::Config(parse_error()))
        );
    }
}
