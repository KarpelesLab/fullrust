#![no_std]
#![no_main]

extern crate fullrust;
use purestd::prelude::*;

fn main() {
    println!("hello from libc-free rust");
}

purestd::entry!(main);
