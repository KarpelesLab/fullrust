#![no_std]
#![no_main]

extern crate fullrust;
use purestd::env;
use purestd::prelude::*;

fn main() -> i32 {
    let args: Vec<String> = env::args().collect();
    println!("argc = {}", args.len());
    for (i, a) in args.iter().enumerate() {
        println!("argv[{i}] = {a:?}");
    }
    match env::var("HOME") {
        Ok(home) => println!("HOME = {home}"),
        Err(_) => println!("HOME is unset"),
    }
    // Exit code = number of arguments, just to show return-value -> exit code.
    args.len() as i32
}

purestd::entry!(main);
