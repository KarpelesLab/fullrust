//! Allocator benchmark + stress harness for the fullrust target.
//!
//! Measures what a modern allocator must get right:
//!   1. single-thread small-object throughput (the hot fast path),
//!   2. multi-thread throughput / lock contention (scaling across cores),
//!   3. producer/consumer cross-thread frees (free on a non-owner thread),
//!   4. fragmentation / RSS under random-size churn with a retained working set.
//!
//! Timing uses `std::time::Instant` (clock_gettime). RSS is read straight from
//! `/proc/self/statm` via raw syscalls, because this libc-free target has no
//! `std::fs` yet. Everything else is plain, unmodified std.

use std::alloc::{alloc, dealloc, Layout};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Raw-syscall RSS probe (no std::fs on this target).
// ---------------------------------------------------------------------------
mod sysc {
    use core::arch::asm;
    #[inline]
    pub unsafe fn syscall3(n: usize, a: usize, b: usize, c: usize) -> isize {
        let r;
        asm!("syscall", inlateout("rax") n => r, in("rdi") a, in("rsi") b, in("rdx") c,
             lateout("rcx") _, lateout("r11") _, options(nostack, preserves_flags));
        r
    }
    pub const OPEN: usize = 2;
    pub const READ: usize = 0;
    pub const CLOSE: usize = 3;
    pub const O_RDONLY: usize = 0;
}

/// Current resident set size in bytes, from /proc/self/statm (field 2 = pages).
fn rss_bytes() -> usize {
    let path = b"/proc/self/statm\0";
    let fd = unsafe { sysc::syscall3(sysc::OPEN, path.as_ptr() as usize, sysc::O_RDONLY, 0) };
    if fd < 0 {
        return 0;
    }
    let mut buf = [0u8; 128];
    let n = unsafe { sysc::syscall3(sysc::READ, fd as usize, buf.as_mut_ptr() as usize, buf.len()) };
    unsafe {
        sysc::syscall3(sysc::CLOSE, fd as usize, 0, 0);
    }
    if n <= 0 {
        return 0;
    }
    // Fields are space-separated decimals: "size resident shared ...".
    let s = &buf[..n as usize];
    let mut it = s.split(|&c| c == b' ');
    let _size = it.next();
    let resident = it.next().unwrap_or(b"0");
    let mut pages = 0usize;
    for &c in resident {
        if c.is_ascii_digit() {
            pages = pages * 10 + (c - b'0') as usize;
        }
    }
    pages * 4096
}

fn mib(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

/// Fault in every page of an allocation so RSS reflects the true working set
/// (touching only byte 0 leaves big allocations mostly non-resident).
#[inline]
fn touch(p: *mut u8, sz: usize) {
    let mut off = 0;
    while off < sz {
        unsafe { *p.add(off) = 1 };
        off += 4096;
    }
}

// Tiny deterministic RNG (xorshift64) so runs are reproducible.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn size(&mut self) -> usize {
        // Skewed toward small allocations, which dominate real workloads.
        let r = self.next();
        match r % 16 {
            0..=9 => 8 + (r as usize % 120),    // 8..128  (most common)
            10..=13 => 128 + (r as usize % 896), // 128..1024
            14 => 1024 + (r as usize % 7168),    // 1K..8K
            _ => 16 * 1024 + (r as usize % 240 * 1024), // 16K..256K (rare, large)
        }
    }
}

// ---------------------------------------------------------------------------
// Benchmark 1: single-thread small-object alloc/free throughput.
// ---------------------------------------------------------------------------
fn bench_single_thread() {
    const ROUNDS: usize = 200;
    const BATCH: usize = 4096;
    let layout = Layout::from_size_align(64, 16).unwrap();
    let mut ptrs = vec![std::ptr::null_mut::<u8>(); BATCH];

    let t = Instant::now();
    for _ in 0..ROUNDS {
        for p in ptrs.iter_mut() {
            let q = unsafe { alloc(layout) };
            // Touch first + last byte so the pages are actually faulted in.
            unsafe {
                *q = 1;
                *q.add(63) = 2;
            }
            *p = q;
        }
        for &p in ptrs.iter() {
            unsafe { dealloc(p, layout) };
        }
    }
    let dt = t.elapsed();
    let ops = (ROUNDS * BATCH) as f64;
    let per_op = dt.as_nanos() as f64 / ops;
    println!(
        "[1] single-thread 64B alloc+free: {:.2} M ops/s  ({:.1} ns/op, {} ops)",
        ops / dt.as_secs_f64() / 1e6,
        per_op,
        ROUNDS * BATCH
    );
}

// ---------------------------------------------------------------------------
// Benchmark 2: multi-thread throughput (scaling / lock contention).
// ---------------------------------------------------------------------------
fn bench_multi_thread(nthreads: usize) {
    const PER_THREAD: usize = 400_000;
    let t = Instant::now();
    let mut handles = Vec::new();
    for _ in 0..nthreads {
        handles.push(thread::spawn(move || {
            let layout = Layout::from_size_align(48, 8).unwrap();
            let mut keep = std::ptr::null_mut::<u8>();
            for i in 0..PER_THREAD {
                let q = unsafe { alloc(layout) };
                unsafe { *q = i as u8 };
                // Free the previous one — steady-state working set of 1.
                if !keep.is_null() {
                    unsafe { dealloc(keep, layout) };
                }
                keep = q;
            }
            if !keep.is_null() {
                unsafe { dealloc(keep, layout) };
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    let dt = t.elapsed();
    let ops = (nthreads * PER_THREAD) as f64;
    println!(
        "[2] {:>2} threads 48B alloc+free:   {:.2} M ops/s  ({:.2} M ops/s/thread)",
        nthreads,
        ops / dt.as_secs_f64() / 1e6,
        ops / dt.as_secs_f64() / 1e6 / nthreads as f64
    );
}

// ---------------------------------------------------------------------------
// Benchmark 3: producer/consumer — frees happen on a non-owner thread.
// This is the case a per-thread allocator must handle without a global lock.
// ---------------------------------------------------------------------------
fn bench_cross_thread() {
    const N: usize = 1_000_000;
    let queue: Arc<Mutex<Vec<(usize, usize)>>> = Arc::new(Mutex::new(Vec::new()));
    let done = Arc::new(Mutex::new(false));

    let q2 = Arc::clone(&queue);
    let d2 = Arc::clone(&done);
    let consumer = thread::spawn(move || {
        let mut freed = 0usize;
        loop {
            let batch: Vec<(usize, usize)> = {
                let mut g = q2.lock().unwrap();
                std::mem::take(&mut *g)
            };
            for (ptr, sz) in batch {
                let layout = Layout::from_size_align(sz, 8).unwrap();
                unsafe { dealloc(ptr as *mut u8, layout) };
                freed += 1;
            }
            if *d2.lock().unwrap() && q2.lock().unwrap().is_empty() {
                break;
            }
        }
        freed
    });

    let t = Instant::now();
    let mut rng = Rng(0x1234_5678);
    let mut local = Vec::with_capacity(1024);
    for _ in 0..N {
        let sz = 8 + (rng.next() as usize % 504); // 8..512
        let layout = Layout::from_size_align(sz, 8).unwrap();
        let p = unsafe { alloc(layout) };
        unsafe { *p = 7 };
        local.push((p as usize, sz));
        if local.len() == 1024 {
            queue.lock().unwrap().extend(local.drain(..));
        }
    }
    queue.lock().unwrap().extend(local.drain(..));
    *done.lock().unwrap() = true;
    let freed = consumer.join().unwrap();
    let dt = t.elapsed();
    println!(
        "[3] cross-thread alloc->free:     {:.2} M ops/s  ({} produced, {} freed elsewhere)",
        N as f64 / dt.as_secs_f64() / 1e6,
        N,
        freed
    );
    assert_eq!(freed, N, "consumer did not free everything");
}

// ---------------------------------------------------------------------------
// Benchmark 4: fragmentation / RSS under random-size churn.
// Keep a working set of ~24k live objects of random sizes, churning a fraction
// each round. A good allocator keeps RSS close to the live bytes and returns
// the rest to the OS; the page-per-alloc one bloats and never shrinks.
// ---------------------------------------------------------------------------
fn bench_fragmentation() {
    const LIVE: usize = 24_000;
    const ROUNDS: usize = 60;
    let rss0 = rss_bytes();
    let mut rng = Rng(0xdead_beef);
    let mut slots: Vec<(usize, usize)> = vec![(0, 0); LIVE]; // (ptr, size)
    let mut live_bytes = 0usize;
    let mut peak_rss = rss0;

    // Initial fill.
    for s in slots.iter_mut() {
        let sz = rng.size();
        let layout = Layout::from_size_align(sz, 8).unwrap();
        let p = unsafe { alloc(layout) };
        touch(p, sz);
        *s = (p as usize, sz);
        live_bytes += sz;
    }
    // Churn: replace a random 30% of the working set each round.
    for _ in 0..ROUNDS {
        for _ in 0..(LIVE * 3 / 10) {
            let idx = (rng.next() as usize) % LIVE;
            let (op, osz) = slots[idx];
            if op != 0 {
                let layout = Layout::from_size_align(osz, 8).unwrap();
                unsafe { dealloc(op as *mut u8, layout) };
                live_bytes -= osz;
            }
            let sz = rng.size();
            let layout = Layout::from_size_align(sz, 8).unwrap();
            let p = unsafe { alloc(layout) };
            touch(p, sz);
            slots[idx] = (p as usize, sz);
            live_bytes += sz;
        }
        let r = rss_bytes();
        if r > peak_rss {
            peak_rss = r;
        }
    }
    // Free everything, then see how much RSS comes back.
    for &(p, sz) in slots.iter() {
        if p != 0 {
            let layout = Layout::from_size_align(sz, 8).unwrap();
            unsafe { dealloc(p as *mut u8, layout) };
        }
    }
    let rss_after = rss_bytes();
    println!(
        "[4] fragmentation churn: live={:.1}MiB  peak_RSS={:.1}MiB  overhead={:.2}x  RSS_after_free={:.1}MiB",
        mib(live_bytes),
        mib(peak_rss),
        peak_rss as f64 / live_bytes.max(1) as f64,
        mib(rss_after)
    );
}

fn main() {
    let ncpu = thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    println!("== fullrust allocator bench ==  (cpus reported: {ncpu})");
    println!("initial RSS: {:.1} MiB", mib(rss_bytes()));

    bench_single_thread();
    bench_multi_thread(1);
    bench_multi_thread(2);
    bench_multi_thread(ncpu.max(2));
    bench_cross_thread();
    bench_fragmentation();

    println!("final RSS: {:.1} MiB", mib(rss_bytes()));
    println!("OK: allocator bench complete");
}
