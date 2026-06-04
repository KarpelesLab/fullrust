//! Process control: exit, abort, and pid.

use crate::sys;

/// Terminate the process with `code`. Never returns.
pub fn exit(code: i32) -> ! {
    fullrust::rt::exit(code)
}

/// Abort the process. Never returns.
pub fn abort() -> ! {
    fullrust::rt::abort()
}

/// This process's PID.
pub fn id() -> u32 {
    unsafe { sys::sc0(sys::nr::GETPID).unwrap_or(0) as u32 }
}

/// A process exit status (minimal stand-in for `std::process::ExitCode`).
#[derive(Clone, Copy, Debug)]
pub struct ExitCode(pub i32);

impl ExitCode {
    pub const SUCCESS: ExitCode = ExitCode(0);
    pub const FAILURE: ExitCode = ExitCode(1);
}
