//! The history store port: pane focus and palette use, per Herdr session.
//!
//! Writers are the event hooks (`history focus`, `history move`,
//! `history created`) and `run`;
//! readers are the palette's "recent" orders and the repeat actions; `open`
//! prunes. Reads return keys newest first and never check them against the
//! live session: callers intersect them with the snapshot.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use crate::domain::history::Session;
use crate::domain::row::{Category, RowId};

/// Remembers which panes were focused and which rows were run.
///
/// Every write is one short transaction; implementations must cope with
/// many processes writing at once (one hook process per pane focus).
pub trait HistoryStore {
    /// Records that `pane` was focused at `at`.
    ///
    /// # Errors
    ///
    /// [`HistoryError`] when the store can't be opened or written.
    fn record_focus(
        &self,
        session: &Session,
        pane: &str,
        at: SystemTime,
    ) -> Result<(), HistoryError>;

    /// Carries a pane's history over to its new id after a move: its focus
    /// row and its jump usage row (`pane:<id>`), in one transaction. Where
    /// `to` already has rows, the newer time wins and use counts add up.
    /// Nothing recorded for `from` is a no-op.
    ///
    /// # Errors
    ///
    /// [`HistoryError`] when the store can't be opened or written.
    fn rename_pane(&self, session: &Session, from: &str, to: &str) -> Result<(), HistoryError>;

    /// Deletes `pane`'s focus row and jump usage row (`pane:<id>`) in
    /// `session` when last written before `before`, in one transaction:
    /// a new pane starts without the history of an earlier pane with the
    /// same id, but keeps rows its own hooks just wrote, since
    /// Herdr reuses pane ids after a restart.
    /// Nothing recorded is a no-op.
    ///
    /// # Errors
    ///
    /// [`HistoryError`] when the store can't be opened or written.
    fn forget_pane(
        &self,
        session: &Session,
        pane: &str,
        before: SystemTime,
    ) -> Result<(), HistoryError>;

    /// Records that `row` was run at `at` (from the palette or a repeat
    /// action): sets its time, adds one to its count. The usage kind is the
    /// row's [`Category`].
    ///
    /// # Errors
    ///
    /// [`HistoryError`] when the store can't be opened or written.
    fn record_usage(
        &self,
        session: &Session,
        row: &RowId,
        at: SystemTime,
    ) -> Result<(), HistoryError>;

    /// Pane ids of `session`, most recently focused first.
    ///
    /// # Errors
    ///
    /// [`HistoryError`] when the store can't be opened or read.
    fn focus_order(&self, session: &Session) -> Result<Vec<String>, HistoryError>;

    /// Rows of `category` run in `session`, most recently run first. Rows
    /// whose stored key no longer parses are left out.
    ///
    /// # Errors
    ///
    /// [`HistoryError`] when the store can't be opened or read.
    fn usage_order(
        &self,
        session: &Session,
        category: Category,
    ) -> Result<Vec<RowId>, HistoryError>;

    /// Deletes stale history: focus and jump usage rows of `session` whose
    /// pane isn't in `live_panes`, and rows of every session older than
    /// `max_age` (`None` keeps them), then stores `now` as `session`'s last
    /// prune time. Never waits long for other writers: returns
    /// [`Prune::Skipped`] when the store is busy.
    ///
    /// # Errors
    ///
    /// [`HistoryError`] when the store can't be opened or written for
    /// another reason than being busy.
    fn prune(
        &self,
        session: &Session,
        live_panes: &[&str],
        max_age: Option<Duration>,
        now: SystemTime,
    ) -> Result<Prune, HistoryError>;

    /// Deletes focus and jump usage rows of `session` whose pane isn't in
    /// `live_panes`, as [`prune`](HistoryStore::prune) does, without the
    /// age step and without touching the last prune time. Cheap: `open`
    /// runs it every time. Never waits long for other writers:
    /// returns [`Prune::Skipped`] when the store is busy.
    ///
    /// # Errors
    ///
    /// [`HistoryError`] when the store can't be opened or written for
    /// another reason than being busy.
    fn prune_closed(&self, session: &Session, live_panes: &[&str]) -> Result<Prune, HistoryError>;

    /// When [`prune`](HistoryStore::prune) last completed for `session`;
    /// `None` if never. Per session: closed panes are only pruned for the
    /// session that prunes, so a prune elsewhere doesn't make it recent.
    ///
    /// # Errors
    ///
    /// [`HistoryError`] when the store can't be opened or read.
    fn last_pruned_at(&self, session: &Session) -> Result<Option<SystemTime>, HistoryError>;
}

/// What a prune did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prune {
    /// Stale rows were deleted.
    Done {
        /// Focus rows deleted.
        focus_rows: usize,
        /// Usage rows deleted.
        usage_rows: usize,
    },
    /// Another process held the write lock; a later open retries.
    Skipped,
}

/// Why the history store couldn't be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HistoryError {
    /// Herdr didn't say where plugin state goes, so there is no store.
    #[error("no history store: HERDR_PLUGIN_STATE_DIR is not set")]
    NoStateDir,
    /// The database couldn't be opened or set up.
    #[error("cannot open {}: {message}", path.display())]
    Open {
        /// The database file.
        path: PathBuf,
        /// The SQLite error, as text.
        message: String,
    },
    /// The database was written by a newer palette with another layout.
    #[error(
        "cannot use {}: schema version {found} is newer than this palette's ({supported})",
        path.display()
    )]
    NewerSchema {
        /// The database file.
        path: PathBuf,
        /// The version found (`PRAGMA user_version`).
        found: i64,
        /// The newest version this build knows.
        supported: i64,
    },
    /// A query failed.
    #[error("cannot read {}: {message}", path.display())]
    Read {
        /// The database file.
        path: PathBuf,
        /// The SQLite error, as text.
        message: String,
    },
    /// A write failed (busy past the timeout, disk full, ...).
    #[error("cannot write {}: {message}", path.display())]
    Write {
        /// The database file.
        path: PathBuf,
        /// The SQLite error, as text.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_name_the_database() {
        let path = PathBuf::from("/st/history.db");
        let message = "database is locked".to_owned();

        assert_eq!(
            HistoryError::NoStateDir.to_string(),
            "no history store: HERDR_PLUGIN_STATE_DIR is not set"
        );
        assert_eq!(
            HistoryError::Open {
                path: path.clone(),
                message: message.clone()
            }
            .to_string(),
            "cannot open /st/history.db: database is locked"
        );
        assert_eq!(
            HistoryError::Read {
                path: path.clone(),
                message: message.clone()
            }
            .to_string(),
            "cannot read /st/history.db: database is locked"
        );
        assert_eq!(
            HistoryError::Write {
                path: path.clone(),
                message
            }
            .to_string(),
            "cannot write /st/history.db: database is locked"
        );
        assert_eq!(
            HistoryError::NewerSchema {
                path,
                found: 3,
                supported: 1
            }
            .to_string(),
            "cannot use /st/history.db: schema version 3 is newer than this palette's (1)"
        );
    }
}
