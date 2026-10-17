//! Where a settings problem is: the dotted key at a byte offset (from a
//! TOML error's span) and line numbers.
//!
//! The `toml` crate's errors carry a span but not the key, so the text is
//! parsed again into its spanned tree (`DeTable`) and searched for the
//! innermost key whose key or value covers the offset. Arrays are not
//! entered: `commands.hide_ids` is precise enough.

use std::ops::Range;

use toml::de::{DeTable, DeValue};

/// The dotted key at byte `offset` of `text` (`popup.min_width`); `None`
/// when the text isn't valid TOML or no key covers the offset.
pub(super) fn key_at(text: &str, offset: usize) -> Option<String> {
    let root = DeTable::parse(text).ok()?;
    keys_at(root.get_ref(), offset).map(|keys| keys.join("."))
}

/// The keys from `table` down to the innermost one covering `offset`.
///
/// A table's span is only its `[header]`, so the tables are searched
/// first and the header (an unknown section) matches last.
fn keys_at(table: &DeTable<'_>, offset: usize) -> Option<Vec<String>> {
    for (key, value) in table {
        let below = match value.get_ref() {
            DeValue::Table(inner) => keys_at(inner, offset),
            _ => None,
        };
        let here = covers(&key.span(), offset) || covers(&value.span(), offset);
        if below.is_some() || here {
            let mut keys = vec![key.get_ref().to_string()];
            keys.extend(below.unwrap_or_default());
            return Some(keys);
        }
    }
    None
}

/// The 1-based line of the dotted `key` in `text`, if the text sets it.
/// The key's own line, not its value's: a value may span lines.
pub(super) fn line_of_key(text: &str, key: &str) -> Option<usize> {
    let root = DeTable::parse(text).ok()?;
    let parts: Vec<&str> = key.split('.').collect();
    let (last, parents) = parts.split_last()?;
    let mut table = root.get_ref();
    for part in parents {
        let (_, value) = table.iter().find(|(name, _)| name.get_ref() == part)?;
        let DeValue::Table(inner) = value.get_ref() else {
            return None;
        };
        table = inner;
    }
    let (name, _) = table.iter().find(|(name, _)| name.get_ref() == last)?;
    Some(line_at(text, name.span().start))
}

/// The 1-based line of byte `offset` in `text`.
pub(super) fn line_at(text: &str, offset: usize) -> usize {
    let newlines = text
        .bytes()
        .take(offset)
        .filter(|byte| *byte == b'\n')
        .count();
    newlines + 1
}

/// Whether `span` contains `offset` (an empty span: starts at it).
fn covers(span: &Range<usize>, offset: usize) -> bool {
    span.contains(&offset) || span.start == offset
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str =
        "editor = \"vim\"\n[popup]\nmin_width = 3\n[commands]\nhide_ids = [\n  \"x\",\n]\n";

    #[test]
    fn finds_top_level_and_nested_keys() {
        assert_eq!(key_at(TEXT, 0).as_deref(), Some("editor"));
        let value = TEXT.find("3\n").unwrap();
        assert_eq!(key_at(TEXT, value).as_deref(), Some("popup.min_width"));
    }

    #[test]
    fn stops_at_arrays() {
        let element = TEXT.find("\"x\"").unwrap();

        assert_eq!(key_at(TEXT, element).as_deref(), Some("commands.hide_ids"));
    }

    #[test]
    fn finds_no_key_outside_keys_or_in_invalid_toml() {
        assert_eq!(key_at("# just a comment\n", 3), None);
        assert_eq!(key_at("[popup\n", 0), None);
    }

    #[test]
    fn finds_the_line_of_a_key() {
        assert_eq!(line_of_key(TEXT, "editor"), Some(1));
        assert_eq!(line_of_key(TEXT, "popup.min_width"), Some(3));
        assert_eq!(line_of_key(TEXT, "commands.hide_ids"), Some(5));
    }

    #[test]
    fn has_no_line_for_missing_keys() {
        assert_eq!(line_of_key(TEXT, "popup.max_width"), None);
        assert_eq!(line_of_key(TEXT, "editor.x"), None);
        assert_eq!(line_of_key("[popup\n", "popup"), None);
    }

    #[test]
    fn counts_lines_from_1() {
        assert_eq!(line_at("a\nb\nc", 0), 1);
        assert_eq!(line_at("a\nb\nc", 4), 3);
        assert_eq!(line_at("a", 99), 1);
    }
}
