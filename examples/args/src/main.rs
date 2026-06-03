#![no_std]
#![no_main]

use fullrust::prelude::*;

fn main() -> i32 {
    let args: Vec<&str> = env::args().collect();
    println!("argc = {}", args.len());
    for (i, a) in args.iter().enumerate() {
        println!("argv[{i}] = {a:?}");
    }
    match env::var("HOME") {
        Some(home) => println!("HOME = {home}"),
        None => println!("HOME is unset"),
    }
    // Exit code = number of arguments, just to show return-value -> exit code.
    args.len() as i32
}

fullrust::entry!(main);
