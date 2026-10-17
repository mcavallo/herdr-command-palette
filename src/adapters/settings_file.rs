//! [`SettingsSource`] reading the plugin's `settings.toml` from disk. A
//! missing file is all defaults; a new one is created only when asked (the "Edit Command Palette settings" row).

use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};

use crate::domain::settings::Settings;
use crate::ports::{SettingsError, SettingsSource};

/// The settings file at a known path (usually
/// [`PluginEnv::settings_path`](crate::domain::env::PluginEnv::settings_path)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TomlSettingsFile {
    /// Where the file is (or would be).
    path: PathBuf,
}

impl TomlSettingsFile {
    /// The settings file at `path`.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl SettingsSource for TomlSettingsFile {
    fn load(&self) -> Result<Settings, SettingsError> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Settings::default()),
            Err(error) => {
                return Err(SettingsError::Read {
                    path: self.path.clone(),
                    message: error.to_string(),
                });
            }
        };
        Settings::from_toml(&text).map_err(|problem| SettingsError::Invalid {
            path: self.path.clone(),
            problem,
        })
    }

    fn create(&self, contents: &str) -> Result<bool, SettingsError> {
        let write_error = |error: io::Error| SettingsError::Write {
            path: self.path.clone(),
            message: error.to_string(),
        };
        // Only `/` and `` have no parent; neither is a settings file.
        let dir = self.path.parent().unwrap_or(Path::new("."));
        fs::create_dir_all(dir).map_err(write_error)?;
        match write_new(&self.path, contents) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(false),
            Err(error) => Err(write_error(error)),
        }
    }
}

/// Writes `contents` to a new file at `path`. `create_new` fails with
/// `AlreadyExists` if the file exists, so a file written by someone else
/// meanwhile is never overwritten.
fn write_new(path: &Path, contents: &str) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(contents.as_bytes())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use crate::domain::settings::SettingsProblem;

    use super::*;

    fn file_in(dir: &Path) -> TomlSettingsFile {
        TomlSettingsFile::new(dir.join("settings.toml"))
    }

    #[test]
    fn reads_and_parses_the_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("settings.toml"), "editor = \"nano\"\n").unwrap();

        let settings = file_in(dir.path()).load().unwrap();

        assert_eq!(settings.editor, "nano");
    }

    #[test]
    fn treats_a_missing_file_as_defaults() {
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(file_in(dir.path()).load(), Ok(Settings::default()));
    }

    #[test]
    fn reports_a_file_it_cannot_read() {
        let dir = tempfile::tempdir().unwrap();

        // A directory exists but can't be read as text.
        let error = TomlSettingsFile::new(dir.path()).load().unwrap_err();

        assert!(
            matches!(&error, SettingsError::Read { path, .. } if path == dir.path()),
            "{error}"
        );
    }

    #[test]
    fn reports_invalid_settings_with_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let settings = file_in(dir.path());
        fs::write(&settings.path, "editor = 3\n").unwrap();

        let error = settings.load().unwrap_err();

        assert_eq!(
            error,
            SettingsError::Invalid {
                path: settings.path.clone(),
                problem: SettingsProblem {
                    key: Some("editor".into()),
                    line: Some(1),
                    message: "invalid type: integer `3`, expected a string".into(),
                },
            }
        );
    }

    #[test]
    fn creates_the_file_and_its_directory() {
        let dir = tempfile::tempdir().unwrap();
        let settings = TomlSettingsFile::new(dir.path().join("new/settings.toml"));

        assert_eq!(settings.create("# hi\n"), Ok(true));

        assert_eq!(fs::read_to_string(&settings.path).unwrap(), "# hi\n");
    }

    #[test]
    fn never_overwrites_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let settings = file_in(dir.path());
        fs::write(&settings.path, "editor = \"nano\"\n").unwrap();

        assert_eq!(settings.create("# hi\n"), Ok(false));

        assert_eq!(
            fs::read_to_string(&settings.path).unwrap(),
            "editor = \"nano\"\n"
        );
    }

    #[test]
    fn reports_a_directory_it_cannot_create() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("file");
        fs::write(&blocker, "").unwrap();
        let settings = TomlSettingsFile::new(blocker.join("settings.toml"));

        let error = settings.create("# hi\n").unwrap_err();

        assert!(matches!(error, SettingsError::Write { .. }), "{error}");
    }

    #[test]
    fn reports_a_file_it_cannot_create() {
        let dir = tempfile::tempdir().unwrap();
        let settings = file_in(dir.path());
        let read_only = fs::Permissions::from_mode(0o500);
        fs::set_permissions(dir.path(), read_only).unwrap();

        let error = settings.create("# hi\n").unwrap_err();

        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        assert!(matches!(error, SettingsError::Write { .. }), "{error}");
    }
}
