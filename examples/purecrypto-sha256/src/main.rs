#![no_std]
#![no_main]

use fullrust::prelude::*;
use purecrypto::hash::sha256;

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn main() {
    // Hash argv[1] if given, else the classic test vector "abc".
    let mut args = env::args();
    let _prog = args.next();
    let input = args.next().unwrap_or("abc");
    let digest = sha256(input.as_bytes());
    println!("SHA-256({input:?}) = {}", hex(&digest));
}

fullrust::entry!(main);
