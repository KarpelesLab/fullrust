// Deliberately overflows the stack to exercise the fullrust stack-overflow
// handler. `main`  -> overflow on the main thread; `thread` -> overflow inside a
// spawned thread. Expected: it prints "thread '<name>' has overflowed its stack"
// and aborts, rather than a silent/garbled SIGSEGV. Driven by an argument so the
// harness can check both paths. Unmodified std; static, no libc.
use std::hint::black_box;

fn recurse(depth: usize) -> usize {
    // A sizeable live local per frame + black_box defeats tail-call / inlining so
    // the stack genuinely grows until it hits the guard page.
    let mut buf = [0u8; 2048];
    buf[0] = depth as u8;
    buf[2047] = depth as u8;
    black_box(&buf);
    black_box(depth) + recurse(black_box(depth + 1))
}

fn main() {
    let which = std::env::args().nth(1).unwrap_or_default();
    match which.as_str() {
        "main" => {
            let _ = black_box(recurse(black_box(0)));
        }
        "thread" => {
            let h = std::thread::Builder::new()
                .name("overflower".into())
                .spawn(|| {
                    let _ = black_box(recurse(black_box(0)));
                })
                .unwrap();
            let _ = h.join();
        }
        "segv" => {
            // A genuine wild write (NOT a stack overflow): the handler must pass
            // it through as an ordinary SIGSEGV, never claim "overflowed stack".
            let p = black_box(0x1234usize) as *mut u8;
            unsafe { *p = 42 };
        }
        _ => {
            eprintln!("usage: stackoverflow-fullrust <main|thread>");
            std::process::exit(2);
        }
    }
}
