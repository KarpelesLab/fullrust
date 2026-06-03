//! Minimal standard I/O: file descriptors, `Write`, and the print macros.

use crate::syscall::{self, Errno};
use core::fmt;

pub const STDIN: i32 = 0;
pub const STDOUT: i32 = 1;
pub const STDERR: i32 = 2;

/// A thin wrapper over a raw file descriptor.
#[derive(Clone, Copy)]
pub struct Fd(pub i32);

impl Fd {
    /// Write the whole buffer, retrying short writes. Returns the byte count.
    pub fn write_all(&self, mut buf: &[u8]) -> Result<(), Errno> {
        while !buf.is_empty() {
            match syscall::write(self.0, buf) {
                Ok(0) => return Err(Errno(5)), // EIO: made no progress
                Ok(n) => buf = &buf[n..],
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Read once into `buf`, returning the number of bytes read (0 at EOF).
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, Errno> {
        syscall::read(self.0, buf)
    }
}

impl fmt::Write for Fd {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_all(s.as_bytes()).map_err(|_| fmt::Error)
    }
}

/// Standard output (fd 1).
#[inline]
pub fn stdout() -> Fd {
    Fd(STDOUT)
}

/// Standard error (fd 2).
#[inline]
pub fn stderr() -> Fd {
    Fd(STDERR)
}

/// Standard input (fd 0).
#[inline]
pub fn stdin() -> Fd {
    Fd(STDIN)
}

// ---- machinery behind the print macros ----

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use fmt::Write;
    let _ = stdout().write_fmt(args);
}

#[doc(hidden)]
pub fn _eprint(args: fmt::Arguments) {
    use fmt::Write;
    let _ = stderr().write_fmt(args);
}

/// Print to standard output. Like `std::print!`.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => { $crate::io::_print(::core::format_args!($($arg)*)) };
}

/// Print to standard output, with a trailing newline. Like `std::println!`.
#[macro_export]
macro_rules! println {
    () => { $crate::io::_print(::core::format_args!("\n")) };
    ($($arg:tt)*) => {
        $crate::io::_print(::core::format_args!("{}\n", ::core::format_args!($($arg)*)))
    };
}

/// Print to standard error. Like `std::eprint!`.
#[macro_export]
macro_rules! eprint {
    ($($arg:tt)*) => { $crate::io::_eprint(::core::format_args!($($arg)*)) };
}

/// Print to standard error, with a trailing newline. Like `std::eprintln!`.
#[macro_export]
macro_rules! eprintln {
    () => { $crate::io::_eprint(::core::format_args!("\n")) };
    ($($arg:tt)*) => {
        $crate::io::_eprint(::core::format_args!("{}\n", ::core::format_args!($($arg)*)))
    };
}
