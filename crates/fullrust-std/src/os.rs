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

    pub mod fs {
        //! Unix extensions for filesystem types.
        use crate::fs::{Metadata, OpenOptions};

        pub trait OpenOptionsExt {
            fn mode(&mut self, mode: u32) -> &mut Self;
            fn custom_flags(&mut self, flags: i32) -> &mut Self;
        }
        impl OpenOptionsExt for OpenOptions {
            fn mode(&mut self, mode: u32) -> &mut Self {
                self.mode = mode;
                self
            }
            fn custom_flags(&mut self, flags: i32) -> &mut Self {
                self.custom_flags = flags;
                self
            }
        }

        pub trait MetadataExt {
            fn mode(&self) -> u32;
            fn uid(&self) -> u32;
            fn gid(&self) -> u32;
            fn size(&self) -> u64;
            fn ino(&self) -> u64;
            fn nlink(&self) -> u64;
            fn mtime(&self) -> i64;
        }
        impl MetadataExt for Metadata {
            fn mode(&self) -> u32 {
                self.mode
            }
            fn uid(&self) -> u32 {
                self.uid
            }
            fn gid(&self) -> u32 {
                self.gid
            }
            fn size(&self) -> u64 {
                self.size
            }
            fn ino(&self) -> u64 {
                self.ino
            }
            fn nlink(&self) -> u64 {
                self.nlink
            }
            fn mtime(&self) -> i64 {
                self.mtime.0
            }
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
