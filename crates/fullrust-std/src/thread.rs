//! Threads and thread-local storage.
//!
//! NOTE: `spawn` currently runs the closure inline (synchronously) and returns
//! a ready `JoinHandle`. This is correct for the offline command paths (which
//! don't spawn) and lets everything compile; real `clone`-backed threads are
//! implemented in a later step for the networking commands. `thread_local!` is
//! single-thread (one shared slot), which suits the program model here.

use crate::sys;
use crate::time::Duration;
use alloc::boxed::Box;
use core::cell::UnsafeCell;

/// Sleep for `dur`.
pub fn sleep(dur: Duration) {
    #[repr(C)]
    struct Timespec {
        tv_sec: i64,
        tv_nsec: i64,
    }
    let ts = Timespec { tv_sec: dur.as_secs() as i64, tv_nsec: dur.subsec_nanos() as i64 };
    unsafe {
        let _ = sys::sc2(sys::nr::NANOSLEEP, &ts as *const _ as usize, 0);
    }
}

/// Yield the current timeslice.
pub fn yield_now() {
    unsafe {
        let _ = sys::sc0(sys::nr::SCHED_YIELD);
    }
}

/// Handle to a (here, already-finished) thread.
pub struct JoinHandle<T> {
    result: Option<T>,
}

impl<T> JoinHandle<T> {
    pub fn join(mut self) -> Result<T, Box<dyn core::any::Any + Send>> {
        Ok(self.result.take().expect("joined twice"))
    }
    pub fn thread(&self) -> Thread {
        Thread { _priv: () }
    }
}

/// Opaque thread handle.
#[derive(Clone)]
pub struct Thread {
    _priv: (),
}
impl Thread {
    pub fn id(&self) -> ThreadId {
        ThreadId(0)
    }
    pub fn name(&self) -> Option<&str> {
        None
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ThreadId(u64);

/// The current thread handle.
pub fn current() -> Thread {
    Thread { _priv: () }
}

/// Spawn a thread (see module note: currently runs inline).
pub fn spawn<F, T>(f: F) -> JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    JoinHandle { result: Some(f()) }
}

/// Builder mirroring `std::thread::Builder`.
#[derive(Default)]
pub struct Builder {
    name: Option<alloc::string::String>,
    stack_size: Option<usize>,
}
impl Builder {
    pub fn new() -> Builder {
        Builder::default()
    }
    pub fn name(mut self, name: alloc::string::String) -> Builder {
        self.name = Some(name);
        self
    }
    pub fn stack_size(mut self, size: usize) -> Builder {
        self.stack_size = Some(size);
        self
    }
    pub fn spawn<F, T>(self, f: F) -> crate::io::Result<JoinHandle<T>>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        Ok(spawn(f))
    }
}

// ---- thread-local storage (single-thread shim) ----

/// A thread-local key. In this single-thread shim it is just a lazily
/// initialized shared slot.
pub struct LocalKey<T: 'static> {
    init: fn() -> T,
    slot: UnsafeCell<Option<T>>,
}

// Safe under the single-thread model this shim targets.
unsafe impl<T: 'static> Sync for LocalKey<T> {}

impl<T: 'static> LocalKey<T> {
    #[doc(hidden)]
    pub const fn new(init: fn() -> T) -> LocalKey<T> {
        LocalKey { init, slot: UnsafeCell::new(None) }
    }

    pub fn with<F, R>(&'static self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        let slot = unsafe { &mut *self.slot.get() };
        if slot.is_none() {
            *slot = Some((self.init)());
        }
        f(slot.as_ref().unwrap())
    }

    pub fn try_with<F, R>(&'static self, f: F) -> Result<R, AccessError>
    where
        F: FnOnce(&T) -> R,
    {
        Ok(self.with(f))
    }
}

/// Error returned by `LocalKey::try_with` (never produced here).
#[derive(Debug)]
pub struct AccessError;

/// Declare thread-local keys (single-thread shim semantics).
#[macro_export]
macro_rules! thread_local {
    () => {};
    ($(#[$attr:meta])* $vis:vis static $name:ident: $t:ty = $init:expr; $($rest:tt)*) => {
        $(#[$attr])* $vis static $name: $crate::thread::LocalKey<$t> = {
            fn __init() -> $t { $init }
            $crate::thread::LocalKey::new(__init)
        };
        $crate::thread_local!($($rest)*);
    };
}
