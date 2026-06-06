//! The sysroot `std` for the fullrust target.
//!
//! `cargo fullrust` compiles this as the crate named `std` and places it in a
//! sysroot, so an **unmodified** crate (`fn main`, `use std::…`, no deps, no
//! attributes) builds into a libc-free static binary. It re-exports the
//! `purestd` API surface as `std::…` and supplies the one piece purestd can't on
//! stable: the `#[lang = "start"]` glue the compiler-generated `main` shim calls.
//!
//! purestd provides the `#[panic_handler]`, the `#[global_allocator]`, and
//! `rust_eh_personality`; the `fullrust` runtime provides `_start` and the
//! `mem*`/unwind/`getauxval` symbols. This crate just wires them together as
//! `std` and bridges `main` → purestd's entry shim.
//!
//! This file is data embedded in `cargo-fullrust`; it is written into a
//! generated crate at build time. It is nightly-only (lang items).

#![no_std]
#![feature(lang_items)]
#![allow(internal_features)]

extern crate alloc;
// Pull in the runtime symbols (_start, mem*, unwind, getauxval).
extern crate fullrust;

// The whole purestd API, re-exported as `std::…` (io, fs, net, time, env,
// process, thread, sync, collections, error, ffi, path, the core/alloc module
// mirrors, and `prelude`). The compiler injects `std::prelude::rust_2021`, which
// purestd defines (with the standard macros).
pub use purestd::*;
// Macros aren't carried by a glob re-export; bring them to the crate root so
// `std::println!` etc. resolve.
pub use purestd::{eprint, eprintln, print, println};

/// The `start` lang item: the compiler-generated `main` shim calls this. We hand
/// off to purestd's entry shim, which records `argc`/`argv`/`envp` and converts
/// the user `main`'s return value via `Termination`. `envp` follows `argv`'s
/// NULL terminator on the initial stack.
#[lang = "start"]
fn lang_start<T: purestd::rt::Termination>(
    main: fn() -> T,
    argc: isize,
    argv: *const *const u8,
    _sigpipe: u8,
) -> isize {
    let argc = argc as usize;
    let envp = unsafe { argv.add(argc + 1) };
    unsafe { purestd::rt::__entry(argc, argv, envp, main) as isize }
}
