//! The single `#[panic_handler]` for any fullrust program.
//!
//! Prints the panic message and location to stderr, then aborts. No unwinding
//! happens (we build with `panic = "abort"`), so this never returns and never
//! touches `_Unwind_*`.

use crate::rt;
use core::fmt::Write;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // `PanicInfo`'s Display includes the message and the source location.
    let mut err = crate::io::stderr();
    let _ = writeln!(err, "panic: {}", info);
    rt::abort()
}
