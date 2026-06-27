//! Exercises native ELF `#[thread_local]` TLS on the fullrust target:
//!   * per-thread isolation of a `thread_local!` value (each thread sees its own),
//!   * thread-local **destructors run at thread exit** (the capability the old
//!     key-based registry leaked — now wired via `destructors::run` in the pal),
//!   * a `RefCell`/`Vec`-backed thread_local that allocates, across many threads.
//! Unmodified std; libc-free + static.

use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

// Counts how many thread-local `Dropper`s have actually been dropped.
static DROPS: AtomicUsize = AtomicUsize::new(0);

struct Dropper(u64);
impl Drop for Dropper {
    fn drop(&mut self) {
        DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

thread_local! {
    // A non-`const` initializer with a `Drop` type forces the lazy/destructor
    // path of the native TLS implementation.
    static GUARD: RefCell<Option<Dropper>> = RefCell::new(None);
    // A heap-backed TLS value, per thread, to stress the allocator under TLS.
    static SCRATCH: RefCell<Vec<u64>> = RefCell::new(Vec::new());
}

fn main() {
    const N: u64 = 16;

    // Each thread stamps its own TLS, fills a per-thread heap Vec, and installs a
    // Dropper whose Drop must fire when the thread exits.
    let mut handles = Vec::new();
    for i in 0..N {
        handles.push(thread::spawn(move || {
            GUARD.with(|g| *g.borrow_mut() = Some(Dropper(i)));
            SCRATCH.with(|s| {
                let mut v = s.borrow_mut();
                for k in 0..1000 {
                    v.push(i * 1000 + k);
                }
                // Each thread must see only its own data.
                assert_eq!(v.len(), 1000);
                assert_eq!(v[0], i * 1000);
            });
            i
        }));
    }

    let mut sum = 0u64;
    for h in handles {
        sum += h.join().unwrap();
    }
    assert_eq!(sum, (0..N).sum::<u64>(), "join returned wrong values");

    // Every spawned thread should have run its Dropper at exit.
    let drops = DROPS.load(Ordering::SeqCst);
    println!("thread-local Droppers run: {drops} (expected {N})");
    assert_eq!(drops as u64, N, "thread-local destructors did not all run!");

    // Main thread's own TLS still isolated and usable.
    GUARD.with(|g| assert!(g.borrow().is_none(), "main GUARD clobbered by a child"));
    SCRATCH.with(|s| s.borrow_mut().push(42));

    println!("OK: native #[thread_local] isolation + destructors + heap-backed TLS, libc-free");

    // Shared-state sanity so the optimizer can't elide the whole run.
    let _keep = Arc::new(drops);
}
