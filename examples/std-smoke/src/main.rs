#![no_std]
#![no_main]

extern crate alloc;
extern crate fullrust;

use alloc::sync::Arc;
use purestd::io::{Read, Write};
use purestd::net::{TcpStream, ToSocketAddrs};
use purestd::prelude::*;
use purestd::sync::Mutex;
use purestd::thread;
use purestd::time::{Instant, SystemTime, UNIX_EPOCH};

fn main() {
    // --- time ---
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => println!("unix time = {} s", d.as_secs()),
        Err(_) => println!("clock before epoch?!"),
    }
    let start = Instant::now();
    println!("instant elapsed = {} ns", start.elapsed().as_nanos());

    // --- DNS resolution (plain DNS, no NSS) ---
    match ("one.one.one.one", 443u16).to_socket_addrs() {
        Ok(it) => {
            for a in it {
                println!("resolved one.one.one.one -> {a}");
            }
        }
        Err(e) => println!("dns error: {e}"),
    }

    // --- raw TCP: HTTP/1.0 GET to example.com:80, print the status line ---
    match TcpStream::connect(("example.com", 80u16)) {
        Ok(mut s) => {
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

    // --- real threads: spawn 4, each returns a square; sum the joins ---
    let handles: Vec<_> = (1u64..=4).map(|i| thread::spawn(move || i * i)).collect();
    let sum: u64 = handles.into_iter().map(|h| h.join().unwrap()).sum();
    println!("threads: sum of squares 1..=4 = {sum} (expect 30)");

    // --- shared counter across threads via Arc<Mutex<_>> ---
    let counter = Arc::new(Mutex::new(0u64));
    let ts: Vec<_> = (0..8)
        .map(|_| {
            let c = counter.clone();
            thread::spawn(move || {
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

    println!("std-smoke OK");
}

purestd::entry!(main);
