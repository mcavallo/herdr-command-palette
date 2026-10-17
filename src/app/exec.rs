//! `exec`: the pane that runs a custom `popup`/`pane` command
//! (`sh -c "$HCP_EXEC"`; Herdr has no API to open an arbitrary popup,
//! so a plugin pane runs it). On failure the pane
//! stays open with the exit status, cwd and `PATH` until a key press, so it
//! doesn't look like it never opened.

use std::io::Write;

use crate::domain::env::HCP_EXEC_ENV;

use super::{CLOSE_HINT, Context, Error, NOTHING_RECEIVED, PROGRAM};

/// Exit status when the shell can't be started: what a shell says for a
/// command it can't run.
const CANNOT_RUN: i32 = 127;

/// Runs the command and returns its exit status.
///
/// # Errors
///
/// [`Error::Output`] when the failure message can't be written.
pub fn exec(context: &Context<'_>, out: &mut dyn Write) -> Result<u8, Error> {
    let command = context.env.exec_command.as_deref();
    let status = match command {
        None => {
            let message = format!("{PROGRAM}: no command received ({HCP_EXEC_ENV} unset)");
            writeln!(out, "{message}")?;
            i32::from(NOTHING_RECEIVED)
        }
        Some(command) => match context.spawner.run_shell(command) {
            Ok(status) => status,
            Err(error) => {
                writeln!(out, "{PROGRAM}: {error}")?;
                CANNOT_RUN
            }
        },
    };
    if status != 0 {
        let cwd = context.cwd.map(|cwd| cwd.display().to_string());
        let message = format!(
            "\n{PROGRAM}: \"{}\" exited with {status} (cwd {})\nPATH={}\n\n{CLOSE_HINT}",
            command.unwrap_or_default(),
            cwd.unwrap_or_default(),
            context.env.path.as_deref().unwrap_or_default(),
        );
        out.write_all(message.as_bytes())?;
        out.flush()?;
        context.terminal.wait_for_key();
    }
    // Statuses outside 0..=255 (none in practice) become a plain failure.
    Ok(u8::try_from(status).unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use crate::app::fakes::{World, spawn_error};
    use crate::cli::Command;

    #[test]
    fn runs_the_command_and_closes_on_success() {
        let mut world = World::new();
        world.env.exec_command = Some("lazygit".into());

        assert_eq!(world.run(&Command::Exec), Ok(0));
        assert_eq!(world.spawner.shell(), ["lazygit"]);
        assert_eq!(world.out(), "");
        assert_eq!(world.terminal.key_waits(), 0);
    }

    #[test]
    fn keeps_failures_on_screen_until_a_key_press() {
        let mut world = World::new();
        world.env.exec_command = Some("false".into());
        world.env.path = Some("/usr/bin".into());
        world.spawner.set_run_shell(Ok(3));

        assert_eq!(world.run(&Command::Exec), Ok(3));
        assert_eq!(
            world.out(),
            "\nherdr-command-palette: \"false\" exited with 3 (cwd /work)\nPATH=/usr/bin\n\n\
             press any key to close"
        );
        assert_eq!(world.terminal.key_waits(), 1);
    }

    #[test]
    fn exits_2_without_a_command() {
        let world = World::new();

        assert_eq!(world.run(&Command::Exec), Ok(2));
        assert!(
            world
                .out()
                .starts_with("herdr-command-palette: no command received (HCP_EXEC unset)\n")
        );
    }

    #[test]
    fn a_shell_that_cannot_start_is_status_127() {
        let mut world = World::new();
        world.env.exec_command = Some("x".into());
        world.spawner.set_run_shell(Err(spawn_error("sh")));

        assert_eq!(world.run(&Command::Exec), Ok(127));
        assert!(
            world
                .out()
                .starts_with("herdr-command-palette: cannot start sh: not found\n")
        );
    }

    #[test]
    fn out_of_range_statuses_are_1() {
        let mut world = World::new();
        world.env.exec_command = Some("x".into());
        world.spawner.set_run_shell(Ok(300));

        assert_eq!(world.run(&Command::Exec), Ok(1));
    }
}
