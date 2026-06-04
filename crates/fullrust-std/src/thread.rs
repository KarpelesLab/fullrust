//! Threads and thread-local storage.
//!
//! `spawn` creates a real kernel thread via the raw `clone` syscall on an
//! `mmap`'d stack, and `JoinHandle::join` blocks on a futex that the kernel
//! clears when the thread exits (`CLONE_CHILD_CLEARTID`). If `clone` is
//! unavailable (e.g. blocked by a sandbox) it transparently falls back to
//! running the closure inline.
//!
//! `thread_local!` is single-slot (shared across threads) — adequate for this
//! program model, which uses thread-locals only for cheap caches.

use crate::sys;
use crate::time::Duration;
use alloc::boxed::Box;
use alloc::sync::Arc;
use core::cell::UnsafeCell;
use core::sync::atomic::{fence, AtomicI32, Ordering};

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

// ---- real clone-based threads ----

const CLONE_VM: usize = 0x100;
const CLONE_FS: usize = 0x200;
const CLONE_FILES: usize = 0x400;
const CLONE_SIGHAND: usize = 0x800;
const CLONE_THREAD: usize = 0x10000;
const CLONE_SYSVSEM: usize = 0x40000;
const CLONE_CHILD_SETTID: usize = 0x0100_0000;
const CLONE_CHILD_CLEARTID: usize = 0x0020_0000;

const STACK_SIZE: usize = 2 * 1024 * 1024;

/// Naked `clone` wrapper. Mirrors musl's `__clone`: arguments arrive per the C
/// ABI; the child runs `fn_(arg)` on the new stack and `exit`s with its return
/// value; the parent returns the new TID (or `-errno`).
#[unsafe(naked)]
unsafe extern "C" fn fr_clone(
    fn_: extern "C" fn(*mut u8) -> i32, // rdi
    stack: *mut u8,                     // rsi
    flags: usize,                       // rdx
    arg: *mut u8,                       // rcx
    ptid: *mut i32,                     // r8
    tls: *mut u8,                       // r9
    ctid: *mut i32,                     // [rsp+8]
) -> isize {
    core::arch::naked_asm!(
        "mov r11, rdi",     // save fn
        "mov rdi, rdx",     // flags  -> syscall arg1
        "mov rdx, r8",      // ptid   -> syscall arg3
        "mov r8, r9",       // tls    -> syscall arg5
        "mov r10, [rsp+8]", // ctid   -> syscall arg4
        "mov r9, r11",      // fn kept in r9 for the child
        "and rsi, -16",     // align child stack
        "sub rsi, 8",
        "mov [rsi], rcx",   // push arg for the child to pop
        "mov rax, 56",      // SYS_clone
        "syscall",
        "test rax, rax",
        "jnz 2f",           // parent -> return tid
        // child:
        "xor ebp, ebp",
        "pop rdi",          // arg
        "call r9",          // fn(arg)
        "mov rdi, rax",     // exit code = fn's return
        "mov rax, 60",      // SYS_exit
        "syscall",
        "2:",
        "ret",
    )
}

struct Shared<T> {
    result: UnsafeCell<Option<T>>,
    ctid: AtomicI32,
}
unsafe impl<T: Send> Sync for Shared<T> {}
unsafe impl<T: Send> Send for Shared<T> {}

struct Payload<F, T> {
    f: F,
    shared: Arc<Shared<T>>,
}

extern "C" fn trampoline<F, T>(arg: *mut u8) -> i32
where
    F: FnOnce() -> T,
{
    let payload = unsafe { *Box::from_raw(arg as *mut Payload<F, T>) };
    let Payload { f, shared } = payload;
    let res = f();
    unsafe {
        *shared.result.get() = Some(res);
    }
    // Publish the result before the kernel clears ctid and wakes the joiner.
    fence(Ordering::Release);
    drop(shared);
    0
}

fn futex_wait(addr: &AtomicI32, expected: i32) {
    unsafe {
        // futex(uaddr, FUTEX_WAIT=0, val, timeout=NULL)
        let _ = sys::sc4(sys::nr::FUTEX, addr as *const _ as usize, 0, expected as usize, 0);
    }
}

/// Handle to a running (or already-finished) thread.
pub struct JoinHandle<T> {
    shared: Arc<Shared<T>>,
    stack: *mut u8,
    stack_size: usize,
    inline: bool,
}

unsafe impl<T: Send> Send for JoinHandle<T> {}
unsafe impl<T: Send> Sync for JoinHandle<T> {}

impl<T> JoinHandle<T> {
    pub fn join(self) -> Result<T, Box<dyn core::any::Any + Send>> {
        if !self.inline {
            loop {
                let v = self.shared.ctid.load(Ordering::Acquire);
                if v == 0 {
                    break;
                }
                futex_wait(&self.shared.ctid, v);
            }
            unsafe {
                let _ = sys::munmap_raw(self.stack, self.stack_size);
            }
        }
        fence(Ordering::Acquire);
        let res = unsafe { (*self.shared.result.get()).take() };
        Ok(res.expect("thread produced no result"))
    }

    pub fn thread(&self) -> Thread {
        Thread { _priv: () }
    }
    pub fn is_finished(&self) -> bool {
        self.inline || self.shared.ctid.load(Ordering::Acquire) == 0
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

/// Spawn a new thread running `f`.
pub fn spawn<F, T>(f: F) -> JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    Builder::new().spawn(f).expect("failed to spawn thread")
}

/// Thread builder (`name`/`stack_size` are accepted; name is currently unused).
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
        let shared = Arc::new(Shared { result: UnsafeCell::new(None), ctid: AtomicI32::new(0) });
        let stack_size = self.stack_size.unwrap_or(STACK_SIZE);

        // Allocate the child stack.
        let stack = match sys::mmap_stack(stack_size) {
            Some(p) => p,
            None => return run_inline(f, shared),
        };
        let top = stack.wrapping_add(stack_size);

        let payload = Box::new(Payload { f, shared: shared.clone() });
        let arg = Box::into_raw(payload) as *mut u8;

        let flags = CLONE_VM
            | CLONE_FS
            | CLONE_FILES
            | CLONE_SIGHAND
            | CLONE_THREAD
            | CLONE_SYSVSEM
            | CLONE_CHILD_SETTID
            | CLONE_CHILD_CLEARTID;
        let ctid = &shared.ctid as *const AtomicI32 as *mut i32;

        let tid = unsafe {
            fr_clone(
                trampoline::<F, T>,
                top,
                flags,
                arg,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                ctid,
            )
        };

        if tid <= 0 {
            // clone failed: reclaim the payload and stack, run inline.
            let payload = unsafe { *Box::from_raw(arg as *mut Payload<F, T>) };
            unsafe {
                let _ = sys::munmap_raw(stack, stack_size);
            }
            return run_inline(payload.f, shared);
        }

        Ok(JoinHandle { shared, stack, stack_size, inline: false })
    }
}

fn run_inline<F, T>(f: F, shared: Arc<Shared<T>>) -> crate::io::Result<JoinHandle<T>>
where
    F: FnOnce() -> T,
{
    let res = f();
    unsafe {
        *shared.result.get() = Some(res);
    }
    Ok(JoinHandle { shared, stack: core::ptr::null_mut(), stack_size: 0, inline: true })
}

// ---- thread-local storage (single-slot shim) ----

/// A thread-local key. In this shim it is a lazily initialized shared slot.
pub struct LocalKey<T: 'static> {
    init: fn() -> T,
    slot: UnsafeCell<Option<T>>,
}

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

/// Declare thread-local keys (single-slot shim semantics).
#[macro_export]
macro_rules! thread_local {
    () => {};
    ($(#[$attr:meta])* $vis:vis static $name:ident: $t:ty = const { $init:expr }; $($rest:tt)*) => {
        $(#[$attr])* $vis static $name: $crate::thread::LocalKey<$t> = {
            fn __init() -> $t { $init }
            $crate::thread::LocalKey::new(__init)
        };
        $crate::thread_local!($($rest)*);
    };
    ($(#[$attr:meta])* $vis:vis static $name:ident: $t:ty = $init:expr; $($rest:tt)*) => {
        $(#[$attr])* $vis static $name: $crate::thread::LocalKey<$t> = {
            fn __init() -> $t { $init }
            $crate::thread::LocalKey::new(__init)
        };
        $crate::thread_local!($($rest)*);
    };
}
