//! How often to retry a Herdr call answered with `ui_busy`.
//!
//! Only the policy value lives here; the socket adapter does the retrying
//! and [`app::retry_policy`](crate::app::retry_policy) picks the policy per
//! use case.

use std::time::Duration;

/// How often to retry a call that Herdr answers with `ui_busy`.
///
/// Herdr allows one popup at a time: while ours is still closing, opening
/// another one fails with `ui_busy`. Each use case picks its
/// policy ([`app::retry_policy`](crate::app::retry_policy)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Attempts in total, including the first; `0` behaves like `1`.
    pub attempts: u32,
    /// Pause between attempts.
    pub delay: Duration,
}

impl RetryPolicy {
    /// No retries: one attempt.
    pub(crate) const NONE: Self = Self {
        attempts: 1,
        delay: Duration::ZERO,
    };
}

/// 40 attempts, 50 ms apart: up to 2 s.
impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            attempts: 40,
            delay: Duration::from_millis(50),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_retries_for_two_seconds_and_none_tries_once() {
        let default = RetryPolicy::default();

        assert_eq!(default.delay * default.attempts, Duration::from_secs(2));
        assert_eq!(RetryPolicy::NONE.attempts, 1);
    }
}
