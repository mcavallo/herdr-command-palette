//! `settings check`: validate the plugin's `settings.toml`. The "Edit
//! Command Palette settings" row runs it after the editor closes, so the
//! `exec` popup stays open with the problem (like Edit Herdr config). It
//! passes the edited file as `--file`; `main.rs` reads
//! that file instead of the environment's ([`settings_path`]).
//!
//! [`settings_path`]: crate::app::settings_path

use super::{Context, Error};

/// Loads the settings; 0 when they are valid (a missing file is).
///
/// # Errors
///
/// [`Error::Settings`] when the file can't be read or isn't valid: the
/// binary prints it (file, key, line, problem) and exits 1.
pub fn check(context: &Context<'_>) -> Result<u8, Error> {
    context.settings.load()?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use crate::app::Error;
    use crate::app::fakes::{World, invalid_settings};
    use crate::cli::{Command, SettingsCommand};

    const CHECK: Command = Command::Settings(SettingsCommand::Check { file: None });

    #[test]
    fn valid_settings_pass_silently() {
        let world = World::new();

        assert_eq!(world.run(&CHECK), Ok(0));
        assert_eq!(world.out(), "");
    }

    #[test]
    fn invalid_settings_are_an_error() {
        let world = World::new();
        world.settings.set(Err(invalid_settings()));

        assert_eq!(world.run(&CHECK), Err(Error::Settings(invalid_settings())));
    }
}
