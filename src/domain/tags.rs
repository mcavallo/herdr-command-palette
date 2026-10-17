//! Tags in the palette's own fzf fields: `{hcp:<name>}`, `<name>` matching
//! `[a-z][a-z0-9_]*`.
//!
//! A row's footer travels to the footer callback as one fzf field (`{4}`):
//! its lines joined with [`Tag::Line`], the parts of a line with
//! [`Tag::Seg`] (`api{hcp:seg}1{hcp:seg}w1:p3{hcp:line}~/src/api`).
//! [`serialize`] writes that field and [`parse`] reads it back. User text
//! (titles, labels, paths, commands, descriptions) could spell a tag, so
//! [`strip`] removes every tag from it before it is serialized; the
//! stripper and [`Tag::text`] share one grammar (`tag_len`).

use crate::domain::row::FooterLine;

/// Starts every tag: `{`, the fixed namespace, `:`.
const OPEN: &str = "{hcp:";

/// Ends every tag.
const CLOSE: char = '}';

/// The tags the palette writes. Each is `{hcp:<name>}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    /// Between two footer lines: `{hcp:line}`.
    Line,
    /// Between two parts of a footer line: `{hcp:seg}`.
    Seg,
}

impl Tag {
    /// Every tag.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 2] = [Self::Line, Self::Seg];

    /// The tag's name, `snake_case`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Seg => "seg",
        }
    }

    /// The tag as it appears in a field: `{hcp:<name>}`.
    ///
    /// # Examples
    ///
    /// ```
    /// use herdr_command_palette::domain::tags::Tag;
    ///
    /// assert_eq!(Tag::Seg.text(), "{hcp:seg}");
    /// ```
    #[must_use]
    pub fn text(self) -> String {
        format!("{OPEN}{}{CLOSE}", self.name())
    }
}

/// The byte length of the tag `text` starts with, if it starts with one:
/// `{hcp:`, a name matching `[a-z][a-z0-9_]*`, `}`. The one grammar the
/// stripper uses; [`Tag::text`] follows it (tested).
fn tag_len(text: &str) -> Option<usize> {
    let rest = text.strip_prefix(OPEN)?;
    let name_len =
        rest.find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))?;
    let name = &rest[..name_len];
    let starts_with_letter = name.starts_with(|c: char| c.is_ascii_lowercase());
    let is_closed = rest[name_len..].starts_with(CLOSE);
    (starts_with_letter && is_closed).then_some(OPEN.len() + name_len + CLOSE.len_utf8())
}

/// `text` without one pass of tags: every `{hcp:<name>}` found from the
/// left is removed.
fn strip_once(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(OPEN) {
        let (before, from) = rest.split_at(start);
        out.push_str(before);
        if let Some(len) = tag_len(from) {
            rest = &from[len..];
        } else {
            // Not a tag: keep the `{` and look again after it.
            out.push('{');
            rest = &from[1..];
        }
    }
    out.push_str(rest);
    out
}

/// `text` with every tag (`{hcp:<name>}`) removed, again and again until
/// none is left, so removing one can't join the text around it into a
/// new one (`{hcp:{hcp:seg}seg}` becomes empty). Text that only looks
/// similar stays (`{hcp:Foo}`, `{hcp:}`, `{other:seg}`).
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::tags;
///
/// assert_eq!(tags::strip("a{hcp:seg}b"), "ab");
/// assert_eq!(tags::strip("{hcp:{hcp:seg}seg}"), "");
/// assert_eq!(tags::strip("{hcp:Foo}"), "{hcp:Foo}");
/// ```
#[must_use]
pub fn strip(text: &str) -> String {
    let mut text = text.to_owned();
    loop {
        let stripped = strip_once(&text);
        // Each pass that finds a tag makes the text shorter: this ends.
        if stripped.len() == text.len() {
            return stripped;
        }
        text = stripped;
    }
}

/// `lines` as one fzf field: lines joined with `{hcp:line}`, the parts of
/// a line with `{hcp:seg}`. Every part is [`strip`]ped first, so user
/// text can't add a line or a part. No lines, and one empty line, both
/// give the empty field.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::row::FooterLine;
/// use herdr_command_palette::domain::tags;
///
/// let lines = [FooterLine::from_parts(["api", "1", "w1:p3"]), FooterLine::from_text("~/src/api")];
/// assert_eq!(tags::serialize(&lines), "api{hcp:seg}1{hcp:seg}w1:p3{hcp:line}~/src/api");
/// ```
#[must_use]
pub fn serialize(lines: &[FooterLine]) -> String {
    let seg = Tag::Seg.text();
    lines
        .iter()
        .map(|line| {
            let parts: Vec<String> = line.parts().iter().map(|part| strip(part)).collect();
            parts.join(&seg)
        })
        .collect::<Vec<_>>()
        .join(&Tag::Line.text())
}

/// The footer lines a [`serialize`]d `field` holds; the empty field holds
/// none.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::row::FooterLine;
/// use herdr_command_palette::domain::tags;
///
/// assert_eq!(
///     tags::parse("api{hcp:seg}w1:p3{hcp:line}~"),
///     [FooterLine::from_parts(["api", "w1:p3"]), FooterLine::from_text("~")],
/// );
/// assert_eq!(tags::parse(""), []);
/// ```
#[must_use]
pub fn parse(field: &str) -> Vec<FooterLine> {
    if field.is_empty() {
        return Vec::new();
    }
    let seg = Tag::Seg.text();
    field
        .split(&Tag::Line.text())
        .map(|line| FooterLine::from_parts(line.split(&seg)))
        .collect()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use crate::domain::fixtures::text;

    use super::*;

    /// Whether `text` has a tag anywhere.
    fn has_tag(text: &str) -> bool {
        text.match_indices(OPEN)
            .any(|(start, _)| tag_len(&text[start..]).is_some())
    }

    #[test]
    fn every_tag_follows_the_grammar() {
        for tag in Tag::ALL {
            let text = tag.text();

            assert_eq!(tag_len(&text), Some(text.len()));
        }
    }

    #[test]
    fn reads_snake_case_names_only() {
        assert_eq!(tag_len("{hcp:a1_b}x"), Some(10));
        assert_eq!(tag_len("{hcp:Foo}"), None);
        assert_eq!(tag_len("{hcp:}"), None);
        assert_eq!(tag_len("{hcp:1a}"), None);
        assert_eq!(tag_len("{hcp:_a}"), None);
        assert_eq!(tag_len("{hcp:seg"), None);
        assert_eq!(tag_len("{other:seg}"), None);
        assert_eq!(tag_len("x{hcp:seg}"), None);
    }

    #[test]
    fn strips_every_tag_name() {
        assert_eq!(strip("a{hcp:line}b{hcp:seg}c{hcp:any_name2}d"), "abcd");
    }

    #[test]
    fn strips_until_no_tag_is_left() {
        assert_eq!(strip("{hcp:{hcp:seg}seg}"), "");
        assert_eq!(strip("{hcp:{hcp:{hcp:seg}seg}line}x"), "x");
    }

    #[test]
    fn keeps_text_that_only_looks_like_a_tag() {
        for text in [
            "{hcp:Foo}",
            "{hcp:}",
            "{other:seg}",
            "{hcp:seg",
            "{{hcp:",
            "{",
        ] {
            assert_eq!(strip(text), text);
        }
    }

    #[test]
    fn keeps_a_brace_before_a_tag() {
        assert_eq!(strip("{{hcp:seg}"), "{");
        assert_eq!(strip("日{hcp:seg}本"), "日本");
    }

    #[test]
    fn serializes_lines_and_parts_with_tags() {
        let lines = [
            FooterLine::from_parts(["api", "feat / x", "w1:p3"]),
            FooterLine::from_text("~/src"),
        ];

        assert_eq!(
            serialize(&lines),
            "api{hcp:seg}feat / x{hcp:seg}w1:p3{hcp:line}~/src"
        );
    }

    #[test]
    fn strips_tags_from_parts_before_serializing() {
        let lines = [FooterLine::from_parts(["a{hcp:seg}b", "{hcp:line}"])];

        assert_eq!(serialize(&lines), "ab{hcp:seg}");
        assert_eq!(
            parse(&serialize(&lines)),
            [FooterLine::from_parts(["ab", ""])]
        );
    }

    #[test]
    fn empty_footers_serialize_to_the_empty_field() {
        assert_eq!(serialize(&[]), "");
        assert_eq!(serialize(&[FooterLine::from_text("")]), "");
        assert_eq!(parse(""), []);
    }

    #[test]
    fn parses_empty_lines_between_tags() {
        assert_eq!(
            parse("a{hcp:line}"),
            [FooterLine::from_text("a"), FooterLine::from_text("")]
        );
    }

    /// A footer part: text mixed with tag fragments (`{hcp:`, `seg}`,
    /// whole tags), so leftovers next to a delimiter come up.
    fn part() -> impl Strategy<Value = String> {
        let piece = prop_oneof![
            text(),
            proptest::sample::select(vec![
                "{hcp:",
                "}",
                "{hcp:seg",
                "seg}",
                "line}",
                "{hcp:line}"
            ])
            .prop_map(str::to_owned),
            "[a-z_{}:]{0,5}",
        ];
        proptest::collection::vec(piece, 0..5).prop_map(|pieces| pieces.concat())
    }

    fn line() -> impl Strategy<Value = FooterLine> {
        proptest::collection::vec(part(), 1..4).prop_map(FooterLine::from_parts)
    }

    proptest! {
        #[test]
        fn stripped_text_has_no_tag(text in text(), names in proptest::collection::vec("[a-zA-Z0-9_{}:]{0,6}", 0..4)) {
            // Tag-like pieces mixed into the text.
            let mut mixed = text;
            for name in names {
                mixed = format!("{{hcp:{name}}}{mixed}{{hcp:{mixed}");
            }

            let stripped = strip(&mixed);

            prop_assert!(!has_tag(&stripped), "{stripped:?}");
            prop_assert_eq!(strip(&stripped), stripped);
        }

        #[test]
        fn parse_reads_back_what_serialize_wrote(lines in proptest::collection::vec(line(), 1..4)) {
            let field = serialize(&lines);
            prop_assume!(!field.is_empty());

            let stripped: Vec<FooterLine> = lines
                .iter()
                .map(|line| FooterLine::from_parts(line.parts().iter().map(|part| strip(part))))
                .collect();
            prop_assert_eq!(parse(&field), stripped);
        }
    }
}
