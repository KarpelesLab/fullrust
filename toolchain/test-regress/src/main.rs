// Regression checks for published-toolchain correctness bugs.
//
// 1. TLS layout (1.88/1.89): the pal rounded the PT_TLS block by max(16) instead
//    of the real p_align, shifting every #[thread_local] by 8 bytes whenever the
//    TLS memsz isn't a multiple of 16. Symptom: a `thread_local!` with a Drop
//    impl, touched on the main thread, segfaults in the dtor registration
//    (every tokio multi-thread binary). Exercised with Drop-carrying and
//    odd-sized thread-locals on the main thread and spawned threads; `run.sh`
//    builds twice (with/without an extra 8-byte TLS pad, `--cfg tls_pad8`) so at
//    least one build has memsz % 16 != 0, verified with `readelf -l`.
// 2. process::exit (1.95): the new sys/exit.rs had no fullrust arm and aborted,
//    so `std::process::exit(N)` did not exit with N.
// 3. libm C symbols (all versions, issue #5): LLVM folds a `sin(x)` + `cos(x)`
//    pair into glibc's `sincos`/`sincosf`, and crates call libm through
//    `extern "C"`; fullrust provided only part of libm, so these failed to
//    link. Now every libm function has its C name. Checks the folded pair
//    (`run.sh` verifies the binary really calls `sincos`/`sincosf`, so this
//    can't pass vacuously) and a sample of the C-ABI functions, including the
//    out-pointer ones.
use std::cell::{Cell, RefCell};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static DROPS: AtomicUsize = AtomicUsize::new(0);

struct Guard(u64);
impl Drop for Guard {
    fn drop(&mut self) {
        DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

thread_local! {
    static WITH_DROP: RefCell<Option<Guard>> = const { RefCell::new(None) };
    static LAZY_DROP: Vec<u64> = vec![1, 2, 3];
    static ODD1: Cell<u8> = const { Cell::new(1) };
    static ODD3: Cell<[u8; 3]> = const { Cell::new([3, 3, 3]) };
    static ODD5: Cell<[u8; 5]> = const { Cell::new([5; 5]) };
    static WORD: Cell<u64> = const { Cell::new(0x1122_3344_5566_7788) };
}
#[cfg(tls_pad8)]
thread_local! {
    static PAD8: Cell<u64> = const { Cell::new(0xdead_beef) };
}

static mut FAILS: u32 = 0;
fn check(name: &str, cond: bool) {
    if cond {
        println!("ok   {name}");
    } else {
        println!("FAIL {name}");
        unsafe { FAILS += 1 };
    }
}

/// Touch every thread-local; returns whether all initial values read back intact.
fn touch(tag: u64) -> bool {
    let mut ok = ODD1.get() == 1 && ODD3.get() == [3; 3] && ODD5.get() == [5; 5];
    ok &= WORD.get() == 0x1122_3344_5566_7788;
    #[cfg(tls_pad8)]
    {
        ok &= PAD8.get() == 0xdead_beef;
        PAD8.set(tag);
        ok &= PAD8.get() == tag;
    }
    ODD1.set(tag as u8);
    ODD3.set([tag as u8; 3]);
    ODD5.set([tag as u8; 5]);
    WORD.set(tag);
    ok &= LAZY_DROP.with(|v| v.len() == 3 && v[2] == 3);
    WITH_DROP.with(|g| *g.borrow_mut() = Some(Guard(tag)));
    ok &= WITH_DROP.with(|g| g.borrow().as_ref().map(|g| g.0) == Some(tag));
    ok &= ODD1.get() == tag as u8 && ODD3.get() == [tag as u8; 3] && ODD5.get() == [tag as u8; 5];
    ok &= WORD.get() == tag;
    ok
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("exit") {
        std::process::exit(args[2].parse().unwrap());
    }

    // --- TLS layout / thread_local! with Drop ---------------------------------
    check("thread_local! with Drop + odd sizes on the main thread", touch(7));
    let handles: Vec<_> = (0..4u64)
        .map(|i| std::thread::spawn(move || touch(100 + i)))
        .collect();
    let all = handles.into_iter().all(|h| h.join().unwrap());
    check("thread_local! with Drop + odd sizes on 4 spawned threads", all);
    check("spawned-thread TLS destructors ran", DROPS.load(Ordering::SeqCst) == 4);
    check("main-thread TLS intact after threads", WORD.get() == 7 && ODD5.get() == [7; 5]);

    // --- process::exit(N) -----------------------------------------------------
    let exe = std::env::current_exe().unwrap();
    for code in [0, 1, 7, 42, 255] {
        let st = Command::new(&exe).args(["exit", &code.to_string()]).status().unwrap();
        check(&format!("process::exit({code}) -> exit status {code}"), st.code() == Some(code));
    }

    // --- libm C symbols ------------------------------------------------------
    let n = args.len() as f64; // 1: a runtime value, so nothing is const-folded
    let (s, c) = (n * 0.5).sin_cos();
    check(
        "folded sin+cos (sincos) matches libm",
        (s - 0.479425538604203).abs() < 1e-15 && (c - 0.8775825618903728).abs() < 1e-15,
    );
    let y = (n * 0.5) as f32;
    let (sf, cf) = (y.sin(), y.cos());
    check(
        "folded sinf+cosf (sincosf) matches libm",
        (sf - 0.47942555).abs() < 1e-6 && (cf - 0.87758255).abs() < 1e-6,
    );
    check("libm C ABI: sincos/frexp/modf/remquo/scalbn/ilogb/exp10/nextafter/lgamma/j0/acosh/atanh", libm_c_abi(n));

    if unsafe { FAILS } != 0 {
        std::process::exit(1);
    }
    println!("ALL OK");
}

unsafe extern "C" {
    fn sincos(x: f64, s: *mut f64, c: *mut f64);
    fn frexp(x: f64, exp: *mut i32) -> f64;
    fn modf(x: f64, iptr: *mut f64) -> f64;
    fn modff(x: f32, iptr: *mut f32) -> f32;
    fn remquo(x: f64, y: f64, quo: *mut i32) -> f64;
    fn scalbn(x: f64, n: i32) -> f64;
    fn ilogb(x: f64) -> i32;
    fn exp10(x: f64) -> f64;
    fn nextafter(x: f64, y: f64) -> f64;
    fn lgamma(x: f64) -> f64;
    fn j0(x: f64) -> f64;
    fn acosh(x: f64) -> f64;
    fn atanh(x: f64) -> f64;
}

/// Calls libm through its C names, the way a crate's `extern "C"` block would.
/// `one` is 1.0 at run time.
fn libm_c_abi(one: f64) -> bool {
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-12 * b.abs().max(1.0);
    let mut ok = true;
    unsafe {
        let (mut s, mut c) = (0.0, 0.0);
        sincos(one, &mut s, &mut c);
        ok &= close(s, 0.8414709848078965) && close(c, 0.5403023058681398);
        let mut e = 0;
        ok &= frexp(12.0 * one, &mut e) == 0.75 && e == 4;
        let mut ip = 0.0;
        ok &= modf(-3.25 * one, &mut ip) == -0.25 && ip == -3.0;
        let mut ipf = 0.0f32;
        ok &= modff(2.5 * one as f32, &mut ipf) == 0.5 && ipf == 2.0;
        let mut q = 0;
        ok &= remquo(10.0 * one, 3.0, &mut q) == 1.0 && q == 3;
        ok &= scalbn(3.0 * one, 4) == 48.0;
        ok &= ilogb(1024.0 * one) == 10;
        ok &= close(exp10(2.0 * one), 100.0);
        ok &= nextafter(one, 2.0) == 1.0 + f64::EPSILON;
        ok &= close(lgamma(5.0 * one), 24f64.ln());
        ok &= close(j0(one), 0.7651976865579666);
        ok &= close(acosh(one), 0.0) && close(atanh(0.5 * one), 0.5493061443340549);
    }
    ok
}
