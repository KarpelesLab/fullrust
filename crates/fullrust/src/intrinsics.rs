//! Symbols the toolchain expects from the environment when no libc is linked.
//!
//! ## `mem*` intrinsics
//! The compiler lowers struct copies, slice fills, etc. to calls to `memcpy`,
//! `memset`, `memmove`, `memcmp`/`bcmp`. With a libc these come from there;
//! without one we must provide them. (On the nightly `build-std` path
//! `compiler_builtins` could also provide them, but we keep a single source of
//! truth here.)
//!
//! These are deliberately simple byte-at-a-time loops. LLVM's loop-idiom pass
//! will *not* rewrite such a loop into a call to the very function it lives in,
//! so naming them `memcpy`/`memset` is safe from self-recursion.
//!
//! ## Unwind stubs
//! The precompiled `liballoc` shipped with the toolchain is built with
//! unwinding, so it carries landing pads that reference `_Unwind_Resume` and
//! `rust_eh_personality`. Under `panic = "abort"` those pads are never
//! executed, but the symbols must still resolve at link time. We provide
//! abort-stubs. (On the `build-std` path `alloc` is recompiled without
//! unwinding and these go unused / are stripped.)

use core::ffi::c_int;

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    let mut i = 0;
    while i < n {
        *dest.add(i) = *src.add(i);
        i += 1;
    }
    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    if (dest as usize) < (src as usize) {
        let mut i = 0;
        while i < n {
            *dest.add(i) = *src.add(i);
            i += 1;
        }
    } else {
        // Copy backwards when regions overlap with dest above src.
        let mut i = n;
        while i > 0 {
            i -= 1;
            *dest.add(i) = *src.add(i);
        }
    }
    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(dest: *mut u8, c: c_int, n: usize) -> *mut u8 {
    let byte = c as u8;
    let mut i = 0;
    while i < n {
        *dest.add(i) = byte;
        i += 1;
    }
    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(a: *const u8, b: *const u8, n: usize) -> c_int {
    let mut i = 0;
    while i < n {
        let (x, y) = (*a.add(i), *b.add(i));
        if x != y {
            return x as c_int - y as c_int;
        }
        i += 1;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bcmp(a: *const u8, b: *const u8, n: usize) -> c_int {
    memcmp(a, b, n)
}

/// Length of a NUL-terminated C string. Used by `core::ffi::CStr::from_ptr`,
/// which we rely on to read `argv`/`envp`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn strlen(s: *const core::ffi::c_char) -> usize {
    let mut n = 0;
    while *s.add(n) != 0 {
        n += 1;
    }
    n
}

// ---- unwind abort-stubs (never executed under panic = "abort") ----

#[unsafe(no_mangle)]
extern "C" fn rust_eh_personality() {}

#[unsafe(no_mangle)]
extern "C" fn _Unwind_Resume() -> ! {
    // Reaching here would mean unwinding is actually happening, which must not
    // occur under panic = "abort". Treat it as a fatal logic error.
    crate::syscall::exit_group(134)
}
