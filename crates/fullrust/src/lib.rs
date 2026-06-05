//! # fullrust
//!
//! A libc-free, fully-static, pure-Rust runtime for Linux. It provides a
//! "std-lite" experience — heap allocation, `print!`/`println!`, command-line
//! arguments and environment access — on top of raw kernel syscalls, with **no
//! libc and no C runtime** linked in.
//!
//! See the workspace `README.md` for the full method and build instructions.
//! In short, a program looks like:
//!
//! ```ignore
//! #![no_std]
//! #![no_main]
//! use fullrust::prelude::*;
//!
//! #[fullrust::main]
//! fn main() {
//!     println!("hello from libc-free rust");
//! }
//! ```
//!
//! ## How it fits together
//!
//! * [`arch`] holds the only architecture-specific code: the raw `syscall`
//!   instruction wrappers, the kernel entry point `_start`, and the syscall
//!   number table.
//! * [`syscall`] turns those into arch-neutral, `Result`-returning wrappers.
//! * [`start`] parses `argc`/`argv`/`envp` from the initial stack and calls the
//!   user `main` (exported as `__fullrust_main` by the [`macro@main`] attribute).
//! * [`allocator`] is an mmap-backed global allocator, enabling [`alloc`]
//!   (`Box`, `Vec`, `String`, `format!`).
//! * [`intrinsics`] supplies the `mem*` functions and unwind abort-stubs that
//!   the compiler and the precompiled `alloc` expect when no libc is present.

#![no_std]
// build-std may define cfgs we don't know about; keep the build quiet.
#![allow(unexpected_cfgs)]

extern crate alloc;

/// Define the program entry point.
///
/// The kernel-level `_start` (in [`arch`]) calls the C-ABI symbol
/// `__fullrust_main`; this macro generates it as a thin wrapper that calls your
/// function and converts its return value into an exit code via
/// [`rt::Termination`]. Place it once, after defining `main`:
///
/// ```ignore
/// #![no_std]
/// #![no_main]
/// use fullrust::prelude::*;
///
/// fn main() {
///     println!("hello from libc-free rust");
/// }
/// fullrust::entry!(main);
/// ```
///
/// The function may return `()`, `i32`, or `Result<_, E: Debug>`.
#[macro_export]
macro_rules! entry {
    ($main:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn __fullrust_main() -> i32 {
            $crate::rt::Termination::report($main())
        }
    };
}

pub mod arch;
pub mod env;
pub mod io;
pub mod prelude;
pub mod rt;
pub mod syscall;
pub mod tls;

mod allocator;
mod intrinsics;

// Binary-level *policy* symbols (`_start`, `#[panic_handler]`, the
// `#[global_allocator]` static, the `__fullrust_main` entry glue). Exactly one
// crate in the final binary may define each of these, so they live behind the
// default `rt` feature. The `entry!` model keeps `rt` on; the sysroot `std`
// crate turns it off and supplies its own policy. The *mechanisms* (syscalls,
// the `Allocator` type, mem intrinsics, io/env/rt helpers) are always available.
#[cfg(feature = "rt")]
mod panic;
#[cfg(feature = "rt")]
mod start;

pub use allocator::Allocator;
