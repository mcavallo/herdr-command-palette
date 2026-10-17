//! POSIX shell quoting for command lines the palette builds itself: the
//! "Edit Herdr config" command, the fzf bindings and their callbacks.
//!
//! Nonempty words containing only ASCII letters, digits or `_@%+=:,./-`
//! remain unquoted. All other words, including empty strings, are wrapped
//! in single quotes. An embedded single quote is written as `'"'"'`:
//! close the single quotes, insert a double-quoted single quote, then reopen
//! the single quotes. This preserves the input as one literal shell argument.

use std::borrow::Cow;

/// `text` as one shell word: unchanged when it only has characters the
/// shell never interprets, else single-quoted.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::shell::quote;
///
/// assert_eq!(quote("/etc/herdr/config.toml"), "/etc/herdr/config.toml");
/// assert_eq!(quote("my config.toml"), "'my config.toml'");
/// assert_eq!(quote("it's"), r#"'it'"'"'s'"#);
/// assert_eq!(quote(""), "''");
/// ```
#[must_use]
pub fn quote(text: &str) -> Cow<'_, str> {
    if !text.is_empty() && text.chars().all(is_safe) {
        // `Cow::Borrowed` hands back the input without copying it.
        return Cow::Borrowed(text);
    }
    Cow::Owned(format!("'{}'", text.replace('\'', r#"'"'"'"#)))
}

/// ASCII characters that can appear in an unquoted literal shell word.
fn is_safe(c: char) -> bool {
    c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c)
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use proptest::prelude::*;

    use super::*;

    /// What `sh` makes of `word` (printed back by `printf %s`).
    fn sh_echo(word: &str) -> String {
        let output = Command::new("sh")
            .arg("-c")
            .arg(format!("printf %s {word}"))
            .output()
            .unwrap();
        String::from_utf8(output.stdout).unwrap()
    }

    #[test]
    fn leaves_safe_words_alone() {
        assert!(matches!(quote("a-Z_0.9/x@y%z+=:,"), Cow::Borrowed(_)));
    }

    #[test]
    fn quotes_shell_metacharacters() {
        for text in ["$HOME", "a b", "~", "*", "a;b", "`x`", "\"", "\\", "日本"] {
            assert_eq!(quote(text), format!("'{text}'"));
        }
    }

    #[test]
    fn shell_reads_back_a_tricky_path() {
        let path = "/tmp/it's a \"$(rm -rf ~)\" `x` \\ config.toml";

        assert_eq!(sh_echo(&quote(path)), path);
    }

    proptest! {
        // Each case starts a shell, so keep the count modest.
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn shell_reads_back_any_text(text in "[^\u{0}]{0,24}") {
            prop_assert_eq!(sh_echo(&quote(&text)), text);
        }
    }
}
