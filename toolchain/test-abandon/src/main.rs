// Reproduces the abandoned-segment retention scenario the adversarial review
// flagged, and verifies the thread-exit scavenge reclaims it:
//   1. producer threads allocate large blocks, hand them off, then EXIT while the
//      blocks are still live -> their segments are abandoned (owner=null) with
//      the blocks pending on other threads;
//   2. the main thread frees every block (cross-thread frees land on the
//      abandoned segments' thread_free lists) -> segments are now fully freeable
//      but still mapped, since nothing has reclaimed them;
//   3. a throwaway thread exits -> scavenge_abandoned folds + munmaps them, so
//      RSS returns near baseline.
// Unmodified std; static, no libc.
use std::sync::mpsc;
use std::thread;

fn rss_kib() -> u64 {
    let s = std::fs::read_to_string("/proc/self/statm").unwrap_or_default();
    let pages: u64 = s.split_whitespace().nth(1).and_then(|f| f.parse().ok()).unwrap_or(0);
    pages * 4
}

const PRODUCERS: usize = 8;
const PER: usize = 40;
const SZ: usize = 262144; // 256 KiB -> large tier

fn main() {
    let mut fails = 0u32;
    let mut check = |name: &str, cond: bool| {
        if cond {
            println!("ok   {name}");
        } else {
            println!("FAIL {name}");
            fails += 1;
        }
    };

    let baseline = rss_kib();

    // (1) Producers allocate large blocks, send them to main, then exit. Because
    // the blocks are still live at producer exit, each producer's segments are
    // abandoned rather than freed.
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let mut producers = Vec::new();
    for t in 0..PRODUCERS {
        let tx = tx.clone();
        producers.push(thread::spawn(move || {
            for k in 0..PER {
                let mut v = vec![0u8; SZ];
                // fault every page so RSS reflects it
                let mut off = 0;
                while off < SZ {
                    v[off] = (t + k) as u8;
                    off += 4096;
                }
                tx.send(v).unwrap();
            }
        }));
    }
    drop(tx);
    for p in producers {
        p.join().unwrap(); // producers have now EXITED (segments abandoned)
    }

    // (2) Main receives and frees everything (cross-thread frees onto abandoned
    // segments). Main never allocated large itself, so it can't reclaim them.
    let mut got = 0usize;
    while let Ok(v) = rx.recv() {
        got += 1;
        drop(v);
    }
    check("received all blocks", got == PRODUCERS * PER);
    let retained = rss_kib();

    // (3) Any thread exit runs the scavenge. Use a throwaway thread.
    thread::spawn(|| {}).join().unwrap();
    // Give the scavenge's munmaps a moment to reflect (they're synchronous, but
    // read RSS after a second exit to be safe).
    thread::spawn(|| {}).join().unwrap();
    let after = rss_kib();

    let total_kib = (PRODUCERS * PER * SZ / 1024) as u64; // ~80 MiB
    println!(
        "     RSS baseline={baseline}KiB retained={retained}KiB after_scavenge={after}KiB (payload ~{total_kib}KiB)"
    );

    // Retained must reflect the abandoned live payload...
    check("abandoned segments retained before scavenge", retained > baseline + total_kib / 2);
    // ...and the scavenge on thread exit must return most of it to the OS.
    check("thread-exit scavenge reclaims abandoned RSS", after < baseline + total_kib / 4);

    if fails == 0 {
        println!("\nALL ABANDON TESTS PASSED");
    } else {
        println!("\n{fails} ABANDON TEST(S) FAILED");
        std::process::exit(1);
    }
}
