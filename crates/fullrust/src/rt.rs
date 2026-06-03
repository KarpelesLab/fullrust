//! Process lifetime: exit, abort, and the `main` return-value conversion.

use crate::syscall;

/// Exit the process with the given status code. Never returns.
#[inline]
pub fn exit(code: i32) -> ! {
    syscall::exit_group(code)
}

/// Abort the process (exit code 134, mimicking `SIGABRT`'s 128+6). Never
/// returns. Used by the panic handler and unreachable fallbacks.
#[inline]
pub fn abort() -> ! {
    syscall::exit_group(134)
}

/// Conversion from a `main` return value into a process exit code.
///
/// Implemented for the types you can return from `#[fullrust::main] fn main`.
/// This mirrors the role of `std::process::Termination`.
pub trait Termination {
    /// Consume the value and produce an exit code.
    fn report(self) -> i32;
}

impl Termination for () {
    #[inline]
    fn report(self) -> i32 {
        0
    }
}

impl Termination for i32 {
    #[inline]
    fn report(self) -> i32 {
        self
    }
}

impl<T: Termination, E: core::fmt::Debug> Termination for Result<T, E> {
    #[inline]
    fn report(self) -> i32 {
        match self {
            Ok(v) => v.report(),
            Err(e) => {
                crate::eprintln!("Error: {:?}", e);
                1
            }
        }
    }
}
