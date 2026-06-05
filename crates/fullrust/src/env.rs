//! Command-line arguments and environment variables.
//!
//! The pointers are captured once from the initial stack by
//! [`crate::start::rust_start`]; everything here is read-only afterwards and the
//! borrowed strings live for the whole process (`'static`).

use core::ffi::CStr;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

static ARGC: AtomicUsize = AtomicUsize::new(0);
static ARGV: AtomicPtr<*const u8> = AtomicPtr::new(core::ptr::null_mut());
static ENVP: AtomicPtr<*const u8> = AtomicPtr::new(core::ptr::null_mut());

/// Record the kernel-provided argument/environment pointers.
///
/// # Safety
/// Must be called exactly once, from the bootstrap, with valid `argv`/`envp`
/// arrays as laid out by the kernel.
pub unsafe fn init(argc: usize, argv: *const *const u8, envp: *const *const u8) {
    ARGC.store(argc, Ordering::Relaxed);
    ARGV.store(argv as *mut *const u8, Ordering::Relaxed);
    ENVP.store(envp as *mut *const u8, Ordering::Relaxed);
}

/// Parse the kernel-provided initial stack (pointer to `argc`) and record the
/// argument/environment pointers. Returns `(argc, argv)` for handing to a C-ABI
/// `main`. Used by the `entry!` bootstrap and the sysroot `std`.
///
/// # Safety
/// `stack` must be the initial stack pointer as supplied by the kernel.
pub unsafe fn init_from_stack(stack: *const usize) -> (isize, *const *const u8) {
    let argc = *stack;
    let argv = stack.add(1) as *const *const u8;
    // envp begins one slot past argv's NULL terminator.
    let envp = stack.add(1 + argc + 1) as *const *const u8;
    init(argc, argv, envp);
    (argc as isize, argv)
}

/// Number of command-line arguments (including the program name).
#[inline]
pub fn argc() -> usize {
    ARGC.load(Ordering::Relaxed)
}

/// Iterator over the command-line arguments as raw byte slices (no NUL).
///
/// The first item is conventionally the program name.
pub fn args_bytes() -> impl Iterator<Item = &'static [u8]> {
    let argv = ARGV.load(Ordering::Relaxed) as *const *const u8;
    let n = argc();
    (0..n).map(move |i| unsafe {
        let p = *argv.add(i);
        CStr::from_ptr(p as *const core::ffi::c_char).to_bytes()
    })
}

/// Iterator over the command-line arguments as `&str`.
///
/// Arguments that are not valid UTF-8 are yielded as an empty string. Use
/// [`args_bytes`] if you need the raw bytes.
pub fn args() -> impl Iterator<Item = &'static str> {
    args_bytes().map(|b| core::str::from_utf8(b).unwrap_or(""))
}

/// Iterator over `(key, value)` environment pairs as `&str`.
///
/// Entries that are not valid UTF-8, or that contain no `=`, are skipped.
pub fn vars() -> impl Iterator<Item = (&'static str, &'static str)> {
    EnvIter::new().filter_map(|entry| {
        let s = core::str::from_utf8(entry).ok()?;
        let eq = s.find('=')?;
        Some((&s[..eq], &s[eq + 1..]))
    })
}

/// Look up a single environment variable by name.
pub fn var(key: &str) -> Option<&'static str> {
    vars().find(|(k, _)| *k == key).map(|(_, v)| v)
}

/// Walks the NULL-terminated `envp` array, yielding raw entry bytes.
struct EnvIter {
    p: *const *const u8,
}

impl EnvIter {
    fn new() -> Self {
        EnvIter {
            p: ENVP.load(Ordering::Relaxed) as *const *const u8,
        }
    }
}

impl Iterator for EnvIter {
    type Item = &'static [u8];
    fn next(&mut self) -> Option<&'static [u8]> {
        if self.p.is_null() {
            return None;
        }
        unsafe {
            let entry = *self.p;
            if entry.is_null() {
                return None;
            }
            self.p = self.p.add(1);
            Some(CStr::from_ptr(entry as *const core::ffi::c_char).to_bytes())
        }
    }
}
