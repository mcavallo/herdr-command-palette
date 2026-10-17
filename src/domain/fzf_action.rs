//! fzf actions with an argument (`reload(...)`, `change-header(...)`), as
//! the fzf callbacks print them.
//!
//! fzf ends an argument at its closing delimiter followed by `+` (the next
//! action), `,` (the next key, in `--bind`) or the end of the text, before
//! any shell sees it, so quoting can't protect a `)` in it. The argument
//! gets the first delimiter whose closing character it doesn't contain.
//! The callbacks name this binary by `"$HCP_PROGRAM"`, not its path, so
//! their arguments normally take `()`; this is a safety net. fzf 0.66
//! (our minimum) through 0.74 accept the same delimiters (`man fzf`,
//! "ACTION ARGUMENT"). `action:arg` (to the end of the text) takes
//! anything but must come last, so chains use the delimited form.

/// The delimiters fzf accepts around an action's argument, opening and
/// closing, in the order we try them: `()` first, what the callbacks get.
const DELIMITERS: [(char, char); 16] = [
    ('(', ')'),
    ('[', ']'),
    ('{', '}'),
    ('<', '>'),
    ('~', '~'),
    ('!', '!'),
    ('@', '@'),
    ('#', '#'),
    ('$', '$'),
    ('%', '%'),
    ('^', '^'),
    ('&', '&'),
    ('*', '*'),
    (';', ';'),
    ('/', '/'),
    ('|', '|'),
];

/// An argument fzf can't take in any delimited form.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "cannot write fzf action `{name}`: its argument `{arg}` contains every closing delimiter fzf accepts"
)]
pub struct ActionError {
    /// The action, e.g. `reload`.
    pub name: String,
    /// The argument it couldn't take.
    pub arg: String,
}

/// `name(arg)`, or `name[arg]`, ... with the first delimiter whose closing
/// character `arg` doesn't contain, so fzf reads `arg` back whole, also
/// when more actions follow with `+`.
///
/// # Errors
///
/// [`ActionError`] when `arg` contains every closing delimiter.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::fzf_action::action;
///
/// assert_eq!(action("reload", "/bin/hcp fzf rows").unwrap(), "reload(/bin/hcp fzf rows)");
/// assert_eq!(action("reload", "'/a)b/hcp'").unwrap(), "reload['/a)b/hcp']");
/// ```
pub fn action(name: &str, arg: &str) -> Result<String, ActionError> {
    DELIMITERS
        .iter()
        .find(|(_, close)| !arg.contains(*close))
        .map(|(open, close)| format!("{name}{open}{arg}{close}"))
        .ok_or_else(|| ActionError {
            name: name.to_owned(),
            arg: arg.to_owned(),
        })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// The argument fzf reads from `text`, which starts with action `name`
    /// (fzf's `maskActionContents`): from the opening character to the
    /// first closing one followed by `+`, `,` or the end of the text.
    fn fzf_arg<'a>(text: &'a str, name: &str) -> Option<&'a str> {
        let rest = text.strip_prefix(name)?;
        let open = rest.chars().next()?;
        let (_, close) = DELIMITERS.iter().find(|(o, _)| *o == open)?;
        let body = &rest[open.len_utf8()..];
        body.char_indices()
            .filter(|(_, c)| c == close)
            .map(|(i, _)| i)
            .find(|&i| {
                let after = &body[i + close.len_utf8()..];
                after.is_empty() || after.starts_with(['+', ','])
            })
            .map(|i| &body[..i])
    }

    #[test]
    fn uses_parentheses_for_a_normal_path() {
        assert_eq!(
            action("reload-sync", "/bin/hcp fzf rows jump_to"),
            Ok("reload-sync(/bin/hcp fzf rows jump_to)".to_owned())
        );
    }

    #[test]
    fn skips_each_delimiter_the_argument_closes() {
        let arg = "'/a)+b]+c}+d>+e/hcp'";

        assert_eq!(action("reload", arg), Ok(format!("reload~{arg}~")));
    }

    #[test]
    fn fails_when_the_argument_has_every_closing_delimiter() {
        let arg: String = DELIMITERS.iter().map(|(_, close)| *close).collect();

        assert_eq!(
            action("reload", &arg),
            Err(ActionError {
                name: "reload".into(),
                arg,
            })
        );
    }

    #[test]
    fn fzf_reads_each_delimited_argument_back() {
        for (_, close) in &DELIMITERS[..DELIMITERS.len() - 1] {
            let arg = format!("'/x{close}+y{close},z{close}/hcp' fzf rows commands");

            let text = format!("{}+first", action("reload", &arg).unwrap());

            assert_eq!(fzf_arg(&text, "reload"), Some(arg.as_str()));
        }
    }

    #[test]
    fn the_test_parser_cuts_at_a_closing_delimiter_before_a_plus() {
        assert_eq!(fzf_arg("reload('/a)+b')+first", "reload"), Some("'/a"));
    }

    proptest! {
        #[test]
        fn fzf_reads_any_argument_back(arg in "[a-z ')\\]}>~!@#$%^&*;/|+,]{0,40}") {
            if let Ok(text) = action("reload", &arg) {
                let chained = format!("{text}+first");
                prop_assert_eq!(fzf_arg(&chained, "reload"), Some(arg.as_str()));
            }
        }
    }
}
