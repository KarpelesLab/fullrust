//! A subset of `std::net`: address types plus TCP/UDP over raw sockets.
//!
//! Hostname resolution (DNS) is not yet wired up — `ToSocketAddrs` accepts IP
//! literals and `(ip, port)` pairs. Real DNS arrives with the networking
//! command work.

use crate::io::{self, Read, Write};
use crate::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, RawFd};
use crate::sys::{self, Errno};
use crate::time::Duration;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

const AF_INET: u16 = 2;
const AF_INET6: u16 = 10;
const SOCK_STREAM: usize = 1;
const SOCK_DGRAM: usize = 2;
const SOCK_CLOEXEC: usize = 0o2000000;
const SOL_SOCKET: usize = 1;
const SO_REUSEADDR: usize = 2;
const SO_RCVTIMEO: usize = 20;
const SO_SNDTIMEO: usize = 21;
const IPPROTO_TCP: usize = 6;
const TCP_NODELAY: usize = 1;

fn e<T>(r: Result<T, Errno>) -> io::Result<T> {
    r.map_err(io::Error::from)
}

// ---- IP addresses ----

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ipv4Addr {
    octets: [u8; 4],
}
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ipv6Addr {
    octets: [u8; 16],
}
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IpAddr {
    V4(Ipv4Addr),
    V6(Ipv6Addr),
}

impl Ipv4Addr {
    pub const LOCALHOST: Ipv4Addr = Ipv4Addr { octets: [127, 0, 0, 1] };
    pub const UNSPECIFIED: Ipv4Addr = Ipv4Addr { octets: [0, 0, 0, 0] };

    pub const fn new(a: u8, b: u8, c: u8, d: u8) -> Ipv4Addr {
        Ipv4Addr { octets: [a, b, c, d] }
    }
    pub const fn octets(&self) -> [u8; 4] {
        self.octets
    }
    pub fn is_loopback(&self) -> bool {
        self.octets[0] == 127
    }
    pub fn is_unspecified(&self) -> bool {
        self.octets == [0, 0, 0, 0]
    }
    /// Map this IPv4 address into the IPv4-mapped IPv6 space (`::ffff:a.b.c.d`).
    pub fn to_ipv6_mapped(&self) -> Ipv6Addr {
        let o = self.octets;
        Ipv6Addr { octets: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, o[0], o[1], o[2], o[3]] }
    }
}

impl Ipv6Addr {
    pub const LOCALHOST: Ipv6Addr = Ipv6Addr { octets: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1] };
    pub const UNSPECIFIED: Ipv6Addr = Ipv6Addr { octets: [0; 16] };

    pub const fn from_octets(octets: [u8; 16]) -> Ipv6Addr {
        Ipv6Addr { octets }
    }
    pub const fn octets(&self) -> [u8; 16] {
        self.octets
    }
    pub fn segments(&self) -> [u16; 8] {
        let o = &self.octets;
        let mut s = [0u16; 8];
        let mut i = 0;
        while i < 8 {
            s[i] = ((o[2 * i] as u16) << 8) | o[2 * i + 1] as u16;
            i += 1;
        }
        s
    }
}

impl IpAddr {
    pub fn is_ipv4(&self) -> bool {
        matches!(self, IpAddr::V4(_))
    }
    pub fn is_ipv6(&self) -> bool {
        matches!(self, IpAddr::V6(_))
    }
    pub fn is_loopback(&self) -> bool {
        match self {
            IpAddr::V4(a) => a.is_loopback(),
            IpAddr::V6(a) => a.octets() == Ipv6Addr::LOCALHOST.octets(),
        }
    }
}

impl From<[u8; 16]> for Ipv6Addr {
    fn from(o: [u8; 16]) -> Ipv6Addr {
        Ipv6Addr { octets: o }
    }
}
impl From<[u8; 4]> for Ipv4Addr {
    fn from(o: [u8; 4]) -> Ipv4Addr {
        Ipv4Addr { octets: o }
    }
}

impl fmt::Display for Ipv4Addr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let o = self.octets;
        write!(f, "{}.{}.{}.{}", o[0], o[1], o[2], o[3])
    }
}
impl fmt::Debug for Ipv4Addr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl fmt::Display for Ipv6Addr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self.segments();
        write!(
            f,
            "{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}",
            s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]
        )
    }
}
impl fmt::Debug for Ipv6Addr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl fmt::Display for IpAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IpAddr::V4(a) => fmt::Display::fmt(a, f),
            IpAddr::V6(a) => fmt::Display::fmt(a, f),
        }
    }
}
impl fmt::Debug for IpAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl core::str::FromStr for Ipv4Addr {
    type Err = AddrParseError;
    fn from_str(s: &str) -> Result<Ipv4Addr, AddrParseError> {
        let mut octets = [0u8; 4];
        let mut parts = s.split('.');
        for o in octets.iter_mut() {
            let p = parts.next().ok_or(AddrParseError)?;
            *o = p.parse().map_err(|_| AddrParseError)?;
        }
        if parts.next().is_some() {
            return Err(AddrParseError);
        }
        Ok(Ipv4Addr { octets })
    }
}

impl core::str::FromStr for Ipv6Addr {
    type Err = AddrParseError;
    fn from_str(s: &str) -> Result<Ipv6Addr, AddrParseError> {
        parse_ipv6(s).ok_or(AddrParseError)
    }
}
impl core::str::FromStr for IpAddr {
    type Err = AddrParseError;
    fn from_str(s: &str) -> Result<IpAddr, AddrParseError> {
        if let Ok(v4) = s.parse::<Ipv4Addr>() {
            return Ok(IpAddr::V4(v4));
        }
        s.parse::<Ipv6Addr>().map(IpAddr::V6)
    }
}

fn parse_ipv6(s: &str) -> Option<Ipv6Addr> {
    // Handles the common forms including a single "::" elision.
    let (head, tail) = match s.split_once("::") {
        Some((h, t)) => (h, Some(t)),
        None => (s, None),
    };
    fn segs(part: &str) -> Option<Vec<u16>> {
        if part.is_empty() {
            return Some(Vec::new());
        }
        let mut out = Vec::new();
        for g in part.split(':') {
            out.push(u16::from_str_radix(g, 16).ok()?);
        }
        Some(out)
    }
    let head_s = segs(head)?;
    let mut all = [0u16; 8];
    match tail {
        None => {
            if head_s.len() != 8 {
                return None;
            }
            all.copy_from_slice(&head_s);
        }
        Some(t) => {
            let tail_s = segs(t)?;
            if head_s.len() + tail_s.len() > 7 {
                return None;
            }
            for (i, v) in head_s.iter().enumerate() {
                all[i] = *v;
            }
            for (i, v) in tail_s.iter().rev().enumerate() {
                all[7 - i] = *v;
            }
        }
    }
    let mut octets = [0u8; 16];
    for i in 0..8 {
        octets[2 * i] = (all[i] >> 8) as u8;
        octets[2 * i + 1] = all[i] as u8;
    }
    Some(Ipv6Addr { octets })
}

/// Error parsing an IP or socket address.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AddrParseError;
impl fmt::Display for AddrParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid IP address syntax")
    }
}
impl core::error::Error for AddrParseError {}

// ---- socket addresses ----

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SocketAddrV4 {
    ip: Ipv4Addr,
    port: u16,
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SocketAddrV6 {
    ip: Ipv6Addr,
    port: u16,
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum SocketAddr {
    V4(SocketAddrV4),
    V6(SocketAddrV6),
}

impl SocketAddrV4 {
    pub fn new(ip: Ipv4Addr, port: u16) -> SocketAddrV4 {
        SocketAddrV4 { ip, port }
    }
    pub fn ip(&self) -> &Ipv4Addr {
        &self.ip
    }
    pub fn port(&self) -> u16 {
        self.port
    }
}
impl SocketAddrV6 {
    pub fn new(ip: Ipv6Addr, port: u16) -> SocketAddrV6 {
        SocketAddrV6 { ip, port }
    }
    pub fn ip(&self) -> &Ipv6Addr {
        &self.ip
    }
    pub fn port(&self) -> u16 {
        self.port
    }
}
impl SocketAddr {
    pub fn new(ip: IpAddr, port: u16) -> SocketAddr {
        match ip {
            IpAddr::V4(a) => SocketAddr::V4(SocketAddrV4::new(a, port)),
            IpAddr::V6(a) => SocketAddr::V6(SocketAddrV6::new(a, port)),
        }
    }
    pub fn ip(&self) -> IpAddr {
        match self {
            SocketAddr::V4(a) => IpAddr::V4(a.ip),
            SocketAddr::V6(a) => IpAddr::V6(a.ip),
        }
    }
    pub fn port(&self) -> u16 {
        match self {
            SocketAddr::V4(a) => a.port,
            SocketAddr::V6(a) => a.port,
        }
    }
    pub fn is_ipv4(&self) -> bool {
        matches!(self, SocketAddr::V4(_))
    }
    pub fn is_ipv6(&self) -> bool {
        matches!(self, SocketAddr::V6(_))
    }
}

impl fmt::Display for SocketAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SocketAddr::V4(a) => write!(f, "{}:{}", a.ip, a.port),
            SocketAddr::V6(a) => write!(f, "[{}]:{}", a.ip, a.port),
        }
    }
}
impl fmt::Debug for SocketAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl From<SocketAddrV4> for SocketAddr {
    fn from(a: SocketAddrV4) -> SocketAddr {
        SocketAddr::V4(a)
    }
}
impl From<SocketAddrV6> for SocketAddr {
    fn from(a: SocketAddrV6) -> SocketAddr {
        SocketAddr::V6(a)
    }
}
impl From<(IpAddr, u16)> for SocketAddr {
    fn from((ip, port): (IpAddr, u16)) -> SocketAddr {
        SocketAddr::new(ip, port)
    }
}

impl core::str::FromStr for SocketAddr {
    type Err = AddrParseError;
    fn from_str(s: &str) -> Result<SocketAddr, AddrParseError> {
        if let Some(rest) = s.strip_prefix('[') {
            let (ip, port) = rest.split_once("]:").ok_or(AddrParseError)?;
            let ip: Ipv6Addr = ip.parse()?;
            let port: u16 = port.parse().map_err(|_| AddrParseError)?;
            return Ok(SocketAddr::V6(SocketAddrV6::new(ip, port)));
        }
        let (ip, port) = s.rsplit_once(':').ok_or(AddrParseError)?;
        let ip: Ipv4Addr = ip.parse()?;
        let port: u16 = port.parse().map_err(|_| AddrParseError)?;
        Ok(SocketAddr::V4(SocketAddrV4::new(ip, port)))
    }
}

// ---- ToSocketAddrs ----

pub trait ToSocketAddrs {
    type Iter: Iterator<Item = SocketAddr>;
    fn to_socket_addrs(&self) -> io::Result<Self::Iter>;
}

impl ToSocketAddrs for SocketAddr {
    type Iter = core::option::IntoIter<SocketAddr>;
    fn to_socket_addrs(&self) -> io::Result<Self::Iter> {
        Ok(Some(*self).into_iter())
    }
}
impl ToSocketAddrs for SocketAddrV4 {
    type Iter = core::option::IntoIter<SocketAddr>;
    fn to_socket_addrs(&self) -> io::Result<Self::Iter> {
        Ok(Some(SocketAddr::V4(*self)).into_iter())
    }
}
impl ToSocketAddrs for SocketAddrV6 {
    type Iter = core::option::IntoIter<SocketAddr>;
    fn to_socket_addrs(&self) -> io::Result<Self::Iter> {
        Ok(Some(SocketAddr::V6(*self)).into_iter())
    }
}
impl ToSocketAddrs for (IpAddr, u16) {
    type Iter = core::option::IntoIter<SocketAddr>;
    fn to_socket_addrs(&self) -> io::Result<Self::Iter> {
        Ok(Some(SocketAddr::new(self.0, self.1)).into_iter())
    }
}
impl ToSocketAddrs for (Ipv4Addr, u16) {
    type Iter = core::option::IntoIter<SocketAddr>;
    fn to_socket_addrs(&self) -> io::Result<Self::Iter> {
        Ok(Some(SocketAddr::V4(SocketAddrV4::new(self.0, self.1))).into_iter())
    }
}
impl ToSocketAddrs for str {
    type Iter = alloc::vec::IntoIter<SocketAddr>;
    fn to_socket_addrs(&self) -> io::Result<Self::Iter> {
        resolve_str(self)
    }
}
impl ToSocketAddrs for String {
    type Iter = alloc::vec::IntoIter<SocketAddr>;
    fn to_socket_addrs(&self) -> io::Result<Self::Iter> {
        resolve_str(self)
    }
}
impl ToSocketAddrs for (&str, u16) {
    type Iter = alloc::vec::IntoIter<SocketAddr>;
    fn to_socket_addrs(&self) -> io::Result<Self::Iter> {
        let ips = dns::lookup_host(self.0)?;
        Ok(ips.into_iter().map(|ip| SocketAddr::new(ip, self.1)).collect::<Vec<_>>().into_iter())
    }
}
impl<T: ToSocketAddrs + ?Sized> ToSocketAddrs for &T {
    type Iter = T::Iter;
    fn to_socket_addrs(&self) -> io::Result<Self::Iter> {
        (**self).to_socket_addrs()
    }
}

fn resolve_str(s: &str) -> io::Result<alloc::vec::IntoIter<SocketAddr>> {
    if let Ok(addr) = s.parse::<SocketAddr>() {
        return Ok(vec![addr].into_iter());
    }
    // "host:port" form — split off the port, resolve the host.
    let (host, port) = if let Some(rest) = s.strip_prefix('[') {
        let (h, p) = rest.split_once("]:").ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid socket address")
        })?;
        (h, p)
    } else {
        s.rsplit_once(':')
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing port"))?
    };
    let port: u16 = port
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid port"))?;
    let ips = dns::lookup_host(host)?;
    Ok(ips.into_iter().map(|ip| SocketAddr::new(ip, port)).collect::<Vec<_>>().into_iter())
}

/// Minimal hostname resolution: IP literal → `/etc/hosts` → DNS over UDP.
mod dns {
    use super::{IpAddr, Ipv4Addr, Ipv6Addr, UdpSocket};
    use crate::io;
    use crate::sys;
    use alloc::vec;
    use alloc::vec::Vec;

    pub fn lookup_host(host: &str) -> io::Result<Vec<IpAddr>> {
        if let Ok(ip) = host.parse::<IpAddr>() {
            return Ok(vec![ip]);
        }
        if let Some(ips) = hosts_file(host) {
            if !ips.is_empty() {
                return Ok(ips);
            }
        }
        let server = resolv_conf_nameserver()
            .unwrap_or(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 53)));
        let mut out = Vec::new();
        // A (IPv4) then AAAA (IPv6).
        if let Ok(mut v) = query(server, host, 1) {
            out.append(&mut v);
        }
        if let Ok(mut v) = query(server, host, 28) {
            out.append(&mut v);
        }
        if out.is_empty() {
            Err(io::Error::new(io::ErrorKind::NotFound, "name resolution failed"))
        } else {
            Ok(out)
        }
    }

    fn hosts_file(host: &str) -> Option<Vec<IpAddr>> {
        let text = crate::fs::read_to_string("/etc/hosts").ok()?;
        let mut out = Vec::new();
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("");
            let mut it = line.split_whitespace();
            let ip = match it.next() {
                Some(ip) => ip,
                None => continue,
            };
            if it.any(|name| name.eq_ignore_ascii_case(host)) {
                if let Ok(ip) = ip.parse::<IpAddr>() {
                    out.push(ip);
                }
            }
        }
        Some(out)
    }

    fn resolv_conf_nameserver() -> Option<IpAddr> {
        let text = crate::fs::read_to_string("/etc/resolv.conf").ok()?;
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if let Some(rest) = line.strip_prefix("nameserver") {
                if let Ok(ip) = rest.trim().parse::<IpAddr>() {
                    return Some(ip);
                }
            }
        }
        None
    }

    fn query(server: IpAddr, host: &str, qtype: u16) -> io::Result<Vec<IpAddr>> {
        let mut id = [0u8; 2];
        unsafe {
            let _ = sys::sc3(sys::nr::GETRANDOM, id.as_mut_ptr() as usize, 2, 0);
        }

        let mut pkt: Vec<u8> = Vec::new();
        pkt.extend_from_slice(&id);
        pkt.extend_from_slice(&[0x01, 0x00]); // RD
        pkt.extend_from_slice(&[0x00, 0x01]); // qdcount=1
        pkt.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        for label in host.split('.') {
            if label.is_empty() || label.len() > 63 {
                continue;
            }
            pkt.push(label.len() as u8);
            pkt.extend_from_slice(label.as_bytes());
        }
        pkt.push(0); // root
        pkt.extend_from_slice(&qtype.to_be_bytes());
        pkt.extend_from_slice(&[0x00, 0x01]); // class IN

        let bind = if server.is_ipv6() { "[::]:0" } else { "0.0.0.0:0" };
        let sock = UdpSocket::bind(bind)?;
        sock.set_read_timeout(Some(crate::time::Duration::from_secs(5)))?;
        sock.send_to(&pkt, super::SocketAddr::new(server, 53))?;

        let mut buf = [0u8; 1500];
        let n = sock.recv(&mut buf)?;
        parse_answers(&buf[..n], qtype)
    }

    fn parse_answers(msg: &[u8], qtype: u16) -> io::Result<Vec<IpAddr>> {
        let inval = || io::Error::new(io::ErrorKind::InvalidData, "bad DNS response");
        if msg.len() < 12 {
            return Err(inval());
        }
        let qd = u16::from_be_bytes([msg[4], msg[5]]) as usize;
        let an = u16::from_be_bytes([msg[6], msg[7]]) as usize;
        let mut pos = 12;
        // Skip the question section.
        for _ in 0..qd {
            pos = skip_name(msg, pos).ok_or_else(inval)?;
            pos += 4; // qtype + qclass
        }
        let mut out = Vec::new();
        for _ in 0..an {
            pos = skip_name(msg, pos).ok_or_else(inval)?;
            if pos + 10 > msg.len() {
                return Err(inval());
            }
            let rtype = u16::from_be_bytes([msg[pos], msg[pos + 1]]);
            let rdlen = u16::from_be_bytes([msg[pos + 8], msg[pos + 9]]) as usize;
            pos += 10;
            if pos + rdlen > msg.len() {
                return Err(inval());
            }
            if rtype == qtype && qtype == 1 && rdlen == 4 {
                out.push(IpAddr::V4(Ipv4Addr::new(
                    msg[pos], msg[pos + 1], msg[pos + 2], msg[pos + 3],
                )));
            } else if rtype == qtype && qtype == 28 && rdlen == 16 {
                let mut o = [0u8; 16];
                o.copy_from_slice(&msg[pos..pos + 16]);
                out.push(IpAddr::V6(Ipv6Addr::from(o)));
            }
            pos += rdlen;
        }
        Ok(out)
    }

    // Returns the position just past a (possibly compressed) name.
    fn skip_name(msg: &[u8], mut pos: usize) -> Option<usize> {
        loop {
            let len = *msg.get(pos)?;
            if len & 0xC0 == 0xC0 {
                return Some(pos + 2); // compression pointer ends the name
            } else if len == 0 {
                return Some(pos + 1);
            } else {
                pos += 1 + len as usize;
            }
        }
    }
}

fn first_addr<A: ToSocketAddrs>(addr: A) -> io::Result<SocketAddr> {
    addr.to_socket_addrs()?
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no addresses"))
}

// ---- raw sockaddr encoding ----

fn encode_sockaddr(addr: &SocketAddr, buf: &mut [u8; 28]) -> usize {
    match addr {
        SocketAddr::V4(a) => {
            buf[0..2].copy_from_slice(&AF_INET.to_ne_bytes());
            buf[2..4].copy_from_slice(&a.port.to_be_bytes());
            buf[4..8].copy_from_slice(&a.ip.octets);
            16
        }
        SocketAddr::V6(a) => {
            buf[0..2].copy_from_slice(&AF_INET6.to_ne_bytes());
            buf[2..4].copy_from_slice(&a.port.to_be_bytes());
            // flowinfo (4) left zero at [4..8]
            buf[8..24].copy_from_slice(&a.ip.octets);
            28
        }
    }
}

fn family_of(addr: &SocketAddr) -> u16 {
    match addr {
        SocketAddr::V4(_) => AF_INET,
        SocketAddr::V6(_) => AF_INET6,
    }
}

fn socket(family: u16, ty: usize) -> io::Result<RawFd> {
    let fd = e(unsafe { sys::sc3(sys::nr::SOCKET, family as usize, ty | SOCK_CLOEXEC, 0) })?;
    Ok(fd as RawFd)
}

fn set_timeout(fd: RawFd, opt: usize, dur: Option<Duration>) -> io::Result<()> {
    #[repr(C)]
    struct Timeval {
        tv_sec: i64,
        tv_usec: i64,
    }
    let tv = match dur {
        Some(d) => Timeval { tv_sec: d.as_secs() as i64, tv_usec: d.subsec_micros() as i64 },
        None => Timeval { tv_sec: 0, tv_usec: 0 },
    };
    e(unsafe {
        sys::sc5(
            sys::nr::SETSOCKOPT,
            fd as usize,
            SOL_SOCKET,
            opt,
            &tv as *const _ as usize,
            core::mem::size_of::<Timeval>(),
        )
    })
    .map(|_| ())
}

// ---- TcpStream ----

pub struct TcpStream {
    fd: RawFd,
}

impl TcpStream {
    pub fn connect<A: ToSocketAddrs>(addr: A) -> io::Result<TcpStream> {
        let addr = first_addr(addr)?;
        let fd = socket(family_of(&addr), SOCK_STREAM)?;
        let mut sa = [0u8; 28];
        let len = encode_sockaddr(&addr, &mut sa);
        match e(unsafe { sys::sc3(sys::nr::CONNECT, fd as usize, sa.as_ptr() as usize, len) }) {
            Ok(_) => Ok(TcpStream { fd }),
            Err(err) => {
                unsafe {
                    let _ = sys::sc1(sys::nr::CLOSE, fd as usize);
                }
                Err(err)
            }
        }
    }

    pub fn set_nodelay(&self, on: bool) -> io::Result<()> {
        let v: i32 = on as i32;
        e(unsafe {
            sys::sc5(
                sys::nr::SETSOCKOPT,
                self.fd as usize,
                IPPROTO_TCP,
                TCP_NODELAY,
                &v as *const _ as usize,
                4,
            )
        })
        .map(|_| ())
    }
    pub fn set_read_timeout(&self, dur: Option<Duration>) -> io::Result<()> {
        set_timeout(self.fd, SO_RCVTIMEO, dur)
    }
    pub fn set_write_timeout(&self, dur: Option<Duration>) -> io::Result<()> {
        set_timeout(self.fd, SO_SNDTIMEO, dur)
    }
    pub fn set_nonblocking(&self, nb: bool) -> io::Result<()> {
        set_nonblocking(self.fd, nb)
    }
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        getname(self.fd, sys::nr::GETPEERNAME)
    }
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        getname(self.fd, sys::nr::GETSOCKNAME)
    }
    pub fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        let how = match how {
            Shutdown::Read => 0,
            Shutdown::Write => 1,
            Shutdown::Both => 2,
        };
        e(unsafe { sys::sc2(sys::nr::SHUTDOWN, self.fd as usize, how) }).map(|_| ())
    }
    pub fn try_clone(&self) -> io::Result<TcpStream> {
        let nfd = e(unsafe { sys::sc3(sys::nr::FCNTL, self.fd as usize, 1030, 0) })?;
        Ok(TcpStream { fd: nfd as RawFd })
    }
}

fn getname(fd: RawFd, which: usize) -> io::Result<SocketAddr> {
    let mut sa = [0u8; 28];
    let mut len: u32 = 28;
    e(unsafe {
        sys::sc3(which, fd as usize, sa.as_mut_ptr() as usize, &mut len as *mut _ as usize)
    })?;
    decode_sockaddr(&sa).ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))
}

fn decode_sockaddr(sa: &[u8; 28]) -> Option<SocketAddr> {
    let fam = u16::from_ne_bytes([sa[0], sa[1]]);
    let port = u16::from_be_bytes([sa[2], sa[3]]);
    if fam == AF_INET {
        let mut o = [0u8; 4];
        o.copy_from_slice(&sa[4..8]);
        Some(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::from(o), port)))
    } else if fam == AF_INET6 {
        let mut o = [0u8; 16];
        o.copy_from_slice(&sa[8..24]);
        Some(SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::from(o), port)))
    } else {
        None
    }
}

fn set_nonblocking(fd: RawFd, nb: bool) -> io::Result<()> {
    // F_GETFL=3, F_SETFL=4, O_NONBLOCK=0o4000
    let flags = e(unsafe { sys::sc3(sys::nr::FCNTL, fd as usize, 3, 0) })?;
    let new = if nb { flags | 0o4000 } else { flags & !0o4000 };
    e(unsafe { sys::sc3(sys::nr::FCNTL, fd as usize, 4, new) }).map(|_| ())
}

impl Read for TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        e(unsafe { sys::sc3(sys::nr::READ, self.fd as usize, buf.as_mut_ptr() as usize, buf.len()) })
    }
}
impl Read for &TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        e(unsafe { sys::sc3(sys::nr::READ, self.fd as usize, buf.as_mut_ptr() as usize, buf.len()) })
    }
}
impl Write for TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        e(unsafe { sys::sc3(sys::nr::WRITE, self.fd as usize, buf.as_ptr() as usize, buf.len()) })
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Write for &TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        e(unsafe { sys::sc3(sys::nr::WRITE, self.fd as usize, buf.as_ptr() as usize, buf.len()) })
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl AsRawFd for TcpStream {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}
impl IntoRawFd for TcpStream {
    fn into_raw_fd(self) -> RawFd {
        let fd = self.fd;
        core::mem::forget(self);
        fd
    }
}
impl FromRawFd for TcpStream {
    unsafe fn from_raw_fd(fd: RawFd) -> TcpStream {
        TcpStream { fd }
    }
}
impl Drop for TcpStream {
    fn drop(&mut self) {
        unsafe {
            let _ = sys::sc1(sys::nr::CLOSE, self.fd as usize);
        }
    }
}

// ---- TcpListener ----

pub struct TcpListener {
    fd: RawFd,
}

impl TcpListener {
    pub fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<TcpListener> {
        let addr = first_addr(addr)?;
        let fd = socket(family_of(&addr), SOCK_STREAM)?;
        let one: i32 = 1;
        let _ = unsafe {
            sys::sc5(
                sys::nr::SETSOCKOPT,
                fd as usize,
                SOL_SOCKET,
                SO_REUSEADDR,
                &one as *const _ as usize,
                4,
            )
        };
        let mut sa = [0u8; 28];
        let len = encode_sockaddr(&addr, &mut sa);
        e(unsafe { sys::sc3(sys::nr::BIND, fd as usize, sa.as_ptr() as usize, len) })?;
        e(unsafe { sys::sc2(sys::nr::LISTEN, fd as usize, 128) })?;
        Ok(TcpListener { fd })
    }

    pub fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
        let mut sa = [0u8; 28];
        let mut len: u32 = 28;
        let fd = e(unsafe {
            sys::sc4(
                sys::nr::ACCEPT4,
                self.fd as usize,
                sa.as_mut_ptr() as usize,
                &mut len as *mut _ as usize,
                SOCK_CLOEXEC,
            )
        })?;
        let peer = decode_sockaddr(&sa).unwrap_or(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)));
        Ok((TcpStream { fd: fd as RawFd }, peer))
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        getname(self.fd, sys::nr::GETSOCKNAME)
    }
    pub fn set_nonblocking(&self, nb: bool) -> io::Result<()> {
        set_nonblocking(self.fd, nb)
    }
    pub fn incoming(&self) -> Incoming<'_> {
        Incoming { listener: self }
    }
}

pub struct Incoming<'a> {
    listener: &'a TcpListener,
}
impl<'a> Iterator for Incoming<'a> {
    type Item = io::Result<TcpStream>;
    fn next(&mut self) -> Option<io::Result<TcpStream>> {
        Some(self.listener.accept().map(|(s, _)| s))
    }
}

impl AsRawFd for TcpListener {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}
impl Drop for TcpListener {
    fn drop(&mut self) {
        unsafe {
            let _ = sys::sc1(sys::nr::CLOSE, self.fd as usize);
        }
    }
}

// ---- UdpSocket ----

pub struct UdpSocket {
    fd: RawFd,
}

impl UdpSocket {
    pub fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<UdpSocket> {
        let addr = first_addr(addr)?;
        let fd = socket(family_of(&addr), SOCK_DGRAM)?;
        let mut sa = [0u8; 28];
        let len = encode_sockaddr(&addr, &mut sa);
        e(unsafe { sys::sc3(sys::nr::BIND, fd as usize, sa.as_ptr() as usize, len) })?;
        Ok(UdpSocket { fd })
    }
    pub fn connect<A: ToSocketAddrs>(&self, addr: A) -> io::Result<()> {
        let addr = first_addr(addr)?;
        let mut sa = [0u8; 28];
        let len = encode_sockaddr(&addr, &mut sa);
        e(unsafe { sys::sc3(sys::nr::CONNECT, self.fd as usize, sa.as_ptr() as usize, len) })
            .map(|_| ())
    }
    pub fn send(&self, buf: &[u8]) -> io::Result<usize> {
        e(unsafe {
            sys::sc6(sys::nr::SENDTO, self.fd as usize, buf.as_ptr() as usize, buf.len(), 0, 0, 0)
        })
    }
    pub fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        e(unsafe {
            sys::sc6(sys::nr::RECVFROM, self.fd as usize, buf.as_mut_ptr() as usize, buf.len(), 0, 0, 0)
        })
    }
    pub fn send_to<A: ToSocketAddrs>(&self, buf: &[u8], addr: A) -> io::Result<usize> {
        let addr = first_addr(addr)?;
        let mut sa = [0u8; 28];
        let len = encode_sockaddr(&addr, &mut sa);
        e(unsafe {
            sys::sc6(
                sys::nr::SENDTO,
                self.fd as usize,
                buf.as_ptr() as usize,
                buf.len(),
                0,
                sa.as_ptr() as usize,
                len,
            )
        })
    }
    pub fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        let mut sa = [0u8; 28];
        let mut len: u32 = 28;
        let n = e(unsafe {
            sys::sc6(
                sys::nr::RECVFROM,
                self.fd as usize,
                buf.as_mut_ptr() as usize,
                buf.len(),
                0,
                sa.as_mut_ptr() as usize,
                &mut len as *mut _ as usize,
            )
        })?;
        let from = decode_sockaddr(&sa).unwrap_or(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)));
        Ok((n, from))
    }
    pub fn set_read_timeout(&self, dur: Option<Duration>) -> io::Result<()> {
        set_timeout(self.fd, SO_RCVTIMEO, dur)
    }
    pub fn set_write_timeout(&self, dur: Option<Duration>) -> io::Result<()> {
        set_timeout(self.fd, SO_SNDTIMEO, dur)
    }
    pub fn set_nonblocking(&self, nb: bool) -> io::Result<()> {
        set_nonblocking(self.fd, nb)
    }
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        getname(self.fd, sys::nr::GETSOCKNAME)
    }
}

impl AsRawFd for UdpSocket {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}
impl Drop for UdpSocket {
    fn drop(&mut self) {
        unsafe {
            let _ = sys::sc1(sys::nr::CLOSE, self.fd as usize);
        }
    }
}

/// How to shut down a socket.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shutdown {
    Read,
    Write,
    Both,
}
