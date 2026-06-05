//! `fullrust-std` — a pure-Rust, libc-free replacement for a useful subset of
//! the standard library, backed by [`fullrust`] syscalls.
//!
//! It is meant to be aliased as `std` on the freestanding `fullrust` target:
//!
//! ```ignore
//! #[cfg(target_vendor = "fullrust")]
//! extern crate fullrust_std as std;
//! ```
//!
//! Then existing `use std::io::Write;`, `std::fs::read(..)`, `std::time::…`,
//! etc. resolve here instead of the real `std`, and the program links with no
//! libc. The goal is source-compatibility for the parts real programs actually
//! use, not 100% fidelity.

#![no_std]
#![allow(clippy::all)]

extern crate alloc;

// ---------------------------------------------------------------------------
// Re-export the bulk of `core` and `alloc` under the `std::` namespace, so the
// many `std::mem`, `std::cmp`, `std::fmt`, `std::vec`, `std::collections::BTreeMap`
// paths resolve unchanged.
// ---------------------------------------------------------------------------

pub use core::{
    any, arch, ascii, cell, char, clone, cmp, convert, default, future, hash, hint, iter, marker,
    mem, num, ops, option, panic as core_panic, pin, primitive, ptr, result, slice, str, task,
};
pub use core::{
    assert, assert_eq, assert_ne, debug_assert, debug_assert_eq, debug_assert_ne, format_args,
    matches, todo, unimplemented, unreachable, write, writeln,
};

pub use alloc::{borrow, boxed, fmt, format, rc, string, vec};

pub mod sync {
    //! Synchronization primitives.
    pub use crate::sync_impl::{
        Mutex, MutexGuard, Once, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard,
    };
    pub use alloc::sync::{Arc, Weak};
    pub use core::sync::atomic;
}

pub mod collections {
    //! Collections: ordered ones from `alloc`, hash ones from `hashbrown`.
    pub use alloc::collections::{
        btree_map, btree_set, BTreeMap, BTreeSet, BinaryHeap, LinkedList, VecDeque,
    };
    pub use hashbrown::hash_map;
    pub use hashbrown::hash_set;
    pub use hashbrown::{HashMap, HashSet};
}

pub mod time {
    //! Time: `Duration` from `core`, plus syscall-backed clocks.
    pub use crate::time_impl::{Instant, SystemTime, SystemTimeError, UNIX_EPOCH};
    pub use core::time::Duration;
}

// OS-backed modules implemented in this crate.
pub mod env;
pub mod error;
pub mod ffi;
pub mod fs;
pub mod io;
pub mod net;
pub mod os;
pub mod path;
pub mod process;
pub mod thread;

#[path = "sync.rs"]
mod sync_impl;
#[path = "time.rs"]
mod time_impl;

mod sys;

pub mod panic {
    //! Panic support. Under `panic = "abort"` there is no real unwinding, so
    //! `catch_unwind` simply runs the closure (a panic aborts the process).
    pub use core::panic::{Location, PanicInfo};

    /// Transparent wrapper mirroring `std::panic::AssertUnwindSafe`.
    pub struct AssertUnwindSafe<T>(pub T);

    impl<T> core::ops::Deref for AssertUnwindSafe<T> {
        type Target = T;
        fn deref(&self) -> &T {
            &self.0
        }
    }

    /// Run `f`. With `panic = "abort"` a panic aborts the process, so this
    /// never actually returns `Err`; the signature mirrors `std`.
    pub fn catch_unwind<F: FnOnce() -> R, R>(
        f: F,
    ) -> Result<R, alloc::boxed::Box<dyn core::any::Any + Send>> {
        Ok(f())
    }
}

/// Print to standard output.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => { $crate::io::_print(::core::format_args!($($arg)*)) };
}
/// Print to standard output, with a trailing newline.
#[macro_export]
macro_rules! println {
    () => { $crate::io::_print(::core::format_args!("\n")) };
    ($($arg:tt)*) => { $crate::io::_print(::core::format_args!("{}\n", ::core::format_args!($($arg)*))) };
}
/// Print to standard error.
#[macro_export]
macro_rules! eprint {
    ($($arg:tt)*) => { $crate::io::_eprint(::core::format_args!($($arg)*)) };
}
/// Print to standard error, with a trailing newline.
#[macro_export]
macro_rules! eprintln {
    () => { $crate::io::_eprint(::core::format_args!("\n")) };
    ($($arg:tt)*) => { $crate::io::_eprint(::core::format_args!("{}\n", ::core::format_args!($($arg)*))) };
}

// A `prelude` mirroring `std`'s. The edition-named submodules (`rust_2021`,
// `rust_2024`) are what the compiler auto-injects when this crate is used as the
// sysroot `std`, so they must include the standard macros (`println!`, `vec!`,
// `format!`, …) as well as the common `alloc` types.
pub mod prelude {
    pub mod v1 {
        pub use crate::{eprint, eprintln, print, println};
        pub use alloc::borrow::ToOwned;
        pub use alloc::boxed::Box;
        pub use alloc::string::{String, ToString};
        pub use alloc::vec::Vec;
        pub use alloc::{format, vec};
        pub use core::prelude::v1::*;
    }
    pub mod rust_2021 {
        pub use crate::prelude::v1::*;
        pub use core::prelude::rust_2021::*;
    }
    pub mod rust_2024 {
        pub use crate::prelude::v1::*;
        pub use core::prelude::rust_2024::*;
    }
    pub use v1::*;
}
