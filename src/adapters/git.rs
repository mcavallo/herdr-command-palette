//! [`BranchLookup`] running the `git` program with a time limit.
//!
//! What to run and how to read it is [`domain::git`](crate::domain::git)'s;
//! this module only runs it: no stdin, stderr discarded, the variables
//! that would redirect git cleared, and killed when the lookup's time is
//! up.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::domain::git::{self, Head};
use crate::ports::BranchLookup;

/// Time for a whole lookup (one or two git runs): git answers in a few
/// milliseconds; the limit only bites on a slow disk or network mount,
/// where a branch that shows a little late beats none.
const TIMEOUT: Duration = Duration::from_millis(200);

/// How often to check whether git has exited, once its output is read.
const POLL: Duration = Duration::from_millis(1);

/// Runs `git` from `PATH`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Git {
    program: PathBuf,
    timeout: Duration,
}

impl Git {
    /// `git` from `PATH`, 200 ms per lookup.
    #[must_use]
    pub fn new() -> Self {
        Self::with_program(git::PROGRAM)
    }

    /// Another program in place of `git` (tests).
    #[must_use]
    pub fn with_program(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            timeout: TIMEOUT,
        }
    }

    /// `args`' stdout when the program exits 0 before `deadline`; `None`
    /// when it can't start, fails, prints something that isn't UTF-8, or
    /// is still running at `deadline` (then it is killed).
    fn output(&self, args: &[OsString], deadline: Instant) -> Option<String> {
        let mut command = Command::new(&self.program);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for name in git::IGNORED_ENV {
            command.env_remove(name);
        }
        let mut child = command.spawn().ok()?;
        let stdout = child.stdout.take()?;
        // Read on a thread so the wait can time out.
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut stdout = stdout;
            let mut text = String::new();
            let read = stdout.read_to_string(&mut text).map(|_| text);
            // The receiver is gone after a timeout: nothing to tell.
            let _ = sender.send(read.ok());
        });
        let remaining = deadline.saturating_duration_since(Instant::now());
        let text = receiver.recv_timeout(remaining).ok().flatten();
        let has_succeeded = text.is_some() && exits_ok(&mut child, deadline);
        if !has_succeeded {
            // Already exited is fine: kill fails. No blocking `wait`: a git
            // stuck in uninterruptible I/O (a hung disk) only dies once that
            // I/O returns, and waiting for it would stall the footer past the
            // limit. One `try_wait` reaps a git that is already gone; one
            // that isn't stays unreaped until this footer process exits,
            // milliseconds later, and init reaps it.
            let _ = child.kill();
            let _ = child.try_wait();
            return None;
        }
        text
    }
}

impl Default for Git {
    fn default() -> Self {
        Self::new()
    }
}

impl BranchLookup for Git {
    fn head(&self, cwd: &Path) -> Option<Head> {
        let deadline = Instant::now() + self.timeout;
        git::head(cwd, |args| self.output(args, deadline))
    }
}

/// Whether `child` exits with status 0 before `deadline`.
fn exits_ok(child: &mut Child, deadline: Instant) -> bool {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => thread::sleep(POLL),
            Ok(None) | Err(_) => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use crate::adapters::scripts;

    use super::*;

    /// Shell assignments that hide the developer's global and system git
    /// config (`core.abbrev`, `init.defaultBranch`, ...) from the tests.
    const NO_CONFIG: &str = "GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1";

    /// Runs real git in `dir` without the developer's config, failing the
    /// test on error.
    fn run_git(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("git ≥ 2.28 on PATH (tests create temp repositories)");
        assert!(output.status.success(), "git {args:?}: {output:?}");
        String::from_utf8(output.stdout).unwrap()
    }

    /// A repository in `dir/name` on branch `main` with one commit.
    fn repo(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        fs::create_dir(&path).unwrap();
        run_git(&path, &["init", "-q", "-b", "main"]);
        run_git(
            &path,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "one",
            ],
        );
        path
    }

    /// The adapter running real git from `PATH` without the developer's
    /// config: a wrapper script in `dir` that hides it and `exec`s git.
    fn git_without_config(dir: &Path) -> Git {
        let body = format!("{NO_CONFIG} exec git \"$@\"");
        Git::with_program(scripts::script(dir, "git-no-config", &body))
    }

    #[test]
    fn runs_git_from_path_by_default() {
        assert_eq!(Git::new(), Git::with_program("git"));
        assert_eq!(Git::default(), Git::new());
    }

    #[test]
    fn reads_the_branch_of_a_repository() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo(dir.path(), "r");
        fs::create_dir(repo.join("src")).unwrap();

        let head = git_without_config(dir.path()).head(&repo.join("src"));

        assert_eq!(head, Some(Head::Branch("main".into())));
    }

    #[test]
    fn reads_the_branch_of_a_linked_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo(dir.path(), "r");
        let worktree = dir.path().join("wt");
        let path = worktree.to_str().unwrap();
        run_git(&repo, &["worktree", "add", "-q", "-b", "feat/x", path]);

        let head = git_without_config(dir.path()).head(&worktree);

        assert_eq!(head, Some(Head::Branch("feat/x".into())));
    }

    #[test]
    fn reads_the_short_hash_of_a_detached_head() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo(dir.path(), "r");
        run_git(&repo, &["checkout", "-q", "--detach"]);
        let hash = run_git(&repo, &["rev-parse", "--short", "HEAD"]);

        let head = git_without_config(dir.path()).head(&repo);

        assert_eq!(head, Some(Head::Detached(hash.trim().to_owned())));
    }

    #[test]
    fn reads_the_head_of_a_bare_repository() {
        let dir = tempfile::tempdir().unwrap();
        let bare = dir.path().join("b.git");
        fs::create_dir(&bare).unwrap();
        run_git(&bare, &["init", "-q", "--bare", "-b", "trunk"]);

        let head = git_without_config(dir.path()).head(&bare);

        assert_eq!(head, Some(Head::Branch("trunk".into())));
    }

    #[test]
    fn reads_an_unborn_branch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new");
        fs::create_dir(&path).unwrap();
        run_git(&path, &["init", "-q", "-b", "start"]);

        let head = git_without_config(dir.path()).head(&path);

        assert_eq!(head, Some(Head::Branch("start".into())));
    }

    #[test]
    fn is_none_outside_a_repository() {
        let dir = tempfile::tempdir().unwrap();

        let head = git_without_config(dir.path()).head(dir.path());

        assert_eq!(head, None);
    }

    #[test]
    fn is_none_for_a_missing_directory() {
        let dir = tempfile::tempdir().unwrap();

        let head = git_without_config(dir.path()).head(&dir.path().join("gone"));

        assert_eq!(head, None);
    }

    #[test]
    fn is_none_without_git() {
        let dir = tempfile::tempdir().unwrap();

        let head = Git::with_program(dir.path().join("git")).head(dir.path());

        assert_eq!(head, None);
    }

    #[test]
    fn is_none_when_git_does_not_know_show_current() {
        // git 2.21: `error: unknown option `show-current'`, status 129.
        let dir = tempfile::tempdir().unwrap();
        let old = scripts::script(dir.path(), "git", "echo usage >&2; exit 129");

        let head = Git::with_program(old).head(dir.path());

        assert_eq!(head, None);
    }

    #[test]
    fn is_none_for_output_that_is_not_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let odd = scripts::script(dir.path(), "git", r"printf '\377\n'");

        let head = Git::with_program(odd).head(dir.path());

        assert_eq!(head, None);
    }

    #[test]
    fn gives_up_on_a_slow_git() {
        let dir = tempfile::tempdir().unwrap();
        let slow = scripts::script(dir.path(), "git", "exec sleep 5");
        let started = Instant::now();

        let head = Git::with_program(slow).head(dir.path());

        assert_eq!(head, None);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn gives_up_on_a_git_that_keeps_running_after_its_output() {
        let dir = tempfile::tempdir().unwrap();
        let lingering = scripts::script(dir.path(), "git", "echo main; exec 1>&-; sleep 5");
        let started = Instant::now();

        let head = Git::with_program(lingering).head(dir.path());

        assert_eq!(head, None);
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
