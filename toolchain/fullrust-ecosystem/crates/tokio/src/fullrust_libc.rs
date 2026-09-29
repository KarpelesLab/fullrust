//! A private, minimal stand-in for the `libc` crate (and for the few
//! `signal-hook-registry` items tokio uses) on the `fullrust` target.
//!
//! `fullrust` (`x86_64-unknown-linux-fullrust`) is the Linux kernel ABI with no
//! C library, so tokio's ordinary Linux code paths (`process` with its pidfd
//! and SIGCHLD reapers, `signal`, `net::unix` incl. `pipe` and `peer_cred`)
//! are compiled unchanged and resolve their `libc::…` paths to this module
//! instead. It supplies exactly the types, constants and functions those paths
//! use, with Linux x86-64 layouts and values, implemented as raw `syscall`s.
//!
//! Functions keep libc's convention (`-1` on failure) but, with no C `errno`,
//! record the error in a thread-local read by [`last_os_error`]; tokio's call
//! sites use the crate-level `last_os_error!()` macro, which is
//! `std::io::Error::last_os_error()` on every other target.
#![allow(dead_code, non_camel_case_types, non_upper_case_globals, non_snake_case)]
#![allow(clippy::missing_safety_doc, unsafe_op_in_unsafe_fn)]

use core::arch::asm;
use std::cell::Cell;
use std::io;

pub(crate) type c_int = core::ffi::c_int;
pub(crate) type c_uint = core::ffi::c_uint;
pub(crate) type c_long = core::ffi::c_long;
pub(crate) type c_void = core::ffi::c_void;
pub(crate) type size_t = usize;
pub(crate) type socklen_t = u32;
pub(crate) type mode_t = u32;
pub(crate) type pid_t = i32;
pub(crate) type uid_t = u32;
pub(crate) type gid_t = u32;

// ---------------------------------------------------------------------------
// Structs (Linux x86-64 layouts)
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct ucred {
    pub pid: pid_t,
    pub uid: uid_t,
    pub gid: gid_t,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct stat {
    pub st_dev: u64,
    pub st_ino: u64,
    pub st_nlink: u64,
    pub st_mode: u32,
    pub st_uid: u32,
    pub st_gid: u32,
    __pad0: i32,
    pub st_rdev: u64,
    pub st_size: i64,
    pub st_blksize: i64,
    pub st_blocks: i64,
    pub st_atime: i64,
    pub st_atime_nsec: i64,
    pub st_mtime: i64,
    pub st_mtime_nsec: i64,
    pub st_ctime: i64,
    pub st_ctime_nsec: i64,
    __unused: [i64; 3],
}

const _: () = {
    assert!(core::mem::size_of::<ucred>() == 12);
    assert!(core::mem::size_of::<stat>() == 144);
};

// ---------------------------------------------------------------------------
// Constants (Linux x86-64 values)
// ---------------------------------------------------------------------------

pub(crate) const ENOSYS: c_int = 38;
pub(crate) const EINPROGRESS: c_int = 115;

pub(crate) const F_GETFL: c_int = 3;
pub(crate) const F_SETFL: c_int = 4;
pub(crate) const O_ACCMODE: c_int = 3;
pub(crate) const O_RDONLY: c_int = 0;
pub(crate) const O_WRONLY: c_int = 1;
pub(crate) const O_RDWR: c_int = 2;
pub(crate) const O_NONBLOCK: c_int = 0o4000;
pub(crate) const S_IFMT: mode_t = 0o170000;
pub(crate) const S_IFIFO: mode_t = 0o010000;

pub(crate) const SOL_SOCKET: c_int = 1;
pub(crate) const SO_PEERCRED: c_int = 17;

pub(crate) const SYS_pidfd_open: c_long = 434;
pub(crate) const PIDFD_NONBLOCK: c_uint = O_NONBLOCK as c_uint;

pub(crate) const SIGHUP: c_int = 1;
pub(crate) const SIGINT: c_int = 2;
pub(crate) const SIGQUIT: c_int = 3;
pub(crate) const SIGILL: c_int = 4;
pub(crate) const SIGFPE: c_int = 8;
pub(crate) const SIGKILL: c_int = 9;
pub(crate) const SIGUSR1: c_int = 10;
pub(crate) const SIGSEGV: c_int = 11;
pub(crate) const SIGUSR2: c_int = 12;
pub(crate) const SIGPIPE: c_int = 13;
pub(crate) const SIGALRM: c_int = 14;
pub(crate) const SIGTERM: c_int = 15;
pub(crate) const SIGCHLD: c_int = 17;
pub(crate) const SIGSTOP: c_int = 19;
pub(crate) const SIGWINCH: c_int = 28;
pub(crate) const SIGIO: c_int = 29;
pub(crate) const SIGPOLL: c_int = SIGIO;

/// glibc's `SIGRTMAX()` (the kernel's `_NSIG` is 64 on x86-64).
pub(crate) fn SIGRTMAX() -> c_int {
    64
}

// ---------------------------------------------------------------------------
// errno stand-in
// ---------------------------------------------------------------------------

std::thread_local! {
    static ERRNO: Cell<i32> = const { Cell::new(0) };
}

/// The error recorded by the last failing function of this module on this
/// thread (what `io::Error::last_os_error()` is for libc).
pub(crate) fn last_os_error() -> io::Error {
    io::Error::from_raw_os_error(ERRNO.with(Cell::get))
}

/// libc convention: `-errno` from the kernel becomes `-1` + errno.
fn ret(r: isize) -> isize {
    if (-4095..0).contains(&r) {
        ERRNO.with(|e| e.set(-r as i32));
        -1
    } else {
        r
    }
}

// ---------------------------------------------------------------------------
// Raw syscalls (x86-64: nr in rax, args rdi rsi rdx r10 r8 r9; rcx/r11 clobbered)
// ---------------------------------------------------------------------------

const NR_FSTAT: usize = 5;
const NR_RT_SIGACTION: usize = 13;
const NR_GETSOCKOPT: usize = 55;
const NR_FCNTL: usize = 72;
const NR_RT_SIGRETURN: usize = 15;

#[inline]
unsafe fn sys2(n: usize, a: usize, b: usize) -> isize {
    let r: isize;
    asm!("syscall", inlateout("rax") n as isize => r, in("rdi") a, in("rsi") b,
        lateout("rcx") _, lateout("r11") _, options(nostack, preserves_flags));
    r
}
#[inline]
unsafe fn sys3(n: usize, a: usize, b: usize, c: usize) -> isize {
    let r: isize;
    asm!("syscall", inlateout("rax") n as isize => r, in("rdi") a, in("rsi") b, in("rdx") c,
        lateout("rcx") _, lateout("r11") _, options(nostack, preserves_flags));
    r
}
#[inline]
unsafe fn sys4(n: usize, a: usize, b: usize, c: usize, d: usize) -> isize {
    let r: isize;
    asm!("syscall", inlateout("rax") n as isize => r, in("rdi") a, in("rsi") b, in("rdx") c,
        in("r10") d, lateout("rcx") _, lateout("r11") _, options(nostack, preserves_flags));
    r
}
#[inline]
unsafe fn sys5(n: usize, a: usize, b: usize, c: usize, d: usize, e: usize) -> isize {
    let r: isize;
    asm!("syscall", inlateout("rax") n as isize => r, in("rdi") a, in("rsi") b, in("rdx") c,
        in("r10") d, in("r8") e, lateout("rcx") _, lateout("r11") _,
        options(nostack, preserves_flags));
    r
}

// `int` arguments are sign-extended as the C ABI would; the kernel truncates.
#[inline]
fn i(v: c_int) -> usize {
    v as isize as usize
}

// ---------------------------------------------------------------------------
// Functions: libc signatures and return convention
// ---------------------------------------------------------------------------

/// `fcntl(fd, cmd, arg)`; the `F_GETFL` call sites pass a dummy `0` (libc's
/// is variadic, Rust functions are not).
pub(crate) unsafe fn fcntl(fd: c_int, cmd: c_int, arg: c_int) -> c_int {
    ret(sys3(NR_FCNTL, i(fd), i(cmd), i(arg))) as c_int
}

pub(crate) unsafe fn fstat(fd: c_int, buf: *mut stat) -> c_int {
    ret(sys2(NR_FSTAT, i(fd), buf as usize)) as c_int
}

pub(crate) unsafe fn getsockopt(
    fd: c_int,
    level: c_int,
    name: c_int,
    value: *mut c_void,
    len: *mut socklen_t,
) -> c_int {
    ret(sys5(NR_GETSOCKOPT, i(fd), i(level), i(name), value as usize, len as usize)) as c_int
}

/// The one `syscall(2)` shape tokio uses: `syscall(SYS_pidfd_open, pid, flags)`.
pub(crate) unsafe fn syscall(nr: c_long, pid: u32, flags: c_uint) -> c_long {
    ret(sys2(nr as usize, pid as usize, flags as usize)) as c_long
}

pub(crate) unsafe fn memchr(cx: *const c_void, c: c_int, n: size_t) -> *mut c_void {
    let s = core::slice::from_raw_parts(cx as *const u8, n);
    match s.iter().position(|&b| b == c as u8) {
        Some(k) => cx.cast::<u8>().add(k) as *mut c_void,
        None => core::ptr::null_mut(),
    }
}

// ---------------------------------------------------------------------------
// signal-hook-registry stand-in
// ---------------------------------------------------------------------------

/// The part of `signal-hook-registry` tokio uses (`register` + `FORBIDDEN`),
/// with the same semantics on Linux: one process-wide `SA_RESTART |
/// SA_SIGINFO` handler per signal that runs every registered action in
/// registration order and then chains to the handler that was installed
/// before it (if that was a function, not `SIG_DFL`/`SIG_IGN`). Actions are
/// never unregistered (tokio never does).
///
/// (The crate itself can't be used: it calls `libc::sigaction`, and its
/// `libc` dependency is empty on this non-`unix` target.)
pub(crate) mod signal_hook_registry {
    use super::*;
    use std::sync::atomic::{AtomicPtr, Ordering};
    use std::sync::Mutex;

    pub(crate) const FORBIDDEN: &[c_int] = &[SIGKILL, SIGSTOP, SIGILL, SIGFPE, SIGSEGV];

    /// Registration handle (tokio ignores it).
    pub(crate) struct SigId;

    const SA_SIGINFO: u64 = 0x4;
    const SA_RESTORER: u64 = 0x0400_0000;
    const SA_RESTART: u64 = 0x1000_0000;
    const SIG_DFL: usize = 0;
    const SIG_IGN: usize = 1;
    const NSIG: usize = 65;

    /// The kernel's `struct sigaction` for `rt_sigaction` on x86-64.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct KSigaction {
        handler: usize,
        flags: u64,
        restorer: usize,
        mask: u64,
    }

    struct Node {
        action: Box<dyn Fn() + Send + Sync>,
        next: AtomicPtr<Node>,
    }

    struct Slot {
        head: AtomicPtr<Node>,
        prev: AtomicPtr<KSigaction>,
    }

    #[allow(clippy::declare_interior_mutable_const)]
    const EMPTY: Slot = Slot {
        head: AtomicPtr::new(core::ptr::null_mut()),
        prev: AtomicPtr::new(core::ptr::null_mut()),
    };
    static SLOTS: [Slot; NSIG] = [EMPTY; NSIG];
    static LOCK: Mutex<()> = Mutex::new(());

    /// `rt_sigreturn` trampoline: x86-64 handlers need an `sa_restorer`.
    #[unsafe(naked)]
    unsafe extern "C" fn restorer() -> ! {
        core::arch::naked_asm!("mov eax, {nr}", "syscall", "ud2", nr = const NR_RT_SIGRETURN)
    }

    extern "C" fn handler(sig: c_int, info: *mut c_void, ctx: *mut c_void) {
        let slot = &SLOTS[sig as usize];
        // Nodes are leaked, only ever appended: safe to walk without a lock.
        let mut n = slot.head.load(Ordering::Acquire);
        while !n.is_null() {
            unsafe {
                ((*n).action)();
                n = (*n).next.load(Ordering::Acquire);
            }
        }
        let prev = slot.prev.load(Ordering::Acquire);
        if !prev.is_null() {
            let p = unsafe { *prev };
            if p.handler != SIG_DFL && p.handler != SIG_IGN {
                unsafe {
                    if p.flags & SA_SIGINFO != 0 {
                        let f: extern "C" fn(c_int, *mut c_void, *mut c_void) =
                            core::mem::transmute(p.handler);
                        f(sig, info, ctx);
                    } else {
                        let f: extern "C" fn(c_int) = core::mem::transmute(p.handler);
                        f(sig);
                    }
                }
            }
        }
    }

    /// Registers `action` to run (in signal-handler context) whenever
    /// `signal` arrives.
    pub(crate) unsafe fn register<F>(signal: c_int, action: F) -> Result<SigId, io::Error>
    where
        F: Fn() + Sync + Send + 'static,
    {
        assert!(
            !FORBIDDEN.contains(&signal),
            "Attempted to register forbidden signal {signal}"
        );
        if signal <= 0 || signal as usize >= NSIG {
            return Err(io::Error::from_raw_os_error(22)); // EINVAL
        }
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let slot = &SLOTS[signal as usize];
        let node = Box::into_raw(Box::new(Node {
            action: Box::new(action),
            next: AtomicPtr::new(core::ptr::null_mut()),
        }));
        let mut tail = &slot.head;
        while !tail.load(Ordering::Acquire).is_null() {
            tail = &(*tail.load(Ordering::Acquire)).next;
        }
        tail.store(node, Ordering::Release);
        if slot.prev.load(Ordering::Acquire).is_null() {
            let new = KSigaction {
                handler: handler as *const () as usize,
                flags: SA_SIGINFO | SA_RESTART | SA_RESTORER,
                restorer: restorer as *const () as usize,
                mask: 0,
            };
            let mut old = KSigaction { handler: 0, flags: 0, restorer: 0, mask: 0 };
            let r = sys4(
                NR_RT_SIGACTION,
                i(signal),
                &new as *const KSigaction as usize,
                &mut old as *mut KSigaction as usize,
                8,
            );
            if r < 0 {
                return Err(io::Error::from_raw_os_error(-r as i32));
            }
            slot.prev.store(Box::into_raw(Box::new(old)), Ordering::Release);
        }
        Ok(SigId)
    }
}
