//! socket2 backend for the `fullrust` target: raw Linux x86_64 syscalls, no
//! libc. The kernel ABI is identical to `x86_64-unknown-linux-gnu`, so this is
//! a straight port of the `unix.rs` logic with libc's thin syscall/type layer
//! replaced by inline `syscall` instructions and in-crate `repr(C)` structs.
//! It mirrors what `std::sys::net` already does on this target.
#![allow(dead_code, non_camel_case_types)]

use std::cmp::min;
use std::io::{self, IoSlice};
use std::marker::PhantomData;
use std::mem::{self, size_of, MaybeUninit};
use std::net::{Ipv4Addr, Ipv6Addr, Shutdown};
use std::num::NonZeroUsize;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
use std::os::fullrust::ffi::OsStrExt;
use std::path::Path;
use std::time::{Duration, Instant};
use std::{ptr, slice};

use crate::{MsgHdr, MsgHdrMut, RecvFlags, SockAddr, TcpKeepalive};

// ---------------------------------------------------------------------------
// Raw syscalls
// ---------------------------------------------------------------------------

// x86_64 Linux socket-related syscall numbers.
const NR_SENDFILE: usize = 40;
const NR_POLL: usize = 7;
const NR_SOCKET: usize = 41;
const NR_CONNECT: usize = 42;
const NR_SENDTO: usize = 44;
const NR_RECVFROM: usize = 45;
const NR_SENDMSG: usize = 46;
const NR_RECVMSG: usize = 47;
const NR_SHUTDOWN: usize = 48;
const NR_BIND: usize = 49;
const NR_LISTEN: usize = 50;
const NR_GETSOCKNAME: usize = 51;
const NR_GETPEERNAME: usize = 52;
const NR_SOCKETPAIR: usize = 53;
const NR_SETSOCKOPT: usize = 54;
const NR_GETSOCKOPT: usize = 55;
const NR_FCNTL: usize = 72;
const NR_ACCEPT4: usize = 288;

#[inline]
unsafe fn sys2(n: usize, a: usize, b: usize) -> isize {
    let r: isize;
    unsafe {
        core::arch::asm!("syscall", inlateout("rax") n as isize => r,
            in("rdi") a, in("rsi") b,
            lateout("rcx") _, lateout("r11") _, options(nostack, preserves_flags));
    }
    r
}
#[inline]
unsafe fn sys3(n: usize, a: usize, b: usize, c: usize) -> isize {
    let r: isize;
    unsafe {
        core::arch::asm!("syscall", inlateout("rax") n as isize => r,
            in("rdi") a, in("rsi") b, in("rdx") c,
            lateout("rcx") _, lateout("r11") _, options(nostack, preserves_flags));
    }
    r
}
#[inline]
unsafe fn sys4(n: usize, a: usize, b: usize, c: usize, d: usize) -> isize {
    let r: isize;
    unsafe {
        core::arch::asm!("syscall", inlateout("rax") n as isize => r,
            in("rdi") a, in("rsi") b, in("rdx") c, in("r10") d,
            lateout("rcx") _, lateout("r11") _, options(nostack, preserves_flags));
    }
    r
}
#[inline]
unsafe fn sys5(n: usize, a: usize, b: usize, c: usize, d: usize, e: usize) -> isize {
    let r: isize;
    unsafe {
        core::arch::asm!("syscall", inlateout("rax") n as isize => r,
            in("rdi") a, in("rsi") b, in("rdx") c, in("r10") d, in("r8") e,
            lateout("rcx") _, lateout("r11") _, options(nostack, preserves_flags));
    }
    r
}
#[inline]
unsafe fn sys6(n: usize, a: usize, b: usize, c: usize, d: usize, e: usize, f: usize) -> isize {
    let r: isize;
    unsafe {
        core::arch::asm!("syscall", inlateout("rax") n as isize => r,
            in("rdi") a, in("rsi") b, in("rdx") c, in("r10") d, in("r8") e, in("r9") f,
            lateout("rcx") _, lateout("r11") _, options(nostack, preserves_flags));
    }
    r
}

/// Turn a raw syscall return into `io::Result`: a negative value is `-errno`.
#[inline]
fn cvt(r: isize) -> io::Result<isize> {
    if r < 0 { Err(io::Error::from_raw_os_error(-r as i32)) } else { Ok(r) }
}

// ---------------------------------------------------------------------------
// Primitive types
// ---------------------------------------------------------------------------

pub(crate) type c_int = core::ffi::c_int; // i32
pub(crate) type Bool = c_int;
pub(crate) type Socket = c_int;
pub(crate) type sa_family_t = u16;
pub(crate) type socklen_t = u32;
type c_void = core::ffi::c_void;
type time_t = i64;
type suseconds_t = i64;
type c_char = i8;
type IovLen = usize;

const MAX_BUF_LEN: usize = isize::MAX as usize;

// ---------------------------------------------------------------------------
// repr(C) structs — Linux x86_64 layouts. Field names are load-bearing: the
// shared `sockaddr.rs`/`socket.rs` code writes/reads them by name.
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct in_addr {
    pub s_addr: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct in6_addr {
    pub s6_addr: [u8; 16],
}
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct sockaddr {
    pub sa_family: sa_family_t,
    pub sa_data: [c_char; 14],
}
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct sockaddr_in {
    pub sin_family: sa_family_t,
    pub sin_port: u16,
    pub sin_addr: in_addr,
    pub sin_zero: [u8; 8],
}
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct sockaddr_in6 {
    pub sin6_family: sa_family_t,
    pub sin6_port: u16,
    pub sin6_flowinfo: u32,
    pub sin6_addr: in6_addr,
    pub sin6_scope_id: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct sockaddr_un {
    pub sun_family: sa_family_t,
    pub sun_path: [c_char; 108],
}
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct sockaddr_storage {
    pub ss_family: sa_family_t,
    __ss_align: u64,
    __ss_pad: [u8; 128 - 16],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct iovec {
    pub iov_base: *mut c_void,
    pub iov_len: usize,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct msghdr {
    pub msg_name: *mut c_void,
    pub msg_namelen: socklen_t,
    pub msg_iov: *mut iovec,
    pub msg_iovlen: usize,
    pub msg_control: *mut c_void,
    pub msg_controllen: usize,
    pub msg_flags: c_int,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct linger {
    pub l_onoff: c_int,
    pub l_linger: c_int,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct ip_mreq {
    pub imr_multiaddr: in_addr,
    pub imr_interface: in_addr,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct ip_mreqn {
    pub imr_multiaddr: in_addr,
    pub imr_address: in_addr,
    pub imr_ifindex: c_int,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct ip_mreq_source {
    pub imr_multiaddr: in_addr,
    pub imr_interface: in_addr,
    pub imr_sourceaddr: in_addr,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct ipv6_mreq {
    pub ipv6mr_multiaddr: in6_addr,
    pub ipv6mr_interface: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct timeval {
    tv_sec: time_t,
    tv_usec: suseconds_t,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct pollfd {
    fd: c_int,
    events: i16,
    revents: i16,
}

// socket2 re-exports these libc names; give them the crate-local spellings.
pub(crate) use ip_mreq as IpMreq;
pub(crate) use ip_mreq_source as IpMreqSource;
pub(crate) use ipv6_mreq as Ipv6Mreq;

// ---------------------------------------------------------------------------
// Constants (Linux x86_64). Only the non-`feature = "all"` surface.
// ---------------------------------------------------------------------------

pub(crate) const AF_UNSPEC: c_int = 0;
pub(crate) const AF_UNIX: c_int = 1;
pub(crate) const AF_INET: c_int = 2;
pub(crate) const AF_INET6: c_int = 10;

pub(crate) const SOCK_STREAM: c_int = 1;
pub(crate) const SOCK_DGRAM: c_int = 2;
pub(crate) const SOCK_RAW: c_int = 3;
pub(crate) const SOCK_SEQPACKET: c_int = 5;
const SOCK_CLOEXEC: c_int = 0o2000000;
const SOCK_NONBLOCK: c_int = 0o4000;

pub(crate) const IPPROTO_IP: c_int = 0;
pub(crate) const IPPROTO_ICMP: c_int = 1;
pub(crate) const IPPROTO_TCP: c_int = 6;
pub(crate) const IPPROTO_UDP: c_int = 17;
pub(crate) const IPPROTO_IPV6: c_int = 41;
pub(crate) const IPPROTO_ICMPV6: c_int = 58;
pub(crate) const IPPROTO_MPTCP: c_int = 262;

pub(crate) const SOL_SOCKET: c_int = 1;
const SOL_IP: c_int = 0;
const SOL_IPV6: c_int = 41;

pub(crate) const SO_REUSEADDR: c_int = 2;
pub(crate) const SO_TYPE: c_int = 3;
pub(crate) const SO_ERROR: c_int = 4;
pub(crate) const SO_BROADCAST: c_int = 6;
pub(crate) const SO_SNDBUF: c_int = 7;
pub(crate) const SO_RCVBUF: c_int = 8;
pub(crate) const SO_KEEPALIVE: c_int = 9;
pub(crate) const SO_OOBINLINE: c_int = 10;
pub(crate) const SO_LINGER: c_int = 13;
pub(crate) const SO_RCVTIMEO: c_int = 20;
pub(crate) const SO_SNDTIMEO: c_int = 21;

pub(crate) const IP_TOS: c_int = 1;
pub(crate) const IP_TTL: c_int = 2;
pub(crate) const IP_HDRINCL: c_int = 3;
pub(crate) const IP_RECVTOS: c_int = 13;
pub(crate) const IP_MULTICAST_IF: c_int = 32;
pub(crate) const IP_MULTICAST_TTL: c_int = 33;
pub(crate) const IP_MULTICAST_LOOP: c_int = 34;
pub(crate) const IP_ADD_MEMBERSHIP: c_int = 35;
pub(crate) const IP_DROP_MEMBERSHIP: c_int = 36;
pub(crate) const IP_ADD_SOURCE_MEMBERSHIP: c_int = 39;
pub(crate) const IP_DROP_SOURCE_MEMBERSHIP: c_int = 40;

pub(crate) const IPV6_UNICAST_HOPS: c_int = 16;
pub(crate) const IPV6_MULTICAST_IF: c_int = 17;
pub(crate) const IPV6_MULTICAST_HOPS: c_int = 18;
pub(crate) const IPV6_MULTICAST_LOOP: c_int = 19;
pub(crate) const IPV6_ADD_MEMBERSHIP: c_int = 20;
pub(crate) const IPV6_DROP_MEMBERSHIP: c_int = 21;
pub(crate) const IPV6_V6ONLY: c_int = 26;
pub(crate) const IPV6_RECVHOPLIMIT: c_int = 51;
pub(crate) const IPV6_RECVTCLASS: c_int = 66;
pub(crate) const IPV6_TCLASS: c_int = 67;

pub(crate) const TCP_NODELAY: c_int = 1;
pub(crate) const TCP_MAXSEG: c_int = 2;
const KEEPALIVE_TIME: c_int = 4; // TCP_KEEPIDLE
pub(crate) const TCP_KEEPINTVL: c_int = 5;
pub(crate) const TCP_KEEPCNT: c_int = 6;
pub(crate) const TCP_USER_TIMEOUT: c_int = 18;

pub(crate) const MSG_OOB: c_int = 1;
pub(crate) const MSG_PEEK: c_int = 2;
pub(crate) const MSG_TRUNC: c_int = 0x20;
pub(crate) const MSG_EOR: c_int = 0x80;
pub(crate) const MSG_CONFIRM: c_int = 0x800;
pub(crate) const MSG_DONTROUTE: c_int = 4;

const F_GETFL: c_int = 3;
const F_SETFL: c_int = 4;
const F_DUPFD_CLOEXEC: c_int = 1030;
const O_NONBLOCK: c_int = 0o4000;

const POLLIN: i16 = 0x001;
const POLLOUT: i16 = 0x004;
const POLLERR: i16 = 0x008;
const POLLHUP: i16 = 0x010;

const SHUT_RD: c_int = 0;
const SHUT_WR: c_int = 1;
const SHUT_RDWR: c_int = 2;

// ---------------------------------------------------------------------------
// RecvFlags + MaybeUninitSlice
// ---------------------------------------------------------------------------

impl RecvFlags {
    /// Check if the message terminates a record (`MSG_EOR`).
    pub const fn is_end_of_record(self) -> bool {
        self.0 & MSG_EOR != 0
    }
    /// Check if the message contains out-of-band data (`MSG_OOB`).
    pub const fn is_out_of_band(self) -> bool {
        self.0 & MSG_OOB != 0
    }
}

// `Debug` for `Domain`/`Type`/`Protocol` is normally generated by the
// `impl_debug!` macro in the (libc-based) unix backend; provide equivalents
// naming the common Linux constants.
impl std::fmt::Debug for crate::Domain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self.0 {
            AF_INET => "AF_INET",
            AF_INET6 => "AF_INET6",
            AF_UNIX => "AF_UNIX",
            AF_UNSPEC => "AF_UNSPEC",
            n => return write!(f, "{n}"),
        };
        f.write_str(s)
    }
}
impl std::fmt::Debug for crate::Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self.0 {
            SOCK_STREAM => "SOCK_STREAM",
            SOCK_DGRAM => "SOCK_DGRAM",
            SOCK_RAW => "SOCK_RAW",
            SOCK_SEQPACKET => "SOCK_SEQPACKET",
            n => return write!(f, "{n}"),
        };
        f.write_str(s)
    }
}
impl std::fmt::Debug for crate::Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self.0 {
            IPPROTO_ICMP => "IPPROTO_ICMP",
            IPPROTO_ICMPV6 => "IPPROTO_ICMPV6",
            IPPROTO_TCP => "IPPROTO_TCP",
            IPPROTO_UDP => "IPPROTO_UDP",
            IPPROTO_MPTCP => "IPPROTO_MPTCP",
            n => return write!(f, "{n}"),
        };
        f.write_str(s)
    }
}

impl std::fmt::Debug for RecvFlags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecvFlags")
            .field("is_end_of_record", &self.is_end_of_record())
            .field("is_out_of_band", &self.is_out_of_band())
            .field("is_truncated", &self.is_truncated())
            .finish()
    }
}

#[repr(transparent)]
pub struct MaybeUninitSlice<'a> {
    vec: iovec,
    _lifetime: PhantomData<&'a mut [MaybeUninit<u8>]>,
}

unsafe impl<'a> Send for MaybeUninitSlice<'a> {}
unsafe impl<'a> Sync for MaybeUninitSlice<'a> {}

impl<'a> MaybeUninitSlice<'a> {
    pub(crate) fn new(buf: &'a mut [MaybeUninit<u8>]) -> MaybeUninitSlice<'a> {
        MaybeUninitSlice {
            vec: iovec { iov_base: buf.as_mut_ptr().cast(), iov_len: buf.len() },
            _lifetime: PhantomData,
        }
    }
    pub(crate) fn as_slice(&self) -> &[MaybeUninit<u8>] {
        unsafe { slice::from_raw_parts(self.vec.iov_base.cast(), self.vec.iov_len) }
    }
    pub(crate) fn as_mut_slice(&mut self) -> &mut [MaybeUninit<u8>] {
        unsafe { slice::from_raw_parts_mut(self.vec.iov_base.cast(), self.vec.iov_len) }
    }
}

// ---------------------------------------------------------------------------
// msghdr helpers
// ---------------------------------------------------------------------------

pub(crate) fn set_msghdr_name(msg: &mut msghdr, name: &SockAddr) {
    msg.msg_name = name.as_ptr() as *mut _;
    msg.msg_namelen = name.len();
}
pub(crate) fn set_msghdr_iov(msg: &mut msghdr, ptr: *mut iovec, len: usize) {
    msg.msg_iov = ptr;
    msg.msg_iovlen = min(len, IovLen::MAX) as IovLen;
}
pub(crate) fn set_msghdr_control(msg: &mut msghdr, ptr: *mut c_void, len: usize) {
    msg.msg_control = ptr;
    msg.msg_controllen = len as _;
}
pub(crate) fn set_msghdr_flags(msg: &mut msghdr, flags: c_int) {
    msg.msg_flags = flags;
}
pub(crate) fn msghdr_flags(msg: &msghdr) -> RecvFlags {
    RecvFlags(msg.msg_flags)
}
pub(crate) fn msghdr_control_len(msg: &msghdr) -> usize {
    msg.msg_controllen as _
}

// ---------------------------------------------------------------------------
// fd ownership glue (Inner = std::net::TcpStream)
// ---------------------------------------------------------------------------

pub(crate) unsafe fn socket_from_raw(socket: Socket) -> crate::socket::Inner {
    unsafe { crate::socket::Inner::from_raw_fd(socket) }
}
pub(crate) fn socket_as_raw(socket: &crate::socket::Inner) -> Socket {
    socket.as_raw_fd()
}
pub(crate) fn socket_into_raw(socket: crate::socket::Inner) -> Socket {
    socket.into_raw_fd()
}

// ---------------------------------------------------------------------------
// Core operations
// ---------------------------------------------------------------------------

pub(crate) fn socket(family: c_int, ty: c_int, protocol: c_int) -> io::Result<Socket> {
    let fd = cvt(unsafe { sys3(NR_SOCKET, family as usize, ty as usize, protocol as usize) })?;
    Ok(fd as Socket)
}

pub(crate) fn bind(fd: Socket, addr: &SockAddr) -> io::Result<()> {
    cvt(unsafe { sys3(NR_BIND, fd as usize, addr.as_ptr() as usize, addr.len() as usize) })?;
    Ok(())
}

pub(crate) fn connect(fd: Socket, addr: &SockAddr) -> io::Result<()> {
    cvt(unsafe { sys3(NR_CONNECT, fd as usize, addr.as_ptr() as usize, addr.len() as usize) })?;
    Ok(())
}

pub(crate) fn poll_connect(socket: &crate::Socket, timeout: Duration) -> io::Result<()> {
    let start = Instant::now();
    let mut pollfd = pollfd { fd: socket.as_raw(), events: POLLIN | POLLOUT, revents: 0 };
    loop {
        let elapsed = start.elapsed();
        if elapsed >= timeout {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let to = (timeout - elapsed).as_millis().clamp(1, c_int::MAX as u128) as c_int;
        match cvt(unsafe { sys3(NR_POLL, ptr::addr_of_mut!(pollfd) as usize, 1, to as usize) }) {
            Ok(0) => return Err(io::ErrorKind::TimedOut.into()),
            Ok(_) => {
                if (pollfd.revents & POLLHUP) != 0 || (pollfd.revents & POLLERR) != 0 {
                    match socket.take_error() {
                        Ok(Some(err)) | Err(err) => return Err(err),
                        Ok(None) => {
                            return Err(io::Error::new(
                                io::ErrorKind::Other,
                                "no error set after POLLHUP",
                            ))
                        }
                    }
                }
                return Ok(());
            }
            Err(ref err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        }
    }
}

pub(crate) fn listen(fd: Socket, backlog: c_int) -> io::Result<()> {
    cvt(unsafe { sys2(NR_LISTEN, fd as usize, backlog as usize) })?;
    Ok(())
}

pub(crate) fn accept(fd: Socket) -> io::Result<(Socket, SockAddr)> {
    // `accept4` with `SOCK_CLOEXEC` avoids leaking the fd across `exec`.
    unsafe {
        SockAddr::try_init(|storage, len| {
            let s = cvt(sys4(
                NR_ACCEPT4,
                fd as usize,
                storage as usize,
                len as usize,
                SOCK_CLOEXEC as usize,
            ))?;
            Ok(s as Socket)
        })
    }
}

pub(crate) fn getsockname(fd: Socket) -> io::Result<SockAddr> {
    unsafe {
        SockAddr::try_init(|storage, len| {
            cvt(sys3(NR_GETSOCKNAME, fd as usize, storage as usize, len as usize)).map(|_| ())
        })
    }
    .map(|(_, addr)| addr)
}

pub(crate) fn getpeername(fd: Socket) -> io::Result<SockAddr> {
    unsafe {
        SockAddr::try_init(|storage, len| {
            cvt(sys3(NR_GETPEERNAME, fd as usize, storage as usize, len as usize)).map(|_| ())
        })
    }
    .map(|(_, addr)| addr)
}

pub(crate) fn try_clone(fd: Socket) -> io::Result<Socket> {
    let n = cvt(unsafe { sys3(NR_FCNTL, fd as usize, F_DUPFD_CLOEXEC as usize, 0) })?;
    Ok(n as Socket)
}

pub(crate) fn set_nonblocking(fd: Socket, nonblocking: bool) -> io::Result<()> {
    let flags = cvt(unsafe { sys3(NR_FCNTL, fd as usize, F_GETFL as usize, 0) })? as c_int;
    let new = if nonblocking { flags | O_NONBLOCK } else { flags & !O_NONBLOCK };
    if new != flags {
        cvt(unsafe { sys3(NR_FCNTL, fd as usize, F_SETFL as usize, new as usize) })?;
    }
    Ok(())
}

pub(crate) fn shutdown(fd: Socket, how: Shutdown) -> io::Result<()> {
    let how = match how {
        Shutdown::Write => SHUT_WR,
        Shutdown::Read => SHUT_RD,
        Shutdown::Both => SHUT_RDWR,
    };
    cvt(unsafe { sys2(NR_SHUTDOWN, fd as usize, how as usize) })?;
    Ok(())
}

pub(crate) fn recv(fd: Socket, buf: &mut [MaybeUninit<u8>], flags: c_int) -> io::Result<usize> {
    // No `recv` syscall on x86_64 Linux; `recvfrom` with a null address is it.
    let n = cvt(unsafe {
        sys6(
            NR_RECVFROM,
            fd as usize,
            buf.as_mut_ptr() as usize,
            min(buf.len(), MAX_BUF_LEN),
            flags as usize,
            0,
            0,
        )
    })?;
    Ok(n as usize)
}

pub(crate) fn recv_from(
    fd: Socket,
    buf: &mut [MaybeUninit<u8>],
    flags: c_int,
) -> io::Result<(usize, SockAddr)> {
    unsafe {
        SockAddr::try_init(|addr, addrlen| {
            let n = cvt(sys6(
                NR_RECVFROM,
                fd as usize,
                buf.as_mut_ptr() as usize,
                min(buf.len(), MAX_BUF_LEN),
                flags as usize,
                addr as usize,
                addrlen as usize,
            ))?;
            Ok(n as usize)
        })
    }
}

pub(crate) fn peek_sender(fd: Socket) -> io::Result<SockAddr> {
    let (_, sender) = recv_from(fd, &mut [MaybeUninit::uninit(); 8], MSG_PEEK)?;
    Ok(sender)
}

pub(crate) fn recv_vectored(
    fd: Socket,
    bufs: &mut [crate::MaybeUninitSlice<'_>],
    flags: c_int,
) -> io::Result<(usize, RecvFlags)> {
    let mut msg = MsgHdrMut::new().with_buffers(bufs);
    let n = recvmsg(fd, &mut msg, flags)?;
    Ok((n, msg.flags()))
}

pub(crate) fn recv_from_vectored(
    fd: Socket,
    bufs: &mut [crate::MaybeUninitSlice<'_>],
    flags: c_int,
) -> io::Result<(usize, RecvFlags, SockAddr)> {
    let mut msg = MsgHdrMut::new().with_buffers(bufs);
    let (n, addr) = unsafe {
        SockAddr::try_init(|storage, len| {
            msg.inner.msg_name = storage.cast();
            msg.inner.msg_namelen = *len;
            let n = recvmsg(fd, &mut msg, flags)?;
            *len = msg.inner.msg_namelen;
            Ok(n)
        })?
    };
    Ok((n, msg.flags(), addr))
}

pub(crate) fn recvmsg(
    fd: Socket,
    msg: &mut MsgHdrMut<'_, '_, '_>,
    flags: c_int,
) -> io::Result<usize> {
    let n = cvt(unsafe {
        sys3(NR_RECVMSG, fd as usize, ptr::addr_of_mut!(msg.inner) as usize, flags as usize)
    })?;
    Ok(n as usize)
}

pub(crate) fn send(fd: Socket, buf: &[u8], flags: c_int) -> io::Result<usize> {
    let n = cvt(unsafe {
        sys6(
            NR_SENDTO,
            fd as usize,
            buf.as_ptr() as usize,
            min(buf.len(), MAX_BUF_LEN),
            flags as usize,
            0,
            0,
        )
    })?;
    Ok(n as usize)
}

pub(crate) fn send_vectored(fd: Socket, bufs: &[IoSlice<'_>], flags: c_int) -> io::Result<usize> {
    let msg = MsgHdr::new().with_buffers(bufs);
    sendmsg(fd, &msg, flags)
}

pub(crate) fn send_to(fd: Socket, buf: &[u8], addr: &SockAddr, flags: c_int) -> io::Result<usize> {
    let n = cvt(unsafe {
        sys6(
            NR_SENDTO,
            fd as usize,
            buf.as_ptr() as usize,
            min(buf.len(), MAX_BUF_LEN),
            flags as usize,
            addr.as_ptr() as usize,
            addr.len() as usize,
        )
    })?;
    Ok(n as usize)
}

pub(crate) fn send_to_vectored(
    fd: Socket,
    bufs: &[IoSlice<'_>],
    addr: &SockAddr,
    flags: c_int,
) -> io::Result<usize> {
    let msg = MsgHdr::new().with_addr(addr).with_buffers(bufs);
    sendmsg(fd, &msg, flags)
}

pub(crate) fn sendmsg(fd: Socket, msg: &MsgHdr<'_, '_, '_>, flags: c_int) -> io::Result<usize> {
    let n = cvt(unsafe {
        sys3(NR_SENDMSG, fd as usize, ptr::addr_of!(msg.inner) as usize, flags as usize)
    })?;
    Ok(n as usize)
}

// ---------------------------------------------------------------------------
// Socket options
// ---------------------------------------------------------------------------

/// Caller must ensure `T` is the correct type for `opt`/`val`.
pub(crate) unsafe fn getsockopt<T>(fd: Socket, opt: c_int, val: c_int) -> io::Result<T> {
    let mut payload: MaybeUninit<T> = MaybeUninit::uninit();
    let mut len = size_of::<T>() as socklen_t;
    cvt(unsafe {
        sys5(
            NR_GETSOCKOPT,
            fd as usize,
            opt as usize,
            val as usize,
            payload.as_mut_ptr() as usize,
            ptr::addr_of_mut!(len) as usize,
        )
    })?;
    debug_assert_eq!(len as usize, size_of::<T>());
    Ok(unsafe { payload.assume_init() })
}

/// Caller must ensure `T` is the correct type for `opt`/`val`.
pub(crate) unsafe fn setsockopt<T>(fd: Socket, opt: c_int, val: c_int, payload: T) -> io::Result<()> {
    cvt(unsafe {
        sys5(
            NR_SETSOCKOPT,
            fd as usize,
            opt as usize,
            val as usize,
            ptr::addr_of!(payload) as usize,
            size_of::<T>(),
        )
    })?;
    Ok(())
}

pub(crate) fn timeout_opt(fd: Socket, opt: c_int, val: c_int) -> io::Result<Option<Duration>> {
    unsafe { getsockopt(fd, opt, val).map(from_timeval) }
}

pub(crate) fn set_timeout_opt(
    fd: Socket,
    opt: c_int,
    val: c_int,
    duration: Option<Duration>,
) -> io::Result<()> {
    let duration = into_timeval(duration);
    unsafe { setsockopt(fd, opt, val, duration) }
}

const fn from_timeval(duration: timeval) -> Option<Duration> {
    if duration.tv_sec == 0 && duration.tv_usec == 0 {
        None
    } else {
        Some(Duration::new(duration.tv_sec as u64, (duration.tv_usec as u32) * 1000))
    }
}

fn into_timeval(duration: Option<Duration>) -> timeval {
    match duration {
        Some(duration) => timeval {
            tv_sec: min(duration.as_secs(), time_t::MAX as u64) as time_t,
            tv_usec: duration.subsec_micros() as suseconds_t,
        },
        None => timeval { tv_sec: 0, tv_usec: 0 },
    }
}

fn into_secs(duration: Duration) -> c_int {
    min(duration.as_secs(), c_int::MAX as u64) as c_int
}

pub(crate) fn set_tcp_keepalive(fd: Socket, keepalive: &TcpKeepalive) -> io::Result<()> {
    if let Some(time) = keepalive.time {
        let secs = into_secs(time);
        unsafe { setsockopt(fd, IPPROTO_TCP, KEEPALIVE_TIME, secs)? }
    }
    if let Some(interval) = keepalive.interval {
        let secs = into_secs(interval);
        unsafe { setsockopt(fd, IPPROTO_TCP, TCP_KEEPINTVL, secs)? }
    }
    if let Some(retries) = keepalive.retries {
        unsafe { setsockopt(fd, IPPROTO_TCP, TCP_KEEPCNT, retries as c_int)? }
    }
    Ok(())
}

pub(crate) fn nonblocking(fd: Socket) -> io::Result<bool> {
    let flags = cvt(unsafe { sys3(NR_FCNTL, fd as usize, F_GETFL as usize, 0) })? as c_int;
    Ok((flags & O_NONBLOCK) != 0)
}

pub(crate) fn keepalive_time(fd: Socket) -> io::Result<Duration> {
    unsafe {
        getsockopt::<c_int>(fd, IPPROTO_TCP, KEEPALIVE_TIME).map(|secs| Duration::from_secs(secs as u64))
    }
}

pub(crate) fn socketpair(family: c_int, ty: c_int, protocol: c_int) -> io::Result<[Socket; 2]> {
    let mut fds = [0 as Socket; 2];
    cvt(unsafe {
        sys4(
            NR_SOCKETPAIR,
            family as usize,
            ty as usize,
            protocol as usize,
            fds.as_mut_ptr() as usize,
        )
    })?;
    Ok(fds)
}

// ---------------------------------------------------------------------------
// Address conversions
// ---------------------------------------------------------------------------

pub(crate) const fn to_in_addr(addr: &Ipv4Addr) -> in_addr {
    in_addr { s_addr: u32::from_ne_bytes(addr.octets()) }
}
pub(crate) fn from_in_addr(in_addr: in_addr) -> Ipv4Addr {
    Ipv4Addr::from(in_addr.s_addr.to_ne_bytes())
}
pub(crate) const fn to_in6_addr(addr: &Ipv6Addr) -> in6_addr {
    in6_addr { s6_addr: addr.octets() }
}
pub(crate) fn from_in6_addr(addr: in6_addr) -> Ipv6Addr {
    Ipv6Addr::from(addr.s6_addr)
}

// ---------------------------------------------------------------------------
// Unix-domain addresses (AF_UNIX). `sun_path` sits right after the 2-byte
// `sun_family` on Linux, so its offset is a constant 2.
// ---------------------------------------------------------------------------

pub(crate) fn offset_of_path(storage: &sockaddr_un) -> usize {
    let base = storage as *const _ as usize;
    (ptr::addr_of!(storage.sun_path) as usize) - base
}

pub(crate) fn unix_sockaddr(path: &Path) -> io::Result<SockAddr> {
    // SAFETY: an all-zero `sockaddr_storage` is a valid (empty) address.
    let mut storage = unsafe { mem::zeroed::<sockaddr_storage>() };
    let len = {
        let storage = unsafe { &mut *ptr::addr_of_mut!(storage).cast::<sockaddr_un>() };
        let bytes = path.as_os_str().as_bytes();
        let too_long = match bytes.first() {
            None => false,
            // Linux abstract namespaces (leading NUL) aren't NUL-terminated.
            Some(&0) => bytes.len() > storage.sun_path.len(),
            Some(_) => bytes.len() >= storage.sun_path.len(),
        };
        if too_long {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path must be shorter than SUN_LEN",
            ));
        }
        storage.sun_family = AF_UNIX as sa_family_t;
        // SAFETY: non-overlapping; storage is zeroed so a pathname stays
        // NUL-terminated.
        unsafe {
            ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                storage.sun_path.as_mut_ptr().cast(),
                bytes.len(),
            );
        }
        offset_of_path(storage)
            + bytes.len()
            + match bytes.first() {
                Some(&0) | None => 0,
                Some(_) => 1,
            }
    };
    Ok(unsafe { SockAddr::new(storage, len as socklen_t) })
}

pub(crate) const fn to_mreqn(
    multiaddr: &Ipv4Addr,
    interface: &crate::socket::InterfaceIndexOrAddress,
) -> ip_mreqn {
    match interface {
        crate::socket::InterfaceIndexOrAddress::Index(interface) => ip_mreqn {
            imr_multiaddr: to_in_addr(multiaddr),
            imr_address: to_in_addr(&Ipv4Addr::UNSPECIFIED),
            imr_ifindex: *interface as _,
        },
        crate::socket::InterfaceIndexOrAddress::Address(interface) => ip_mreqn {
            imr_multiaddr: to_in_addr(multiaddr),
            imr_address: to_in_addr(interface),
            imr_ifindex: 0,
        },
    }
}

// ---------------------------------------------------------------------------
// Platform methods socket2 defines in its `sys/unix.rs` `impl Socket` block.
// fullrust uses this file instead of unix.rs, so the ones we support live here.
// ---------------------------------------------------------------------------

#[cfg(feature = "all")]
impl crate::Socket {
    /// Copies data between a `file` and this socket using the `sendfile(2)`
    /// syscall (zero-copy), starting at `offset` in the file. `length` bounds
    /// the number of bytes; `None` sends as many as the kernel allows in one
    /// call. Returns the number of bytes sent.
    pub fn sendfile<F>(
        &self,
        file: &F,
        offset: usize,
        length: Option<NonZeroUsize>,
    ) -> io::Result<usize>
    where
        F: AsRawFd,
    {
        // Linux `sendfile(out_fd, in_fd, offset*, count)`. A null offset would
        // advance the file's own position; we pass a mutable one like socket2.
        let count = match length {
            Some(n) => n.get(),
            // The most the Linux kernel writes in a single call.
            None => 0x7fff_f000,
        };
        let mut offset = offset as i64; // loff_t
        let n = cvt(unsafe {
            sys4(
                NR_SENDFILE,
                self.as_raw() as usize,
                file.as_raw_fd() as usize,
                ptr::addr_of_mut!(offset) as usize,
                count,
            )
        })?;
        Ok(n as usize)
    }
}

// ---------------------------------------------------------------------------
// `std::os::fd` interop for `crate::Socket`
// ---------------------------------------------------------------------------

impl AsFd for crate::Socket {
    fn as_fd(&self) -> BorrowedFd<'_> {
        unsafe { BorrowedFd::borrow_raw(self.as_raw()) }
    }
}
impl AsRawFd for crate::Socket {
    fn as_raw_fd(&self) -> RawFd {
        self.as_raw()
    }
}
impl IntoRawFd for crate::Socket {
    fn into_raw_fd(self) -> RawFd {
        self.into_raw()
    }
}
impl FromRawFd for crate::Socket {
    unsafe fn from_raw_fd(fd: RawFd) -> crate::Socket {
        crate::Socket::from_raw(fd)
    }
}
impl From<crate::Socket> for OwnedFd {
    fn from(sock: crate::Socket) -> OwnedFd {
        unsafe { OwnedFd::from_raw_fd(sock.into_raw()) }
    }
}
impl From<OwnedFd> for crate::Socket {
    fn from(fd: OwnedFd) -> crate::Socket {
        unsafe { crate::Socket::from_raw_fd(fd.into_raw_fd()) }
    }
}
