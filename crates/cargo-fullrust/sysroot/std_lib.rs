//! The sysroot `std` for the fullrust target.
//!
//! `cargo fullrust` compiles this as the crate named `std` and places it in a
//! sysroot, so an **unmodified** crate (`fn main`, `use std::…`, no deps, no
//! attributes) builds into a libc-free static binary. It re-exports the
//! `fullrust-std` API surface and supplies the binary policy that the real std
//! would otherwise provide: program entry, panic handler, and global allocator.
//!
//! This file is data embedded in `cargo-fullrust`; it is written into a
//! generated crate at build time. It is nightly-only (lang items + naked fns).

#![no_std]
#![feature(lang_items)]
#![allow(internal_features)]

extern crate alloc;

// The whole fullrust-std API, re-exported as `std::…` (io, fs, net, time, env,
// process, thread, sync, collections, error, ffi, path, os, plus the core/alloc
// module mirrors and `prelude`). The compiler injects `std::prelude::rust_2021`,
// which fullrust-std defines (with the standard macros).
pub use fullrust_std::*;
// Macros aren't carried by a glob re-export; bring them to the crate root so
// `std::println!` etc. resolve.
pub use fullrust_std::{eprint, eprintln, print, println};

// ---- binary policy (what the real std runtime provides) ----

#[global_allocator]
static GLOBAL: fullrust::Allocator = fullrust::Allocator::new();

/// The `start` lang item: the compiler-generated `main` shim calls this. We run
/// the user's `main` and exit 0 (return-type/`Termination` handling is TODO).
#[lang = "start"]
fn lang_start<T>(main: fn() -> T, _argc: isize, _argv: *const *const u8, _sigpipe: u8) -> isize {
    main();
    0
}

extern "C" {
    // The C-ABI entry the compiler emits for a normal `fn main` crate.
    fn main(argc: isize, argv: *const *const u8) -> isize;
}

/// Kernel entry: capture the initial stack, parse argc/argv/envp, run `main`.
#[unsafe(no_mangle)]
#[unsafe(naked)]
extern "C" fn _start() -> ! {
    core::arch::naked_asm!(
        "xor rbp, rbp",
        "mov rdi, rsp",       // pointer to argc
        "and rsp, -16",
        "call {entry}",
        entry = sym rt_entry,
    );
}

unsafe extern "C" fn rt_entry(stack: *const usize) -> ! {
    let (argc, argv) = fullrust::env::init_from_stack(stack);
    let code = main(argc, argv);
    fullrust::rt::exit(code as i32)
}

#[panic_handler]
fn rust_panic(info: &core::panic::PanicInfo) -> ! {
    use core::fmt::Write;
    let mut err = fullrust::io::stderr();
    let _ = writeln!(err, "panic: {info}");
    fullrust::rt::abort()
}
