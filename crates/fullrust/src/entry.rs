//! Process entry point — crt0's job.
//!
//! The kernel jumps to `_start` with the initial stack laid out as:
//!
//! ```text
//! sp ->  argc                       (usize)
//!        argv[0..argc-1], NULL       (*const u8 each)
//!        envp[0..],       NULL       (*const u8 each)
//!        auxv ...
//! ```
//!
//! A naked `_start` captures `sp` untouched, then `rust_start` decodes
//! `argc`/`argv`/`envp` and tail-calls purestd's `main` (emitted by
//! `purestd::entry!`). When `main` returns, we `exit_group` with its code.

extern "C" {
    /// The program entry, emitted by `purestd::entry!` as the C `main` symbol.
    fn main(argc: usize, argv: *const *const u8, envp: *const *const u8) -> i32;
}

#[cfg(target_arch = "x86_64")]
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    core::arch::naked_asm!(
        "xor rbp, rbp",
        "mov rdi, rsp",    // pointer to argc
        "and rsp, -16",
        "call {s}",
        s = sym rust_start,
    )
}

#[cfg(target_arch = "aarch64")]
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    core::arch::naked_asm!(
        "mov x0, sp",
        "and x1, x0, #-16",
        "mov sp, x1",
        "b {s}",
        s = sym rust_start,
    )
}

unsafe extern "C" fn rust_start(stack: *const usize) -> ! {
    let argc = *stack;
    let argv = stack.add(1) as *const *const u8;
    // argv[argc] is NULL; envp follows it.
    let envp = argv.add(argc + 1);
    let code = main(argc, argv, envp);
    exit_group(code)
}

/// `exit_group(code)` — terminate the whole process. Never returns.
unsafe fn exit_group(code: i32) -> ! {
    #[cfg(target_arch = "x86_64")]
    core::arch::asm!(
        "syscall",
        in("rax") 231usize, // SYS_exit_group
        in("rdi") code as usize,
        options(noreturn, nostack),
    );
    #[cfg(target_arch = "aarch64")]
    core::arch::asm!(
        "svc #0",
        in("x8") 94usize,   // SYS_exit_group
        in("x0") code as usize,
        options(noreturn, nostack),
    );
}
