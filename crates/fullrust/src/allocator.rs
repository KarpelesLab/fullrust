//! A small global allocator backed directly by `mmap`.
//!
//! Strategy: **segregated free lists**. Requests up to 64 KiB are rounded up to
//! one of a fixed set of size classes and served from a per-class free list; a
//! bump pointer carves fresh class-sized blocks out of 1 MiB arenas obtained
//! via `mmap`. Freed blocks are pushed back onto their class's list (the free
//! list is intrusive — a freed block's first word holds the `next` pointer).
//! Requests larger than the biggest class get their own `mmap`/`munmap`.
//!
//! This is intentionally simple (no cross-class coalescing), which is fine for
//! typical program workloads. It is correct and `Sync` via a spinlock; fullrust
//! programs are single-threaded, so the lock is effectively uncontended.

use crate::syscall::{self, PROT_READ, PROT_WRITE};
use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

const PAGE: usize = 4096;
const ARENA: usize = 1 << 20; // 1 MiB bump arenas

/// Size classes. Each is a power of two so we can align to it cheaply.
const BUCKETS: [usize; 13] = [
    16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384, 32768, 65536,
];
const MAX_BUCKET: usize = 65536;

#[inline]
fn round_up(x: usize, to: usize) -> usize {
    (x + to - 1) & !(to - 1)
}

#[inline]
fn bucket_index(need: usize) -> usize {
    let mut i = 0;
    while BUCKETS[i] < need {
        i += 1;
    }
    i
}

struct Inner {
    free: [*mut u8; BUCKETS.len()],
    cur: *mut u8,
    end: *mut u8,
}

impl Inner {
    const fn new() -> Self {
        Inner {
            free: [ptr::null_mut(); BUCKETS.len()],
            cur: ptr::null_mut(),
            end: ptr::null_mut(),
        }
    }

    /// Replace the bump region with a fresh `mmap` arena big enough for `min`.
    fn refill(&mut self, min: usize) -> bool {
        let size = if ARENA >= min {
            ARENA
        } else {
            round_up(min, PAGE)
        };
        match syscall::mmap_anon(size, PROT_READ | PROT_WRITE) {
            Ok(p) => {
                self.cur = p;
                self.end = p.wrapping_add(size);
                true
            }
            Err(_) => false,
        }
    }

    fn alloc(&mut self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();
        if size == 0 {
            // ZST / zero-size request: hand back a non-null, aligned, never-read
            // pointer. `dealloc` ignores these.
            return align as *mut u8;
        }
        let need = if size > align { size } else { align };

        // Large allocation: its own mapping. mmap is page-aligned, which covers
        // any alignment up to a page.
        if need > MAX_BUCKET || align > PAGE {
            let sz = round_up(size, PAGE);
            return syscall::mmap_anon(sz, PROT_READ | PROT_WRITE).unwrap_or(ptr::null_mut());
        }

        let b = bucket_index(need);
        let bsize = BUCKETS[b];

        // Reuse a freed block of this class if one is available.
        let head = self.free[b];
        if !head.is_null() {
            self.free[b] = unsafe { *(head as *const *mut u8) };
            return head;
        }

        // Otherwise carve a fresh `bsize` block from the bump region, aligned to
        // the class size (a power of two, so this also satisfies `align`).
        let mut aligned = round_up(self.cur as usize, bsize);
        if self.cur.is_null() || aligned + bsize > self.end as usize {
            if !self.refill(bsize) {
                return ptr::null_mut();
            }
            aligned = round_up(self.cur as usize, bsize);
            if aligned + bsize > self.end as usize {
                return ptr::null_mut();
            }
        }
        self.cur = (aligned + bsize) as *mut u8;
        aligned as *mut u8
    }

    unsafe fn dealloc(&mut self, p: *mut u8, layout: Layout) {
        let size = layout.size();
        let align = layout.align();
        if size == 0 {
            return;
        }
        let need = if size > align { size } else { align };

        if need > MAX_BUCKET || align > PAGE {
            let sz = round_up(size, PAGE);
            let _ = syscall::munmap(p, sz);
            return;
        }

        let b = bucket_index(need);
        // Intrusive push onto the class free list.
        *(p as *mut *mut u8) = self.free[b];
        self.free[b] = p;
    }
}

/// The fullrust global allocator. Installed below via `#[global_allocator]`.
pub struct Allocator {
    locked: AtomicBool,
    inner: UnsafeCell<Inner>,
}

// Safe: all access to `inner` goes through the spinlock.
unsafe impl Sync for Allocator {}

impl Allocator {
    /// Create a new, empty allocator.
    pub const fn new() -> Self {
        Allocator {
            locked: AtomicBool::new(false),
            inner: UnsafeCell::new(Inner::new()),
        }
    }

    #[inline]
    fn acquire(&self) {
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
    }

    #[inline]
    fn release(&self) {
        self.locked.store(false, Ordering::Release);
    }
}

impl Default for Allocator {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.acquire();
        let r = (*self.inner.get()).alloc(layout);
        self.release();
        r
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.acquire();
        (*self.inner.get()).dealloc(ptr, layout);
        self.release();
    }
}

#[global_allocator]
static GLOBAL: Allocator = Allocator::new();
