//! Raw syscall layer for the shim (x86-64 Linux).
//!
//! Reuses [`fullrust::arch`] for the actual `syscall` instruction and adds the
//! numbers/wrappers the shim needs beyond fullrust's own small set.

#![allow(dead_code)]

use fullrust::arch::{syscall0, syscall1, syscall2, syscall3, syscall4, syscall5, syscall6};

pub use fullrust::syscall::Errno;

/// x86-64 Linux syscall numbers used by the shim.
pub mod nr {
    pub const READ: usize = 0;
    pub const WRITE: usize = 1;
    pub const CLOSE: usize = 3;
    pub const FSTAT: usize = 5;
    pub const LSEEK: usize = 8;
    pub const MMAP: usize = 9;
    pub const MUNMAP: usize = 11;
    pub const RT_SIGACTION: usize = 13;
    pub const IOCTL: usize = 16;
    pub const NANOSLEEP: usize = 35;
    pub const GETPID: usize = 39;
    pub const SOCKET: usize = 41;
    pub const CONNECT: usize = 42;
    pub const SENDTO: usize = 44;
    pub const RECVFROM: usize = 45;
    pub const SHUTDOWN: usize = 48;
    pub const BIND: usize = 49;
    pub const LISTEN: usize = 50;
    pub const GETSOCKNAME: usize = 51;
    pub const GETPEERNAME: usize = 52;
    pub const SETSOCKOPT: usize = 54;
    pub const GETSOCKOPT: usize = 55;
    pub const CLONE: usize = 56;
    pub const EXIT: usize = 60;
    pub const FCNTL: usize = 72;
    pub const FTRUNCATE: usize = 77;
    pub const GETTID: usize = 186;
    pub const FUTEX: usize = 202;
    pub const SCHED_YIELD: usize = 24;
    pub const GETDENTS64: usize = 217;
    pub const SET_TID_ADDRESS: usize = 218;
    pub const CLOCK_GETTIME: usize = 228;
    pub const EXIT_GROUP: usize = 231;
    pub const NEWFSTATAT: usize = 262;
    pub const OPENAT: usize = 257;
    pub const MKDIRAT: usize = 258;
    pub const UNLINKAT: usize = 263;
    pub const RENAMEAT: usize = 264;
    pub const ACCEPT4: usize = 288;
    pub const GETRANDOM: usize = 318;
    pub const STATX: usize = 332;
}

pub const AT_FDCWD: isize = -100;
pub const AT_REMOVEDIR: usize = 0x200;

/// `mmap` an anonymous private RW region for use as a thread stack.
pub fn mmap_stack(size: usize) -> Option<*mut u8> {
    // PROT_READ|PROT_WRITE = 3, MAP_PRIVATE|MAP_ANONYMOUS = 0x22, fd = -1.
    let ret = unsafe { syscall6(nr::MMAP, 0, size, 3, 0x22, usize::MAX, 0) };
    match r(ret) {
        Ok(p) => Some(p as *mut u8),
        Err(_) => None,
    }
}

/// `munmap` a region previously returned by [`mmap_stack`].
///
/// # Safety
/// `addr`/`len` must describe a live mapping.
pub unsafe fn munmap_raw(addr: *mut u8, len: usize) -> Result<usize, Errno> {
    sc2(nr::MUNMAP, addr as usize, len)
}

/// Convert a raw syscall return into `Result`, mapping `-errno` to `Err`.
#[inline]
pub fn r(ret: usize) -> Result<usize, Errno> {
    let s = ret as isize;
    if (-4095..0).contains(&s) {
        Err(Errno(-s as i32))
    } else {
        Ok(ret)
    }
}

// Thin generic wrappers (unsafe: caller upholds the kernel contract).
#[inline]
pub unsafe fn sc0(n: usize) -> Result<usize, Errno> {
    r(syscall0(n))
}
#[inline]
pub unsafe fn sc1(n: usize, a: usize) -> Result<usize, Errno> {
    r(syscall1(n, a))
}
#[inline]
pub unsafe fn sc2(n: usize, a: usize, b: usize) -> Result<usize, Errno> {
    r(syscall2(n, a, b))
}
#[inline]
pub unsafe fn sc3(n: usize, a: usize, b: usize, c: usize) -> Result<usize, Errno> {
    r(syscall3(n, a, b, c))
}
#[inline]
pub unsafe fn sc4(n: usize, a: usize, b: usize, c: usize, d: usize) -> Result<usize, Errno> {
    r(syscall4(n, a, b, c, d))
}
#[inline]
pub unsafe fn sc5(
    n: usize,
    a: usize,
    b: usize,
    c: usize,
    d: usize,
    e: usize,
) -> Result<usize, Errno> {
    r(syscall5(n, a, b, c, d, e))
}
#[inline]
pub unsafe fn sc6(
    n: usize,
    a: usize,
    b: usize,
    c: usize,
    d: usize,
    e: usize,
    f: usize,
) -> Result<usize, Errno> {
    r(syscall6(n, a, b, c, d, e, f))
}
