//! A subset of `std::io`: `Error`/`ErrorKind`, `Read`/`Write`/`BufRead`/`Seek`,
//! `BufReader`/`BufWriter`/`Cursor`, and the standard streams.

use crate::sys::{self, Errno};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

pub type Result<T> = core::result::Result<T, Error>;

/// Categories of I/O error (subset of `std::io::ErrorKind`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum ErrorKind {
    NotFound,
    PermissionDenied,
    ConnectionRefused,
    ConnectionReset,
    ConnectionAborted,
    NotConnected,
    AddrInUse,
    AddrNotAvailable,
    BrokenPipe,
    AlreadyExists,
    WouldBlock,
    InvalidInput,
    InvalidData,
    TimedOut,
    WriteZero,
    Interrupted,
    UnexpectedEof,
    Unsupported,
    OutOfMemory,
    Other,
}

impl ErrorKind {
    fn as_str(&self) -> &'static str {
        use ErrorKind::*;
        match self {
            NotFound => "entity not found",
            PermissionDenied => "permission denied",
            ConnectionRefused => "connection refused",
            ConnectionReset => "connection reset",
            ConnectionAborted => "connection aborted",
            NotConnected => "not connected",
            AddrInUse => "address in use",
            AddrNotAvailable => "address not available",
            BrokenPipe => "broken pipe",
            AlreadyExists => "entity already exists",
            WouldBlock => "operation would block",
            InvalidInput => "invalid input parameter",
            InvalidData => "invalid data",
            TimedOut => "timed out",
            WriteZero => "write zero",
            Interrupted => "operation interrupted",
            UnexpectedEof => "unexpected end of file",
            Unsupported => "unsupported",
            OutOfMemory => "out of memory",
            Other => "other error",
        }
    }
}

fn errno_kind(e: i32) -> ErrorKind {
    use ErrorKind::*;
    match e {
        2 => NotFound,
        1 | 13 => PermissionDenied,
        111 => ConnectionRefused,
        104 => ConnectionReset,
        103 => ConnectionAborted,
        107 => NotConnected,
        98 => AddrInUse,
        99 => AddrNotAvailable,
        32 => BrokenPipe,
        17 => AlreadyExists,
        11 => WouldBlock,
        22 => InvalidInput,
        110 => TimedOut,
        4 => Interrupted,
        12 => OutOfMemory,
        _ => Other,
    }
}

enum Repr {
    Os(i32),
    Simple(ErrorKind),
    Custom(ErrorKind, Box<dyn core::error::Error + Send + Sync>),
}

/// The error type for I/O operations (subset of `std::io::Error`).
pub struct Error(Repr);

impl Error {
    /// Create an error from a kind and an arbitrary payload.
    pub fn new<E>(kind: ErrorKind, error: E) -> Error
    where
        E: Into<Box<dyn core::error::Error + Send + Sync>>,
    {
        Error(Repr::Custom(kind, error.into()))
    }

    /// Create an error carrying only a kind (no message).
    pub fn from_kind(kind: ErrorKind) -> Error {
        Error(Repr::Simple(kind))
    }

    /// Build from a raw errno.
    pub fn from_raw_os_error(code: i32) -> Error {
        Error(Repr::Os(code))
    }

    /// The raw OS error, if this came from one.
    pub fn raw_os_error(&self) -> Option<i32> {
        match self.0 {
            Repr::Os(c) => Some(c),
            _ => None,
        }
    }

    /// The error's category.
    pub fn kind(&self) -> ErrorKind {
        match &self.0 {
            Repr::Os(c) => errno_kind(*c),
            Repr::Simple(k) => *k,
            Repr::Custom(k, _) => *k,
        }
    }
}

impl From<ErrorKind> for Error {
    fn from(k: ErrorKind) -> Error {
        Error(Repr::Simple(k))
    }
}

impl From<Errno> for Error {
    fn from(e: Errno) -> Error {
        Error(Repr::Os(e.0))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Repr::Os(c) => write!(f, "{} (os error {})", errno_kind(*c).as_str(), c),
            Repr::Simple(k) => f.write_str(k.as_str()),
            Repr::Custom(_, e) => fmt::Display::fmt(e, f),
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Error {{ kind: {:?}, msg: \"", self.kind())?;
        fmt::Display::fmt(self, f)?;
        f.write_str("\" }")
    }
}

impl core::error::Error for Error {}

#[inline]
fn cvt(r: core::result::Result<usize, Errno>) -> Result<usize> {
    r.map_err(Error::from)
}

/// Seek reference point.
#[derive(Clone, Copy, Debug)]
pub enum SeekFrom {
    Start(u64),
    End(i64),
    Current(i64),
}

// ---- Read ----

pub trait Read {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize>;

    fn read_exact(&mut self, mut buf: &mut [u8]) -> Result<()> {
        while !buf.is_empty() {
            match self.read(buf) {
                Ok(0) => return Err(ErrorKind::UnexpectedEof.into()),
                Ok(n) => buf = &mut buf[n..],
                Err(ref e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> Result<usize> {
        let start = buf.len();
        let mut tmp = [0u8; 8192];
        loop {
            match self.read(&mut tmp) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(ref e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(buf.len() - start)
    }

    fn read_to_string(&mut self, buf: &mut String) -> Result<usize> {
        let mut bytes = Vec::new();
        let n = self.read_to_end(&mut bytes)?;
        let s = core::str::from_utf8(&bytes).map_err(|_| Error::from(ErrorKind::InvalidData))?;
        buf.push_str(s);
        Ok(n)
    }

    fn by_ref(&mut self) -> &mut Self
    where
        Self: Sized,
    {
        self
    }
}

// ---- Write ----

pub trait Write {
    fn write(&mut self, buf: &[u8]) -> Result<usize>;
    fn flush(&mut self) -> Result<()>;

    fn write_all(&mut self, mut buf: &[u8]) -> Result<()> {
        while !buf.is_empty() {
            match self.write(buf) {
                Ok(0) => return Err(ErrorKind::WriteZero.into()),
                Ok(n) => buf = &buf[n..],
                Err(ref e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> Result<()> {
        // Adapter so core::fmt can drive our Write.
        struct Adapter<'a, T: ?Sized + 'a> {
            inner: &'a mut T,
            error: Result<()>,
        }
        impl<T: Write + ?Sized> fmt::Write for Adapter<'_, T> {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                match self.inner.write_all(s.as_bytes()) {
                    Ok(()) => Ok(()),
                    Err(e) => {
                        self.error = Err(e);
                        Err(fmt::Error)
                    }
                }
            }
        }
        let mut a = Adapter {
            inner: self,
            error: Ok(()),
        };
        match fmt::write(&mut a, args) {
            Ok(()) => Ok(()),
            Err(_) => a.error,
        }
    }

    fn by_ref(&mut self) -> &mut Self
    where
        Self: Sized,
    {
        self
    }
}

// ---- Seek ----

pub trait Seek {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64>;
    fn stream_position(&mut self) -> Result<u64> {
        self.seek(SeekFrom::Current(0))
    }
}

// ---- BufRead ----

pub trait BufRead: Read {
    fn fill_buf(&mut self) -> Result<&[u8]>;
    fn consume(&mut self, amt: usize);

    fn read_until(&mut self, delim: u8, buf: &mut Vec<u8>) -> Result<usize> {
        let mut read = 0;
        loop {
            let (done, used) = {
                let available = match self.fill_buf() {
                    Ok(b) => b,
                    Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                };
                if available.is_empty() {
                    (true, 0)
                } else if let Some(i) = available.iter().position(|&b| b == delim) {
                    buf.extend_from_slice(&available[..=i]);
                    (true, i + 1)
                } else {
                    buf.extend_from_slice(available);
                    (false, available.len())
                }
            };
            self.consume(used);
            read += used;
            if done || used == 0 {
                return Ok(read);
            }
        }
    }

    fn read_line(&mut self, buf: &mut String) -> Result<usize> {
        let mut bytes = Vec::new();
        let n = self.read_until(b'\n', &mut bytes)?;
        let s = core::str::from_utf8(&bytes).map_err(|_| Error::from(ErrorKind::InvalidData))?;
        buf.push_str(s);
        Ok(n)
    }
}

// blanket impls for &mut T
impl<R: Read + ?Sized> Read for &mut R {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        (**self).read(buf)
    }
}
impl<W: Write + ?Sized> Write for &mut W {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        (**self).write(buf)
    }
    fn flush(&mut self) -> Result<()> {
        (**self).flush()
    }
}

// ---- impls for byte slices / Vec ----

impl Read for &[u8] {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let n = core::cmp::min(buf.len(), self.len());
        let (head, tail) = self.split_at(n);
        buf[..n].copy_from_slice(head);
        *self = tail;
        Ok(n)
    }
}

impl Write for &mut [u8] {
    fn write(&mut self, data: &[u8]) -> Result<usize> {
        let n = core::cmp::min(data.len(), self.len());
        let (head, tail) = core::mem::take(self).split_at_mut(n);
        head.copy_from_slice(&data[..n]);
        *self = tail;
        Ok(n)
    }
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

impl Write for Vec<u8> {
    fn write(&mut self, data: &[u8]) -> Result<usize> {
        self.extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

// ---- Cursor ----

pub struct Cursor<T> {
    inner: T,
    pos: u64,
}

impl<T> Cursor<T> {
    pub fn new(inner: T) -> Cursor<T> {
        Cursor { inner, pos: 0 }
    }
    pub fn into_inner(self) -> T {
        self.inner
    }
    pub fn get_ref(&self) -> &T {
        &self.inner
    }
    pub fn position(&self) -> u64 {
        self.pos
    }
    pub fn set_position(&mut self, pos: u64) {
        self.pos = pos;
    }
}

impl<T: AsRef<[u8]>> Read for Cursor<T> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let data = self.inner.as_ref();
        let pos = core::cmp::min(self.pos as usize, data.len());
        let mut slice = &data[pos..];
        let n = Read::read(&mut slice, buf)?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Write for Cursor<Vec<u8>> {
    fn write(&mut self, data: &[u8]) -> Result<usize> {
        let pos = self.pos as usize;
        if pos > self.inner.len() {
            self.inner.resize(pos, 0);
        }
        let end = pos + data.len();
        if end > self.inner.len() {
            self.inner.resize(end, 0);
        }
        self.inner[pos..end].copy_from_slice(data);
        self.pos = end as u64;
        Ok(data.len())
    }
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

// ---- BufReader ----

pub struct BufReader<R> {
    inner: R,
    buf: Vec<u8>,
    pos: usize,
    cap: usize,
}

impl<R: Read> BufReader<R> {
    pub fn new(inner: R) -> BufReader<R> {
        Self::with_capacity(8192, inner)
    }
    pub fn with_capacity(cap: usize, inner: R) -> BufReader<R> {
        BufReader {
            inner,
            buf: alloc::vec![0; cap],
            pos: 0,
            cap: 0,
        }
    }
    pub fn get_ref(&self) -> &R {
        &self.inner
    }
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.inner
    }
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read> Read for BufReader<R> {
    fn read(&mut self, out: &mut [u8]) -> Result<usize> {
        if self.pos == self.cap && out.len() >= self.buf.len() {
            return self.inner.read(out);
        }
        let avail = self.fill_buf()?;
        let n = core::cmp::min(avail.len(), out.len());
        out[..n].copy_from_slice(&avail[..n]);
        self.consume(n);
        Ok(n)
    }
}

impl<R: Read> BufRead for BufReader<R> {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        if self.pos >= self.cap {
            self.cap = self.inner.read(&mut self.buf)?;
            self.pos = 0;
        }
        Ok(&self.buf[self.pos..self.cap])
    }
    fn consume(&mut self, amt: usize) {
        self.pos = core::cmp::min(self.pos + amt, self.cap);
    }
}

// ---- BufWriter ----

pub struct BufWriter<W: Write> {
    inner: Option<W>,
    buf: Vec<u8>,
}

impl<W: Write> BufWriter<W> {
    pub fn new(inner: W) -> BufWriter<W> {
        BufWriter {
            inner: Some(inner),
            buf: Vec::with_capacity(8192),
        }
    }
    pub fn get_ref(&self) -> &W {
        self.inner.as_ref().unwrap()
    }
    pub fn get_mut(&mut self) -> &mut W {
        self.inner.as_mut().unwrap()
    }
    fn flush_buf(&mut self) -> Result<()> {
        if !self.buf.is_empty() {
            let inner = self.inner.as_mut().unwrap();
            inner.write_all(&self.buf)?;
            self.buf.clear();
        }
        Ok(())
    }
    pub fn into_inner(mut self) -> core::result::Result<W, Error> {
        self.flush_buf()?;
        Ok(self.inner.take().unwrap())
    }
}

impl<W: Write> Write for BufWriter<W> {
    fn write(&mut self, data: &[u8]) -> Result<usize> {
        if self.buf.len() + data.len() > self.buf.capacity() {
            self.flush_buf()?;
        }
        if data.len() >= self.buf.capacity() {
            self.inner.as_mut().unwrap().write(data)
        } else {
            self.buf.extend_from_slice(data);
            Ok(data.len())
        }
    }
    fn flush(&mut self) -> Result<()> {
        self.flush_buf()?;
        self.inner.as_mut().unwrap().flush()
    }
}

impl<W: Write> Drop for BufWriter<W> {
    fn drop(&mut self) {
        if self.inner.is_some() {
            let _ = self.flush_buf();
        }
    }
}

// ---- standard streams ----

fn fd_read(fd: i32, buf: &mut [u8]) -> Result<usize> {
    cvt(unsafe {
        sys::sc3(
            sys::nr::READ,
            fd as usize,
            buf.as_mut_ptr() as usize,
            buf.len(),
        )
    })
}
fn fd_write(fd: i32, buf: &[u8]) -> Result<usize> {
    cvt(unsafe {
        sys::sc3(
            sys::nr::WRITE,
            fd as usize,
            buf.as_ptr() as usize,
            buf.len(),
        )
    })
}

pub struct Stdin;
pub struct Stdout;
pub struct Stderr;
pub struct StdinLock;
pub struct StdoutLock;
pub struct StderrLock;

pub fn stdin() -> Stdin {
    Stdin
}
pub fn stdout() -> Stdout {
    Stdout
}
pub fn stderr() -> Stderr {
    Stderr
}

/// Read one line (through the newline) directly from `fd`, one byte at a time
/// so we never consume past the line (important for pipes).
fn read_line_fd(fd: i32, out: &mut String) -> Result<usize> {
    let mut bytes = Vec::new();
    let mut b = [0u8; 1];
    loop {
        match fd_read(fd, &mut b)? {
            0 => break,
            _ => {
                bytes.push(b[0]);
                if b[0] == b'\n' {
                    break;
                }
            }
        }
    }
    let s = core::str::from_utf8(&bytes).map_err(|_| Error::from(ErrorKind::InvalidData))?;
    out.push_str(s);
    Ok(bytes.len())
}

impl Stdin {
    pub fn lock(&self) -> StdinLock {
        StdinLock
    }
    pub fn read_line(&self, buf: &mut String) -> Result<usize> {
        read_line_fd(0, buf)
    }
}
impl StdinLock {
    pub fn read_line(&mut self, buf: &mut String) -> Result<usize> {
        read_line_fd(0, buf)
    }
}

/// Query whether a stream refers to a terminal (`std::io::IsTerminal`).
pub trait IsTerminal {
    fn is_terminal(&self) -> bool;
}

fn isatty(fd: i32) -> bool {
    // ioctl(fd, TCGETS, &termios) succeeds only for terminals.
    let mut termios = [0u8; 64];
    unsafe {
        sys::sc3(
            sys::nr::IOCTL,
            fd as usize,
            0x5401,
            termios.as_mut_ptr() as usize,
        )
        .is_ok()
    }
}

impl IsTerminal for Stdin {
    fn is_terminal(&self) -> bool {
        isatty(0)
    }
}
impl IsTerminal for StdinLock {
    fn is_terminal(&self) -> bool {
        isatty(0)
    }
}
impl IsTerminal for Stdout {
    fn is_terminal(&self) -> bool {
        isatty(1)
    }
}
impl IsTerminal for StdoutLock {
    fn is_terminal(&self) -> bool {
        isatty(1)
    }
}
impl IsTerminal for Stderr {
    fn is_terminal(&self) -> bool {
        isatty(2)
    }
}
impl IsTerminal for StderrLock {
    fn is_terminal(&self) -> bool {
        isatty(2)
    }
}
impl Stdout {
    pub fn lock(&self) -> StdoutLock {
        StdoutLock
    }
}
impl Stderr {
    pub fn lock(&self) -> StderrLock {
        StderrLock
    }
}

impl Read for Stdin {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        fd_read(0, buf)
    }
}
impl Read for StdinLock {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        fd_read(0, buf)
    }
}

macro_rules! out_stream {
    ($t:ty, $fd:expr) => {
        impl Write for $t {
            fn write(&mut self, buf: &[u8]) -> Result<usize> {
                fd_write($fd, buf)
            }
            fn flush(&mut self) -> Result<()> {
                Ok(())
            }
        }
    };
}
out_stream!(Stdout, 1);
out_stream!(StdoutLock, 1);
out_stream!(Stderr, 2);
out_stream!(StderrLock, 2);

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    let _ = Stdout.write_fmt(args);
}
#[doc(hidden)]
pub fn _eprint(args: fmt::Arguments) {
    let _ = Stderr.write_fmt(args);
}

/// Copy all bytes from `reader` to `writer`.
pub fn copy<R: Read + ?Sized, W: Write + ?Sized>(reader: &mut R, writer: &mut W) -> Result<u64> {
    let mut buf = [0u8; 8192];
    let mut total = 0u64;
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(ref e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        writer.write_all(&buf[..n])?;
        total += n as u64;
    }
    Ok(total)
}
