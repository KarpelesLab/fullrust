// Correctness + stress for the fullrust allocator's new medium/large in-segment
// tier and over-aligned routing. Drives std::alloc directly for exact (size,
// align) control, plus cross-thread frees, realloc across tiers, and RSS release.
// Unmodified std; static, no libc.
use std::alloc::{alloc, alloc_zeroed, dealloc, realloc, Layout};
use std::sync::mpsc;
use std::thread;

static mut FAILS: u32 = 0;

fn check(name: &str, cond: bool) {
    if cond {
        println!("ok   {name}");
    } else {
        println!("FAIL {name}");
        unsafe { FAILS += 1 };
    }
}

/// Resident set size in KiB from /proc/self/statm (field 1 = resident pages).
fn rss_kib() -> u64 {
    let s = std::fs::read_to_string("/proc/self/statm").unwrap_or_default();
    let pages: u64 = s.split_whitespace().nth(1).and_then(|f| f.parse().ok()).unwrap_or(0);
    pages * 4 // 4 KiB pages
}

/// Alloc `size` at `align`, assert alignment, write a byte pattern keyed by
/// `seed`, read it back, then free. Returns true on full success.
unsafe fn roundtrip(size: usize, align: usize, seed: u8) -> bool {
    let layout = Layout::from_size_align(size, align).unwrap();
    let p = alloc(layout);
    if p.is_null() {
        return false;
    }
    if p.addr() % align != 0 {
        dealloc(p, layout);
        return false;
    }
    // Touch every byte (also faults in the whole allocation).
    for i in 0..size {
        *p.add(i) = seed.wrapping_add((i & 0xff) as u8);
    }
    let mut ok = true;
    for i in 0..size {
        if *p.add(i) != seed.wrapping_add((i & 0xff) as u8) {
            ok = false;
            break;
        }
    }
    dealloc(p, layout);
    ok
}

fn main() {
    // --- throughput first (clean heap): repeated alloc+free at medium/large
    //     sizes. Previously each iteration was an mmap+munmap syscall pair
    //     (~5 us); the committed empty-segment cache recycles in-segment. ---
    for &(sz, label) in &[(65536usize, "64KiB (medium)"), (262144, "256KiB (large)")] {
        let layout = Layout::from_size_align(sz, 16).unwrap();
        let iters = 500_000u64;
        use std::hint::black_box;
        let start = std::time::Instant::now();
        let mut acc = 0u8;
        for _ in 0..iters {
            let p = black_box(unsafe { alloc(black_box(layout)) });
            unsafe {
                *p = acc;
                acc = acc.wrapping_add(*p).wrapping_add(1);
                dealloc(black_box(p), layout);
            }
        }
        let el = start.elapsed();
        let mops = iters as f64 / el.as_secs_f64() / 1e6;
        let nspop = el.as_nanos() as f64 / iters as f64;
        println!("     bench {label}: {mops:.1} M ops/s ({nspop:.1} ns/op) [acc={acc}]");
    }

    // --- tier coverage: sizes spanning small/medium/large/huge boundaries ---
    let sizes = [
        8192usize, 8193, 10000, 16384, 65536, 100000, 131072, 131073, 200000, 262144, 400000,
        524288, 524289, 1_000_000,
    ];
    let mut all = true;
    for (i, &sz) in sizes.iter().enumerate() {
        all &= unsafe { roundtrip(sz, 16, i as u8 + 1) };
    }
    check("aligned round-trips across all tiers", all);

    // --- over-aligned routing: every (size, align) must return an aligned ptr,
    //     survive a full read/write, and free cleanly ---
    let aligns = [32usize, 64, 256, 4096, 8192, 65536, 262144, 524288, 1_048_576];
    let mut over_ok = true;
    let mut misaligned = None;
    for &al in &aligns {
        for &sz in &[1usize, 100, 5000, 20000, 130000, 260000, 520000] {
            // size must not exceed align-rounded isize::MAX etc; these are fine.
            let sz = sz.max(1);
            let layout = Layout::from_size_align(sz, al).unwrap();
            let p = unsafe { alloc(layout) };
            if p.is_null() || p.addr() % al != 0 {
                over_ok = false;
                misaligned = Some((sz, al, p.addr()));
                if !p.is_null() {
                    unsafe { dealloc(p, layout) };
                }
                continue;
            }
            // Fill every byte and verify: catches overlap with metadata or a
            // neighbouring block, not just a bad base alignment.
            unsafe {
                for i in 0..sz {
                    *p.add(i) = (i & 0xff) as u8 ^ (al as u8);
                }
                for i in 0..sz {
                    if *p.add(i) != (i & 0xff) as u8 ^ (al as u8) {
                        over_ok = false;
                        break;
                    }
                }
                dealloc(p, layout);
            }
        }
    }
    if let Some((sz, al, addr)) = misaligned {
        println!("     misaligned: size={sz} align={al} -> addr%align={}", addr % al);
    }
    check("over-aligned allocations are correctly aligned", over_ok);

    // --- alloc_zeroed on a reused paged block must actually be zero ---
    unsafe {
        let l = Layout::from_size_align(60000, 16).unwrap();
        // dirty a block, free it (page keeps the slot / segment stays mapped)
        let d = alloc(l);
        for i in 0..60000 {
            *d.add(i) = 0xFF;
        }
        dealloc(d, l);
        // request the same class zeroed; must be clean even though memory is reused
        let z = alloc_zeroed(l);
        let mut zeroed = true;
        for i in 0..60000 {
            if *z.add(i) != 0 {
                zeroed = false;
                break;
            }
        }
        dealloc(z, l);
        check("alloc_zeroed clears reused medium memory", zeroed);
    }

    // --- realloc: grow/shrink within a tier and ACROSS tiers, preserving data ---
    unsafe {
        let l0 = Layout::from_size_align(10000, 16).unwrap();
        let p0 = alloc(l0);
        for i in 0..10000 {
            *p0.add(i) = (i & 0xff) as u8;
        }
        // medium grow (10000 -> 50000)
        let p1 = realloc(p0, l0, 50000);
        let mut ok = !p1.is_null();
        for i in 0..10000 {
            ok &= *p1.add(i) == (i & 0xff) as u8;
        }
        // cross-tier grow medium -> large (50000 -> 300000)
        let l1 = Layout::from_size_align(50000, 16).unwrap();
        let p2 = realloc(p1, l1, 300000);
        ok &= !p2.is_null();
        for i in 0..10000 {
            ok &= *p2.add(i) == (i & 0xff) as u8;
        }
        // shrink large -> medium (300000 -> 20000)
        let l2 = Layout::from_size_align(300000, 16).unwrap();
        let p3 = realloc(p2, l2, 20000);
        ok &= !p3.is_null();
        for i in 0..10000 {
            ok &= *p3.add(i) == (i & 0xff) as u8;
        }
        dealloc(p3, Layout::from_size_align(20000, 16).unwrap());
        check("realloc preserves data across tiers", ok);
    }

    // --- cross-thread free: producers alloc medium/large boxes, a collector on
    //     another thread frees them (exercises the shared thread_free path) ---
    {
        const PRODUCERS: usize = 4;
        const PER: usize = 3000;
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let collector = thread::spawn(move || {
            let mut n = 0u64;
            let mut sum = 0u64;
            while let Ok(v) = rx.recv() {
                // touch + drop (free) on a thread that did not allocate it
                sum = sum.wrapping_add(v[0] as u64).wrapping_add(v[v.len() - 1] as u64);
                n += 1;
                drop(v);
            }
            (n, sum)
        });
        let mut producers = Vec::new();
        for t in 0..PRODUCERS {
            let tx = tx.clone();
            producers.push(thread::spawn(move || {
                for k in 0..PER {
                    // sizes rotating through medium and large tiers
                    let sz = match k % 4 {
                        0 => 9000,
                        1 => 60000,
                        2 => 200000,
                        _ => 500000,
                    };
                    let mut v = vec![0u8; sz];
                    v[0] = t as u8;
                    v[sz - 1] = k as u8;
                    tx.send(v).unwrap();
                }
            }));
        }
        drop(tx);
        for p in producers {
            p.join().unwrap();
        }
        let (n, _sum) = collector.join().unwrap();
        check("cross-thread medium/large free", n == (PRODUCERS * PER) as u64);
    }

    // --- RSS release: a burst of large allocs, all freed, must return memory ---
    {
        let before = rss_kib();
        let layout = Layout::from_size_align(262144, 16).unwrap(); // 256 KiB (large tier)
        let mut ps = Vec::new();
        for _ in 0..400 {
            let p = unsafe { alloc(layout) };
            // Fault in every OS page so RSS actually reflects the allocation.
            let mut off = 0;
            while off < 262144 {
                unsafe { *p.add(off) = 1 };
                off += 4096;
            }
            ps.push(p);
        }
        let peak = rss_kib();
        for p in ps {
            unsafe { dealloc(p, layout) };
        }
        let after = rss_kib();
        // 400 * 256 KiB = 100 MiB; peak must reflect it, after must drop most back.
        let grew = peak > before + 50_000;
        let released = after < before + 20_000;
        println!("     RSS before={before}KiB peak={peak}KiB after={after}KiB");
        check("large burst grows RSS", grew);
        check("freeing large burst releases RSS", released);
    }

    let fails = unsafe { FAILS };
    if fails == 0 {
        println!("\nALL LARGEALLOC TESTS PASSED");
    } else {
        println!("\n{fails} LARGEALLOC TEST(S) FAILED");
        std::process::exit(1);
    }
}
