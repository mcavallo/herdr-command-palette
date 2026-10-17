//! [`Spawner`]: starts the detached `run` that outlives the popup, custom
//! `shell` commands, and the `exec` pane's foreground `sh -c`. Both shell
//! roles use the same configured shell (`sh` by default).
//!
//! Detaching puts the child in its own process group with stdio away from
//! the popup's terminal, so closing the popup doesn't take it down. std's
//! `process_group` does this without `unsafe`; a new session (`setsid`)
//! would need `pre_exec`.

use std::fs::{File, OpenOptions};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::ports::{Detached, SpawnError, Spawner};

/// Starts real processes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Processes {
    shell: PathBuf,
}

impl Processes {
    /// Runs shell commands with `sh` from `PATH`.
    #[must_use]
    pub fn new() -> Self {
        Self::with_shell("sh")
    }

    /// Runs shell commands with another shell (tests).
    #[must_use]
    pub fn with_shell(shell: impl Into<PathBuf>) -> Self {
        Self {
            shell: shell.into(),
        }
    }
}

impl Default for Processes {
    fn default() -> Self {
        Self::new()
    }
}

impl Spawner for Processes {
    fn spawn_detached(&self, command: &Detached) -> Result<(), SpawnError> {
        let spawn_error = |message: String| SpawnError {
            program: command.program.clone(),
            message,
        };
        let mut child = Command::new(&command.program);
        child.args(&command.args).stdin(Stdio::null());
        if let Some(cwd) = &command.cwd {
            child.current_dir(cwd);
        }
        match &command.log {
            Some(path) => {
                // Opened once per stream: each `Stdio` takes ownership.
                let open = || -> Result<File, SpawnError> {
                    OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                        .map_err(|error| spawn_error(format!("{}: {error}", path.display())))
                };
                child.stdout(open()?).stderr(open()?);
            }
            None => {
                child.stdout(Stdio::null()).stderr(Stdio::null());
            }
        }
        // `process_group(0)`: a new group led by the child. Not waited for;
        // when this process exits, init reaps it.
        child
            .process_group(0)
            .spawn()
            .map(drop)
            .map_err(|error| spawn_error(error.to_string()))
    }

    fn spawn_shell_detached(&self, command: &str, cwd: Option<&Path>) -> Result<(), SpawnError> {
        self.spawn_detached(&Detached {
            program: self.shell.clone(),
            args: vec!["-c".into(), command.into()],
            cwd: cwd.map(Path::to_owned),
            log: None,
        })
    }

    fn run_shell(&self, command: &str) -> Result<i32, SpawnError> {
        let spawn_error = |error: std::io::Error| SpawnError {
            program: self.shell.clone(),
            message: error.to_string(),
        };
        let status = Command::new(&self.shell)
            .arg("-c")
            .arg(command)
            .status()
            .map_err(spawn_error)?;
        // Killed by signal n: report 128 + n, like a shell.
        Ok(status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or_default()))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use crate::adapters::scripts;

    use super::*;

    /// Waits up to 5 s for `path` to contain `expected`; returns its text.
    fn wait_for(path: &Path, expected: &str) -> String {
        let read = || fs::read_to_string(path).unwrap_or_default();
        scripts::wait_until(Duration::from_secs(5), || read().contains(expected));
        read()
    }

    #[test]
    fn detached_processes_log_and_run_in_their_cwd_and_group() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("run.log");

        Processes::new()
            .spawn_detached(&Detached {
                program: PathBuf::from("sh"),
                args: vec![
                    "-c".into(),
                    "pwd; ps -o pgid= -p $$; echo $$; echo oops >&2".into(),
                ],
                cwd: Some(dir.path().to_owned()),
                log: Some(log.clone()),
            })
            .unwrap();

        let text = wait_for(&log, "oops");
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        assert_eq!(
            lines[0],
            dir.path().canonicalize().unwrap().to_str().unwrap()
        );
        // Its own process group: group id == its pid.
        assert_eq!(lines[1], lines[2]);
        assert_eq!(lines[3], "oops");
    }

    #[test]
    fn detached_output_is_discarded_without_a_log() {
        let result = Processes::new().spawn_detached(&Detached {
            program: PathBuf::from("true"),
            args: vec![],
            cwd: None,
            log: None,
        });

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn a_missing_program_is_a_spawn_error_naming_it() {
        let missing = Processes::new()
            .spawn_detached(&Detached {
                program: PathBuf::from("/nonexistent/x"),
                args: vec![],
                cwd: None,
                log: None,
            })
            .unwrap_err();

        assert_eq!(missing.program, Path::new("/nonexistent/x"));
    }

    #[test]
    fn an_unwritable_log_is_a_spawn_error_naming_it() {
        let bad_log = Processes::new()
            .spawn_detached(&Detached {
                program: PathBuf::from("true"),
                args: vec![],
                cwd: None,
                log: Some(PathBuf::from("/nonexistent/dir/run.log")),
            })
            .unwrap_err();

        assert!(bad_log.message.starts_with("/nonexistent/dir/run.log: "));
    }

    #[test]
    fn detached_shell_commands_use_the_configured_shell() {
        let dir = tempfile::tempdir().unwrap();
        let shell = dir.path().join("my-sh");
        std::os::unix::fs::symlink("/bin/sh", &shell).unwrap();
        let marker = dir.path().join("ran");

        Processes::with_shell(&shell)
            .spawn_shell_detached("echo \"$0\" > ran", Some(dir.path()))
            .unwrap();

        assert_eq!(wait_for(&marker, "my-sh").trim(), shell.to_str().unwrap());
    }

    #[test]
    fn shell_commands_report_their_status() {
        assert_eq!(Processes::new().run_shell("exit 3"), Ok(3));
    }

    #[test]
    fn shell_commands_killed_by_a_signal_report_128_plus_it() {
        assert_eq!(Processes::new().run_shell("kill -TERM $$"), Ok(128 + 15));
    }

    #[test]
    fn a_missing_shell_is_a_spawn_error() {
        let error = Processes::with_shell("/nonexistent/sh")
            .run_shell("true")
            .unwrap_err();

        assert_eq!(error.program, Path::new("/nonexistent/sh"));
    }

    #[test]
    fn default_is_new() {
        assert_eq!(Processes::default(), Processes::new());
    }
}
