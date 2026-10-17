//! History keys and time rules: which session a history row belongs to,
//! the key a row's use is stored under, how timestamps are stored, and
//! when pruning is due.
//!
//! The store itself is a port ([`HistoryStore`](crate::ports::HistoryStore));
//! ordering rows by history is [`recent`](crate::domain::recent).

use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::domain::config::CustomCommand;
use crate::domain::row::{Kind, RowId};

/// Pruning runs at most this often.
pub const PRUNE_INTERVAL: Duration = Duration::from_hours(1);

/// How recent a new pane's history may be and still be its own:
/// Herdr starts the `pane.created` and `pane.focused` hooks together, in
/// either order, so a focus row written just before `history created`
/// must survive it. Older rows are a previous pane's with the same id
/// (ids are reused only after a server restart).
pub const CREATED_GRACE: Duration = Duration::from_secs(2);

/// The Herdr session history rows belong to: its socket path
/// (`HERDR_SOCKET_PATH`), fixed per session name and stable across server
/// restarts, unlike the server's pid.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Session(String);

impl Session {
    /// The session whose server listens on `socket`. A non-UTF-8 path is
    /// stored lossily; it still names one session consistently.
    #[must_use]
    pub fn from_socket_path(socket: &Path) -> Self {
        Self(socket.to_string_lossy().into_owned())
    }

    /// The key as stored.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The key a row's use is recorded under: its kind and payload, except
/// for custom commands. Their payload is an index into `[[keys.command]]`,
/// which shifts when the config changes, so they are keyed by a hash of
/// their `type` and `command` instead: editing
/// the config can't retarget history, and changing the command starts
/// afresh. `None` for a custom index that isn't in `commands`.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::history::usage_key;
/// use herdr_command_palette::domain::row::{Kind, RowId};
///
/// let key = usage_key(Kind::Builtin, "zoom", &[]);
/// assert_eq!(key, Some(RowId::new(Kind::Builtin, "zoom")));
/// assert_eq!(usage_key(Kind::Custom, "0", &[]), None);
/// ```
#[must_use]
pub fn usage_key(kind: Kind, payload: &str, commands: &[CustomCommand]) -> Option<RowId> {
    if kind != Kind::Custom {
        return Some(RowId::new(kind, payload));
    }
    let index: usize = payload.parse().ok()?;
    let command = commands.get(index)?;
    let text = format!(
        "{}\n{}",
        command.kind.as_str(),
        command.command.as_deref().unwrap_or_default()
    );
    Some(RowId::new(kind, format!("{:016x}", fnv1a(text.as_bytes()))))
}

/// 64-bit FNV-1a: tiny, and stable across Rust releases and platforms,
/// which std's `DefaultHasher` doesn't promise (stored keys must not
/// change). Not for security: only names rows.
fn fnv1a(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// `time` as milliseconds since the Unix epoch, the store's timestamp
/// format. Times before the epoch are 0; times past `i64::MAX` ms (year
/// 292 million) saturate.
///
/// # Examples
///
/// ```
/// use std::time::{Duration, SystemTime};
/// use herdr_command_palette::domain::history::to_epoch_millis;
///
/// let time = SystemTime::UNIX_EPOCH + Duration::from_millis(1_500);
/// assert_eq!(to_epoch_millis(time), 1_500);
/// ```
#[must_use]
pub fn to_epoch_millis(time: SystemTime) -> i64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
        })
}

/// The inverse of [`to_epoch_millis`]; negative values are the epoch.
#[must_use]
pub fn from_epoch_millis(millis: i64) -> SystemTime {
    let millis = u64::try_from(millis).unwrap_or(0);
    SystemTime::UNIX_EPOCH + Duration::from_millis(millis)
}

/// The time before which a pane created at `now` can't have history of
/// its own: `now` minus [`CREATED_GRACE`] (the epoch if that is earlier).
///
/// ```
/// use std::time::{Duration, SystemTime};
/// use herdr_command_palette::domain::history::created_cutoff;
///
/// let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10);
/// assert_eq!(created_cutoff(now), SystemTime::UNIX_EPOCH + Duration::from_secs(8));
/// ```
#[must_use]
pub fn created_cutoff(now: SystemTime) -> SystemTime {
    now.checked_sub(CREATED_GRACE)
        .map_or(SystemTime::UNIX_EPOCH, |cutoff| {
            cutoff.max(SystemTime::UNIX_EPOCH)
        })
}

/// Whether pruning should run now: never ran, ran at least
/// [`PRUNE_INTERVAL`] ago, or ran "in the future" (the clock went back).
#[must_use]
pub fn is_prune_due(last_pruned_at: Option<SystemTime>, now: SystemTime) -> bool {
    match last_pruned_at {
        None => true,
        Some(last) => now
            .duration_since(last)
            .map_or(true, |since| since >= PRUNE_INTERVAL),
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::config::{CommandType, HerdrConfig};

    use super::*;

    fn commands(toml: &str) -> Vec<CustomCommand> {
        HerdrConfig::from_toml(toml).unwrap().keys.command
    }

    #[test]
    fn rows_other_than_custom_are_keyed_by_kind_and_payload() {
        assert_eq!(
            usage_key(Kind::Pane, "w1:p2", &[]),
            Some(RowId::new(Kind::Pane, "w1:p2"))
        );
    }

    #[test]
    fn custom_commands_are_keyed_by_a_stable_hash_of_type_and_command() {
        let commands = commands(
            r#"
            [[keys.command]]
            type = "popup"
            command = "lazygit"
            description = "Popup: lazygit"
            "#,
        );

        let key = usage_key(Kind::Custom, "0", &commands);

        // Pinned: a different hash would orphan every stored custom key.
        assert_eq!(key, Some(RowId::new(Kind::Custom, "9a6d91da43dd4ca5")));
    }

    #[test]
    fn custom_keys_follow_the_command_not_its_index_or_description() {
        let first = commands(
            r#"
            [[keys.command]]
            command = "make"
            description = "Build"
            "#,
        );
        let moved = commands(
            r#"
            [[keys.command]]
            command = "ls"

            [[keys.command]]
            command = "make"
            description = "Build it"
            "#,
        );

        assert_eq!(
            usage_key(Kind::Custom, "0", &first),
            usage_key(Kind::Custom, "1", &moved)
        );
        assert_ne!(
            usage_key(Kind::Custom, "0", &first),
            usage_key(Kind::Custom, "0", &moved)
        );
    }

    #[test]
    fn custom_keys_differ_by_type() {
        let shell = vec![CustomCommand {
            command: Some("htop".into()),
            ..CustomCommand::default()
        }];
        let popup = vec![CustomCommand {
            kind: CommandType::Popup,
            ..shell[0].clone()
        }];

        assert_ne!(
            usage_key(Kind::Custom, "0", &shell),
            usage_key(Kind::Custom, "0", &popup)
        );
    }

    #[test]
    fn unknown_custom_indexes_have_no_key() {
        assert_eq!(usage_key(Kind::Custom, "x", &[]), None);
        assert_eq!(usage_key(Kind::Custom, "3", &[]), None);
    }

    fn at(millis: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_millis(millis)
    }

    #[test]
    fn a_session_is_its_socket_path() {
        let session = Session::from_socket_path(Path::new("/c/herdr/herdr.sock"));

        assert_eq!(session.as_str(), "/c/herdr/herdr.sock");
    }

    #[test]
    fn millis_round_trip() {
        assert_eq!(from_epoch_millis(to_epoch_millis(at(42_123))), at(42_123));
    }

    #[test]
    fn times_before_the_epoch_are_zero() {
        let before = SystemTime::UNIX_EPOCH - Duration::from_secs(1);

        assert_eq!(to_epoch_millis(before), 0);
        assert_eq!(from_epoch_millis(-5), SystemTime::UNIX_EPOCH);
    }

    #[test]
    fn prune_is_due_when_it_never_ran() {
        assert!(is_prune_due(None, at(0)));
    }

    #[test]
    fn prune_is_due_an_hour_after_the_last_one() {
        let last = at(1_000);

        assert!(!is_prune_due(Some(last), last + Duration::from_secs(3_599)));
        assert!(is_prune_due(Some(last), last + PRUNE_INTERVAL));
    }

    #[test]
    fn prune_is_due_when_the_clock_went_back() {
        assert!(is_prune_due(Some(at(5_000)), at(1_000)));
    }

    #[test]
    fn created_cutoff_is_the_grace_before_now() {
        assert_eq!(created_cutoff(at(10_000)), at(8_000));
    }

    #[test]
    fn created_cutoff_stops_at_the_epoch() {
        assert_eq!(created_cutoff(at(1_000)), SystemTime::UNIX_EPOCH);
    }
}
