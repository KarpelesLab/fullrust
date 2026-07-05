// Exercises panic backtraces on the fullrust target (frame-pointer unwind +
// gimli/DWARF symbolization reading /proc/self/exe). `panic` triggers the auto
// backtrace via RUST_BACKTRACE; `capture` uses the std::backtrace API directly.
// Build with debuginfo (see Cargo.toml) so symbol names + file:line resolve.
use std::backtrace::Backtrace;
use std::hint::black_box;

#[inline(never)]
fn deep_c(x: u32) -> u32 {
    black_box(x);
    // The frame that actually panics; its callers must appear in the trace.
    panic!("boom at depth {x}");
}

#[inline(never)]
fn deep_b(x: u32) -> u32 {
    black_box(deep_c(black_box(x + 1)))
}

#[inline(never)]
fn deep_a(x: u32) -> u32 {
    black_box(deep_b(black_box(x + 1)))
}

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("capture") => {
            // Direct API: force a full capture and print it.
            let bt = Backtrace::force_capture();
            println!("status={:?}", bt.status());
            print!("{bt}");
        }
        Some("panic") | None => {
            // Auto backtrace on panic (RUST_BACKTRACE controls it).
            let _ = black_box(deep_a(black_box(0)));
        }
        _ => {
            eprintln!("usage: backtrace-fullrust <panic|capture>");
            std::process::exit(2);
        }
    }
}
