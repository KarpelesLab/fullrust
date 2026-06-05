//! Syscall-backed clocks: `SystemTime` (wall clock) and `Instant` (monotonic).

use crate::sys;
use core::time::Duration;

const CLOCK_REALTIME: usize = 0;
const CLOCK_MONOTONIC: usize = 1;

#[repr(C)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: i64,
}

fn clock_gettime(which: usize) -> Duration {
    let mut ts = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        let _ = sys::sc2(sys::nr::CLOCK_GETTIME, which, &mut ts as *mut _ as usize);
    }
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

/// The Unix epoch — the start of `SystemTime`.
pub const UNIX_EPOCH: SystemTime = SystemTime(Duration::ZERO);

/// A measurement of the system wall clock.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct SystemTime(Duration);

/// Error returned when a `SystemTime` is earlier than expected.
#[derive(Clone, Copy, Debug)]
pub struct SystemTimeError(Duration);

impl SystemTimeError {
    /// The amount of time by which the comparison failed.
    pub fn duration(&self) -> Duration {
        self.0
    }
}

impl core::fmt::Display for SystemTimeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "second time provided was later than self")
    }
}
impl core::error::Error for SystemTimeError {}

impl SystemTime {
    /// The current wall-clock time.
    pub fn now() -> SystemTime {
        SystemTime(clock_gettime(CLOCK_REALTIME))
    }

    /// Duration elapsed from `earlier` to `self`.
    pub fn duration_since(&self, earlier: SystemTime) -> Result<Duration, SystemTimeError> {
        self.0
            .checked_sub(earlier.0)
            .ok_or_else(|| SystemTimeError(earlier.0.saturating_sub(self.0)))
    }

    /// Duration since this instant of wall-clock time.
    pub fn elapsed(&self) -> Result<Duration, SystemTimeError> {
        SystemTime::now().duration_since(*self)
    }

    pub fn checked_add(&self, d: Duration) -> Option<SystemTime> {
        self.0.checked_add(d).map(SystemTime)
    }
    pub fn checked_sub(&self, d: Duration) -> Option<SystemTime> {
        self.0.checked_sub(d).map(SystemTime)
    }
}

impl core::ops::Add<Duration> for SystemTime {
    type Output = SystemTime;
    fn add(self, d: Duration) -> SystemTime {
        SystemTime(self.0 + d)
    }
}
impl core::ops::Sub<Duration> for SystemTime {
    type Output = SystemTime;
    fn sub(self, d: Duration) -> SystemTime {
        SystemTime(self.0 - d)
    }
}

/// A monotonically non-decreasing clock.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Instant(Duration);

impl Instant {
    /// The current monotonic instant.
    pub fn now() -> Instant {
        Instant(clock_gettime(CLOCK_MONOTONIC))
    }

    /// Duration since `earlier` (saturating at zero).
    pub fn duration_since(&self, earlier: Instant) -> Duration {
        self.0.checked_sub(earlier.0).unwrap_or(Duration::ZERO)
    }
    pub fn saturating_duration_since(&self, earlier: Instant) -> Duration {
        self.duration_since(earlier)
    }
    pub fn checked_duration_since(&self, earlier: Instant) -> Option<Duration> {
        self.0.checked_sub(earlier.0)
    }

    /// Duration elapsed since this instant.
    pub fn elapsed(&self) -> Duration {
        Instant::now().duration_since(*self)
    }
}

impl core::ops::Add<Duration> for Instant {
    type Output = Instant;
    fn add(self, d: Duration) -> Instant {
        Instant(self.0 + d)
    }
}
impl core::ops::Sub<Duration> for Instant {
    type Output = Instant;
    fn sub(self, d: Duration) -> Instant {
        Instant(self.0 - d)
    }
}
impl core::ops::Sub<Instant> for Instant {
    type Output = Duration;
    fn sub(self, earlier: Instant) -> Duration {
        self.duration_since(earlier)
    }
}
