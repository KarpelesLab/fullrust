//! `cargo fullrust` — build a crate as a libc-free, fully-static binary.
//!
//! This is the installable equivalent of the repo's `./x` wrapper: it resolves
//! the Rust-bundled LLVM linker, selects a freestanding target, and invokes
//! `cargo` with the right linker / flags / `-Z build-std` so you don't have to.
//!
//! ```text
//! cargo install cargo-fullrust
//! cargo fullrust build --release      # in a crate that uses the fullrust runtime
//! cargo fullrust run -- arg1 arg2
//! cargo fullrust --stable build       # precompiled core/alloc instead of build-std
//! ```
//!
//! Two paths, mirroring `./x`:
//!   * default (nightly): a custom freestanding target (vendor = "fullrust") +
//!     `-Z build-std`. Robust for real crates — host build scripts / proc-macros
//!     still build for the real host with the system linker. Needs a nightly
//!     toolchain with the `rust-src` component.
//!   * `--stable`: target the host `-gnu` triple and link the precompiled
//!     core/alloc. No extra components, but `-C panic=abort` is forced, which
//!     can conflict with proc-macro / build-script dependencies.
//!
//! The crate being built must still use the fullrust runtime (`#![no_std]`,
//! `#![no_main]`, `fullrust::entry!(main)`); this tool only makes the *build*
//! transparent.

use std::path::PathBuf;
use std::process::{exit, Command};

/// The freestanding target spec, embedded so the installed binary is
/// self-contained (the file ships inside this crate). `./x` reads the same file.
const TARGET_JSON: &str = include_str!("../x86_64-fullrust-linux.json");
const TARGET_STEM: &str = "x86_64-fullrust-linux";

fn main() {
    // When invoked as `cargo fullrust ...`, argv is
    // ["cargo-fullrust", "fullrust", <rest>]; drop the injected subcommand name.
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|s| s == "fullrust").unwrap_or(false) {
        args.remove(0);
    }

    let mut toolchain = "nightly".to_string();
    let mut cargo_args: Vec<String> = Vec::new(); // before any `--`
    let mut prog_args: Vec<String> = Vec::new(); // from `--` onward
    let mut saw_dd = false;

    for a in args {
        if saw_dd {
            prog_args.push(a);
            continue;
        }
        match a.as_str() {
            "--stable" => toolchain = "stable".into(),
            "--nightly" => toolchain = "nightly".into(),
            "-h" | "--help" => {
                print_help();
                return;
            }
            "--" => {
                saw_dd = true;
                prog_args.push(a);
            }
            _ => cargo_args.push(a),
        }
    }

    if cargo_args.is_empty() {
        cargo_args.push("build".into()); // sensible default subcommand
    }

    // Host / sysroot for the chosen toolchain.
    let host = rustc_field(&toolchain, "host");
    if !(host.starts_with("x86_64") && host.contains("linux")) {
        fail(&format!(
            "unsupported host `{host}`: fullrust currently targets x86_64 Linux only"
        ));
    }
    let sysroot = run(&toolchain, "rustc", &["--print", "sysroot"]);
    let lld = PathBuf::from(sysroot.trim())
        .join("lib/rustlib")
        .join(&host)
        .join("bin/gcc-ld/ld.lld");
    if !lld.exists() {
        fail(&format!(
            "ld.lld not found at {}\n       (the toolchain may be missing rust-lld / the self-contained linker)",
            lld.display()
        ));
    }

    let mut cmd = Command::new("cargo");
    cmd.env("RUSTUP_TOOLCHAIN", &toolchain);

    let (target, env_triple, mut extra): (String, String, Vec<String>) = if toolchain == "nightly" {
        let target_path = write_target_json();
        let env_triple = env_triple_of(TARGET_STEM);
        // Most settings (relocation model, panic=abort, no unwind tables) live in
        // the target JSON; only -static is needed via flags.
        cmd.env(
            format!("CARGO_TARGET_{env_triple}_RUSTFLAGS"),
            "-C link-args=-static",
        );
        (
            target_path.to_string_lossy().into_owned(),
            env_triple,
            vec![
                "-Zbuild-std=core,alloc,compiler_builtins".into(),
                "-Zjson-target-spec".into(),
            ],
        )
    } else {
        let env_triple = env_triple_of(&host);
        cmd.env(
            format!("CARGO_TARGET_{env_triple}_RUSTFLAGS"),
            "-C relocation-model=static -C linker-flavor=ld -C link-args=-static -C panic=abort",
        );
        (host.clone(), env_triple, Vec::new())
    };

    cmd.env(format!("CARGO_TARGET_{env_triple}_LINKER"), &lld);

    cmd.args(&cargo_args);
    cmd.arg("--target").arg(&target);
    cmd.args(&extra);
    extra.clear();
    cmd.args(&prog_args);

    match cmd.status() {
        Ok(status) => exit(status.code().unwrap_or(1)),
        Err(e) => fail(&format!("failed to run cargo: {e}")),
    }
}

/// Write the embedded target JSON to a stable temp path named after the triple
/// (the file stem becomes the target name cargo uses).
fn write_target_json() -> PathBuf {
    let dir = std::env::temp_dir().join("cargo-fullrust");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        fail(&format!("cannot create {}: {e}", dir.display()));
    }
    let path = dir.join(format!("{TARGET_STEM}.json"));
    if let Err(e) = std::fs::write(&path, TARGET_JSON) {
        fail(&format!("cannot write target spec {}: {e}", path.display()));
    }
    path
}

/// Run `<bin> <args>` under the toolchain and return stdout (trimmed on error).
fn run(toolchain: &str, bin: &str, args: &[&str]) -> String {
    let out = Command::new(bin)
        .env("RUSTUP_TOOLCHAIN", toolchain)
        .args(args)
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        Ok(o) => fail(&format!(
            "`{bin} {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(e) => fail(&format!(
            "could not run `{bin}` (is rustup installed?): {e}"
        )),
    }
}

/// Parse a single `key: value` field out of `rustc -vV`.
fn rustc_field(toolchain: &str, key: &str) -> String {
    let vv = run(toolchain, "rustc", &["-vV"]);
    for line in vv.lines() {
        if let Some(v) = line.strip_prefix(&format!("{key}: ")) {
            return v.trim().to_string();
        }
    }
    fail(&format!("could not find `{key}` in rustc -vV output"))
}

/// Cargo's env-var spelling of a target triple: uppercase, non-alnum -> `_`.
fn env_triple_of(triple: &str) -> String {
    triple
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn print_help() {
    println!(
        "cargo fullrust — build a crate as a libc-free, fully-static binary\n\n\
         USAGE:\n    cargo fullrust [--stable|--nightly] <cargo-subcommand> [cargo args] [-- prog args]\n\n\
         EXAMPLES:\n    cargo fullrust build --release\n    cargo fullrust run -- --flag\n    cargo fullrust --stable build\n\n\
         The target crate must use the fullrust runtime (#![no_std] + fullrust::entry!).\n\
         Default path uses a nightly toolchain with rust-src (-Z build-std); --stable\n\
         links the precompiled core/alloc instead."
    );
}

fn fail(msg: &str) -> ! {
    eprintln!("cargo-fullrust: {msg}");
    exit(1);
}
