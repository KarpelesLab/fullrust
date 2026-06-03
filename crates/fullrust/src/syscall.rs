//! Arch-neutral, `Result`-returning syscall wrappers.
//!
//! Built on [`crate::arch`]. The kernel returns errors as a small negative
//! value (`-errno`); [`from_ret`] converts that into [`Result`].

use crate::arch::{self, nr};

/// A raw Linux error number (e.g. `2` = `ENOENT`, `9` = `EBADF`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Errno(pub i32);

/// Convert a raw syscall return value into a `Result`.
///
/// Linux signals errors with a return in `[-4095, -1]`, interpreted as
/// `-errno`. Anything else is a successful value.
#[inline]
pub fn from_ret(ret: usize) -> Result<usize, Errno> {
    let s = ret as isize;
    if (-4095..0).contains(&s) {
        Err(Errno(-s as i32))
    } else {
        Ok(ret)
    }
}

// ---- open flags (subset) ----
pub const O_RDONLY: usize = 0o0;
pub const O_WRONLY: usize = 0o1;
pub const O_RDWR: usize = 0o2;
pub const O_CREAT: usize = 0o100;
pub const O_TRUNC: usize = 0o1000;
pub const O_APPEND: usize = 0o2000;

// ---- mmap protection / flags ----
pub const PROT_READ: usize = 0x1;
pub const PROT_WRITE: usize = 0x2;
pub const MAP_PRIVATE: usize = 0x2;
pub const MAP_ANONYMOUS: usize = 0x20;

/// `read(fd, buf)` — returns the number of bytes read (0 at EOF).
#[inline]
pub fn read(fd: i32, buf: &mut [u8]) -> Result<usize, Errno> {
    from_ret(unsafe { arch::syscall3(nr::READ, fd as usize, buf.as_mut_ptr() as usize, buf.len()) })
}

/// `write(fd, buf)` — returns the number of bytes written.
#[inline]
pub fn write(fd: i32, buf: &[u8]) -> Result<usize, Errno> {
    from_ret(unsafe { arch::syscall3(nr::WRITE, fd as usize, buf.as_ptr() as usize, buf.len()) })
}

/// `close(fd)`.
#[inline]
pub fn close(fd: i32) -> Result<(), Errno> {
    from_ret(unsafe { arch::syscall1(nr::CLOSE, fd as usize) }).map(|_| ())
}

/// `openat(AT_FDCWD, path, flags, mode)`. `path` must be NUL-terminated.
#[inline]
pub fn open(path: &core::ffi::CStr, flags: usize, mode: u32) -> Result<i32, Errno> {
    let r = unsafe {
        arch::syscall4(
            nr::OPENAT,
            arch::AT_FDCWD as usize,
            path.as_ptr() as usize,
            flags,
            mode as usize,
        )
    };
    from_ret(r).map(|fd| fd as i32)
}

/// Anonymous private `mmap` of `len` bytes with the given protection.
///
/// Returns a pointer to the mapping, or `Err` on failure.
#[inline]
pub fn mmap_anon(len: usize, prot: usize) -> Result<*mut u8, Errno> {
    // addr=0 (let kernel choose), fd=-1, offset=0.
    let r = unsafe {
        arch::syscall6(
            nr::MMAP,
            0,
            len,
            prot,
            MAP_PRIVATE | MAP_ANONYMOUS,
            usize::MAX, // -1 as fd
            0,
        )
    };
    from_ret(r).map(|p| p as *mut u8)
}

/// `munmap(addr, len)`.
///
/// # Safety
/// `addr`/`len` must describe a mapping previously returned by [`mmap_anon`].
#[inline]
pub unsafe fn munmap(addr: *mut u8, len: usize) -> Result<(), Errno> {
    from_ret(arch::syscall2(nr::MUNMAP, addr as usize, len)).map(|_| ())
}

/// Fill `buf` with random bytes from the kernel CSPRNG (`getrandom`).
#[inline]
pub fn getrandom(buf: &mut [u8]) -> Result<usize, Errno> {
    from_ret(unsafe {
        arch::syscall3(nr::GETRANDOM, buf.as_mut_ptr() as usize, buf.len(), 0)
    })
}

/// Terminate the whole process (all threads) with `code`. Never returns.
#[inline]
pub fn exit_group(code: i32) -> ! {
    unsafe {
        arch::syscall1(nr::EXIT_GROUP, code as usize);
        // If exit_group somehow returns, fall back to exiting this thread.
        arch::syscall1(nr::EXIT, code as usize);
    }
    // Unreachable; satisfy the `!` return type without pulling in panic
    // machinery.
    loop {
        core::hint::spin_loop();
    }
}
