//! A subset of `std::fs`, backed by the `*at` syscalls.

use crate::io::{self, Read, Seek, SeekFrom, Write};
use crate::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, RawFd};
use crate::path::Path;
use crate::sys::{self, Errno};
use crate::time::SystemTime;
use alloc::ffi::CString;
use alloc::string::String;
use alloc::vec::Vec;
use core::time::Duration;

const O_RDONLY: usize = 0;
const O_WRONLY: usize = 1;
const O_RDWR: usize = 2;
const O_CREAT: usize = 0o100;
const O_EXCL: usize = 0o200;
const O_TRUNC: usize = 0o1000;
const O_APPEND: usize = 0o2000;
const O_DIRECTORY: usize = 0o200000;
const O_CLOEXEC: usize = 0o2000000;

const S_IFMT: u32 = 0o170000;
const S_IFDIR: u32 = 0o040000;
const S_IFREG: u32 = 0o100000;
const S_IFLNK: u32 = 0o120000;

fn cstr<P: AsRef<Path>>(p: P) -> io::Result<CString> {
    CString::new(p.as_ref().to_string_lossy().as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
}

fn e<T>(r: Result<T, Errno>) -> io::Result<T> {
    r.map_err(io::Error::from)
}

// x86-64 `struct stat`.
#[repr(C)]
#[derive(Default)]
struct Stat {
    st_dev: u64,
    st_ino: u64,
    st_nlink: u64,
    st_mode: u32,
    st_uid: u32,
    st_gid: u32,
    __pad0: u32,
    st_rdev: u64,
    st_size: i64,
    st_blksize: i64,
    st_blocks: i64,
    st_atime: i64,
    st_atime_nsec: i64,
    st_mtime: i64,
    st_mtime_nsec: i64,
    st_ctime: i64,
    st_ctime_nsec: i64,
    __unused: [i64; 3],
}

/// Metadata for a filesystem object.
#[derive(Clone)]
pub struct Metadata {
    pub(crate) mode: u32,
    pub(crate) size: u64,
    pub(crate) mtime: (i64, i64),
    pub(crate) uid: u32,
    pub(crate) gid: u32,
    pub(crate) ino: u64,
    pub(crate) nlink: u64,
}

/// File type query helper.
#[derive(Clone, Copy)]
pub struct FileType(u32);

/// Unix permission bits.
#[derive(Clone, Copy)]
pub struct Permissions(u32);

impl Metadata {
    pub fn is_file(&self) -> bool {
        self.mode & S_IFMT == S_IFREG
    }
    pub fn is_dir(&self) -> bool {
        self.mode & S_IFMT == S_IFDIR
    }
    pub fn is_symlink(&self) -> bool {
        self.mode & S_IFMT == S_IFLNK
    }
    pub fn len(&self) -> u64 {
        self.size
    }
    pub fn file_type(&self) -> FileType {
        FileType(self.mode & S_IFMT)
    }
    pub fn permissions(&self) -> Permissions {
        Permissions(self.mode & 0o7777)
    }
    pub fn modified(&self) -> io::Result<SystemTime> {
        Ok(crate::time::UNIX_EPOCH + Duration::new(self.mtime.0 as u64, self.mtime.1 as u32))
    }
}

impl FileType {
    pub fn is_file(&self) -> bool {
        self.0 == S_IFREG
    }
    pub fn is_dir(&self) -> bool {
        self.0 == S_IFDIR
    }
    pub fn is_symlink(&self) -> bool {
        self.0 == S_IFLNK
    }
}

impl Permissions {
    pub fn readonly(&self) -> bool {
        self.0 & 0o200 == 0
    }
    pub fn set_readonly(&mut self, ro: bool) {
        if ro {
            self.0 &= !0o222;
        } else {
            self.0 |= 0o200;
        }
    }
    pub fn mode(&self) -> u32 {
        self.0
    }
}

fn stat_to_meta(s: &Stat) -> Metadata {
    Metadata {
        mode: s.st_mode,
        size: s.st_size as u64,
        mtime: (s.st_mtime, s.st_mtime_nsec),
        uid: s.st_uid,
        gid: s.st_gid,
        ino: s.st_ino,
        nlink: s.st_nlink,
    }
}

/// An open file.
pub struct File {
    fd: RawFd,
}

impl File {
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<File> {
        OpenOptions::new().read(true).open(path)
    }
    pub fn create<P: AsRef<Path>>(path: P) -> io::Result<File> {
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
    }
    pub fn metadata(&self) -> io::Result<Metadata> {
        let mut s = Stat::default();
        e(unsafe { sys::sc2(sys::nr::FSTAT, self.fd as usize, &mut s as *mut _ as usize) })?;
        Ok(stat_to_meta(&s))
    }
    pub fn set_len(&self, size: u64) -> io::Result<()> {
        e(unsafe { sys::sc2(sys::nr::FTRUNCATE, self.fd as usize, size as usize) }).map(|_| ())
    }
    pub fn sync_all(&self) -> io::Result<()> {
        Ok(())
    }
    pub fn sync_data(&self) -> io::Result<()> {
        Ok(())
    }
    pub fn try_clone(&self) -> io::Result<File> {
        // F_DUPFD_CLOEXEC = 1030
        let nfd = e(unsafe { sys::sc3(sys::nr::FCNTL, self.fd as usize, 1030, 0) })?;
        Ok(File { fd: nfd as RawFd })
    }
}

fn do_read(fd: RawFd, buf: &mut [u8]) -> io::Result<usize> {
    e(unsafe {
        sys::sc3(
            sys::nr::READ,
            fd as usize,
            buf.as_mut_ptr() as usize,
            buf.len(),
        )
    })
}
fn do_write(fd: RawFd, buf: &[u8]) -> io::Result<usize> {
    e(unsafe {
        sys::sc3(
            sys::nr::WRITE,
            fd as usize,
            buf.as_ptr() as usize,
            buf.len(),
        )
    })
}
fn do_seek(fd: RawFd, pos: SeekFrom) -> io::Result<u64> {
    let (whence, off): (usize, i64) = match pos {
        SeekFrom::Start(n) => (0, n as i64),
        SeekFrom::Current(n) => (1, n),
        SeekFrom::End(n) => (2, n),
    };
    let r = e(unsafe { sys::sc3(sys::nr::LSEEK, fd as usize, off as usize, whence) })?;
    Ok(r as u64)
}

impl Read for File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        do_read(self.fd, buf)
    }
}
impl Read for &File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        do_read(self.fd, buf)
    }
}
impl Write for File {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        do_write(self.fd, buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Write for &File {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        do_write(self.fd, buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Seek for File {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        do_seek(self.fd, pos)
    }
}

impl AsRawFd for File {
    fn as_raw_fd(&self) -> RawFd {
        self.fd
    }
}
impl IntoRawFd for File {
    fn into_raw_fd(self) -> RawFd {
        let fd = self.fd;
        core::mem::forget(self);
        fd
    }
}
impl FromRawFd for File {
    unsafe fn from_raw_fd(fd: RawFd) -> File {
        File { fd }
    }
}

impl Drop for File {
    fn drop(&mut self) {
        unsafe {
            let _ = sys::sc1(sys::nr::CLOSE, self.fd as usize);
        }
    }
}

/// Builder for opening files with custom flags.
#[derive(Clone, Debug)]
pub struct OpenOptions {
    read: bool,
    write: bool,
    append: bool,
    truncate: bool,
    create: bool,
    create_new: bool,
    pub(crate) mode: u32,
    pub(crate) custom_flags: i32,
}

impl Default for OpenOptions {
    fn default() -> OpenOptions {
        OpenOptions {
            read: false,
            write: false,
            append: false,
            truncate: false,
            create: false,
            create_new: false,
            mode: 0o666,
            custom_flags: 0,
        }
    }
}

impl OpenOptions {
    pub fn new() -> OpenOptions {
        OpenOptions::default()
    }
    pub fn read(&mut self, v: bool) -> &mut Self {
        self.read = v;
        self
    }
    pub fn write(&mut self, v: bool) -> &mut Self {
        self.write = v;
        self
    }
    pub fn append(&mut self, v: bool) -> &mut Self {
        self.append = v;
        self
    }
    pub fn truncate(&mut self, v: bool) -> &mut Self {
        self.truncate = v;
        self
    }
    pub fn create(&mut self, v: bool) -> &mut Self {
        self.create = v;
        self
    }
    pub fn create_new(&mut self, v: bool) -> &mut Self {
        self.create_new = v;
        self
    }

    pub fn open<P: AsRef<Path>>(&self, path: P) -> io::Result<File> {
        let mut flags = if self.read && (self.write || self.append) {
            O_RDWR
        } else if self.write || self.append {
            O_WRONLY
        } else {
            O_RDONLY
        };
        flags |= O_CLOEXEC;
        if self.append {
            flags |= O_APPEND;
        }
        if self.truncate {
            flags |= O_TRUNC;
        }
        if self.create {
            flags |= O_CREAT;
        }
        if self.create_new {
            flags |= O_CREAT | O_EXCL;
        }
        flags |= self.custom_flags as usize & 0o7777_7777;
        let c = cstr(&path)?;
        let fd = e(unsafe {
            sys::sc4(
                sys::nr::OPENAT,
                sys::AT_FDCWD as usize,
                c.as_ptr() as usize,
                flags,
                self.mode as usize,
            )
        })?;
        Ok(File { fd: fd as RawFd })
    }
}

// ---- free functions ----

pub fn metadata<P: AsRef<Path>>(path: P) -> io::Result<Metadata> {
    let c = cstr(&path)?;
    let mut s = Stat::default();
    e(unsafe {
        sys::sc4(
            sys::nr::NEWFSTATAT,
            sys::AT_FDCWD as usize,
            c.as_ptr() as usize,
            &mut s as *mut _ as usize,
            0,
        )
    })?;
    Ok(stat_to_meta(&s))
}

pub fn read<P: AsRef<Path>>(path: P) -> io::Result<Vec<u8>> {
    let mut f = File::open(path)?;
    let mut v = Vec::new();
    f.read_to_end(&mut v)?;
    Ok(v)
}

pub fn read_to_string<P: AsRef<Path>>(path: P) -> io::Result<String> {
    let mut f = File::open(path)?;
    let mut s = String::new();
    f.read_to_string(&mut s)?;
    Ok(s)
}

pub fn write<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) -> io::Result<()> {
    let mut f = File::create(path)?;
    f.write_all(contents.as_ref())
}

pub fn remove_file<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let c = cstr(&path)?;
    e(unsafe {
        sys::sc3(
            sys::nr::UNLINKAT,
            sys::AT_FDCWD as usize,
            c.as_ptr() as usize,
            0,
        )
    })
    .map(|_| ())
}

pub fn create_dir<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let c = cstr(&path)?;
    e(unsafe {
        sys::sc3(
            sys::nr::MKDIRAT,
            sys::AT_FDCWD as usize,
            c.as_ptr() as usize,
            0o777,
        )
    })
    .map(|_| ())
}

pub fn create_dir_all<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let p = path.as_ref();
    match create_dir(p) {
        Ok(()) => Ok(()),
        Err(ref e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => {
            // Try to create the parent first, then retry.
            if let Some(parent) = p.parent() {
                if !parent.is_empty() {
                    create_dir_all(parent)?;
                    return match create_dir(p) {
                        Ok(()) => Ok(()),
                        Err(ref e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
                        Err(e) => Err(e),
                    };
                }
            }
            Err(e)
        }
    }
}

pub fn rename<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> io::Result<()> {
    let f = cstr(&from)?;
    let t = cstr(&to)?;
    e(unsafe {
        sys::sc4(
            sys::nr::RENAMEAT,
            sys::AT_FDCWD as usize,
            f.as_ptr() as usize,
            sys::AT_FDCWD as usize,
            t.as_ptr() as usize,
        )
    })
    .map(|_| ())
}

pub fn remove_dir<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let c = cstr(&path)?;
    e(unsafe {
        sys::sc3(
            sys::nr::UNLINKAT,
            sys::AT_FDCWD as usize,
            c.as_ptr() as usize,
            sys::AT_REMOVEDIR,
        )
    })
    .map(|_| ())
}

#[repr(C)]
struct Dirent64Hdr {
    d_ino: u64,
    d_off: i64,
    d_reclen: u16,
    d_type: u8,
    // followed by NUL-terminated name
}

pub fn remove_dir_all<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let path = path.as_ref();
    let c = cstr(path)?;
    let dirfd = e(unsafe {
        sys::sc4(
            sys::nr::OPENAT,
            sys::AT_FDCWD as usize,
            c.as_ptr() as usize,
            O_RDONLY | O_DIRECTORY | O_CLOEXEC,
            0,
        )
    })? as RawFd;

    let mut buf = [0u8; 4096];
    loop {
        let n = e(unsafe {
            sys::sc3(
                sys::nr::GETDENTS64,
                dirfd as usize,
                buf.as_mut_ptr() as usize,
                buf.len(),
            )
        })?;
        if n == 0 {
            break;
        }
        let mut off = 0usize;
        while off < n {
            let hdr = unsafe { &*(buf.as_ptr().add(off) as *const Dirent64Hdr) };
            let reclen = hdr.d_reclen as usize;
            let name_ptr = unsafe { buf.as_ptr().add(off + 19) }; // 8+8+2+1
            let name = unsafe { core::ffi::CStr::from_ptr(name_ptr as *const i8) }.to_bytes();
            if name != b"." && name != b".." {
                let child = path.join(Path::new(core::str::from_utf8(name).unwrap_or("")));
                // DT_DIR = 4
                if hdr.d_type == 4 {
                    remove_dir_all(&child)?;
                } else {
                    remove_file(&child)?;
                }
            }
            off += reclen;
        }
    }
    unsafe {
        let _ = sys::sc1(sys::nr::CLOSE, dirfd as usize);
    }
    remove_dir(path)
}
