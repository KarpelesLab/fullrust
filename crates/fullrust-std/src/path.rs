//! A simplified `std::path`: UTF-8 paths over `String`/`str`.

use crate::ffi::{OsStr, OsString};
use alloc::borrow::ToOwned;
use alloc::string::String;
use core::fmt;

/// An owned, mutable path.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct PathBuf {
    inner: String,
}

/// A borrowed path slice.
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Path {
    inner: str,
}

impl Path {
    pub fn new<S: AsRef<str> + ?Sized>(s: &S) -> &Path {
        unsafe { &*(s.as_ref() as *const str as *const Path) }
    }
    pub fn to_str(&self) -> Option<&str> {
        Some(&self.inner)
    }
    pub fn to_string_lossy(&self) -> &str {
        &self.inner
    }
    pub fn as_os_str(&self) -> &OsStr {
        OsStr::new(&self.inner)
    }
    pub fn to_path_buf(&self) -> PathBuf {
        PathBuf { inner: self.inner.to_owned() }
    }
    pub fn display(&self) -> &str {
        &self.inner
    }
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
    pub fn exists(&self) -> bool {
        crate::fs::metadata(self).is_ok()
    }
    pub fn is_file(&self) -> bool {
        crate::fs::metadata(self).map(|m| m.is_file()).unwrap_or(false)
    }
    pub fn is_dir(&self) -> bool {
        crate::fs::metadata(self).map(|m| m.is_dir()).unwrap_or(false)
    }
    pub fn file_name(&self) -> Option<&OsStr> {
        let s = self.inner.trim_end_matches('/');
        if s.is_empty() {
            return None;
        }
        let name = match s.rfind('/') {
            Some(i) => &s[i + 1..],
            None => s,
        };
        Some(OsStr::new(name))
    }
    pub fn parent(&self) -> Option<&Path> {
        let s = self.inner.trim_end_matches('/');
        match s.rfind('/') {
            Some(0) => Some(Path::new("/")),
            Some(i) => Some(Path::new(&s[..i])),
            None => None,
        }
    }
    pub fn extension(&self) -> Option<&OsStr> {
        let name = self.file_name()?.to_str()?;
        match name.rfind('.') {
            Some(0) | None => None,
            Some(i) => Some(OsStr::new(&name[i + 1..])),
        }
    }
    pub fn join<P: AsRef<Path>>(&self, p: P) -> PathBuf {
        let mut b = self.to_path_buf();
        b.push(p);
        b
    }
}

impl PathBuf {
    pub fn new() -> PathBuf {
        PathBuf { inner: String::new() }
    }
    pub fn from<S: Into<String>>(s: S) -> PathBuf {
        PathBuf { inner: s.into() }
    }
    pub fn as_path(&self) -> &Path {
        Path::new(&self.inner)
    }
    pub fn push<P: AsRef<Path>>(&mut self, p: P) {
        let p = &p.as_ref().inner;
        if p.starts_with('/') {
            self.inner = p.to_owned();
        } else {
            if !self.inner.is_empty() && !self.inner.ends_with('/') {
                self.inner.push('/');
            }
            self.inner.push_str(p);
        }
    }
    pub fn pop(&mut self) -> bool {
        match self.as_path().parent() {
            Some(parent) => {
                self.inner = parent.inner.to_owned();
                true
            }
            None => false,
        }
    }
    pub fn into_os_string(self) -> OsString {
        OsString::from(self.inner)
    }
    pub fn display(&self) -> &str {
        &self.inner
    }
}

impl core::ops::Deref for PathBuf {
    type Target = Path;
    fn deref(&self) -> &Path {
        self.as_path()
    }
}

impl AsRef<Path> for Path {
    fn as_ref(&self) -> &Path {
        self
    }
}
impl AsRef<Path> for PathBuf {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}
impl AsRef<Path> for str {
    fn as_ref(&self) -> &Path {
        Path::new(self)
    }
}
impl AsRef<Path> for String {
    fn as_ref(&self) -> &Path {
        Path::new(self.as_str())
    }
}
impl AsRef<Path> for OsStr {
    fn as_ref(&self) -> &Path {
        Path::new(self.to_str().unwrap_or(""))
    }
}
impl AsRef<str> for Path {
    fn as_ref(&self) -> &str {
        &self.inner
    }
}

impl From<String> for PathBuf {
    fn from(s: String) -> PathBuf {
        PathBuf { inner: s }
    }
}
impl From<&str> for PathBuf {
    fn from(s: &str) -> PathBuf {
        PathBuf { inner: s.to_owned() }
    }
}
impl From<&Path> for PathBuf {
    fn from(p: &Path) -> PathBuf {
        p.to_path_buf()
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.inner)
    }
}
impl fmt::Debug for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", &self.inner)
    }
}
impl fmt::Display for PathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.inner)
    }
}
impl fmt::Debug for PathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", &self.inner)
    }
}
