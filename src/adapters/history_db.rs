//! [`HistoryStore`] in a SQLite file (`history.db` in Herdr's plugin state
//! dir), tuned for many short-lived writer processes.
//!
//! Every connection: WAL journal, `synchronous=NORMAL`, no checkpoint on
//! close (closing processes would otherwise each checkpoint: ~14 ms instead
//! of ~2.5 ms per hook), a 250 ms busy timeout, the WAL trimmed back to
//! ~1 MB after checkpoints. Every write is one `BEGIN IMMEDIATE`
//! transaction, so the busy timeout applies when it starts instead of
//! failing on a lock upgrade halfway.
//!
//! The database is opened on first use, so subcommands that never touch
//! history never open it.

use std::cell::RefCell;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rusqlite::config::DbConfig;
use rusqlite::{
    Connection, ErrorCode, OptionalExtension, Transaction, TransactionBehavior, params,
};

use crate::domain::history::{self, Session};
use crate::domain::row::{Category, Kind, RowId};
use crate::ports::{HistoryError, HistoryStore, Prune};

/// How long a write waits for another process's write lock.
const BUSY_TIMEOUT: Duration = Duration::from_millis(250);

/// How long pruning waits before giving up: it is housekeeping and must
/// never hold up `open`.
const PRUNE_BUSY_TIMEOUT: Duration = Duration::from_millis(20);

/// WAL size kept after a checkpoint, in bytes (~1 MB).
const JOURNAL_SIZE_LIMIT: i64 = 1024 * 1024;

/// Layout version in `PRAGMA user_version`; bump it with a migration step
/// in [`set_up`].
const SCHEMA_VERSION: i64 = 1;

/// Version 1. Aggregated rows (one upsert per write, bounded size), keyed by
/// session. Times are milliseconds since the Unix epoch.
/// `usage.kind` holds the row's category (`commands` or `jump_to`), not
/// its row kind. `meta.value` has no declared type: SQLite stores what it
/// is given. [`set_up`] writes `user_version` from [`SCHEMA_VERSION`].
const SCHEMA_V1: &str = "
CREATE TABLE IF NOT EXISTS focus (
  session  TEXT NOT NULL,
  pane_key TEXT NOT NULL,
  last_at  INTEGER NOT NULL,
  PRIMARY KEY (session, pane_key)
);
CREATE TABLE IF NOT EXISTS usage (
  session  TEXT NOT NULL,
  row_key  TEXT NOT NULL,
  kind     TEXT NOT NULL,
  last_at  INTEGER NOT NULL,
  count    INTEGER NOT NULL DEFAULT 1,
  PRIMARY KEY (session, row_key)
);
CREATE TABLE IF NOT EXISTS meta (
  key   TEXT PRIMARY KEY,
  value NOT NULL
);
";

const UPSERT_FOCUS: &str = "
INSERT INTO focus (session, pane_key, last_at) VALUES (?1, ?2, ?3)
ON CONFLICT (session, pane_key) DO UPDATE SET last_at = excluded.last_at";

const UPSERT_USAGE: &str = "
INSERT INTO usage (session, row_key, kind, last_at) VALUES (?1, ?2, ?3, ?4)
ON CONFLICT (session, row_key) DO UPDATE
SET last_at = excluded.last_at, count = usage.count + 1";

// Renames: copy the old row onto the new key (merging with a row already
// there), then delete the old one.
const RENAME_FOCUS: &str = "
INSERT INTO focus (session, pane_key, last_at)
SELECT session, ?3, last_at FROM focus WHERE session = ?1 AND pane_key = ?2
ON CONFLICT (session, pane_key) DO UPDATE SET last_at = max(focus.last_at, excluded.last_at)";
const DELETE_FOCUS: &str = "DELETE FROM focus WHERE session = ?1 AND pane_key = ?2";
const RENAME_USAGE: &str = "
INSERT INTO usage (session, row_key, kind, last_at, count)
SELECT session, ?3, kind, last_at, count FROM usage WHERE session = ?1 AND row_key = ?2
ON CONFLICT (session, row_key) DO UPDATE
SET last_at = max(usage.last_at, excluded.last_at), count = usage.count + excluded.count";
const DELETE_USAGE: &str = "DELETE FROM usage WHERE session = ?1 AND row_key = ?2";

// A new pane's rows from before `?3` (`history created`).
const FORGET_FOCUS: &str =
    "DELETE FROM focus WHERE session = ?1 AND pane_key = ?2 AND last_at < ?3";
const FORGET_USAGE: &str = "DELETE FROM usage WHERE session = ?1 AND row_key = ?2 AND last_at < ?3";

const FOCUS_ORDER: &str =
    "SELECT pane_key FROM focus WHERE session = ?1 ORDER BY last_at DESC, pane_key";
const USAGE_ORDER: &str =
    "SELECT row_key FROM usage WHERE session = ?1 AND kind = ?2 ORDER BY last_at DESC, row_key";

// Pruning. `?2` is a JSON array of the live pane ids.
const PRUNE_CLOSED_FOCUS: &str = "
DELETE FROM focus WHERE session = ?1 AND pane_key NOT IN (SELECT value FROM json_each(?2))";
const PRUNE_CLOSED_JUMPS: &str = "
DELETE FROM usage WHERE session = ?1 AND kind = ?3
AND row_key NOT IN (SELECT ?4 || value FROM json_each(?2))";
const PRUNE_OLD_FOCUS: &str = "DELETE FROM focus WHERE last_at < ?1";
const PRUNE_OLD_USAGE: &str = "DELETE FROM usage WHERE last_at < ?1";

/// `meta` key prefix of a session's last prune time (`last_pruned_at:<session>`).
const LAST_PRUNED_PREFIX: &str = "last_pruned_at:";
const SET_META: &str = "
INSERT INTO meta (key, value) VALUES (?1, ?2)
ON CONFLICT (key) DO UPDATE SET value = excluded.value";
const GET_META: &str = "SELECT value FROM meta WHERE key = ?1";

/// The history database, opened on first use.
#[derive(Debug)]
pub struct SqliteHistory {
    /// `None` when Herdr gave no plugin state dir.
    path: Option<PathBuf>,
    connection: RefCell<Option<Connection>>,
}

impl SqliteHistory {
    /// A store at `path` (created on first use, parent dirs included).
    /// `None`: every call fails with [`HistoryError::NoStateDir`].
    #[must_use]
    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            connection: RefCell::new(None),
        }
    }

    /// Runs `action` on the connection, opening it first if needed.
    fn with_connection<T>(
        &self,
        action: impl FnOnce(&mut Connection, &Path) -> Result<T, HistoryError>,
    ) -> Result<T, HistoryError> {
        let path = self.path.as_deref().ok_or(HistoryError::NoStateDir)?;
        let mut slot = self.connection.borrow_mut();
        // `take` moves the connection out so `action` can borrow it mutably;
        // it goes back in afterwards.
        let mut connection = match slot.take() {
            Some(connection) => connection,
            None => open(path)?,
        };
        let result = action(&mut connection, path);
        *slot = Some(connection);
        result
    }

    /// Runs `body` in one `BEGIN IMMEDIATE` transaction.
    fn write<T>(
        &self,
        body: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<T>,
    ) -> Result<T, HistoryError> {
        self.with_connection(|connection, path| {
            in_transaction(connection, body).map_err(|error| write_error(path, &error))
        })
    }

    /// Runs `query` against the connection.
    fn read<T>(
        &self,
        query: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> Result<T, HistoryError> {
        self.with_connection(|connection, path| {
            query(connection).map_err(|error| read_error(path, &error))
        })
    }

    /// Runs a prune `body` in one transaction, waiting at most
    /// [`PRUNE_BUSY_TIMEOUT`] for the write lock: [`Prune::Skipped`] when
    /// another process holds it. The usual timeout is restored afterwards.
    fn housekeeping(
        &self,
        body: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<Prune>,
    ) -> Result<Prune, HistoryError> {
        self.with_connection(|connection, path| {
            let result = connection
                .busy_timeout(PRUNE_BUSY_TIMEOUT)
                .and_then(|()| in_transaction(connection, body));
            let restored = connection.busy_timeout(BUSY_TIMEOUT);
            match result.and_then(|outcome| restored.map(|()| outcome)) {
                Err(error) if is_busy(&error) => Ok(Prune::Skipped),
                result => result.map_err(|error| write_error(path, &error)),
            }
        })
    }
}

impl HistoryStore for SqliteHistory {
    fn record_focus(
        &self,
        session: &Session,
        pane: &str,
        at: SystemTime,
    ) -> Result<(), HistoryError> {
        let at = history::to_epoch_millis(at);
        self.write(|tx| {
            tx.execute(UPSERT_FOCUS, params![session.as_str(), pane, at])
                .map(drop)
        })
    }

    fn rename_pane(&self, session: &Session, from: &str, to: &str) -> Result<(), HistoryError> {
        let (from_row, to_row) = (pane_row_key(from), pane_row_key(to));
        self.write(|tx| {
            tx.execute(RENAME_FOCUS, params![session.as_str(), from, to])?;
            tx.execute(DELETE_FOCUS, params![session.as_str(), from])?;
            tx.execute(RENAME_USAGE, params![session.as_str(), from_row, to_row])?;
            tx.execute(DELETE_USAGE, params![session.as_str(), from_row])?;
            Ok(())
        })
    }

    fn forget_pane(
        &self,
        session: &Session,
        pane: &str,
        before: SystemTime,
    ) -> Result<(), HistoryError> {
        let (row, before) = (pane_row_key(pane), history::to_epoch_millis(before));
        self.write(|tx| {
            tx.execute(FORGET_FOCUS, params![session.as_str(), pane, before])?;
            tx.execute(FORGET_USAGE, params![session.as_str(), row, before])?;
            Ok(())
        })
    }

    fn record_usage(
        &self,
        session: &Session,
        row: &RowId,
        at: SystemTime,
    ) -> Result<(), HistoryError> {
        let (key, kind) = (row.to_string(), row.kind.category().as_str());
        let at = history::to_epoch_millis(at);
        self.write(|tx| {
            tx.execute(UPSERT_USAGE, params![session.as_str(), key, kind, at])
                .map(drop)
        })
    }

    fn focus_order(&self, session: &Session) -> Result<Vec<String>, HistoryError> {
        self.read(|connection| {
            let mut statement = connection.prepare(FOCUS_ORDER)?;
            let panes = statement.query_map([session.as_str()], |row| row.get(0))?;
            panes.collect()
        })
    }

    fn usage_order(
        &self,
        session: &Session,
        category: Category,
    ) -> Result<Vec<RowId>, HistoryError> {
        let keys: Vec<String> = self.read(|connection| {
            let mut statement = connection.prepare(USAGE_ORDER)?;
            let keys =
                statement.query_map([session.as_str(), category.as_str()], |row| row.get(0))?;
            keys.collect()
        })?;
        // A key that doesn't parse was written by another version: skip it.
        Ok(keys.iter().filter_map(|key| key.parse().ok()).collect())
    }

    fn prune(
        &self,
        session: &Session,
        live_panes: &[&str],
        max_age: Option<Duration>,
        now: SystemTime,
    ) -> Result<Prune, HistoryError> {
        // No cut-off when ages are kept, or `max_age` reaches before the epoch.
        let cutoff = max_age
            .and_then(|age| now.checked_sub(age))
            .map(history::to_epoch_millis);
        let now = history::to_epoch_millis(now);
        let last_pruned_key = last_pruned_key(session);
        self.housekeeping(|tx| {
            let (mut focus_rows, mut usage_rows) = delete_closed(tx, session, live_panes)?;
            if let Some(cutoff) = cutoff {
                focus_rows += tx.execute(PRUNE_OLD_FOCUS, [cutoff])?;
                usage_rows += tx.execute(PRUNE_OLD_USAGE, [cutoff])?;
            }
            tx.execute(SET_META, params![last_pruned_key, now])?;
            Ok(Prune::Done {
                focus_rows,
                usage_rows,
            })
        })
    }

    fn prune_closed(&self, session: &Session, live_panes: &[&str]) -> Result<Prune, HistoryError> {
        self.housekeeping(|tx| {
            let (focus_rows, usage_rows) = delete_closed(tx, session, live_panes)?;
            Ok(Prune::Done {
                focus_rows,
                usage_rows,
            })
        })
    }

    fn last_pruned_at(&self, session: &Session) -> Result<Option<SystemTime>, HistoryError> {
        let millis: Option<i64> = self.read(|connection| {
            connection
                .query_row(GET_META, [last_pruned_key(session)], |row| row.get(0))
                .optional()
        })?;
        Ok(millis.map(history::from_epoch_millis))
    }
}

/// Opens (creating if needed) and configures the database, then brings its
/// schema up to date.
///
/// Fast path, every hook after the very first: the file is already in WAL
/// mode with the current schema, so opening takes one read. A newer schema
/// fails just as fast, untouched. Otherwise the setup runs under
/// an exclusive file lock: SQLite answers "database is locked" without
/// waiting when several connections switch a new file to WAL at once.
fn open(path: &Path) -> Result<Connection, HistoryError> {
    path.parent()
        .map_or(Ok(()), fs::create_dir_all)
        .map_err(|error| open_error(path, &error))?;
    let mut connection = connect(path).map_err(|error| open_error(path, &error))?;
    if needs_set_up(path, &connection)? {
        let _lock = setup_lock(path).map_err(|error| open_error(path, &error))?;
        set_up(&mut connection).map_err(|error| open_error(path, &error))?;
        // Again: a newer palette may have set the file up meanwhile.
        needs_set_up(path, &connection)?;
    }
    Ok(connection)
}

/// Whether the file needs [`set_up`]: not when its schema is current;
/// [`HistoryError::NewerSchema`] when it is newer. A version that can't be
/// read counts as "needs it" (the setup then reports the problem).
fn needs_set_up(path: &Path, connection: &Connection) -> Result<bool, HistoryError> {
    match schema_version(connection) {
        Ok(SCHEMA_VERSION) => Ok(false),
        Ok(found) if found > SCHEMA_VERSION => Err(HistoryError::NewerSchema {
            path: path.to_owned(),
            found,
            supported: SCHEMA_VERSION,
        }),
        _ => Ok(true),
    }
}

/// A connection with the settings in the module docs that last as long as
/// it does.
fn connect(path: &Path) -> rusqlite::Result<Connection> {
    let connection = Connection::open(path)?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)?;
    connection.execute_batch(&format!(
        "PRAGMA synchronous = NORMAL; PRAGMA journal_size_limit = {JOURNAL_SIZE_LIMIT};"
    ))?;
    Ok(connection)
}

fn schema_version(connection: &Connection) -> rusqlite::Result<i64> {
    connection.query_row("PRAGMA user_version", [], |row| row.get(0))
}

/// Waits for and takes the setup lock (`history.db.lock`); released when
/// the returned file is dropped.
fn setup_lock(path: &Path) -> io::Result<File> {
    let mut lock_path = path.as_os_str().to_owned();
    lock_path.push(".lock");
    let file = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path)?;
    file.lock()?;
    Ok(file)
}

/// Switches the file to WAL (stored in the file) and creates the schema
/// when it is older than [`SCHEMA_VERSION`]. Idempotent; the caller holds
/// the setup lock. A current or newer schema (set up since the caller
/// looked) is left as it is: the process that set it up already switched
/// to WAL, and asking again from a connection opened before the switch
/// fails with "database is locked", without waiting, while other
/// processes write.
fn set_up(connection: &mut Connection) -> rusqlite::Result<()> {
    let version = schema_version(connection)?;
    if version >= SCHEMA_VERSION {
        return Ok(());
    }
    // Returns the new mode; a file system without shared memory would stay
    // in rollback mode, which still works, only slower.
    let _mode: String = connection.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    in_transaction(connection, |tx| {
        tx.execute_batch(SCHEMA_V1)?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)
    })
}

/// Runs `body` in a `BEGIN IMMEDIATE` transaction and commits it; an error
/// rolls it back (when the transaction is dropped).
fn in_transaction<T>(
    connection: &mut Connection,
    body: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<T>,
) -> rusqlite::Result<T> {
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let value = body(&tx)?;
    tx.commit()?;
    Ok(value)
}

/// Deletes `session`'s focus and jump usage rows of panes not in
/// `live_panes`; returns (focus rows, usage rows) deleted.
fn delete_closed(
    tx: &Transaction<'_>,
    session: &Session,
    live_panes: &[&str],
) -> rusqlite::Result<(usize, usize)> {
    let live = serde_json::to_string(live_panes).unwrap_or_default();
    let jumps = Category::JumpTo.as_str();
    let focus_rows = tx.execute(PRUNE_CLOSED_FOCUS, params![session.as_str(), live])?;
    let usage_rows = tx.execute(
        PRUNE_CLOSED_JUMPS,
        params![session.as_str(), live, jumps, pane_row_key("")],
    )?;
    Ok((focus_rows, usage_rows))
}

/// Another connection holds the lock past the busy timeout.
fn is_busy(error: &rusqlite::Error) -> bool {
    matches!(
        error.sqlite_error_code(),
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

fn open_error(path: &Path, error: &impl ToString) -> HistoryError {
    HistoryError::Open {
        path: path.to_owned(),
        message: error.to_string(),
    }
}

fn read_error(path: &Path, error: &rusqlite::Error) -> HistoryError {
    HistoryError::Read {
        path: path.to_owned(),
        message: error.to_string(),
    }
}

fn write_error(path: &Path, error: &rusqlite::Error) -> HistoryError {
    HistoryError::Write {
        path: path.to_owned(),
        message: error.to_string(),
    }
}

/// The `meta` key of `session`'s last prune time.
fn last_pruned_key(session: &Session) -> String {
    format!("{LAST_PRUNED_PREFIX}{}", session.as_str())
}

/// The usage key of a jump to `pane` (its [`RowId`]).
fn pane_row_key(pane: &str) -> String {
    RowId::new(Kind::Pane, pane).to_string()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Instant;

    use tempfile::TempDir;

    use super::*;

    fn session() -> Session {
        Session::from_socket_path(Path::new("/c/herdr.sock"))
    }

    fn other_session() -> Session {
        Session::from_socket_path(Path::new("/c/sessions/work/herdr.sock"))
    }

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn pane(id: &str) -> RowId {
        RowId::new(Kind::Pane, id)
    }

    fn builtin(name: &str) -> RowId {
        RowId::new(Kind::Builtin, name)
    }

    /// A store in a fresh temp dir (kept alive by the returned guard).
    fn store() -> (TempDir, SqliteHistory) {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteHistory::new(Some(db_path(&dir)));
        (dir, store)
    }

    fn db_path(dir: &TempDir) -> PathBuf {
        dir.path().join("state").join("history.db")
    }

    /// A second, plain connection to the same file (another process).
    fn raw(dir: &TempDir) -> Connection {
        Connection::open(db_path(dir)).unwrap()
    }

    fn usage_count(dir: &TempDir, row: &RowId) -> i64 {
        raw(dir)
            .query_row(
                "SELECT count FROM usage WHERE row_key = ?1",
                [row.to_string()],
                |r| r.get(0),
            )
            .unwrap()
    }

    #[test]
    fn creates_the_schema_on_first_use() {
        let (dir, store) = store();

        store.record_focus(&session(), "w1:p1", at(1)).unwrap();

        let raw = raw(&dir);
        let version: i64 = raw
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        let mode: String = raw
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(mode, "wal");
    }

    #[test]
    fn closing_leaves_the_wal_for_the_next_writer() {
        let (dir, store) = store();
        store.record_focus(&session(), "w1:p1", at(1)).unwrap();

        drop(store);

        // Without SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE the last connection would
        // checkpoint and delete it.
        assert!(dir.path().join("state").join("history.db-wal").exists());
    }

    #[test]
    fn reopening_keeps_the_history() {
        let (dir, store) = store();
        store.record_focus(&session(), "w1:p1", at(1)).unwrap();
        drop(store);

        let reopened = SqliteHistory::new(Some(db_path(&dir)));

        assert_eq!(reopened.focus_order(&session()).unwrap(), ["w1:p1"]);
    }

    #[test]
    fn focus_order_is_newest_first() {
        let (_dir, store) = store();
        store.record_focus(&session(), "w1:p1", at(1)).unwrap();
        store.record_focus(&session(), "w1:p2", at(2)).unwrap();
        store.record_focus(&session(), "w1:p1", at(3)).unwrap();

        let order = store.focus_order(&session()).unwrap();

        assert_eq!(order, ["w1:p1", "w1:p2"]);
    }

    #[test]
    fn history_is_per_session() {
        let (_dir, store) = store();
        store.record_focus(&session(), "w1:p1", at(1)).unwrap();
        store
            .record_focus(&other_session(), "w1:p2", at(2))
            .unwrap();
        store
            .record_usage(&other_session(), &builtin("zoom"), at(2))
            .unwrap();

        assert_eq!(store.focus_order(&session()).unwrap(), ["w1:p1"]);
        assert_eq!(
            store.usage_order(&session(), Category::Commands).unwrap(),
            []
        );
    }

    #[test]
    fn usage_order_is_newest_first_per_category() {
        let (_dir, store) = store();
        store
            .record_usage(&session(), &builtin("zoom"), at(1))
            .unwrap();
        store
            .record_usage(&session(), &pane("w1:p2"), at(2))
            .unwrap();
        store
            .record_usage(&session(), &RowId::new(Kind::Plugin, "x.y"), at(3))
            .unwrap();

        assert_eq!(
            store.usage_order(&session(), Category::Commands).unwrap(),
            [RowId::new(Kind::Plugin, "x.y"), builtin("zoom")]
        );
        assert_eq!(
            store.usage_order(&session(), Category::JumpTo).unwrap(),
            [pane("w1:p2")]
        );
    }

    #[test]
    fn usage_counts_every_run() {
        let (dir, store) = store();

        store
            .record_usage(&session(), &builtin("zoom"), at(1))
            .unwrap();
        store
            .record_usage(&session(), &builtin("zoom"), at(2))
            .unwrap();

        assert_eq!(usage_count(&dir, &builtin("zoom")), 2);
    }

    #[test]
    fn usage_order_skips_keys_it_cannot_parse() {
        let (dir, store) = store();
        store
            .record_usage(&session(), &builtin("zoom"), at(1))
            .unwrap();
        raw(&dir)
            .execute(
                "INSERT INTO usage VALUES (?1, 'frecency:zoom', 'commands', 9, 1)",
                [session().as_str()],
            )
            .unwrap();

        let order = store.usage_order(&session(), Category::Commands).unwrap();

        assert_eq!(order, [builtin("zoom")]);
    }

    #[test]
    fn rename_moves_focus_and_jump_usage() {
        let (dir, store) = store();
        store.record_focus(&session(), "w1:p2", at(5)).unwrap();
        store
            .record_usage(&session(), &pane("w1:p2"), at(5))
            .unwrap();

        store.rename_pane(&session(), "w1:p2", "w2:p1").unwrap();

        assert_eq!(store.focus_order(&session()).unwrap(), ["w2:p1"]);
        assert_eq!(
            store.usage_order(&session(), Category::JumpTo).unwrap(),
            [pane("w2:p1")]
        );
        assert_eq!(usage_count(&dir, &pane("w2:p1")), 1);
    }

    #[test]
    fn rename_merges_into_rows_of_the_new_id() {
        let (dir, store) = store();
        store.record_focus(&session(), "w1:p2", at(9)).unwrap();
        store.record_focus(&session(), "w2:p1", at(4)).unwrap();
        store
            .record_usage(&session(), &pane("w1:p2"), at(1))
            .unwrap();
        store
            .record_usage(&session(), &pane("w2:p1"), at(2))
            .unwrap();
        store.record_focus(&session(), "w1:p1", at(6)).unwrap();

        store.rename_pane(&session(), "w1:p2", "w2:p1").unwrap();

        assert_eq!(store.focus_order(&session()).unwrap(), ["w2:p1", "w1:p1"]);
        assert_eq!(usage_count(&dir, &pane("w2:p1")), 2);
    }

    #[test]
    fn rename_leaves_other_sessions_alone() {
        let (_dir, store) = store();
        store
            .record_focus(&other_session(), "w1:p2", at(1))
            .unwrap();

        store.rename_pane(&session(), "w1:p2", "w2:p1").unwrap();

        assert_eq!(store.focus_order(&other_session()).unwrap(), ["w1:p2"]);
        assert_eq!(store.focus_order(&session()).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn forget_deletes_the_panes_older_focus_and_jump_rows() {
        let (_dir, store) = store();
        store.record_focus(&session(), "w3:p1", at(5)).unwrap();
        store
            .record_usage(&session(), &pane("w3:p1"), at(5))
            .unwrap();

        store.forget_pane(&session(), "w3:p1", at(10)).unwrap();

        assert_eq!(store.focus_order(&session()).unwrap(), Vec::<String>::new());
        assert_eq!(store.usage_order(&session(), Category::JumpTo).unwrap(), []);
    }

    #[test]
    fn forget_keeps_rows_written_since_the_cutoff() {
        let (_dir, store) = store();
        store.record_focus(&session(), "w3:p1", at(10)).unwrap();
        store
            .record_usage(&session(), &pane("w3:p1"), at(11))
            .unwrap();

        store.forget_pane(&session(), "w3:p1", at(10)).unwrap();

        assert_eq!(store.focus_order(&session()).unwrap(), ["w3:p1"]);
        assert_eq!(
            store.usage_order(&session(), Category::JumpTo).unwrap(),
            [pane("w3:p1")]
        );
    }

    #[test]
    fn forget_leaves_other_panes_sessions_and_commands_alone() {
        let (_dir, store) = store();
        store.record_focus(&session(), "w3:p2", at(1)).unwrap();
        store
            .record_focus(&other_session(), "w3:p1", at(1))
            .unwrap();
        store
            .record_usage(&session(), &builtin("zoom"), at(1))
            .unwrap();

        store.forget_pane(&session(), "w3:p1", at(10)).unwrap();

        assert_eq!(store.focus_order(&session()).unwrap(), ["w3:p2"]);
        assert_eq!(store.focus_order(&other_session()).unwrap(), ["w3:p1"]);
        assert_eq!(
            store.usage_order(&session(), Category::Commands).unwrap(),
            [builtin("zoom")]
        );
    }

    #[test]
    fn prune_deletes_closed_panes_of_the_session() {
        let (_dir, store) = store();
        store.record_focus(&session(), "w1:p1", at(1)).unwrap();
        store.record_focus(&session(), "w1:p2", at(2)).unwrap();
        store
            .record_usage(&session(), &pane("w1:p2"), at(2))
            .unwrap();
        store
            .record_usage(&session(), &builtin("zoom"), at(2))
            .unwrap();
        store
            .record_focus(&other_session(), "w1:p2", at(2))
            .unwrap();

        let outcome = store.prune(&session(), &["w1:p1"], None, at(10));

        assert_eq!(
            outcome,
            Ok(Prune::Done {
                focus_rows: 1,
                usage_rows: 1
            })
        );
        assert_eq!(store.focus_order(&session()).unwrap(), ["w1:p1"]);
        assert_eq!(
            store.usage_order(&session(), Category::Commands).unwrap(),
            [builtin("zoom")]
        );
        assert_eq!(store.focus_order(&other_session()).unwrap(), ["w1:p2"]);
    }

    #[test]
    fn prune_deletes_rows_older_than_max_age_in_every_session() {
        let (_dir, store) = store();
        store.record_focus(&session(), "w1:p1", at(100)).unwrap();
        store
            .record_focus(&other_session(), "w1:p1", at(10))
            .unwrap();
        store
            .record_usage(&session(), &builtin("zoom"), at(10))
            .unwrap();
        store
            .record_usage(&session(), &builtin("detach"), at(100))
            .unwrap();

        let outcome = store.prune(
            &session(),
            &["w1:p1"],
            Some(Duration::from_secs(50)),
            at(120),
        );

        assert_eq!(
            outcome,
            Ok(Prune::Done {
                focus_rows: 1,
                usage_rows: 1
            })
        );
        assert_eq!(
            store.focus_order(&other_session()).unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            store.usage_order(&session(), Category::Commands).unwrap(),
            [builtin("detach")]
        );
    }

    #[test]
    fn prune_with_a_max_age_before_the_epoch_keeps_old_rows() {
        let (_dir, store) = store();
        store.record_focus(&session(), "w1:p1", at(1)).unwrap();

        store
            .prune(
                &session(),
                &["w1:p1"],
                Some(Duration::from_secs(50)),
                at(20),
            )
            .unwrap();

        assert_eq!(store.focus_order(&session()).unwrap(), ["w1:p1"]);
    }

    #[test]
    fn prune_closed_deletes_closed_panes_but_keeps_old_rows() {
        let (_dir, store) = store();
        store.record_focus(&session(), "w1:p1", at(1)).unwrap();
        store.record_focus(&session(), "w6:p1", at(2)).unwrap();
        store
            .record_usage(&session(), &pane("w6:p1"), at(2))
            .unwrap();
        store
            .record_usage(&session(), &builtin("zoom"), at(1))
            .unwrap();
        store
            .record_focus(&other_session(), "w6:p1", at(2))
            .unwrap();

        let outcome = store.prune_closed(&session(), &["w1:p1"]);

        assert_eq!(
            outcome,
            Ok(Prune::Done {
                focus_rows: 1,
                usage_rows: 1
            })
        );
        assert_eq!(store.focus_order(&session()).unwrap(), ["w1:p1"]);
        assert_eq!(
            store.usage_order(&session(), Category::Commands).unwrap(),
            [builtin("zoom")]
        );
        assert_eq!(store.focus_order(&other_session()).unwrap(), ["w6:p1"]);
    }

    #[test]
    fn prune_closed_leaves_the_last_prune_time_alone() {
        let (_dir, store) = store();

        store.prune_closed(&session(), &[]).unwrap();

        assert_eq!(store.last_pruned_at(&session()), Ok(None));
    }

    #[test]
    fn prune_closed_skips_at_once_when_another_process_writes() {
        let (dir, store) = store();
        store.record_focus(&session(), "w1:p1", at(1)).unwrap();
        let mut holder = raw(&dir);
        let lock = holder
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();

        let start = Instant::now();
        let outcome = store.prune_closed(&session(), &[]);

        assert_eq!(outcome, Ok(Prune::Skipped));
        assert!(start.elapsed() < BUSY_TIMEOUT, "took {:?}", start.elapsed());
        drop(lock);
        assert_eq!(store.focus_order(&session()).unwrap(), ["w1:p1"]);
    }

    #[test]
    fn prune_remembers_when_it_ran() {
        let (_dir, store) = store();
        assert_eq!(store.last_pruned_at(&session()), Ok(None));

        store.prune(&session(), &[], None, at(42)).unwrap();

        assert_eq!(store.last_pruned_at(&session()), Ok(Some(at(42))));
    }

    #[test]
    fn a_prune_in_one_session_leaves_another_due() {
        let (_dir, store) = store();

        store.prune(&session(), &[], None, at(42)).unwrap();

        let other = store.last_pruned_at(&other_session()).unwrap();
        assert!(history::is_prune_due(other, at(43)), "{other:?}");
    }

    #[test]
    fn prune_skips_at_once_when_another_process_writes() {
        let (dir, store) = store();
        store.record_focus(&session(), "w1:p1", at(1)).unwrap();
        let mut holder = raw(&dir);
        let lock = holder
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();

        let start = Instant::now();
        let outcome = store.prune(&session(), &[], None, at(10));

        assert_eq!(outcome, Ok(Prune::Skipped));
        assert!(start.elapsed() < BUSY_TIMEOUT, "took {:?}", start.elapsed());
        drop(lock);
        assert_eq!(store.focus_order(&session()).unwrap(), ["w1:p1"]);
    }

    #[test]
    fn prune_restores_the_busy_timeout_for_later_writes() {
        let (dir, store) = store();
        store.record_focus(&session(), "w1:p1", at(1)).unwrap();
        let mut holder = raw(&dir);
        let _lock = holder
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        assert_eq!(
            store.prune(&session(), &[], None, at(10)),
            Ok(Prune::Skipped)
        );

        let start = Instant::now();
        let error = store.record_focus(&session(), "w1:p2", at(2));
        let waited = start.elapsed();

        assert!(error.is_err());
        assert!(waited >= BUSY_TIMEOUT, "{waited:?}");
    }

    #[test]
    fn writes_wait_for_the_busy_timeout_then_fail() {
        let (dir, store) = store();
        store.record_focus(&session(), "w1:p1", at(1)).unwrap();
        let mut holder = raw(&dir);
        let _lock = holder
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();

        let start = Instant::now();
        let error = store.record_focus(&session(), "w1:p2", at(2)).unwrap_err();

        assert!(start.elapsed() >= BUSY_TIMEOUT);
        assert_eq!(
            error,
            HistoryError::Write {
                path: db_path(&dir),
                message: "database is locked".into()
            }
        );
    }

    #[test]
    fn prune_reports_other_failures() {
        let (dir, store) = store();
        store.record_focus(&session(), "w1:p1", at(1)).unwrap();
        raw(&dir).execute_batch("DROP TABLE usage").unwrap();

        let error = store.prune(&session(), &[], None, at(10)).unwrap_err();

        assert_eq!(
            error,
            HistoryError::Write {
                path: db_path(&dir),
                message: "no such table: usage".into()
            }
        );
    }

    #[test]
    fn reads_report_query_failures() {
        let (dir, store) = store();
        store.record_focus(&session(), "w1:p1", at(1)).unwrap();
        raw(&dir).execute_batch("DROP TABLE usage").unwrap();

        let error = store
            .usage_order(&session(), Category::Commands)
            .unwrap_err();

        assert_eq!(
            error,
            HistoryError::Read {
                path: db_path(&dir),
                message: "no such table: usage".into()
            }
        );
    }

    #[test]
    fn every_call_fails_without_a_state_dir() {
        let store = SqliteHistory::new(None);

        assert_eq!(
            store.last_pruned_at(&session()),
            Err(HistoryError::NoStateDir)
        );
        assert_eq!(
            store.record_focus(&session(), "w1:p1", at(1)),
            Err(HistoryError::NoStateDir)
        );
    }

    #[test]
    fn a_state_dir_that_cannot_be_created_fails_to_open() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("state"), "a file").unwrap();
        let store = SqliteHistory::new(Some(db_path(&dir)));

        let error = store.focus_order(&session()).unwrap_err();

        assert!(matches!(error, HistoryError::Open { .. }), "{error:?}");
    }

    #[test]
    fn a_file_that_is_not_a_database_fails_to_open() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("state")).unwrap();
        fs::write(db_path(&dir), "x".repeat(4096)).unwrap();
        let store = SqliteHistory::new(Some(db_path(&dir)));

        let error = store.focus_order(&session()).unwrap_err();

        assert_eq!(
            error,
            HistoryError::Open {
                path: db_path(&dir),
                message: "file is not a database".into()
            }
        );
    }

    #[test]
    fn fails_to_open_when_the_setup_lock_cannot_be_created() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("state").join("history.db.lock")).unwrap();
        let store = SqliteHistory::new(Some(db_path(&dir)));

        let error = store.focus_order(&session()).unwrap_err();

        assert!(matches!(error, HistoryError::Open { .. }), "{error:?}");
    }

    #[test]
    fn a_directory_in_place_of_the_database_fails_to_open() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(db_path(&dir)).unwrap();
        let store = SqliteHistory::new(Some(db_path(&dir)));

        let error = store.focus_order(&session()).unwrap_err();

        assert!(matches!(error, HistoryError::Open { .. }), "{error:?}");
    }

    #[test]
    fn a_newer_schema_is_left_alone() {
        let (dir, store) = store();
        store.record_focus(&session(), "w1:p1", at(1)).unwrap();
        raw(&dir).execute_batch("PRAGMA user_version = 7").unwrap();
        let newer = SqliteHistory::new(Some(db_path(&dir)));

        let error = newer.focus_order(&session()).unwrap_err();

        assert_eq!(
            error,
            HistoryError::NewerSchema {
                path: db_path(&dir),
                found: 7,
                supported: SCHEMA_VERSION
            }
        );
    }

    /// A database in rollback (`delete`) journal mode with `user_version`
    /// 7, as a newer palette could leave it.
    fn newer_rollback_database(dir: &TempDir) {
        fs::create_dir_all(dir.path().join("state")).unwrap();
        raw(dir)
            .execute_batch("CREATE TABLE future (x); PRAGMA user_version = 7;")
            .unwrap();
    }

    fn journal_mode(dir: &TempDir) -> String {
        raw(dir)
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn a_newer_schema_keeps_its_journal_mode() {
        let dir = tempfile::tempdir().unwrap();
        newer_rollback_database(&dir);
        let store = SqliteHistory::new(Some(db_path(&dir)));

        let error = store.focus_order(&session()).unwrap_err();

        assert!(
            matches!(error, HistoryError::NewerSchema { .. }),
            "{error:?}"
        );
        assert_eq!(journal_mode(&dir), "delete");
    }

    #[test]
    fn a_newer_schema_fails_without_taking_the_setup_lock() {
        let dir = tempfile::tempdir().unwrap();
        newer_rollback_database(&dir);
        // The lock can't be taken: a directory where its file goes.
        fs::create_dir_all(dir.path().join("state").join("history.db.lock")).unwrap();
        let store = SqliteHistory::new(Some(db_path(&dir)));

        let error = store.focus_order(&session()).unwrap_err();

        assert!(
            matches!(error, HistoryError::NewerSchema { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn set_up_leaves_a_schema_written_since_alone() {
        let dir = tempfile::tempdir().unwrap();
        newer_rollback_database(&dir);
        let mut connection = raw(&dir);

        set_up(&mut connection).unwrap();

        assert_eq!(journal_mode(&dir), "delete");
    }

    #[test]
    fn set_up_again_on_a_current_schema_changes_nothing() {
        // What a process finds when another one set the file up while it
        // waited for the setup lock (otherwise only reached by chance in
        // `concurrent_first_use_opens_everywhere_and_loses_no_write`).
        let (dir, store) = store();
        store.record_focus(&session(), "w1:p1", at(1)).unwrap();
        let mut connection = raw(&dir);

        set_up(&mut connection).unwrap();

        assert_eq!(journal_mode(&dir), "wal");
        assert_eq!(schema_version(&connection), Ok(SCHEMA_VERSION));
        assert_eq!(store.focus_order(&session()).unwrap(), ["w1:p1"]);
    }

    #[test]
    fn creating_the_schema_fails_while_another_process_holds_the_lock() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("state")).unwrap();
        let mut holder = raw(&dir);
        holder.execute_batch("PRAGMA journal_mode = WAL").unwrap();
        let _lock = holder
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let store = SqliteHistory::new(Some(db_path(&dir)));

        let error = store.focus_order(&session()).unwrap_err();

        assert_eq!(
            error,
            HistoryError::Open {
                path: db_path(&dir),
                message: "database is locked".into()
            }
        );
    }

    #[test]
    fn concurrent_first_use_opens_everywhere_and_loses_no_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = db_path(&dir);
        let writers = 16;
        let barrier = Arc::new(Barrier::new(writers));

        let handles: Vec<_> = (0..writers)
            .map(|n| {
                let (path, barrier) = (path.clone(), Arc::clone(&barrier));
                thread::spawn(move || {
                    // Own connection per thread, like one hook process each.
                    let store = SqliteHistory::new(Some(path));
                    barrier.wait();
                    let focus = store.record_focus(&session(), &format!("w1:p{n}"), at(1));
                    let usage = store.record_usage(&session(), &builtin("zoom"), at(1));
                    (focus, usage)
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

        // Every process opens the new file. A write may still give up after
        // the busy timeout (16 writers on a loaded machine; a hook drops
        // such a write by design), but none that succeeded may be missing.
        for result in results.iter().flat_map(|(focus, usage)| [focus, usage]) {
            let opened = !matches!(result, Err(HistoryError::Open { .. }));
            assert!(opened, "{results:?}");
        }
        let focus_ok = results.iter().filter(|(focus, _)| focus.is_ok()).count();
        let usage_ok = results.iter().filter(|(_, usage)| usage.is_ok()).count();
        let store = SqliteHistory::new(Some(path));
        assert_eq!(store.focus_order(&session()).unwrap().len(), focus_ok);
        assert_eq!(
            usage_count(&dir, &builtin("zoom")),
            i64::try_from(usage_ok).unwrap()
        );
    }
}
