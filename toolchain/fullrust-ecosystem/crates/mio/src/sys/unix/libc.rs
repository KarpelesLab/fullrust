//! A private, minimal stand-in for the `libc` crate on the `fullrust` target.
//!
//! `fullrust` (`x86_64-unknown-linux-fullrust`) is the Linux kernel ABI with no
//! C library, so mio's ordinary Linux code paths (epoll selector, eventfd
//! waker, `net`/`tcp`/`udp`, `pipe`) are compiled unchanged and resolve their
//! `libc::…` paths to this module instead. It supplies exactly the types,
//! constants and functions those paths use, with Linux x86_64 layouts and
//! values, implemented as raw `syscall` instructions.
//!
//! One deliberate difference from libc: the functions return the **raw kernel
//! result** (`-errno` on failure) instead of `-1` + `errno`, since there is no
//! C `errno` for `io::Error::last_os_error()` to read. mio's `syscall!` macro
//! has a matching `fullrust` arm that maps negative results to
//! `io::Error::from_raw_os_error(-res)`; the only direct (non-`syscall!`) libc
//! calls on the Linux path, in `pipe.rs`, are routed through `syscall!` too.
#![allow(dead_code, non_camel_case_types, non_upper_case_globals, clippy::missing_safety_doc)]

use core::arch::asm;

// ---------------------------------------------------------------------------
// Primitive types
// ---------------------------------------------------------------------------

pub(crate) type c_int = core::ffi::c_int;
pub(crate) type c_uint = core::ffi::c_uint;
pub(crate) type c_char = core::ffi::c_char;
pub(crate) type c_void = core::ffi::c_void;
pub(crate) type c_ulong = core::ffi::c_ulong;
pub(crate) type size_t = usize;
pub(crate) type ssize_t = isize;
pub(crate) type socklen_t = u32;
pub(crate) type sa_family_t = u16;

// ---------------------------------------------------------------------------
// Structs (Linux x86_64 layouts)
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct in_addr {
    pub s_addr: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct in6_addr {
    pub s6_addr: [u8; 16],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct sockaddr {
    pub sa_family: sa_family_t,
    pub sa_data: [c_char; 14],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct sockaddr_in {
    pub sin_family: sa_family_t,
    pub sin_port: u16,
    pub sin_addr: in_addr,
    pub sin_zero: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct sockaddr_in6 {
    pub sin6_family: sa_family_t,
    pub sin6_port: u16,
    pub sin6_flowinfo: u32,
    pub sin6_addr: in6_addr,
    pub sin6_scope_id: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct sockaddr_storage {
    pub ss_family: sa_family_t,
    __ss_pad1: [u8; 6],
    __ss_align: u64,
    __ss_pad2: [u8; 112],
}

/// `struct epoll_event`. On x86_64 the kernel declares it
/// `__attribute__((packed))` (12 bytes), unlike every other architecture.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub(crate) struct epoll_event {
    pub events: u32,
    pub u64: u64,
}

// Compile-time layout checks against the kernel ABI.
const _: () = {
    assert!(core::mem::size_of::<epoll_event>() == 12);
    assert!(core::mem::size_of::<sockaddr>() == 16);
    assert!(core::mem::size_of::<sockaddr_in>() == 16);
    assert!(core::mem::size_of::<sockaddr_in6>() == 28);
    assert!(core::mem::size_of::<sockaddr_storage>() == 128);
    assert!(core::mem::align_of::<sockaddr_storage>() == 8);
};

// ---------------------------------------------------------------------------
// Constants (Linux x86_64 values)
// ---------------------------------------------------------------------------

// errno
pub(crate) const EINTR: c_int = 4;
pub(crate) const EAGAIN: c_int = 11;
pub(crate) const EINPROGRESS: c_int = 115;

// socket domains / types / options
pub(crate) const AF_UNSPEC: c_int = 0;
pub(crate) const AF_UNIX: c_int = 1;
pub(crate) const AF_INET: c_int = 2;
pub(crate) const AF_INET6: c_int = 10;
pub(crate) const SOCK_STREAM: c_int = 1;
pub(crate) const SOCK_DGRAM: c_int = 2;
pub(crate) const SOCK_NONBLOCK: c_int = 0o4000;
pub(crate) const SOCK_CLOEXEC: c_int = 0o2000000;
pub(crate) const SOL_SOCKET: c_int = 1;
pub(crate) const SO_REUSEADDR: c_int = 2;
pub(crate) const IPPROTO_IPV6: c_int = 41;
pub(crate) const IPV6_V6ONLY: c_int = 26;
pub(crate) const SOMAXCONN: c_int = 4096;

// fcntl / ioctl / open flags
pub(crate) const F_SETFD: c_int = 2;
pub(crate) const F_GETFL: c_int = 3;
pub(crate) const F_SETFL: c_int = 4;
pub(crate) const FD_CLOEXEC: c_int = 1;
pub(crate) const O_NONBLOCK: c_int = 0o4000;
pub(crate) const O_CLOEXEC: c_int = 0o2000000;
pub(crate) const FIONBIO: c_ulong = 0x5421;

// eventfd
pub(crate) const EFD_NONBLOCK: c_int = 0o4000;
pub(crate) const EFD_CLOEXEC: c_int = 0o2000000;

// epoll
pub(crate) const EPOLL_CLOEXEC: c_int = 0o2000000;
pub(crate) const EPOLL_CTL_ADD: c_int = 1;
pub(crate) const EPOLL_CTL_DEL: c_int = 2;
pub(crate) const EPOLL_CTL_MOD: c_int = 3;
pub(crate) const EPOLLIN: c_int = 0x1;
pub(crate) const EPOLLPRI: c_int = 0x2;
pub(crate) const EPOLLOUT: c_int = 0x4;
pub(crate) const EPOLLERR: c_int = 0x8;
pub(crate) const EPOLLHUP: c_int = 0x10;
pub(crate) const EPOLLRDNORM: c_int = 0x40;
pub(crate) const EPOLLRDBAND: c_int = 0x80;
pub(crate) const EPOLLWRNORM: c_int = 0x100;
pub(crate) const EPOLLWRBAND: c_int = 0x200;
pub(crate) const EPOLLMSG: c_int = 0x400;
pub(crate) const EPOLLRDHUP: c_int = 0x2000;
pub(crate) const EPOLLEXCLUSIVE: c_int = 1 << 28;
pub(crate) const EPOLLWAKEUP: c_int = 1 << 29;
pub(crate) const EPOLLONESHOT: c_int = 1 << 30;
pub(crate) const EPOLLET: c_int = 1 << 31;

// ---------------------------------------------------------------------------
// Raw syscalls (x86_64: nr in rax, args rdi rsi rdx r10 r8 r9; rcx/r11 clobbered)
// ---------------------------------------------------------------------------

const NR_IOCTL: usize = 16;
const NR_CLOSE: usize = 3;
const NR_SOCKET: usize = 41;
const NR_CONNECT: usize = 42;
const NR_BIND: usize = 49;
const NR_LISTEN: usize = 50;
const NR_SOCKETPAIR: usize = 53;
const NR_SETSOCKOPT: usize = 54;
const NR_GETSOCKOPT: usize = 55;
const NR_FCNTL: usize = 72;
const NR_EPOLL_WAIT: usize = 232;
const NR_EPOLL_CTL: usize = 233;
const NR_ACCEPT4: usize = 288;
const NR_EVENTFD2: usize = 290;
const NR_EPOLL_CREATE1: usize = 291;
const NR_PIPE2: usize = 293;

#[inline]
unsafe fn sys1(n: usize, a: usize) -> isize {
    let r: isize;
    asm!("syscall", inlateout("rax") n as isize => r, in("rdi") a,
        lateout("rcx") _, lateout("r11") _, options(nostack, preserves_flags));
    r
}
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

// Integer arguments are sign-extended (`as isize as usize`) exactly as the C
// ABI would pass an `int` in a 64-bit register; the kernel truncates to `int`.
#[inline]
fn i(v: c_int) -> usize {
    v as isize as usize
}

// ---------------------------------------------------------------------------
// Functions: libc signatures, raw kernel return values (`-errno` on error)
// ---------------------------------------------------------------------------

pub(crate) unsafe fn close(fd: c_int) -> c_int {
    sys1(NR_CLOSE, i(fd)) as c_int
}

pub(crate) unsafe fn socket(domain: c_int, ty: c_int, protocol: c_int) -> c_int {
    sys3(NR_SOCKET, i(domain), i(ty), i(protocol)) as c_int
}

pub(crate) unsafe fn socketpair(domain: c_int, ty: c_int, protocol: c_int, sv: *mut c_int) -> c_int {
    sys4(NR_SOCKETPAIR, i(domain), i(ty), i(protocol), sv as usize) as c_int
}

pub(crate) unsafe fn bind(fd: c_int, addr: *const sockaddr, len: socklen_t) -> c_int {
    sys3(NR_BIND, i(fd), addr as usize, len as usize) as c_int
}

pub(crate) unsafe fn connect(fd: c_int, addr: *const sockaddr, len: socklen_t) -> c_int {
    sys3(NR_CONNECT, i(fd), addr as usize, len as usize) as c_int
}

pub(crate) unsafe fn listen(fd: c_int, backlog: c_int) -> c_int {
    sys2(NR_LISTEN, i(fd), i(backlog)) as c_int
}

pub(crate) unsafe fn accept4(
    fd: c_int,
    addr: *mut sockaddr,
    len: *mut socklen_t,
    flags: c_int,
) -> c_int {
    sys4(NR_ACCEPT4, i(fd), addr as usize, len as usize, i(flags)) as c_int
}

pub(crate) unsafe fn setsockopt(
    fd: c_int,
    level: c_int,
    name: c_int,
    value: *const c_void,
    len: socklen_t,
) -> c_int {
    sys5(NR_SETSOCKOPT, i(fd), i(level), i(name), value as usize, len as usize) as c_int
}

pub(crate) unsafe fn getsockopt(
    fd: c_int,
    level: c_int,
    name: c_int,
    value: *mut c_void,
    len: *mut socklen_t,
) -> c_int {
    sys5(NR_GETSOCKOPT, i(fd), i(level), i(name), value as usize, len as usize) as c_int
}

/// Three-argument `fcntl(fd, cmd, arg)` (all mio uses on Linux-ABI paths).
pub(crate) unsafe fn fcntl(fd: c_int, cmd: c_int, arg: c_int) -> c_int {
    sys3(NR_FCNTL, i(fd), i(cmd), i(arg)) as c_int
}

pub(crate) unsafe fn ioctl(fd: c_int, request: c_ulong, arg: *const c_int) -> c_int {
    sys3(NR_IOCTL, i(fd), request as usize, arg as usize) as c_int
}

pub(crate) unsafe fn pipe2(fds: *mut c_int, flags: c_int) -> c_int {
    sys2(NR_PIPE2, fds as usize, i(flags)) as c_int
}

pub(crate) unsafe fn eventfd(initval: c_uint, flags: c_int) -> c_int {
    sys2(NR_EVENTFD2, initval as usize, i(flags)) as c_int
}

pub(crate) unsafe fn epoll_create1(flags: c_int) -> c_int {
    sys1(NR_EPOLL_CREATE1, i(flags)) as c_int
}

pub(crate) unsafe fn epoll_ctl(epfd: c_int, op: c_int, fd: c_int, event: *mut epoll_event) -> c_int {
    sys4(NR_EPOLL_CTL, i(epfd), i(op), i(fd), event as usize) as c_int
}

pub(crate) unsafe fn epoll_wait(
    epfd: c_int,
    events: *mut epoll_event,
    maxevents: c_int,
    timeout: c_int,
) -> c_int {
    sys4(NR_EPOLL_WAIT, i(epfd), events as usize, i(maxevents), i(timeout)) as c_int
}
