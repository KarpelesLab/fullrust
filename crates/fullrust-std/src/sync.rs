//! Synchronization primitives: `Mutex`, `Condvar`, `RwLock`, `Once`, `OnceLock`.
//!
//! `Mutex`/`Condvar`/`RwLock` block via Linux `futex` (a short adaptive spin,
//! then a real kernel wait) — the same design as `std`. `Once`/`OnceLock` use a
//! light spin since they are typically uncontended.

use crate::sys;
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering::*};

#[inline]
fn yield_now() {
    unsafe {
        let _ = sys::sc0(sys::nr::SCHED_YIELD);
    }
}

/// Poison wrapper (we never actually poison; mirrors `std`'s `lock()` result).
pub struct PoisonError<T> {
    guard: T,
}
impl<T> PoisonError<T> {
    pub fn into_inner(self) -> T {
        self.guard
    }
    pub fn get_ref(&self) -> &T {
        &self.guard
    }
}
impl<T> core::fmt::Debug for PoisonError<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("PoisonError")
    }
}
impl<T> core::fmt::Display for PoisonError<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("poisoned lock")
    }
}

pub type LockResult<G> = Result<G, PoisonError<G>>;
pub type TryLockResult<G> = Result<G, TryLockError<G>>;

pub enum TryLockError<T> {
    Poisoned(PoisonError<T>),
    WouldBlock,
}

// ---- Mutex (futex, 3-state: 0 unlocked / 1 locked / 2 locked + waiters) ----

pub struct Mutex<T: ?Sized> {
    state: AtomicU32,
    data: UnsafeCell<T>,
}

unsafe impl<T: ?Sized + Send> Send for Mutex<T> {}
unsafe impl<T: ?Sized + Send> Sync for Mutex<T> {}

pub struct MutexGuard<'a, T: ?Sized + 'a> {
    lock: &'a Mutex<T>,
}

impl<T> Mutex<T> {
    pub const fn new(t: T) -> Mutex<T> {
        Mutex {
            state: AtomicU32::new(0),
            data: UnsafeCell::new(t),
        }
    }
    pub fn into_inner(self) -> LockResult<T> {
        Ok(self.data.into_inner())
    }
}

impl<T: ?Sized> Mutex<T> {
    #[inline]
    fn raw_lock(&self) {
        if self.state.compare_exchange(0, 1, Acquire, Relaxed).is_err() {
            self.lock_contended();
        }
    }

    #[cold]
    fn lock_contended(&self) {
        let mut state = self.spin();
        if state == 0 {
            match self.state.compare_exchange(0, 1, Acquire, Relaxed) {
                Ok(_) => return,
                Err(s) => state = s,
            }
        }
        loop {
            // Mark as "locked with waiters" and grab it if it was free.
            if state != 2 && self.state.swap(2, Acquire) == 0 {
                return;
            }
            sys::futex_wait(&self.state, 2);
            state = self.spin();
        }
    }

    fn spin(&self) -> u32 {
        let mut spin = 100;
        loop {
            let state = self.state.load(Relaxed);
            if state != 1 || spin == 0 {
                return state;
            }
            core::hint::spin_loop();
            spin -= 1;
        }
    }

    /// # Safety: caller must hold the lock.
    #[inline]
    unsafe fn raw_unlock(&self) {
        if self.state.swap(0, Release) == 2 {
            sys::futex_wake(&self.state, 1);
        }
    }

    pub fn lock(&self) -> LockResult<MutexGuard<'_, T>> {
        self.raw_lock();
        Ok(MutexGuard { lock: self })
    }

    pub fn try_lock(&self) -> TryLockResult<MutexGuard<'_, T>> {
        match self.state.compare_exchange(0, 1, Acquire, Relaxed) {
            Ok(_) => Ok(MutexGuard { lock: self }),
            Err(_) => Err(TryLockError::WouldBlock),
        }
    }

    pub fn get_mut(&mut self) -> LockResult<&mut T> {
        Ok(unsafe { &mut *self.data.get() })
    }
}

impl<T: ?Sized> Deref for MutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}
impl<T: ?Sized> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}
impl<T: ?Sized> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        unsafe { self.lock.raw_unlock() }
    }
}

impl<T: Default> Default for Mutex<T> {
    fn default() -> Self {
        Mutex::new(T::default())
    }
}

// ---- Condvar (futex) ----

pub struct Condvar {
    // Incremented on every notify; waiters block on the old value.
    futex: AtomicU32,
}

impl Condvar {
    pub const fn new() -> Condvar {
        Condvar {
            futex: AtomicU32::new(0),
        }
    }

    pub fn notify_one(&self) {
        self.futex.fetch_add(1, Release);
        sys::futex_wake(&self.futex, 1);
    }

    pub fn notify_all(&self) {
        self.futex.fetch_add(1, Release);
        sys::futex_wake(&self.futex, i32::MAX);
    }

    /// Atomically release `guard`'s mutex and block until notified, then
    /// re-acquire and return the guard.
    pub fn wait<'a, T: ?Sized>(&self, guard: MutexGuard<'a, T>) -> LockResult<MutexGuard<'a, T>> {
        let mutex = guard.lock;
        let value = self.futex.load(Relaxed);
        // Release the mutex without running the guard's destructor twice.
        core::mem::forget(guard);
        unsafe { mutex.raw_unlock() };
        sys::futex_wait(&self.futex, value);
        mutex.raw_lock();
        Ok(MutexGuard { lock: mutex })
    }

    /// `wait` in a loop until `condition` returns false.
    pub fn wait_while<'a, T: ?Sized, F>(
        &self,
        mut guard: MutexGuard<'a, T>,
        mut condition: F,
    ) -> LockResult<MutexGuard<'a, T>>
    where
        F: FnMut(&mut T) -> bool,
    {
        while condition(&mut guard) {
            guard = self.wait(guard)?;
        }
        Ok(guard)
    }
}

impl Default for Condvar {
    fn default() -> Self {
        Condvar::new()
    }
}

// ---- RwLock (futex): bit 31 = write-locked, low bits = reader count ----

const WRITE_BIT: u32 = 1 << 31;

pub struct RwLock<T: ?Sized> {
    state: AtomicU32,
    data: UnsafeCell<T>,
}

unsafe impl<T: ?Sized + Send> Send for RwLock<T> {}
unsafe impl<T: ?Sized + Send + Sync> Sync for RwLock<T> {}

pub struct RwLockReadGuard<'a, T: ?Sized + 'a> {
    lock: &'a RwLock<T>,
}
pub struct RwLockWriteGuard<'a, T: ?Sized + 'a> {
    lock: &'a RwLock<T>,
}

impl<T> RwLock<T> {
    pub const fn new(t: T) -> RwLock<T> {
        RwLock {
            state: AtomicU32::new(0),
            data: UnsafeCell::new(t),
        }
    }
    pub fn into_inner(self) -> LockResult<T> {
        Ok(self.data.into_inner())
    }
}

impl<T: ?Sized> RwLock<T> {
    pub fn read(&self) -> LockResult<RwLockReadGuard<'_, T>> {
        loop {
            let s = self.state.load(Acquire);
            if s & WRITE_BIT == 0 {
                if self
                    .state
                    .compare_exchange_weak(s, s + 1, Acquire, Relaxed)
                    .is_ok()
                {
                    return Ok(RwLockReadGuard { lock: self });
                }
            } else {
                sys::futex_wait(&self.state, s);
            }
        }
    }

    pub fn try_read(&self) -> TryLockResult<RwLockReadGuard<'_, T>> {
        let s = self.state.load(Acquire);
        if s & WRITE_BIT == 0
            && self
                .state
                .compare_exchange(s, s + 1, Acquire, Relaxed)
                .is_ok()
        {
            Ok(RwLockReadGuard { lock: self })
        } else {
            Err(TryLockError::WouldBlock)
        }
    }

    pub fn write(&self) -> LockResult<RwLockWriteGuard<'_, T>> {
        loop {
            match self.state.compare_exchange(0, WRITE_BIT, Acquire, Relaxed) {
                Ok(_) => return Ok(RwLockWriteGuard { lock: self }),
                Err(s) => sys::futex_wait(&self.state, s),
            }
        }
    }

    pub fn try_write(&self) -> TryLockResult<RwLockWriteGuard<'_, T>> {
        match self.state.compare_exchange(0, WRITE_BIT, Acquire, Relaxed) {
            Ok(_) => Ok(RwLockWriteGuard { lock: self }),
            Err(_) => Err(TryLockError::WouldBlock),
        }
    }

    pub fn get_mut(&mut self) -> LockResult<&mut T> {
        Ok(unsafe { &mut *self.data.get() })
    }
}

impl<T: ?Sized> Deref for RwLockReadGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}
impl<T: ?Sized> Drop for RwLockReadGuard<'_, T> {
    fn drop(&mut self) {
        // Last reader out wakes a waiting writer.
        if self.lock.state.fetch_sub(1, Release) == 1 {
            sys::futex_wake(&self.lock.state, i32::MAX);
        }
    }
}
impl<T: ?Sized> Deref for RwLockWriteGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}
impl<T: ?Sized> DerefMut for RwLockWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}
impl<T: ?Sized> Drop for RwLockWriteGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.state.store(0, Release);
        sys::futex_wake(&self.lock.state, i32::MAX);
    }
}

// ---- Once / OnceLock ----

const INCOMPLETE: usize = 0;
const RUNNING: usize = 1;
const COMPLETE: usize = 2;

pub struct Once {
    state: AtomicUsize,
}

impl Once {
    pub const fn new() -> Once {
        Once {
            state: AtomicUsize::new(INCOMPLETE),
        }
    }
    pub fn is_completed(&self) -> bool {
        self.state.load(Acquire) == COMPLETE
    }
    pub fn call_once<F: FnOnce()>(&self, f: F) {
        if self.state.load(Acquire) == COMPLETE {
            return;
        }
        loop {
            match self
                .state
                .compare_exchange(INCOMPLETE, RUNNING, Acquire, Acquire)
            {
                Ok(_) => {
                    f();
                    self.state.store(COMPLETE, Release);
                    return;
                }
                Err(COMPLETE) => return,
                Err(_) => yield_now(),
            }
        }
    }
}

impl Default for Once {
    fn default() -> Self {
        Once::new()
    }
}

pub struct OnceLock<T> {
    once: Once,
    value: UnsafeCell<Option<T>>,
}

unsafe impl<T: Send + Sync> Sync for OnceLock<T> {}
unsafe impl<T: Send> Send for OnceLock<T> {}

impl<T> OnceLock<T> {
    pub const fn new() -> OnceLock<T> {
        OnceLock {
            once: Once::new(),
            value: UnsafeCell::new(None),
        }
    }
    pub fn get(&self) -> Option<&T> {
        if self.once.is_completed() {
            unsafe { (*self.value.get()).as_ref() }
        } else {
            None
        }
    }
    pub fn set(&self, val: T) -> Result<(), T> {
        let mut val = Some(val);
        self.once.call_once(|| unsafe {
            *self.value.get() = val.take();
        });
        match val {
            None => Ok(()),
            Some(v) => Err(v),
        }
    }
    pub fn get_or_init<F: FnOnce() -> T>(&self, f: F) -> &T {
        self.once.call_once(|| {
            let v = f();
            unsafe {
                *self.value.get() = Some(v);
            }
        });
        unsafe { (*self.value.get()).as_ref().unwrap() }
    }
}

impl<T> Default for OnceLock<T> {
    fn default() -> Self {
        OnceLock::new()
    }
}
