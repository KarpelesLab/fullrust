#![no_std]
#![no_main]
extern crate alloc;
#[macro_use]
extern crate fullrust_std;

use fullrust_std::net::ToSocketAddrs;
use fullrust_std::time::{Instant, SystemTime, UNIX_EPOCH};

fn main() {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => println!("unix time = {} s", d.as_secs()),
        Err(_) => println!("clock before epoch?!"),
    }
    let start = Instant::now();
    let e = start.elapsed();
    println!(
        "instant elapsed = {} ns (monotonic ok if small)",
        e.as_nanos()
    );

    match ("one.one.one.one", 443u16).to_socket_addrs() {
        Ok(it) => {
            for a in it {
                println!("resolved one.one.one.one -> {a}");
            }
        }
        Err(e) => println!("dns error: {e}"),
    }

    // Raw TCP: HTTP/1.0 GET to example.com:80, print the status line.
    use fullrust_std::io::{Read, Write};
    use fullrust_std::net::TcpStream;
    use fullrust_std::time::Duration;
    match TcpStream::connect(("example.com", 80u16)) {
        Ok(mut s) => {
            s.set_read_timeout(Some(Duration::from_secs(10))).ok();
            s.write_all(b"GET / HTTP/1.0\r\nHost: example.com\r\n\r\n")
                .unwrap();
            let mut buf = [0u8; 256];
            let n = s.read(&mut buf).unwrap_or(0);
            let text = core::str::from_utf8(&buf[..n]).unwrap_or("<non-utf8>");
            println!(
                "TCP GET example.com:80 -> {}",
                text.lines().next().unwrap_or("(no data)")
            );
        }
        Err(e) => println!("tcp connect error: {e}"),
    }

    // Real threads: spawn 4, each returns a square; sum the joins.
    use alloc::vec::Vec;
    let handles: Vec<_> = (1u64..=4)
        .map(|i| fullrust_std::thread::spawn(move || i * i))
        .collect();
    let sum: u64 = handles.into_iter().map(|h| h.join().unwrap()).sum();
    println!("threads: sum of squares 1..=4 = {sum} (expect 30)");

    // Shared counter across threads via Arc<Mutex<_>>.
    use alloc::sync::Arc;
    use fullrust_std::sync::Mutex;
    let counter = Arc::new(Mutex::new(0u64));
    let ts: Vec<_> = (0..8)
        .map(|_| {
            let c = counter.clone();
            fullrust_std::thread::spawn(move || {
                for _ in 0..1000 {
                    *c.lock().unwrap() += 1;
                }
            })
        })
        .collect();
    for t in ts {
        t.join().unwrap();
    }
    println!(
        "threads: shared counter = {} (expect 8000)",
        *counter.lock().unwrap()
    );
}

fullrust::entry!(main);
