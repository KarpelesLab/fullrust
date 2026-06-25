// An ordinary crate — no #![no_std], no attributes, plain `fn main`, using std.
// Built with the fullrust fork toolchain for x86_64-unknown-linux-fullrust, this
// becomes a statically-linked, libc-free ELF.
use std::collections::BTreeMap;

fn main() {
    let mut counts = BTreeMap::new();
    for w in "the quick brown fox the fox".split_whitespace() {
        *counts.entry(w).or_insert(0) += 1;
    }
    println!("hello from libc-free std: {counts:?}");

    let args: Vec<String> = std::env::args().collect();
    println!("argc={}, arg0={}", args.len(), args.first().map(|s| s.as_str()).unwrap_or("?"));
}
