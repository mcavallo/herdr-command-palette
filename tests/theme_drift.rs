//! Drift check: our mirrored theme tables against Herdr's source.
//!
//! `mise run theme-drift [tag]` fetches Herdr's `src/app/state.rs`,
//! `src/app/mod.rs` and `src/config/theme.rs` at a tag and runs the ignored
//! test here with `HERDR_SRC` pointing at them. It compares, for every
//! built-in theme, the colors we use (`panel_bg`, `overlay0`, `subtext0`,
//! `accent`, `text`, `surface0`, `surface_dim`, `mauve`), the theme names, the aliases and the `auto_switch` siblings, and fails
//! listing every difference. The parsers are line based and know Herdr's
//! formatting (rustfmt); a format change shows up as drift, not silence.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use herdr_command_palette::domain::color::Color;
use herdr_command_palette::domain::theme::{
    THEME_ALIASES, THEME_COLORS, THEME_SIBLINGS, ThemeColors, siblings, theme_by_name,
};

/// The text between the first `start` and the next `end` after it.
fn between<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
    let (_, rest) = text
        .split_once(start)
        .unwrap_or_else(|| panic!("`{start}` not found"));
    rest.split_once(end).map_or(rest, |(inside, _)| inside)
}

/// The double-quoted strings in `text`, in order.
fn quoted(text: &str) -> Vec<&str> {
    text.split('"').skip(1).step_by(2).collect()
}

/// A ratatui color expression as Herdr writes it (`Color::Rgb(24, 24, 37)`,
/// `Color::Reset`, `Color::Gray`, `Color::Indexed(8)`).
fn color(expr: &str) -> Color {
    let expr = expr
        .trim()
        .trim_end_matches(',')
        .trim_start_matches("Color::");
    let numbers = |inner: &str| -> Vec<u8> {
        inner
            .split(',')
            .map(|n| n.trim().parse().expect("a color component"))
            .collect()
    };
    if let Some(inner) = expr.strip_prefix("Rgb(").and_then(|e| e.strip_suffix(')')) {
        let [r, g, b] = numbers(inner)[..] else {
            panic!("bad color `{expr}`")
        };
        return Color::Rgb(r, g, b);
    }
    if let Some(inner) = expr
        .strip_prefix("Indexed(")
        .and_then(|e| e.strip_suffix(')'))
    {
        return Color::Ansi(numbers(inner)[0]);
    }
    // Named colors and `Reset` parse like a `[theme.custom]` value.
    Color::parse(expr)
}

/// The colors we mirror per canonical theme name, from `state.rs`'s
/// `Palette::from_name` and `Palette::<theme>()` functions.
fn herdr_palettes(state_rs: &str) -> BTreeMap<String, ThemeColors> {
    let from_name = between(state_rs, "pub fn from_name(", "_ => None");
    let mut palettes = BTreeMap::new();
    for line in from_name.lines() {
        let Some((name, call)) = line.split_once("=> Some(Self::") else {
            continue;
        };
        let name = quoted(name)[0];
        let function = call.split_once('(').expect("a call").0;
        let body = between(
            state_rs,
            &format!("pub fn {function}() -> Self {{"),
            "\n    }",
        );
        // The field's own line (`text:` is also the end of `subtext0:`),
        // without a trailing `// comment`.
        let token = |token: &str| {
            let line = body
                .lines()
                .find_map(|line| line.trim().strip_prefix(&format!("{token}:")))
                .unwrap_or_else(|| panic!("`{token}` not found in `{function}`"));
            color(line.split_once("//").map_or(line, |(expr, _)| expr))
        };
        palettes.insert(
            name.to_owned(),
            ThemeColors {
                panel_bg: token("panel_bg"),
                overlay0: token("overlay0"),
                subtext0: token("subtext0"),
                accent: token("accent"),
                text: token("text"),
                surface0: token("surface0"),
                surface_dim: token("surface_dim"),
                mauve: token("mauve"),
            },
        );
    }
    palettes
}

/// `THEME_NAMES` and the (alias, canonical) pairs of
/// `canonical_theme_name`, from `config/theme.rs`.
fn herdr_names(theme_rs: &str) -> (BTreeSet<String>, BTreeSet<(String, String)>) {
    let names = quoted(between(theme_rs, "THEME_NAMES: &[&str] = &[", "];"))
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut aliases = BTreeSet::new();
    for line in between(theme_rs, "fn canonical_theme_name(", "_ => None").lines() {
        let Some((names, canonical)) = line.split_once("=> Some(") else {
            continue;
        };
        let canonical = quoted(canonical)[0];
        for name in quoted(names) {
            if name != canonical {
                aliases.insert((name.to_owned(), canonical.to_owned()));
            }
        }
    }
    (names, aliases)
}

/// Each `sibling_theme_names` arm of `app/mod.rs`: the names it matches and
/// its (dark, light) pair.
fn herdr_siblings(mod_rs: &str) -> Vec<(Vec<String>, (String, String))> {
    let body = between(mod_rs, "fn sibling_theme_names(", "_ => (name");
    let mut arms = Vec::new();
    let mut names = Vec::new();
    for line in body.lines() {
        if line.trim_end().ends_with("=> {") {
            names = quoted(line).into_iter().map(str::to_owned).collect();
        } else if line.contains(".to_string(),") {
            let pair = quoted(line);
            arms.push((
                std::mem::take(&mut names),
                (pair[0].to_owned(), pair[1].to_owned()),
            ));
        }
    }
    arms
}

/// Every difference between Herdr's sources in `src` and our tables.
fn drift(src: &Path) -> Vec<String> {
    let read = |file: &str| {
        std::fs::read_to_string(src.join(file))
            .unwrap_or_else(|error| panic!("cannot read {file}: {error}"))
    };
    let mut problems = Vec::new();

    let palettes = herdr_palettes(&read("src/app/state.rs"));
    let ours: BTreeMap<String, ThemeColors> = THEME_COLORS
        .iter()
        .map(|(name, colors)| ((*name).to_owned(), *colors))
        .collect();
    for (name, theirs) in &palettes {
        match ours.get(name) {
            None => problems.push(format!("theme `{name}` missing from THEME_COLORS")),
            Some(ours) if ours != theirs => {
                problems.push(format!("theme `{name}`: ours {ours:?}, herdr {theirs:?}"));
            }
            Some(_) => {}
        }
    }
    for name in ours.keys().filter(|name| !palettes.contains_key(*name)) {
        problems.push(format!("theme `{name}` is no longer in herdr"));
    }

    let (names, aliases) = herdr_names(&read("src/config/theme.rs"));
    let our_names: BTreeSet<String> = ours.keys().cloned().collect();
    if names != our_names {
        problems.push(format!("THEME_NAMES: ours {our_names:?}, herdr {names:?}"));
    }
    let our_aliases: BTreeSet<(String, String)> = THEME_ALIASES
        .iter()
        .map(|(alias, canonical)| ((*alias).to_owned(), (*canonical).to_owned()))
        .collect();
    if aliases != our_aliases {
        problems.push(format!(
            "aliases: only ours {:?}, only herdr {:?}",
            our_aliases.difference(&aliases).collect::<Vec<_>>(),
            aliases.difference(&our_aliases).collect::<Vec<_>>()
        ));
    }

    let arms = herdr_siblings(&read("src/app/mod.rs"));
    let pairs: BTreeSet<(String, String)> = arms.iter().map(|(_, pair)| pair.clone()).collect();
    let our_pairs: BTreeSet<(String, String)> = THEME_SIBLINGS
        .iter()
        .map(|(dark, light)| ((*dark).to_owned(), (*light).to_owned()))
        .collect();
    if pairs != our_pairs {
        problems.push(format!(
            "THEME_SIBLINGS: ours {our_pairs:?}, herdr {pairs:?}"
        ));
    }
    for (names, pair) in &arms {
        for name in names {
            if &siblings(name) != pair {
                problems.push(format!(
                    "siblings(`{name}`): ours {:?}, herdr {pair:?}",
                    siblings(name)
                ));
            }
        }
    }
    for name in &names {
        if theme_by_name(name).is_none() {
            problems.push(format!("theme `{name}` doesn't resolve"));
        }
    }
    problems
}

#[test]
#[ignore = "needs Herdr's source: run `mise run theme-drift`"]
fn theme_tables_match_herdr() {
    let src = std::env::var("HERDR_SRC").expect("HERDR_SRC: run `mise run theme-drift`");

    let problems = drift(Path::new(&src));

    assert!(
        problems.is_empty(),
        "theme tables drifted from herdr:\n{}",
        problems.join("\n")
    );
    println!("theme tables match herdr ({} themes)", THEME_COLORS.len());
}

/// The parsers on a tiny excerpt in Herdr's format, so a broken parser
/// fails here and not only when someone runs the drift check.
#[test]
fn parsers_read_herdr_formatting() {
    let state = "    pub fn from_name(name: &str) -> Option<Self> {\n\
                 \x20           \"terminal\" => Some(Self::terminal()),\n\
                 \x20           _ => None,\n\
                 \x20   pub fn terminal() -> Self {\n\
                 \x20       Self {\n\
                 \x20           accent: Color::Blue, // blue\n\
                 \x20           panel_bg: Color::Reset,\n\
                 \x20           surface0: Color::Reset,\n\
                 \x20           surface_dim: Color::DarkGray,\n\
                 \x20           overlay0: Color::Gray,\n\
                 \x20           mauve: Color::Rgb(7, 8, 9),  // iris\n\
                 \x20           text: Color::Rgb(4, 5, 6),\n\
                 \x20           subtext0: Color::Rgb(1, 2, 3),\n\
                 \x20       }\n\
                 \x20   }\n";
    let theme = "pub const THEME_NAMES: &[&str] = &[\n    \"terminal\",\n];\n\
                 pub(crate) fn canonical_theme_name(name: &str) -> Option<&'static str> {\n\
                 \x20       \"one-dark\" | \"onedark\" => Some(\"one-dark\"),\n\
                 \x20       _ => None,\n";
    let module = "fn sibling_theme_names(name: &str) -> (String, String) {\n\
                  \x20       \"kanagawa\" | \"lotus\" => {\n\
                  \x20           (\"kanagawa\".to_string(), \"kanagawa-lotus\".to_string())\n\
                  \x20       }\n\
                  \x20       _ => (name.to_string(), name.to_string()),\n";

    assert_eq!(
        herdr_palettes(state)["terminal"],
        ThemeColors {
            panel_bg: Color::Reset,
            overlay0: Color::Ansi(7),
            subtext0: Color::Rgb(1, 2, 3),
            accent: Color::Ansi(4),
            text: Color::Rgb(4, 5, 6),
            surface0: Color::Reset,
            surface_dim: Color::Ansi(8),
            mauve: Color::Rgb(7, 8, 9),
        }
    );
    let (names, aliases) = herdr_names(theme);
    assert_eq!(names, BTreeSet::from(["terminal".to_owned()]));
    assert_eq!(
        aliases,
        BTreeSet::from([("onedark".to_owned(), "one-dark".to_owned())])
    );
    assert_eq!(
        herdr_siblings(module),
        [(
            vec!["kanagawa".to_owned(), "lotus".to_owned()],
            ("kanagawa".to_owned(), "kanagawa-lotus".to_owned())
        )]
    );
    assert_eq!(color("Color::Indexed(8),"), Color::Ansi(8));
}
