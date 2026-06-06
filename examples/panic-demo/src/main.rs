#![no_std]
#![no_main]

extern crate fullrust;
use purestd::prelude::*;

fn main() {
    let v: Vec<i32> = vec![1, 2, 3];
    println!("about to index out of bounds...");
    // This panics; purestd's panic handler prints to stderr and exits 101.
    let _ = v[10];
    println!("unreachable");
}

purestd::entry!(main);
