//! `std::os` — the Unix fd and OsStr extension traits.

pub mod unix {
    pub mod io {
        /// A raw file descriptor.
        pub type RawFd = i32;

        pub trait AsRawFd {
            fn as_raw_fd(&self) -> RawFd;
        }
        pub trait FromRawFd {
            /// # Safety
            /// `fd` must be a valid, owned descriptor.
            unsafe fn from_raw_fd(fd: RawFd) -> Self;
        }
        pub trait IntoRawFd {
            fn into_raw_fd(self) -> RawFd;
        }
    }

    pub mod ffi {
        use crate::ffi::{OsStr, OsString};
        use alloc::string::String;
        use alloc::vec::Vec;

        pub trait OsStrExt {
            fn as_bytes(&self) -> &[u8];
        }
        impl OsStrExt for OsStr {
            fn as_bytes(&self) -> &[u8] {
                self.to_str().unwrap_or("").as_bytes()
            }
        }

        pub trait OsStringExt {
            fn from_vec(vec: Vec<u8>) -> Self;
            fn into_vec(self) -> Vec<u8>;
        }
        impl OsStringExt for OsString {
            fn from_vec(vec: Vec<u8>) -> Self {
                OsString::from(String::from_utf8(vec).unwrap_or_default())
            }
            fn into_vec(self) -> Vec<u8> {
                self.into_string().unwrap_or_default().into_bytes()
            }
        }
    }
}

/// `std::os::fd` re-exports.
pub mod fd {
    pub use super::unix::io::{AsRawFd, FromRawFd, IntoRawFd, RawFd};
}
