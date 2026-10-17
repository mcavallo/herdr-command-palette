//! [`ConfigSource`] reading Herdr's `config.toml` from disk. A missing file
//! is an empty config, as for Herdr itself.

use std::io::ErrorKind;
use std::path::PathBuf;

use crate::domain::config::HerdrConfig;
use crate::ports::{ConfigError, ConfigSource};

/// Herdr's `config.toml` at a known path (usually
/// [`PluginEnv::config_path`](crate::domain::env::PluginEnv::config_path)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TomlConfigFile {
    /// Where the file is (or would be).
    path: PathBuf,
}

impl TomlConfigFile {
    /// The config file at `path`.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl ConfigSource for TomlConfigFile {
    fn load(&self) -> Result<HerdrConfig, ConfigError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(HerdrConfig::default()),
            Err(error) => {
                return Err(ConfigError::Read {
                    path: self.path.clone(),
                    message: error.to_string(),
                });
            }
        };
        HerdrConfig::from_toml(&text).map_err(|error| ConfigError::Parse {
            path: self.path.clone(),
            message: error.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_parses_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[keys]\nzoom = \"prefix+f\"\n").unwrap();

        let config = TomlConfigFile::new(&path).load().unwrap();

        assert_eq!(config.keys.bindings["zoom"].display(), "prefix+f");
    }

    #[test]
    fn treats_a_missing_file_as_an_empty_config() {
        let dir = tempfile::tempdir().unwrap();

        let config = TomlConfigFile::new(dir.path().join("config.toml")).load();

        assert_eq!(config, Ok(HerdrConfig::default()));
    }

    #[test]
    fn reports_a_file_it_cannot_read() {
        let dir = tempfile::tempdir().unwrap();

        // A directory exists but can't be read as text.
        let error = TomlConfigFile::new(dir.path()).load().unwrap_err();

        assert!(
            matches!(&error, ConfigError::Read { path, .. } if path == dir.path()),
            "{error}"
        );
    }

    #[test]
    fn reports_invalid_toml_with_its_location() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[keys\n").unwrap();

        let error = TomlConfigFile::new(&path).load().unwrap_err();

        let message = HerdrConfig::from_toml("[keys\n").unwrap_err().to_string();
        assert!(message.contains("line 1"), "{message}");
        assert_eq!(error, ConfigError::Parse { path, message });
    }
}
