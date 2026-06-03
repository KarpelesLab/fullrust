#![no_std]
#![no_main]

use fullrust::prelude::*;

fn main() {
    println!("hello from libc-free rust");
}

fullrust::entry!(main);
