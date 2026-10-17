//! What git says is checked out in a directory, for Jump to's footer.
//!
//! The only module that knows git: the commands to run and how to read
//! their output. The adapter runs what [`head`] asks for and hands back
//! the output; the rest of the code sees a [`Head`] and its
//! [`label`](Head::label).

use std::ffi::OsString;
use std::path::Path;

/// The program to run, looked up on `PATH`.
pub const PROGRAM: &str = "git";

/// Environment variables that would point git at another repository than
/// the one around the directory asked about; cleared for every run.
pub const IGNORED_ENV: [&str; 3] = ["GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR"];

/// Prints the branch HEAD points to (git 2.22 or later): also an unborn branch's
/// name and a bare repository's HEAD; nothing when HEAD is detached
/// (`checkout --detach`, a rebase, bisect).
const SHOW_CURRENT: [&str; 2] = ["branch", "--show-current"];

/// Prints HEAD's abbreviated commit hash.
const SHORT_HASH: [&str; 3] = ["rev-parse", "--short", "HEAD"];

/// What is checked out in a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    /// A branch (also an unborn one, or a bare repository's HEAD).
    Branch(String),
    /// A detached HEAD (`checkout --detach`, a rebase, bisect): its short
    /// hash. A merge in progress keeps the branch.
    Detached(String),
}

impl Head {
    /// What the footer shows: the branch name or the short hash.
    ///
    /// # Examples
    ///
    /// ```
    /// use herdr_command_palette::domain::git::Head;
    ///
    /// assert_eq!(Head::Branch("main".into()).label(), "main");
    /// assert_eq!(Head::Detached("1a2b3c4".into()).label(), "1a2b3c4");
    /// ```
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Self::Branch(name) | Self::Detached(name) => name,
        }
    }
}

/// What is checked out in `cwd`, asking `git`: it runs [`PROGRAM`] with
/// the arguments given and returns its stdout when it exits 0, `None`
/// otherwise (missing, failed, too slow).
///
/// First `git -C <cwd> branch --show-current`; when that prints nothing
/// (detached HEAD), `git -C <cwd> rev-parse --short HEAD`. `None` when
/// either fails: not a repository, git refusing it (`safe.directory`),
/// or git older than 2.22, which doesn't know `--show-current` (so no
/// separate version check is needed).
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// use herdr_command_palette::domain::git::{self, Head};
///
/// let head = git::head(Path::new("/src/api"), |args| {
///     let args: Vec<_> = args.iter().map(|arg| arg.to_string_lossy()).collect();
///     Some(if args.contains(&"--show-current".into()) { "\n" } else { "1a2b3c4\n" }.to_owned())
/// });
/// assert_eq!(head, Some(Head::Detached("1a2b3c4".into())));
/// ```
pub fn head(cwd: &Path, mut git: impl FnMut(&[OsString]) -> Option<String>) -> Option<Head> {
    let current = git(&args(cwd, &SHOW_CURRENT))?;
    if let Some(name) = first_line(&current) {
        return Some(Head::Branch(name));
    }
    let hash = git(&args(cwd, &SHORT_HASH))?;
    first_line(&hash).map(Head::Detached)
}

/// `-C <cwd>` followed by `command`.
fn args(cwd: &Path, command: &[&str]) -> Vec<OsString> {
    let mut args = vec![OsString::from("-C"), cwd.as_os_str().to_owned()];
    args.extend(command.iter().map(OsString::from));
    args
}

/// The first line of `output`, trimmed; `None` when that is empty.
fn first_line(output: &str) -> Option<String> {
    output
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A git answering `show_current` and `short_hash` (stdout, or `None`
    /// for a failure); records the commands it ran.
    fn git<'a>(
        show_current: Option<&'a str>,
        short_hash: Option<&'a str>,
        calls: &'a mut Vec<Vec<String>>,
    ) -> impl FnMut(&[OsString]) -> Option<String> + 'a {
        move |args| {
            let args: Vec<String> = args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            let answer = if args.contains(&"--show-current".to_owned()) {
                show_current
            } else {
                short_hash
            };
            calls.push(args);
            answer.map(str::to_owned)
        }
    }

    fn head_of(show_current: Option<&str>, short_hash: Option<&str>) -> Option<Head> {
        head(
            Path::new("/r"),
            git(show_current, short_hash, &mut Vec::new()),
        )
    }

    #[test]
    fn reads_the_branch_and_asks_nothing_else() {
        let mut calls = Vec::new();

        let head = head(Path::new("/r"), git(Some("main\n"), None, &mut calls));

        assert_eq!(head, Some(Head::Branch("main".into())));
        assert_eq!(calls, [["-C", "/r", "branch", "--show-current"]]);
    }

    #[test]
    fn falls_back_to_the_short_hash_when_detached() {
        let mut calls = Vec::new();

        let head = head(
            Path::new("/r"),
            git(Some(""), Some("1a2b3c4\n"), &mut calls),
        );

        assert_eq!(head, Some(Head::Detached("1a2b3c4".into())));
        assert_eq!(calls[1], ["-C", "/r", "rev-parse", "--short", "HEAD"]);
    }

    #[test]
    fn is_none_when_git_fails() {
        // Not a repository, `safe.directory`, git < 2.22, missing, timeout.
        assert_eq!(head_of(None, Some("1a2b3c4")), None);
        assert_eq!(head_of(Some("\n"), None), None);
    }

    #[test]
    fn is_none_when_git_prints_nothing_at_all() {
        assert_eq!(head_of(Some(""), Some("  \n")), None);
    }

    #[test]
    fn keeps_only_the_first_line_trimmed() {
        assert_eq!(
            head_of(Some("  feat/x  \nnoise\n"), None),
            Some(Head::Branch("feat/x".into()))
        );
    }
}
