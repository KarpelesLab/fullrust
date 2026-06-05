//! Per-thread thread-local storage (x86-64, ELF variant II).
//!
//! There is no dynamic loader to set up TLS for us, so we do it ourselves:
//! parse the `PT_TLS` program header (the `.tdata`/`.tbss` template) from the
//! aux vector, then give each thread its own copy and point the thread pointer
//! (`%fs`) at it. With that in place, the compiler's `#[thread_local]` accesses
//! (`%fs`-relative, local-exec model) resolve to per-thread data.
//!
//! Layout (variant II): the static TLS block sits *below* the thread pointer at
//! `[tp - tls_size, tp)`, and a small TCB (whose first word is a self-pointer)
//! sits at `tp`.

use crate::{arch, syscall};
use core::sync::atomic::{AtomicUsize, Ordering};

const PT_TLS: u32 = 7;
const AT_NULL: usize = 0;
const AT_PHDR: usize = 3;
const AT_PHENT: usize = 4;
const AT_PHNUM: usize = 5;

const SYS_ARCH_PRCTL: usize = 158;
const ARCH_SET_FS: usize = 0x1002;

const TCB_SIZE: usize = 64;

// The TLS template, captured once from the program headers at startup.
static TPL: AtomicUsize = AtomicUsize::new(0); // template address (0 = no TLS)
static FILESZ: AtomicUsize = AtomicUsize::new(0);
static MEMSZ: AtomicUsize = AtomicUsize::new(0);
static ALIGN: AtomicUsize = AtomicUsize::new(1);

#[inline]
fn align_up(x: usize, a: usize) -> usize {
    if a <= 1 {
        x
    } else {
        (x + a - 1) & !(a - 1)
    }
}

/// Parse `PT_TLS` from the aux vector and install TLS for the main thread.
///
/// # Safety
/// `stack` must be the kernel-provided initial stack pointer.
pub unsafe fn install_main(stack: *const usize) {
    parse_phdr_tls(stack);
    let (_, _, tp) = new_block();
    if tp != 0 {
        set_fs(tp);
    }
}

unsafe fn parse_phdr_tls(stack: *const usize) {
    // argc, argv[argc], NULL, envp..., NULL, auxv...
    let argc = *stack;
    let mut p = stack.add(1 + argc + 1); // envp
    while *p != 0 {
        p = p.add(1);
    }
    p = p.add(1); // auxv

    let (mut phdr, mut phent, mut phnum) = (0usize, 0usize, 0usize);
    loop {
        let t = *p;
        let v = *p.add(1);
        match t {
            AT_NULL => break,
            AT_PHDR => phdr = v,
            AT_PHENT => phent = v,
            AT_PHNUM => phnum = v,
            _ => {}
        }
        p = p.add(2);
    }
    if phdr == 0 || phent == 0 {
        return;
    }
    for i in 0..phnum {
        let h = (phdr + i * phent) as *const u8;
        if *(h as *const u32) == PT_TLS {
            // Elf64_Phdr: p_vaddr@16, p_filesz@32, p_memsz@40, p_align@48.
            // Static, non-PIE binary: p_vaddr is the absolute load address.
            TPL.store(*(h.add(16) as *const u64) as usize, Ordering::Relaxed);
            FILESZ.store(*(h.add(32) as *const u64) as usize, Ordering::Relaxed);
            MEMSZ.store(*(h.add(40) as *const u64) as usize, Ordering::Relaxed);
            let a = *(h.add(48) as *const u64) as usize;
            ALIGN.store(if a == 0 { 1 } else { a }, Ordering::Relaxed);
            return;
        }
    }
}

/// Allocate and initialize a fresh TLS block. Returns
/// `(mapping_base, mapping_len, thread_pointer)`; the base/len are for
/// `munmap` when the owning thread is joined. Returns zeros on failure.
///
/// # Safety
/// Must be called after [`install_main`] has captured the TLS template. The
/// returned thread pointer must be installed as `%fs` (via `CLONE_SETTLS` or
/// `arch_prctl`) before any `#[thread_local]` access on that thread.
pub unsafe fn new_block() -> (*mut u8, usize, usize) {
    let memsz = MEMSZ.load(Ordering::Relaxed);
    let filesz = FILESZ.load(Ordering::Relaxed);
    let tpl = TPL.load(Ordering::Relaxed);
    let align = ALIGN.load(Ordering::Relaxed).max(16);

    let tls_size = align_up(memsz, align);
    let total = align_up(tls_size + TCB_SIZE + align, 4096);
    let base = match syscall::mmap_anon(total, syscall::PROT_READ | syscall::PROT_WRITE) {
        Ok(p) => p,
        Err(_) => return (core::ptr::null_mut(), 0, 0),
    };

    let static_start = align_up(base as usize, align);
    let tp = static_start + tls_size;

    // Copy .tdata; .tbss is already zero (fresh mmap).
    if tpl != 0 && filesz > 0 {
        core::ptr::copy_nonoverlapping(tpl as *const u8, static_start as *mut u8, filesz);
    }
    // TCB self-pointer at [tp].
    *(tp as *mut usize) = tp;

    (base, total, tp)
}

unsafe fn set_fs(tp: usize) {
    let _ = arch::syscall2(SYS_ARCH_PRCTL, ARCH_SET_FS, tp);
}
