//! `target_os = "fullrust"`: a private, libc-free stand-in for the `libc`
//! crate, covering exactly what socket2's `sys/unix.rs` uses on Linux.
//!
//! fullrust is ABI-identical to `x86_64-unknown-linux-gnu` but links no libc,
//! so instead of forking socket2's backend we let upstream's own Linux code
//! (every `target_os = "linux"` branch) run on top of this module:
//!
//! * types are the Linux x86-64 (glibc) layouts, checked at compile time
//!   against sizes/alignments taken from the real `libc` crate;
//! * constants were generated from `libc` 0.2.189 on x86_64-unknown-linux-gnu;
//! * functions are raw `syscall` instructions. They return the kernel's
//!   `-errno` directly (there is no errno variable); the fullrust arm of
//!   socket2's `syscall!` macro turns a negative result into
//!   `io::Error::from_raw_os_error`. They are only ever called through that
//!   macro.
//!
//! Plus one fullrust-only extension (not in upstream socket2):
//! `Socket::peer_cred` (`SO_PEERCRED`).
#![allow(dead_code, non_camel_case_types, clippy::missing_safety_doc)]

use std::mem::{align_of, size_of};

// ---------------------------------------------------------------------------
// Primitive types (x86-64 Linux).
// ---------------------------------------------------------------------------

pub(crate) use std::ffi::{c_char, c_int, c_short, c_uint, c_ulong, c_ushort, c_void};
pub(crate) type c_ulonglong = u64;
pub(crate) type size_t = usize;
pub(crate) type ssize_t = isize;
pub(crate) type socklen_t = u32;
pub(crate) type sa_family_t = u16;
pub(crate) type in_addr_t = u32;
pub(crate) type in_port_t = u16;
pub(crate) type off_t = i64;
pub(crate) type time_t = i64;
pub(crate) type suseconds_t = i64;
pub(crate) type nfds_t = c_ulong;
pub(crate) type pid_t = i32;
pub(crate) type uid_t = u32;
pub(crate) type gid_t = u32;

// ---------------------------------------------------------------------------
// Structs: Linux x86-64 layouts, field names as in the `libc` crate.
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct in_addr {
    pub s_addr: in_addr_t,
}

#[repr(C)]
#[repr(align(4))]
#[derive(Copy, Clone)]
pub(crate) struct in6_addr {
    pub s6_addr: [u8; 16],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct sockaddr {
    pub sa_family: sa_family_t,
    pub sa_data: [c_char; 14],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct sockaddr_in {
    pub sin_family: sa_family_t,
    pub sin_port: in_port_t,
    pub sin_addr: in_addr,
    pub sin_zero: [u8; 8],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct sockaddr_in6 {
    pub sin6_family: sa_family_t,
    pub sin6_port: in_port_t,
    pub sin6_flowinfo: u32,
    pub sin6_addr: in6_addr,
    pub sin6_scope_id: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct sockaddr_un {
    pub sun_family: sa_family_t,
    pub sun_path: [c_char; 108],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct sockaddr_storage {
    pub ss_family: sa_family_t,
    __ss_pad2: [u8; 128 - 2 - 8],
    __ss_align: size_t,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct sockaddr_vm {
    pub svm_family: sa_family_t,
    pub svm_reserved1: c_ushort,
    pub svm_port: c_uint,
    pub svm_cid: c_uint,
    pub svm_zero: [u8; 4],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct iovec {
    pub iov_base: *mut c_void,
    pub iov_len: size_t,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct msghdr {
    pub msg_name: *mut c_void,
    pub msg_namelen: socklen_t,
    pub msg_iov: *mut iovec,
    pub msg_iovlen: size_t,
    pub msg_control: *mut c_void,
    pub msg_controllen: size_t,
    pub msg_flags: c_int,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct cmsghdr {
    pub cmsg_len: size_t,
    pub cmsg_level: c_int,
    pub cmsg_type: c_int,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct linger {
    pub l_onoff: c_int,
    pub l_linger: c_int,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct timeval {
    pub tv_sec: time_t,
    pub tv_usec: suseconds_t,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct pollfd {
    pub fd: c_int,
    pub events: c_short,
    pub revents: c_short,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct ip_mreq {
    pub imr_multiaddr: in_addr,
    pub imr_interface: in_addr,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct ip_mreqn {
    pub imr_multiaddr: in_addr,
    pub imr_address: in_addr,
    pub imr_ifindex: c_int,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct ip_mreq_source {
    pub imr_multiaddr: in_addr,
    pub imr_interface: in_addr,
    pub imr_sourceaddr: in_addr,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct ipv6_mreq {
    pub ipv6mr_multiaddr: in6_addr,
    pub ipv6mr_interface: c_uint,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct sock_filter {
    pub code: u16,
    pub jt: u8,
    pub jf: u8,
    pub k: u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct sock_fprog {
    pub len: c_ushort,
    pub filter: *mut sock_filter,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct ucred {
    pub pid: pid_t,
    pub uid: uid_t,
    pub gid: gid_t,
}

// ---------------------------------------------------------------------------
// Syscalls. Each returns the raw kernel result (`-errno` on failure).
// ---------------------------------------------------------------------------

mod nr {
    pub const POLL: usize = 7;
    pub const SENDFILE: usize = 40;
    pub const SOCKET: usize = 41;
    pub const CONNECT: usize = 42;
    pub const ACCEPT: usize = 43;
    pub const SENDTO: usize = 44;
    pub const RECVFROM: usize = 45;
    pub const SENDMSG: usize = 46;
    pub const RECVMSG: usize = 47;
    pub const SHUTDOWN: usize = 48;
    pub const BIND: usize = 49;
    pub const LISTEN: usize = 50;
    pub const GETSOCKNAME: usize = 51;
    pub const GETPEERNAME: usize = 52;
    pub const SOCKETPAIR: usize = 53;
    pub const SETSOCKOPT: usize = 54;
    pub const GETSOCKOPT: usize = 55;
    pub const FCNTL: usize = 72;
    pub const ACCEPT4: usize = 288;
}

#[inline]
unsafe fn sc(n: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> isize {
    let r: isize;
    // SAFETY: the caller upholds the syscall's own contract.
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") n as isize => r,
            in("rdi") a, in("rsi") b, in("rdx") c,
            in("r10") d, in("r8") e, in("r9") f,
            lateout("rcx") _, lateout("r11") _,
            options(nostack, preserves_flags)
        );
    }
    r
}

pub(crate) unsafe fn socket(domain: c_int, ty: c_int, protocol: c_int) -> c_int {
    unsafe { sc(nr::SOCKET, domain as usize, ty as usize, protocol as usize, 0, 0, 0) as c_int }
}
pub(crate) unsafe fn socketpair(domain: c_int, ty: c_int, protocol: c_int, sv: *mut c_int) -> c_int {
    unsafe {
        sc(nr::SOCKETPAIR, domain as usize, ty as usize, protocol as usize, sv as usize, 0, 0)
            as c_int
    }
}
pub(crate) unsafe fn bind(fd: c_int, addr: *const sockaddr, len: socklen_t) -> c_int {
    unsafe { sc(nr::BIND, fd as usize, addr as usize, len as usize, 0, 0, 0) as c_int }
}
pub(crate) unsafe fn connect(fd: c_int, addr: *const sockaddr, len: socklen_t) -> c_int {
    unsafe { sc(nr::CONNECT, fd as usize, addr as usize, len as usize, 0, 0, 0) as c_int }
}
pub(crate) unsafe fn listen(fd: c_int, backlog: c_int) -> c_int {
    unsafe { sc(nr::LISTEN, fd as usize, backlog as usize, 0, 0, 0, 0) as c_int }
}
pub(crate) unsafe fn accept(fd: c_int, addr: *mut sockaddr, len: *mut socklen_t) -> c_int {
    unsafe { sc(nr::ACCEPT, fd as usize, addr as usize, len as usize, 0, 0, 0) as c_int }
}
pub(crate) unsafe fn accept4(
    fd: c_int,
    addr: *mut sockaddr,
    len: *mut socklen_t,
    flags: c_int,
) -> c_int {
    unsafe {
        sc(nr::ACCEPT4, fd as usize, addr as usize, len as usize, flags as usize, 0, 0) as c_int
    }
}
pub(crate) unsafe fn getsockname(fd: c_int, addr: *mut sockaddr, len: *mut socklen_t) -> c_int {
    unsafe { sc(nr::GETSOCKNAME, fd as usize, addr as usize, len as usize, 0, 0, 0) as c_int }
}
pub(crate) unsafe fn getpeername(fd: c_int, addr: *mut sockaddr, len: *mut socklen_t) -> c_int {
    unsafe { sc(nr::GETPEERNAME, fd as usize, addr as usize, len as usize, 0, 0, 0) as c_int }
}
pub(crate) unsafe fn shutdown(fd: c_int, how: c_int) -> c_int {
    unsafe { sc(nr::SHUTDOWN, fd as usize, how as usize, 0, 0, 0, 0) as c_int }
}
pub(crate) unsafe fn getsockopt(
    fd: c_int,
    level: c_int,
    name: c_int,
    val: *mut c_void,
    len: *mut socklen_t,
) -> c_int {
    unsafe {
        sc(nr::GETSOCKOPT, fd as usize, level as usize, name as usize, val as usize, len as usize, 0)
            as c_int
    }
}
pub(crate) unsafe fn setsockopt(
    fd: c_int,
    level: c_int,
    name: c_int,
    val: *const c_void,
    len: socklen_t,
) -> c_int {
    unsafe {
        sc(nr::SETSOCKOPT, fd as usize, level as usize, name as usize, val as usize, len as usize, 0)
            as c_int
    }
}
pub(crate) unsafe fn recv(fd: c_int, buf: *mut c_void, len: size_t, flags: c_int) -> ssize_t {
    unsafe { recvfrom(fd, buf, len, flags, core::ptr::null_mut(), core::ptr::null_mut()) }
}
pub(crate) unsafe fn recvfrom(
    fd: c_int,
    buf: *mut c_void,
    len: size_t,
    flags: c_int,
    addr: *mut sockaddr,
    addrlen: *mut socklen_t,
) -> ssize_t {
    unsafe {
        sc(nr::RECVFROM, fd as usize, buf as usize, len, flags as usize, addr as usize, addrlen as usize)
    }
}
pub(crate) unsafe fn send(fd: c_int, buf: *const c_void, len: size_t, flags: c_int) -> ssize_t {
    unsafe { sendto(fd, buf, len, flags, core::ptr::null(), 0) }
}
pub(crate) unsafe fn sendto(
    fd: c_int,
    buf: *const c_void,
    len: size_t,
    flags: c_int,
    addr: *const sockaddr,
    addrlen: socklen_t,
) -> ssize_t {
    unsafe {
        sc(nr::SENDTO, fd as usize, buf as usize, len, flags as usize, addr as usize, addrlen as usize)
    }
}
pub(crate) unsafe fn recvmsg(fd: c_int, msg: *mut msghdr, flags: c_int) -> ssize_t {
    unsafe { sc(nr::RECVMSG, fd as usize, msg as usize, flags as usize, 0, 0, 0) }
}
pub(crate) unsafe fn sendmsg(fd: c_int, msg: *const msghdr, flags: c_int) -> ssize_t {
    unsafe { sc(nr::SENDMSG, fd as usize, msg as usize, flags as usize, 0, 0, 0) }
}
pub(crate) unsafe fn poll(fds: *mut pollfd, nfds: nfds_t, timeout: c_int) -> c_int {
    unsafe { sc(nr::POLL, fds as usize, nfds as usize, timeout as usize, 0, 0, 0) as c_int }
}
/// `fcntl(fd, cmd[, arg])`: C-variadic in libc; the fullrust `syscall!` arm
/// passes `0` for the two-argument form (the kernel ignores it).
pub(crate) unsafe fn fcntl(fd: c_int, cmd: c_int, arg: c_int) -> c_int {
    unsafe { sc(nr::FCNTL, fd as usize, cmd as usize, arg as usize, 0, 0, 0) as c_int }
}
pub(crate) unsafe fn sendfile(out_fd: c_int, in_fd: c_int, offset: *mut off_t, count: size_t) -> ssize_t {
    unsafe { sc(nr::SENDFILE, out_fd as usize, in_fd as usize, offset as usize, count, 0, 0) }
}

// ---------------------------------------------------------------------------
// fullrust-only extension: SO_PEERCRED.
// ---------------------------------------------------------------------------

/// Credentials of the peer process of a connected `AF_UNIX` socket, as read
/// with `SO_PEERCRED` (shape follows std's `std::os::unix::net::UCred`).
///
/// **fullrust extension** — upstream socket2 has no `peer_cred`; on Linux
/// callers use `libc::getsockopt(.., SO_PEERCRED, ..)`, which is unavailable
/// without libc.
#[cfg(feature = "all")]
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub struct UCred {
    /// The user ID of the peer process.
    pub uid: u32,
    /// The group ID of the peer process.
    pub gid: u32,
    /// The process ID of the peer process (always `Some` on Linux, as in std).
    pub pid: Option<i32>,
}

#[cfg(feature = "all")]
impl crate::Socket {
    /// Get the credentials of the peer process (`SO_PEERCRED`).
    ///
    /// **fullrust extension** (not part of upstream socket2's API). Valid on
    /// connected `AF_UNIX` stream/seqpacket sockets, including both ends of
    /// [`Socket::pair`](crate::Socket::pair).
    pub fn peer_cred(&self) -> std::io::Result<UCred> {
        use std::os::fd::AsRawFd;
        let mut cred = ucred { pid: 0, uid: 0, gid: 0 };
        let mut len = size_of::<ucred>() as socklen_t;
        let r = unsafe {
            getsockopt(
                self.as_raw_fd(),
                SOL_SOCKET,
                SO_PEERCRED,
                (&mut cred as *mut ucred).cast(),
                &mut len,
            )
        };
        if r < 0 {
            return Err(std::io::Error::from_raw_os_error(-r));
        }
        debug_assert_eq!(len as usize, size_of::<ucred>());
        Ok(UCred {
            uid: cred.uid,
            gid: cred.gid,
            pid: Some(cred.pid),
        })
    }
}

// ---------------------------------------------------------------------------
// Layout checks: size/alignment of each struct as reported by the real
// `libc` crate on x86_64-unknown-linux-gnu.
// ---------------------------------------------------------------------------

const _: () = assert!(size_of::<in6_addr>() == 16 && align_of::<in6_addr>() == 4);
const _: () = assert!(size_of::<in_addr>() == 4 && align_of::<in_addr>() == 4);
const _: () = assert!(size_of::<iovec>() == 16 && align_of::<iovec>() == 8);
const _: () = assert!(size_of::<ip_mreq>() == 8 && align_of::<ip_mreq>() == 4);
const _: () = assert!(size_of::<ip_mreq_source>() == 12 && align_of::<ip_mreq_source>() == 4);
const _: () = assert!(size_of::<ip_mreqn>() == 12 && align_of::<ip_mreqn>() == 4);
const _: () = assert!(size_of::<ipv6_mreq>() == 20 && align_of::<ipv6_mreq>() == 4);
const _: () = assert!(size_of::<linger>() == 8 && align_of::<linger>() == 4);
const _: () = assert!(size_of::<msghdr>() == 56 && align_of::<msghdr>() == 8);
const _: () = assert!(size_of::<pollfd>() == 8 && align_of::<pollfd>() == 4);
const _: () = assert!(size_of::<sock_filter>() == 8 && align_of::<sock_filter>() == 4);
const _: () = assert!(size_of::<sock_fprog>() == 16 && align_of::<sock_fprog>() == 8);
const _: () = assert!(size_of::<sockaddr>() == 16 && align_of::<sockaddr>() == 2);
const _: () = assert!(size_of::<sockaddr_in>() == 16 && align_of::<sockaddr_in>() == 4);
const _: () = assert!(size_of::<sockaddr_in6>() == 28 && align_of::<sockaddr_in6>() == 4);
const _: () = assert!(size_of::<sockaddr_storage>() == 128 && align_of::<sockaddr_storage>() == 8);
const _: () = assert!(size_of::<sockaddr_un>() == 110 && align_of::<sockaddr_un>() == 2);
const _: () = assert!(size_of::<sockaddr_vm>() == 16 && align_of::<sockaddr_vm>() == 4);
const _: () = assert!(size_of::<timeval>() == 16 && align_of::<timeval>() == 8);
const _: () = assert!(size_of::<ucred>() == 12 && align_of::<ucred>() == 4);

// ---------------------------------------------------------------------------
// Constants (generated from libc 0.2.189, x86_64-unknown-linux-gnu).
// ---------------------------------------------------------------------------

pub const AF_INET: c_int = 2;
pub const AF_INET6: c_int = 10;
pub const AF_PACKET: c_int = 17;
pub const AF_UNIX: c_int = 1;
pub const AF_UNSPEC: c_int = 0;
pub const AF_VSOCK: c_int = 40;
pub const DCCP_SOCKOPT_AVAILABLE_CCIDS: c_int = 12;
pub const DCCP_SOCKOPT_CCID: c_int = 13;
pub const DCCP_SOCKOPT_GET_CUR_MPS: c_int = 5;
pub const DCCP_SOCKOPT_QPOLICY_TXQLEN: c_int = 17;
pub const DCCP_SOCKOPT_RECV_CSCOV: c_int = 11;
pub const DCCP_SOCKOPT_RX_CCID: c_int = 15;
pub const DCCP_SOCKOPT_SEND_CSCOV: c_int = 10;
pub const DCCP_SOCKOPT_SERVER_TIMEWAIT: c_int = 6;
pub const DCCP_SOCKOPT_SERVICE: c_int = 2;
pub const DCCP_SOCKOPT_TX_CCID: c_int = 14;
pub const EINPROGRESS: c_int = 115;
pub const FD_CLOEXEC: c_int = 1;
pub const F_DUPFD_CLOEXEC: c_int = 1030;
pub const F_GETFD: c_int = 1;
pub const F_GETFL: c_int = 3;
pub const F_SETFD: c_int = 2;
pub const F_SETFL: c_int = 4;
pub const IFNAMSIZ: usize = 16;
pub const IP6T_SO_ORIGINAL_DST: c_int = 80;
pub const IPPROTO_DCCP: c_int = 33;
pub const IPPROTO_ICMP: c_int = 1;
pub const IPPROTO_ICMPV6: c_int = 58;
pub const IPPROTO_IP: c_int = 0;
pub const IPPROTO_IPV6: c_int = 41;
pub const IPPROTO_MPTCP: c_int = 262;
pub const IPPROTO_SCTP: c_int = 132;
pub const IPPROTO_TCP: c_int = 6;
pub const IPPROTO_UDP: c_int = 17;
pub const IPPROTO_UDPLITE: c_int = 136;
pub const IPV6_ADD_MEMBERSHIP: c_int = 20;
pub const IPV6_DROP_MEMBERSHIP: c_int = 21;
pub const IPV6_FREEBIND: c_int = 78;
pub const IPV6_HDRINCL: c_int = 36;
pub const IPV6_MULTICAST_ALL: c_int = 29;
pub const IPV6_MULTICAST_HOPS: c_int = 18;
pub const IPV6_MULTICAST_IF: c_int = 17;
pub const IPV6_MULTICAST_LOOP: c_int = 19;
pub const IPV6_RECVHOPLIMIT: c_int = 51;
pub const IPV6_RECVTCLASS: c_int = 66;
pub const IPV6_TCLASS: c_int = 67;
pub const IPV6_TRANSPARENT: c_int = 75;
pub const IPV6_UNICAST_HOPS: c_int = 16;
pub const IPV6_V6ONLY: c_int = 26;
pub const IP_ADD_MEMBERSHIP: c_int = 35;
pub const IP_ADD_SOURCE_MEMBERSHIP: c_int = 39;
pub const IP_DROP_MEMBERSHIP: c_int = 36;
pub const IP_DROP_SOURCE_MEMBERSHIP: c_int = 40;
pub const IP_FREEBIND: c_int = 15;
pub const IP_HDRINCL: c_int = 3;
pub const IP_MULTICAST_ALL: c_int = 49;
pub const IP_MULTICAST_IF: c_int = 32;
pub const IP_MULTICAST_LOOP: c_int = 34;
pub const IP_MULTICAST_TTL: c_int = 33;
pub const IP_RECVTOS: c_int = 13;
pub const IP_TOS: c_int = 1;
pub const IP_TRANSPARENT: c_int = 19;
pub const IP_TTL: c_int = 2;
pub const MSG_CONFIRM: c_int = 2048;
pub const MSG_DONTROUTE: c_int = 4;
pub const MSG_EOR: c_int = 128;
pub const MSG_OOB: c_int = 1;
pub const MSG_PEEK: c_int = 2;
pub const MSG_TRUNC: c_int = 32;
pub const O_NONBLOCK: c_int = 2048;
pub const POLLERR: c_short = 8;
pub const POLLHUP: c_short = 16;
pub const POLLIN: c_short = 1;
pub const POLLOUT: c_short = 4;
pub const SHUT_RD: c_int = 0;
pub const SHUT_RDWR: c_int = 2;
pub const SHUT_WR: c_int = 1;
pub const SOCK_CLOEXEC: c_int = 524288;
pub const SOCK_DCCP: c_int = 6;
pub const SOCK_DGRAM: c_int = 2;
pub const SOCK_NONBLOCK: c_int = 2048;
pub const SOCK_RAW: c_int = 3;
pub const SOCK_RDM: c_int = 4;
pub const SOCK_SEQPACKET: c_int = 5;
pub const SOCK_STREAM: c_int = 1;
pub const SOL_DCCP: c_int = 269;
pub const SOL_IP: c_int = 0;
pub const SOL_IPV6: c_int = 41;
pub const SOL_SOCKET: c_int = 1;
pub const SO_ACCEPTCONN: c_int = 30;
pub const SO_ATTACH_FILTER: c_int = 26;
pub const SO_BINDTODEVICE: c_int = 25;
pub const SO_BINDTOIFINDEX: c_int = 62;
pub const SO_BROADCAST: c_int = 6;
pub const SO_BUSY_POLL: c_int = 46;
pub const SO_COOKIE: c_int = 57;
pub const SO_DETACH_FILTER: c_int = 27;
pub const SO_DOMAIN: c_int = 39;
pub const SO_ERROR: c_int = 4;
pub const SO_INCOMING_CPU: c_int = 49;
pub const SO_KEEPALIVE: c_int = 9;
pub const SO_LINGER: c_int = 13;
pub const SO_MARK: c_int = 36;
pub const SO_OOBINLINE: c_int = 10;
pub const SO_ORIGINAL_DST: c_int = 80;
pub const SO_PASSCRED: c_int = 16;
pub const SO_PRIORITY: c_int = 12;
pub const SO_PROTOCOL: c_int = 38;
pub const SO_RCVBUF: c_int = 8;
pub const SO_RCVTIMEO: c_int = 20;
pub const SO_REUSEADDR: c_int = 2;
pub const SO_REUSEPORT: c_int = 15;
pub const SO_SNDBUF: c_int = 7;
pub const SO_SNDTIMEO: c_int = 21;
pub const SO_TYPE: c_int = 3;
pub const TCP_CONGESTION: c_int = 13;
pub const TCP_CORK: c_int = 3;
pub const TCP_KEEPCNT: c_int = 6;
pub const TCP_KEEPIDLE: c_int = 4;
pub const TCP_KEEPINTVL: c_int = 5;
pub const TCP_MAXSEG: c_int = 2;
pub const TCP_NODELAY: c_int = 1;
pub const TCP_NOTSENT_LOWAT: c_int = 25;
pub const TCP_QUICKACK: c_int = 12;
pub const TCP_THIN_LINEAR_TIMEOUTS: c_int = 16;
pub const TCP_USER_TIMEOUT: c_int = 18;
pub const SO_PEERCRED: c_int = 17;
