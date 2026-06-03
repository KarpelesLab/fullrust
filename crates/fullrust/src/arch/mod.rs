//! Architecture-specific support.
//!
//! Everything that knows about a particular CPU/ABI lives here: the raw
//! `syscall` instruction sequence, the kernel entry point `_start`, and the
//! Linux syscall number table for the target. To port fullrust to another
//! architecture, add a sibling module and re-export it below — the rest of the
//! crate is written against [`syscallN`](self) and [`nr`](self::nr) only.

#[cfg(target_arch = "x86_64")]
mod x86_64;
#[cfg(target_arch = "x86_64")]
pub use x86_64::*;

#[cfg(not(target_arch = "x86_64"))]
compile_error!(
    "fullrust currently only supports target_arch = \"x86_64\". \
     Add an arch module under src/arch/ and re-export it in arch/mod.rs."
);
