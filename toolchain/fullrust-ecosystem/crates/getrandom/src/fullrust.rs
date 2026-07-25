//! Implementation for the `fullrust` target (libc-free Rust std on the Linux
//! kernel). Entropy comes from the Linux `getrandom(2)` syscall issued directly
//! — exactly what `std::sys::random` does on this target — so no libc and no
//! `/dev/urandom` file descriptor are needed.
use crate::Error;
use core::{arch::asm, mem::MaybeUninit, num::NonZeroU32};

// x86_64 Linux syscall numbers / errno used here. `fullrust`'s `llvm_target` is
// `x86_64-unknown-linux-gnu`, so the kernel ABI is identical to Linux.
const SYS_GETRANDOM: isize = 318;
const EINTR: u32 = 4;

pub fn getrandom_inner(mut dest: &mut [MaybeUninit<u8>]) -> Result<(), Error> {
    while !dest.is_empty() {
        // getrandom(buf, buflen, flags = 0): draw from the urandom source,
        // blocking only until it has been seeded (early boot). Returns the
        // number of bytes written, or a negated errno.
        let ret: isize;
        unsafe {
            asm!(
                "syscall",
                inlateout("rax") SYS_GETRANDOM => ret,
                in("rdi") dest.as_mut_ptr(),
                in("rsi") dest.len(),
                in("rdx") 0,           // flags
                lateout("rcx") _,      // clobbered by syscall
                lateout("r11") _,      // clobbered by syscall
                options(nostack, preserves_flags),
            );
        }
        match ret {
            // Progress: advance past the bytes the kernel filled.
            n if n > 0 => dest = dest.get_mut(n as usize..).ok_or(Error::UNEXPECTED)?,
            n if n < 0 => {
                let errno = (-n) as u32;
                if errno == EINTR {
                    continue;
                }
                // `errno` is a small positive value, far below `INTERNAL_START`,
                // and non-zero because `ret < 0`.
                return Err(Error::from(NonZeroU32::new(errno).ok_or(Error::UNEXPECTED)?));
            }
            // ret == 0: EOF from an infinite random stream is impossible.
            _ => return Err(Error::UNEXPECTED),
        }
    }
    Ok(())
}
