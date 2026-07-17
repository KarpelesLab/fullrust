// Exercises real stack unwinding on the fullrust target (pure-Rust `unwinding`
// backend + DWARF `.eh_frame` located via program headers): catch_unwind catches
// a panic, RAII Drops run while the stack unwinds, panic payloads survive across
// the catch boundary, and unwinding works from a spawned thread too.
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};

static DROPS: AtomicUsize = AtomicUsize::new(0);

static mut FAILS: u32 = 0;
fn check(name: &str, cond: bool) {
    if cond {
        println!("ok   {name}");
    } else {
        println!("FAIL {name}");
        unsafe { FAILS += 1 };
    }
}

struct Guard(&'static str);
impl Drop for Guard {
    fn drop(&mut self) {
        DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

#[inline(never)]
fn deep(n: u32) {
    let _g = Guard("frame"); // must be dropped as the stack unwinds
    std::hint::black_box(n);
    if n == 0 {
        panic!("unwind me at the bottom");
    }
    deep(n - 1);
}

fn main() {
    // Quiet the default panic hook so the expected panics don't spam stderr.
    std::panic::set_hook(Box::new(|_| {}));

    // 1. catch_unwind catches a panic and yields Err, with Drops run for every
    //    unwound frame (10 Guards created by deep(9)).
    DROPS.store(0, Ordering::SeqCst);
    let r = catch_unwind(|| deep(9));
    check("catch_unwind returns Err on panic", r.is_err());
    check("all frames' Drops ran during unwind", DROPS.load(Ordering::SeqCst) == 10);

    // 2. The panic payload survives across the catch boundary (payload may be
    //    a `String` or a `&'static str` depending on formatting).
    let r = catch_unwind(|| panic!("payload {}", 42));
    let msg = r.err().map(|e| {
        if let Some(s) = e.downcast_ref::<String>() {
            s.clone()
        } else if let Some(s) = e.downcast_ref::<&str>() {
            s.to_string()
        } else {
            String::from("<unknown payload type>")
        }
    });
    check("panic payload propagates", msg.as_deref() == Some("payload 42"));

    // 3. A caught panic does NOT abort — control resumes normally afterwards.
    let mut resumed = false;
    let _ = catch_unwind(AssertUnwindSafe(|| panic!("boom")));
    resumed = true;
    check("execution resumes after a caught panic", resumed);

    // 4. Non-panicking closure returns its value through catch_unwind.
    let v = catch_unwind(|| 7 + 5);
    check("catch_unwind passes through Ok value", matches!(v, Ok(12)));

    // 5. Unwinding works inside a spawned thread; the panic is delivered as the
    //    thread's join error (thread stacks also carry `.eh_frame`).
    DROPS.store(0, Ordering::SeqCst);
    let h = std::thread::spawn(|| deep(4)); // 5 Guards
    let joined = h.join();
    check("spawned thread panic caught by join", joined.is_err());
    check("thread frames unwound (Drops ran)", DROPS.load(Ordering::SeqCst) == 5);

    let fails = unsafe { FAILS };
    if fails == 0 {
        println!("\nALL UNWIND TESTS PASSED");
    } else {
        println!("\n{fails} UNWIND TEST(S) FAILED");
        std::process::exit(1);
    }
}
