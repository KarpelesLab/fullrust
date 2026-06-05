//! x86-64 Linux: syscall instruction wrappers, entry point, syscall numbers.
//!
//! ## Syscall ABI
//! Number in `rax`; arguments in `rdi, rsi, rdx, r10, r8, r9`; result in `rax`.
//! The `syscall` instruction clobbers `rcx` and `r11`. Errors are returned as
//! `-errno` in the range `[-4095, -1]` (handled in [`crate::syscall`]).
//!
//! ## Process entry ABI
//! On `execve`, the kernel jumps to `_start` with `rsp` pointing at `argc`,
//! immediately followed by the `argv` pointers, a NULL, the `envp` pointers, a
//! NULL, and finally the auxiliary vector. There is no return address and no
//! C runtime: `_start` must never return.

use core::arch::asm;
#[cfg(feature = "rt")]
use core::arch::naked_asm;

macro_rules! syscall_fn {
    ($name:ident; $($arg:ident => $reg:tt),*) => {
        /// Issue a raw `syscall`. `n` is the syscall number; see [`nr`].
        ///
        /// # Safety
        /// The caller must uphold the kernel's contract for syscall `n`:
        /// valid pointers/lengths, correct argument count, etc.
        #[inline]
        pub unsafe fn $name(n: usize $(, $arg: usize)*) -> usize {
            let ret;
            asm!(
                "syscall",
                inlateout("rax") n => ret,
                $(in($reg) $arg,)*
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack, preserves_flags),
            );
            ret
        }
    };
}

syscall_fn!(syscall0;);
syscall_fn!(syscall1; a => "rdi");
syscall_fn!(syscall2; a => "rdi", b => "rsi");
syscall_fn!(syscall3; a => "rdi", b => "rsi", c => "rdx");
syscall_fn!(syscall4; a => "rdi", b => "rsi", c => "rdx", d => "r10");
syscall_fn!(syscall5; a => "rdi", b => "rsi", c => "rdx", d => "r10", e => "r8");
syscall_fn!(syscall6; a => "rdi", b => "rsi", c => "rdx", d => "r10", e => "r8", f => "r9");

/// Kernel entry point. The bootstrap that replaces crt0/`_start` from libc.
///
/// It captures the initial stack pointer (which points at `argc`), aligns the
/// stack to 16 bytes as the SysV ABI requires before a `call`, and hands off to
/// the Rust-level [`crate::start::rust_start`], which never returns.
#[cfg(feature = "rt")]
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    naked_asm!(
        "xor rbp, rbp",            // mark the outermost frame (ABI hygiene)
        "mov rdi, rsp",            // arg 0 = pointer to argc on the stack
        "and rsp, -16",            // 16-byte align the stack before the call
        "call {start}",
        start = sym crate::start::rust_start,
    )
}

/// Linux/x86-64 syscall numbers (the subset fullrust uses).
pub mod nr {
    pub const READ: usize = 0;
    pub const WRITE: usize = 1;
    pub const CLOSE: usize = 3;
    pub const LSEEK: usize = 8;
    pub const MMAP: usize = 9;
    pub const MUNMAP: usize = 11;
    pub const EXIT: usize = 60;
    pub const EXIT_GROUP: usize = 231;
    pub const OPENAT: usize = 257;
    pub const GETRANDOM: usize = 318;
}

/// `dirfd` value meaning "paths are relative to the current working directory".
pub const AT_FDCWD: isize = -100;
