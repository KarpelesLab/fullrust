//! `std::ffi`: `CStr`/`CString` re-exported, plus UTF-8-backed `OsStr`/`OsString`.

pub use alloc::ffi::CString;
pub use core::ffi::{c_char, c_int, c_void, CStr};

use alloc::borrow::ToOwned;
use alloc::string::String;
use core::fmt;

/// Borrowed OS string (UTF-8 only in this shim).
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct OsStr {
    inner: str,
}

/// Owned OS string (UTF-8 only in this shim).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct OsString {
    inner: String,
}

impl OsStr {
    pub fn new<S: AsRef<str> + ?Sized>(s: &S) -> &OsStr {
        unsafe { &*(s.as_ref() as *const str as *const OsStr) }
    }
    pub fn to_str(&self) -> Option<&str> {
        Some(&self.inner)
    }
    pub fn to_string_lossy(&self) -> &str {
        &self.inner
    }
    pub fn to_os_string(&self) -> OsString {
        OsString {
            inner: self.inner.to_owned(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

impl OsString {
    pub fn new() -> OsString {
        OsString {
            inner: String::new(),
        }
    }
    pub fn from<S: Into<String>>(s: S) -> OsString {
        OsString { inner: s.into() }
    }
    pub fn as_os_str(&self) -> &OsStr {
        OsStr::new(&self.inner)
    }
    pub fn into_string(self) -> Result<String, OsString> {
        Ok(self.inner)
    }
    pub fn to_str(&self) -> Option<&str> {
        Some(&self.inner)
    }
    pub fn push<S: AsRef<str>>(&mut self, s: S) {
        self.inner.push_str(s.as_ref());
    }
}

impl core::ops::Deref for OsString {
    type Target = OsStr;
    fn deref(&self) -> &OsStr {
        OsStr::new(&self.inner)
    }
}

impl From<String> for OsString {
    fn from(s: String) -> OsString {
        OsString { inner: s }
    }
}
impl From<&str> for OsString {
    fn from(s: &str) -> OsString {
        OsString {
            inner: s.to_owned(),
        }
    }
}
impl AsRef<OsStr> for OsStr {
    fn as_ref(&self) -> &OsStr {
        self
    }
}
impl AsRef<OsStr> for str {
    fn as_ref(&self) -> &OsStr {
        OsStr::new(self)
    }
}
impl AsRef<OsStr> for String {
    fn as_ref(&self) -> &OsStr {
        OsStr::new(self.as_str())
    }
}
impl AsRef<OsStr> for OsString {
    fn as_ref(&self) -> &OsStr {
        self.as_os_str()
    }
}

impl fmt::Debug for OsStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", &self.inner)
    }
}
impl fmt::Display for OsStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.inner)
    }
}
impl fmt::Debug for OsString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", &self.inner)
    }
}
impl fmt::Display for OsString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.inner)
    }
}
