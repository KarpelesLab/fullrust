//! Rust-level bootstrap: runs after the assembly `_start`, before user `main`.
//!
//! The initial stack laid out by the kernel is:
//!
//! ```text
//! rsp ->  argc                 (usize)
//!         argv[0] .. argv[argc-1], NULL   (*const u8 each)
//!         envp[0] .. envp[m-1],   NULL    (*const u8 each)
//!         auxv ...                        (pairs, terminated by AT_NULL)
//! ```

use crate::{env, rt, syscall};

extern "C" {
    /// Defined by the user's program via `#[fullrust::main]`.
    fn __fullrust_main() -> i32;
}

/// Entry called by the assembly `_start` with a pointer to `argc`.
///
/// # Safety
/// `stack` must be the initial stack pointer as supplied by the kernel.
pub(crate) unsafe extern "C" fn rust_start(stack: *const usize) -> ! {
    let _ = env::init_from_stack(stack);

    let code = __fullrust_main();
    syscall::exit_group(code);

    // exit_group never returns, but keep the type-checker happy.
    #[allow(unreachable_code)]
    rt::abort()
}
