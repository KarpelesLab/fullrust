//! Adversarial correctness stress for the fullrust allocator, aimed at the
//! cross-thread / abandoned-segment paths that the throughput bench doesn't hit:
//!
//!   * **Producer→consumer with producer exit.** Producer threads allocate many
//!     blocks, write a known pattern, hand them to consumers via a queue, then
//!     **exit while their blocks are still live** — forcing their segments to be
//!     *abandoned*. Consumers verify the pattern and free the blocks (a
//!     cross-thread free into an abandoned segment), then allocate themselves,
//!     forcing *reclaim* of those abandoned segments. If accounting is wrong
//!     this corrupts data or crashes.
//!
//!   * **Mixed sizes** spanning every regime: tiny, small, the largest page
//!     class, and huge (dedicated-mmap) allocations, plus an over-aligned class.
//!
//!   * **Churn with retained set** to exercise page retire / segment free.
//!
//! Pure std; libc-free + static. Success is exit 0 with all checks passing.

use std::alloc::{alloc, dealloc, Layout};
use std::sync::{Arc, Mutex};
use std::thread;

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
}

// A handed-off allocation: pointer, size, align, and the byte pattern written.
#[derive(Clone, Copy)]
struct Parcel {
    ptr: usize,
    size: usize,
    align: usize,
    tag: u8,
}
unsafe impl Send for Parcel {}

fn pick_size(r: &mut Rng) -> (usize, usize) {
    match r.next() % 32 {
        0..=18 => (1 + (r.next() as usize % 256), 8), // tiny/small (most common)
        19..=24 => (256 + (r.next() as usize % 3840), 8), // small/medium
        25..=27 => (8192, 8),                         // largest page class exactly
        28..=29 => (8193 + (r.next() as usize % 200_000), 8), // huge (dedicated mmap)
        _ => (1 + (r.next() as usize % 1024), 64),    // over-aligned (align=64 path)
    }
}

fn main() {
    const PRODUCERS: usize = 8;
    const PER_PRODUCER: usize = 60_000;

    let queue: Arc<Mutex<Vec<Parcel>>> = Arc::new(Mutex::new(Vec::new()));
    let total_freed = Arc::new(Mutex::new(0usize));

    // Producers: allocate, stamp, hand off, then exit (abandoning live segments).
    let mut producers = Vec::new();
    for pid in 0..PRODUCERS {
        let q = Arc::clone(&queue);
        producers.push(thread::spawn(move || {
            let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ (pid as u64 + 1).wrapping_mul(2654435761));
            let mut local = Vec::with_capacity(512);
            for i in 0..PER_PRODUCER {
                let (size, align) = pick_size(&mut rng);
                let layout = Layout::from_size_align(size, align).unwrap();
                let p = unsafe { alloc(layout) };
                assert!(!p.is_null(), "alloc returned null");
                assert_eq!(p as usize % align, 0, "alignment violated");
                let tag = (pid as u8).wrapping_mul(31).wrapping_add(i as u8);
                // Stamp first/last byte (and a middle byte) with the tag.
                unsafe {
                    *p = tag;
                    *p.add(size - 1) = tag;
                    *p.add(size / 2) = tag;
                }
                local.push(Parcel { ptr: p as usize, size, align, tag });
                if local.len() == 512 {
                    q.lock().unwrap().extend(local.drain(..));
                }
            }
            q.lock().unwrap().extend(local.drain(..));
            // Thread exits here: its still-live blocks force segment abandonment.
        }));
    }

    // Consumers: drain the queue, verify the pattern, free (cross-thread), and
    // allocate scratch (forcing reclaim of abandoned segments).
    const CONSUMERS: usize = 4;
    let done = Arc::new(Mutex::new(false));
    let mut consumers = Vec::new();
    for cid in 0..CONSUMERS {
        let q = Arc::clone(&queue);
        let d = Arc::clone(&done);
        let freed = Arc::clone(&total_freed);
        consumers.push(thread::spawn(move || {
            let mut rng = Rng(0xD1B5_4A32_D192_ED03 ^ (cid as u64 + 1));
            let mut my_freed = 0usize;
            let mut scratch: Vec<(usize, usize, usize)> = Vec::new();
            loop {
                let batch: Vec<Parcel> = {
                    let mut g = q.lock().unwrap();
                    let n = g.len().min(256);
                    g.drain(..n).collect()
                };
                if batch.is_empty() {
                    if *d.lock().unwrap() {
                        break;
                    }
                    thread::yield_now();
                    continue;
                }
                for parcel in batch {
                    let p = parcel.ptr as *mut u8;
                    // Verify the producer's pattern survived the hand-off.
                    unsafe {
                        assert_eq!(*p, parcel.tag, "head byte corrupted");
                        assert_eq!(*p.add(parcel.size - 1), parcel.tag, "tail byte corrupted");
                        assert_eq!(*p.add(parcel.size / 2), parcel.tag, "mid byte corrupted");
                    }
                    let layout = Layout::from_size_align(parcel.size, parcel.align).unwrap();
                    unsafe { dealloc(p, layout) };
                    my_freed += 1;

                    // Interleave own allocations to force reclaim + reuse.
                    if rng.next() % 4 == 0 {
                        let (size, align) = pick_size(&mut rng);
                        let l = Layout::from_size_align(size, align).unwrap();
                        let q2 = unsafe { alloc(l) };
                        assert!(!q2.is_null());
                        unsafe { *q2 = 0xAB };
                        scratch.push((q2 as usize, size, align));
                        if scratch.len() > 2000 {
                            let (sp, ss, sa) = scratch.remove(0);
                            let l = Layout::from_size_align(ss, sa).unwrap();
                            unsafe { dealloc(sp as *mut u8, l) };
                        }
                    }
                }
            }
            // Drain remaining scratch.
            for (sp, ss, sa) in scratch {
                let l = Layout::from_size_align(ss, sa).unwrap();
                unsafe { dealloc(sp as *mut u8, l) };
            }
            *freed.lock().unwrap() += my_freed;
        }));
    }

    for h in producers {
        h.join().unwrap();
    }
    *done.lock().unwrap() = true;
    for h in consumers {
        h.join().unwrap();
    }

    let freed = *total_freed.lock().unwrap();
    let expected = PRODUCERS * PER_PRODUCER;
    println!("handed off and freed across threads: {freed} (expected {expected})");
    assert_eq!(freed, expected, "lost or double-counted parcels");
    assert!(queue.lock().unwrap().is_empty(), "queue not drained");

    println!("OK: cross-thread + abandoned-segment reclaim + mixed sizes, no corruption");
}
