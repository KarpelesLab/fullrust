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

    if unsafe { FAILS } != 0 {
        std::process::exit(1);
    }
    println!("ALL OK");
}
