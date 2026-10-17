//! Palette state dirs, the per-open temp dirs: how they are named and when
//! a leftover one is stale.
//!
//! The palette removes its directory when it exits, but not when Herdr
//! kills it (closing the popup's workspace, `plugin.pane.close`, server
//! stop: SIGHUP). The next palette sweeps such leftovers. This module only
//! decides; listing and removing are the `Files` port's job.

use std::time::{Duration, SystemTime};

/// Start of every palette state dir name: `herdr-command-palette-<pid>-<n>`.
const PREFIX: &str = "herdr-command-palette-";

/// Age after which a leftover palette state dir is removed. No palette stays
/// open this long; a live one younger than this is never touched.
pub(crate) const STALE_AFTER: Duration = Duration::from_hours(24);

/// The name of the `attempt`-th palette state dir of process `pid`.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::palette_state;
///
/// assert_eq!(palette_state::name(42, 0), "herdr-command-palette-42-0");
/// ```
#[must_use]
pub fn name(pid: u32, attempt: u32) -> String {
    format!("{PREFIX}{pid}-{attempt}")
}

/// Whether `name` is exactly a palette state dir name (`<PREFIX><digits>-<digits>`),
/// so the sweep never matches anything else.
///
/// # Examples
///
/// ```
/// use herdr_command_palette::domain::palette_state;
///
/// assert!(palette_state::is_name("herdr-command-palette-42-0"));
/// assert!(!palette_state::is_name("herdr-command-palette-42-0.bak"));
/// ```
#[must_use]
pub fn is_name(name: &str) -> bool {
    let is_number = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    match name
        .strip_prefix(PREFIX)
        .and_then(|rest| rest.split_once('-'))
    {
        Some((pid, attempt)) => is_number(pid) && is_number(attempt),
        None => false,
    }
}

/// Whether a palette state dir last modified at `modified` is stale at
/// `now`. A modification time in the future (clock changes) is not stale.
#[must_use]
pub fn is_stale(modified: SystemTime, now: SystemTime) -> bool {
    now.duration_since(modified)
        .is_ok_and(|age| age >= STALE_AFTER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_recognized_and_nothing_else_is() {
        assert!(is_name(&name(811_637, 0)));
        assert!(is_name(&name(1, 99)));
        for other in [
            "herdr-command-palette-",
            "herdr-command-palette-1",
            "herdr-command-palette-1-",
            "herdr-command-palette--1",
            "herdr-command-palette-x-1",
            "herdr-command-palette-1-2-3",
            "herdr-command-palette-1-+2",
            "herdr-command-palette-history",
            "my-herdr-command-palette-1-2",
            "tmp.abc123",
        ] {
            assert!(!is_name(other), "{other}");
        }
    }

    #[test]
    fn a_dir_is_stale_from_one_day_old() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_hours(10 * 24);
        let second = Duration::from_secs(1);

        assert!(is_stale(now - STALE_AFTER, now));
        assert!(is_stale(SystemTime::UNIX_EPOCH, now));
        assert!(!is_stale(now - STALE_AFTER + second, now));
        assert!(!is_stale(now, now));
        assert!(!is_stale(now + second, now));
    }
}
