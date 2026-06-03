#![no_std]
#![no_main]

use fullrust::prelude::*;

fn main() {
    // Vec growth exercises the segregated free-list allocator across classes.
    let mut squares: Vec<u64> = Vec::new();
    for i in 0..20 {
        squares.push(i * i);
    }
    let sum: u64 = squares.iter().sum();

    // String + format! exercise the formatting machinery, all heap-backed.
    let mut s = String::new();
    for x in &squares {
        s.push_str(&format!("{x} "));
    }
    println!("squares: {}", s.trim_end());
    println!("sum     = {sum}");

    // A large allocation goes through the direct-mmap path.
    let big = vec![7u8; 200_000];
    println!("big vec : {} bytes, first = {}", big.len(), big[0]);

    // Box on the heap.
    let boxed = Box::new(("answer", 42));
    println!("boxed   : {} = {}", boxed.0, boxed.1);
}

fullrust::entry!(main);
