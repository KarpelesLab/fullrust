use std::collections::BTreeMap;

fn main() {
    let text = "the quick brown fox the lazy fox";
    let mut counts: BTreeMap<&str, u32> = BTreeMap::new();
    for word in text.split_whitespace() {
        *counts.entry(word).or_insert(0) += 1;
    }
    for (word, count) in &counts {
        println!("{word}: {count}");
    }
    println!("(built with no #![no_std], no deps, no attributes)");
}
