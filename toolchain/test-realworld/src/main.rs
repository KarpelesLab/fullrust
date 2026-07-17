// First real third-party crate on the libc-free fullrust target: serde + serde_json.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Debug)]
struct Server {
    name: String,
    port: u16,
    tags: Vec<String>,
    limits: BTreeMap<String, i64>,
    enabled: bool,
}

fn main() {
    let input = r#"
      { "name": "edge-1", "port": 8080, "tags": ["prod","eu"],
        "limits": {"conns": 1024, "rps": 50000}, "enabled": true }
    "#;
    // Parse into a typed struct via derive.
    let mut s: Server = serde_json::from_str(input).expect("parse");
    println!("parsed: {s:?}");

    // Mutate + round-trip through the untyped Value API too.
    s.port += 1;
    s.tags.push("canary".into());
    *s.limits.entry("rps".into()).or_insert(0) += 1;

    let pretty = serde_json::to_string_pretty(&s).expect("serialize");
    println!("{pretty}");

    // Untyped parse of the re-serialized output, checking a nested field.
    let v: serde_json::Value = serde_json::from_str(&pretty).unwrap();
    assert_eq!(v["port"], 8081);
    assert_eq!(v["limits"]["rps"], 50001);
    assert_eq!(v["tags"].as_array().unwrap().len(), 3);
    println!("\nOK: serde + serde_json round-trip on libc-free fullrust");
}
