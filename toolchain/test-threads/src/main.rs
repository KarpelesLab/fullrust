// Exercises real threads on the fullrust target: thread::spawn (clone(2)),
// join (CLONE_CHILD_CLEARTID futex), Arc<Mutex> under contention (futex sync),
// and per-thread thread_local! (key-based TLS). Unmodified std; libc-free + static.
use std::cell::Cell;
use std::sync::{Arc, Mutex};
use std::thread;

thread_local! {
    static TL: Cell<u64> = const { Cell::new(0) };
}

fn main() {
    const N: u64 = 8;
    const ITERS: u64 = 100_000;

    println!("available_parallelism = {:?}", thread::available_parallelism());

    let counter = Arc::new(Mutex::new(0u64));
    let mut handles = Vec::new();
    for i in 0..N {
        let c = Arc::clone(&counter);
        handles.push(thread::spawn(move || {
            TL.with(|t| t.set(i)); // each thread stamps its own TLS
            for _ in 0..ITERS {
                *c.lock().unwrap() += 1;
            }
            let seen = TL.with(|t| t.get());
            assert_eq!(seen, i, "thread_local leaked across threads!");
            seen
        }));
    }

    let mut sum_ids = 0u64;
    for h in handles {
        sum_ids += h.join().unwrap();
    }

    let total = *counter.lock().unwrap();
    println!("counter = {total} (expected {})", N * ITERS);
    assert_eq!(total, N * ITERS, "Mutex lost updates under contention");
    assert_eq!(sum_ids, (0..N).sum::<u64>(), "join returned wrong values");
    assert_eq!(TL.with(|t| t.get()), 0, "main thread TLS clobbered");
    println!("OK: thread::spawn + Arc<Mutex> + thread_local! all correct, libc-free");
}
