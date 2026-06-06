//! # fullrust — the libc-free runtime for [`purestd`].
//!
//! purestd is the standard library; fullrust is the small runtime it pairs with
//! to produce **fully-static, libc-free** binaries. It provides exactly the
//! pieces a freestanding binary needs that a hosted build gets from **crt0**,
//! **compiler_builtins**, and the **unwinder** — never from std:
//!
//! * the process entry point `_start` (in [`entry`]), which decodes
//!   `argc`/`argv`/`envp` from the initial stack and calls purestd's `main`;
//! * the `mem*` intrinsics, `strlen`, and (on aarch64) `getauxval`;
//! * the `_Unwind_Resume` abort-stub.
//!
//! `rust_eh_personality`, the `#[panic_handler]`, and the `#[global_allocator]`
//! are purestd's job (a real std provides those), so they are deliberately *not*
//! here. A program links both crates:
//!
//! ```ignore
//! #![no_std]
//! #![no_main]
//! extern crate fullrust;            // _start + the toolchain symbols
//! use purestd::prelude::*;
//!
//! fn main() {
//!     println!("hello from libc-free rust");
//! }
//! purestd::entry!(main);
//! ```

#![no_std]
#![allow(internal_features)]

mod entry;
mod intrinsics;
