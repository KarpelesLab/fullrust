#![no_std]
#![no_main]

use fullrust::prelude::*;

fn main() {
    let v: Vec<i32> = vec![1, 2, 3];
    println!("about to index out of bounds...");
    // This panics; the fullrust panic handler prints to stderr and aborts (134).
    let _ = v[10];
    println!("unreachable");
}

fullrust::entry!(main);
