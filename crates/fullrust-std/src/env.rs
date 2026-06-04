//! Command-line arguments and environment variables (backed by `fullrust::env`).

use crate::path::PathBuf;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Iterator over the process arguments as `String`s.
pub struct Args {
    inner: alloc::vec::IntoIter<String>,
}

impl Iterator for Args {
    type Item = String;
    fn next(&mut self) -> Option<String> {
        self.inner.next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}
impl ExactSizeIterator for Args {
    fn len(&self) -> usize {
        self.inner.len()
    }
}
impl DoubleEndedIterator for Args {
    fn next_back(&mut self) -> Option<String> {
        self.inner.next_back()
    }
}

/// The process arguments (including argv[0]).
pub fn args() -> Args {
    let v: Vec<String> = fullrust::env::args().map(|s| s.to_string()).collect();
    Args { inner: v.into_iter() }
}

/// Error from [`var`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum VarError {
    NotPresent,
    NotUnicode,
}

impl core::fmt::Display for VarError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            VarError::NotPresent => f.write_str("environment variable not found"),
            VarError::NotUnicode => f.write_str("environment variable was not valid unicode"),
        }
    }
}
impl core::error::Error for VarError {}

/// Look up an environment variable.
pub fn var<K: AsRef<str>>(key: K) -> Result<String, VarError> {
    match fullrust::env::var(key.as_ref()) {
        Some(v) => Ok(v.to_string()),
        None => Err(VarError::NotPresent),
    }
}

/// Iterator over `(key, value)` environment pairs.
pub fn vars() -> impl Iterator<Item = (String, String)> {
    fullrust::env::vars().map(|(k, v)| (k.to_string(), v.to_string()))
}

/// The system temporary directory (`$TMPDIR` or `/tmp`).
pub fn temp_dir() -> PathBuf {
    match var("TMPDIR") {
        Ok(d) if !d.is_empty() => PathBuf::from(d),
        _ => PathBuf::from("/tmp"),
    }
}
