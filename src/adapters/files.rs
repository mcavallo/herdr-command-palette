//! [`Files`] on the local filesystem: the palette's per-open state
//! directory, private to the user, and the leftovers of
//! killed palettes.

use std::fs::{self, DirBuilder};
use std::io::ErrorKind;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};

use crate::domain::palette_state;
use crate::ports::{Files, FilesError, TempDirEntry};

/// Names tried before giving up on a free temp dir name.
const ATTEMPTS: u32 = 100;

/// Files under a base temp directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TempFiles {
    base: PathBuf,
}

impl TempFiles {
    /// Temp dirs under the system's (`$TMPDIR`, else `/tmp`).
    #[must_use]
    pub fn new() -> Self {
        Self::with_base(std::env::temp_dir())
    }

    /// Temp dirs under `base`.
    #[must_use]
    pub fn with_base(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }
}

impl Default for TempFiles {
    fn default() -> Self {
        Self::new()
    }
}

impl Files for TempFiles {
    fn create_temp_dir(&self) -> Result<PathBuf, FilesError> {
        let pid = std::process::id();
        for attempt in 0..ATTEMPTS {
            let dir = self.base.join(palette_state::name(pid, attempt));
            // 0o700: only the user can read the rows (pane names, cwds).
            match DirBuilder::new().mode(0o700).create(&dir) {
                Ok(()) => return Ok(dir),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(FilesError::Create {
                        path: dir,
                        message: error.to_string(),
                    });
                }
            }
        }
        Err(FilesError::Create {
            path: self.base.clone(),
            message: "no free temporary directory name".into(),
        })
    }

    fn read(&self, path: &Path) -> Result<Option<String>, FilesError> {
        match fs::read_to_string(path) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(FilesError::Read {
                path: path.to_owned(),
                message: error.to_string(),
            }),
        }
    }

    fn write(&self, path: &Path, contents: &str) -> Result<(), FilesError> {
        fs::write(path, contents).map_err(|error| FilesError::Write {
            path: path.to_owned(),
            message: error.to_string(),
        })
    }

    fn remove_dir(&self, path: &Path) {
        // A leftover temp dir is harmless; nothing to report.
        let _ = fs::remove_dir_all(path);
    }

    fn sibling_temp_dirs(&self, dir: &Path) -> Vec<TempDirEntry> {
        let (Some(parent), Ok(own)) = (dir.parent(), fs::symlink_metadata(dir)) else {
            return Vec::new();
        };
        // An unreadable parent or entry is skipped: the sweep is best effort.
        fs::read_dir(parent)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.path() != dir)
            .filter_map(|entry| sibling(&entry, own.uid()))
            .collect()
    }
}

/// `entry` as a palette temp dir owned by `owner`, if it is one.
fn sibling(entry: &fs::DirEntry, owner: u32) -> Option<TempDirEntry> {
    // The name check comes first: the temp dir can hold thousands of
    // entries, and only ours need an lstat.
    let name = entry.file_name();
    if !name.to_str().is_some_and(palette_state::is_name) {
        return None;
    }
    // `DirEntry::metadata` doesn't follow symlinks: a link named like ours
    // is not a directory here, so the sweep never follows one.
    let metadata = entry.metadata().ok()?;
    let is_match = metadata.is_dir() && metadata.uid() == owner;
    let modified = metadata.modified().ok()?;
    is_match.then(|| TempDirEntry {
        path: entry.path(),
        modified,
    })
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn creates_private_dirs_reads_writes_and_removes() {
        let base = tempfile::tempdir().unwrap();
        let files = TempFiles::with_base(base.path());

        let dir = files.create_temp_dir().unwrap();
        let second = files.create_temp_dir().unwrap();
        let file = dir.join("category");
        files.write(&file, "jump_to").unwrap();

        assert_ne!(dir, second);
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(files.read(&file), Ok(Some("jump_to".into())));
        assert_eq!(files.read(&dir.join("missing")), Ok(None));
        files.remove_dir(&dir);
        assert!(!dir.exists());
    }

    #[test]
    fn lists_only_other_palette_dirs_next_to_a_dir() {
        let base = tempfile::tempdir().unwrap();
        let files = TempFiles::with_base(base.path());
        let dir = files.create_temp_dir().unwrap();
        let leftover = base.path().join(palette_state::name(1, 0));
        fs::create_dir(&leftover).unwrap();
        fs::create_dir(base.path().join("herdr-command-palette-history")).unwrap();
        fs::create_dir(base.path().join("other")).unwrap();
        fs::write(base.path().join(palette_state::name(2, 0)), "a file").unwrap();
        std::os::unix::fs::symlink(&leftover, base.path().join(palette_state::name(3, 0))).unwrap();

        let found = files.sibling_temp_dirs(&dir);

        assert_eq!(
            found,
            [TempDirEntry {
                path: leftover.clone(),
                modified: fs::metadata(&leftover).unwrap().modified().unwrap(),
            }]
        );
    }

    #[test]
    fn lists_nothing_next_to_a_missing_dir() {
        let files = TempFiles::new();

        assert_eq!(files.sibling_temp_dirs(Path::new("/nonexistent/x")), []);
    }

    #[test]
    fn lists_nothing_next_to_the_root_which_has_no_parent() {
        let files = TempFiles::new();

        assert_eq!(files.sibling_temp_dirs(Path::new("/")), []);
    }

    #[test]
    fn create_errors_name_the_dir() {
        let files = TempFiles::with_base("/nonexistent/base");

        let created = files.create_temp_dir().unwrap_err();

        let pid = std::process::id();
        let prefix = format!("cannot create /nonexistent/base/herdr-command-palette-{pid}-0: ");
        assert!(created.to_string().starts_with(&prefix), "{created}");
    }

    #[test]
    fn write_errors_name_the_file() {
        let written = TempFiles::new()
            .write(Path::new("/nonexistent/x"), "")
            .unwrap_err();

        assert!(
            matches!(written, FilesError::Write { path, .. } if path == Path::new("/nonexistent/x"))
        );
    }

    #[test]
    fn read_errors_name_the_file() {
        let read = TempFiles::new().read(Path::new("/")).unwrap_err();

        assert!(matches!(read, FilesError::Read { path, .. } if path == Path::new("/")));
    }

    #[test]
    fn default_is_new() {
        assert_eq!(TempFiles::default(), TempFiles::new());
    }

    #[test]
    fn gives_up_when_every_name_is_taken() {
        let base = tempfile::tempdir().unwrap();
        let pid = std::process::id();
        for attempt in 0..ATTEMPTS {
            fs::create_dir(base.path().join(palette_state::name(pid, attempt))).unwrap();
        }

        let error = TempFiles::with_base(base.path())
            .create_temp_dir()
            .unwrap_err();

        assert_eq!(
            error,
            FilesError::Create {
                path: base.path().to_owned(),
                message: "no free temporary directory name".into(),
            }
        );
    }
}
