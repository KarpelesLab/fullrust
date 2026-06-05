//! Synchronization primitives: `Mutex`, `RwLock`, `Once`, `OnceLock`.
//!
//! These use atomics with `sched_yield` backoff. fullrust programs are mostly
//! single-threaded; the spin path is correct (Linux preempts) if contended.

use crate::sys;
use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicUsize, Ordering};

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

// ---- Mutex ----

pub struct Mutex<T: ?Sized> {
    locked: AtomicUsize,
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
            locked: AtomicUsize::new(0),
            data: UnsafeCell::new(t),
        }
    }
    pub fn into_inner(self) -> LockResult<T> {
        Ok(self.data.into_inner())
    }
}

impl<T: ?Sized> Mutex<T> {
    pub fn lock(&self) -> LockResult<MutexGuard<'_, T>> {
        while self
            .locked
            .compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            yield_now();
        }
        Ok(MutexGuard { lock: self })
    }

    pub fn try_lock(&self) -> TryLockResult<MutexGuard<'_, T>> {
        match self
            .locked
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
        {
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
        self.lock.locked.store(0, Ordering::Release);
    }
}

impl<T: Default> Default for Mutex<T> {
    fn default() -> Self {
        Mutex::new(T::default())
    }
}

// ---- RwLock ----

const WRITER: usize = usize::MAX;

pub struct RwLock<T: ?Sized> {
    state: AtomicUsize, // 0 = free, WRITER = write-locked, n = n readers
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
            state: AtomicUsize::new(0),
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
            let s = self.state.load(Ordering::Relaxed);
            if s != WRITER
                && self
                    .state
                    .compare_exchange_weak(s, s + 1, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
            {
                return Ok(RwLockReadGuard { lock: self });
            }
            yield_now();
        }
    }
    pub fn write(&self) -> LockResult<RwLockWriteGuard<'_, T>> {
        while self
            .state
            .compare_exchange_weak(0, WRITER, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            yield_now();
        }
        Ok(RwLockWriteGuard { lock: self })
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
        self.lock.state.fetch_sub(1, Ordering::Release);
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
        self.lock.state.store(0, Ordering::Release);
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
        self.state.load(Ordering::Acquire) == COMPLETE
    }
    pub fn call_once<F: FnOnce()>(&self, f: F) {
        if self.state.load(Ordering::Acquire) == COMPLETE {
            return;
        }
        loop {
            match self.state.compare_exchange(
                INCOMPLETE,
                RUNNING,
                Ordering::Acquire,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    f();
                    self.state.store(COMPLETE, Ordering::Release);
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
