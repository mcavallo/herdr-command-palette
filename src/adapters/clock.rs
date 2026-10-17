//! [`Clock`] reading the system time.

use std::time::SystemTime;

use crate::ports::Clock;

/// The system's wall clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_system_time() {
        let before = SystemTime::now();

        let now = SystemClock.now();

        assert!(now >= before);
        assert!(now <= SystemTime::now());
    }
}
