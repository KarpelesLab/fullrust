//! `cargo fullrust` — build a crate as a libc-free, fully-static binary.
//!
//! Two modes:
//!
//! * **zero-touch (default).** Builds and caches a sysroot whose `std` is
//!   fullrust's own standard library, then compiles your crate against it with
//!   `--sysroot`. An **unmodified** crate — plain `fn main`, `use std::…`, no
//!   deps, no attributes — becomes a pure-syscall static binary. Nightly +
//!   `rust-src`.
//!
//! * **`--runtime` (explicit).** For crates that opt into the fullrust runtime
//!   directly (`#![no_std]` + `fullrust::entry!`). Uses `-Z build-std` (nightly)
//!   or, with `--stable`, the precompiled core/alloc.
//!
//! ```text
//! cargo install cargo-fullrust
//! cargo fullrust build --release        # zero-touch
//! cargo fullrust run -- arg1
//! cargo fullrust --runtime build        # explicit-runtime crate
//! ```

use std::path::{Path, PathBuf};
use std::process::{exit, Command};

const TARGET_JSON: &str = include_str!("../x86_64-fullrust-linux.json");
const TARGET_STEM: &str = "x86_64-fullrust-linux";
/// Source of the sysroot `std` crate (compiled into the sysroot at build time).
const SYSROOT_STD: &str = include_str!("../sysroot/std_lib.rs");
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|s| s == "fullrust").unwrap_or(false) {
        args.remove(0);
    }

    let mut toolchain = "nightly".to_string();
    let mut runtime = false; // explicit fullrust-runtime mode
    let mut cargo_args: Vec<String> = Vec::new();
    let mut prog_args: Vec<String> = Vec::new();
    let mut saw_dd = false;

    for a in args {
        if saw_dd {
            prog_args.push(a);
            continue;
        }
        match a.as_str() {
            "--stable" => {
                toolchain = "stable".into();
                runtime = true; // stable implies the precompiled runtime path
            }
            "--nightly" => toolchain = "nightly".into(),
            "--runtime" => runtime = true,
            "-h" | "--help" => return print_help(),
            "--" => {
                saw_dd = true;
                prog_args.push(a);
            }
            _ => cargo_args.push(a),
        }
    }
    if cargo_args.is_empty() {
        cargo_args.push("build".into());
    }

    let host = rustc_field(&toolchain, "host");
    if !(host.starts_with("x86_64") && host.contains("linux")) {
        fail(&format!(
            "unsupported host `{host}`: fullrust currently targets x86_64 Linux only"
        ));
    }
    let lld = resolve_lld(&toolchain, &host);
    let target_path = write_cache_file("x86_64-fullrust-linux.json", TARGET_JSON);

    let mut cmd = Command::new("cargo");
    cmd.env("RUSTUP_TOOLCHAIN", &toolchain);
    let env_triple = env_triple_of(TARGET_STEM);
    cmd.env(format!("CARGO_TARGET_{env_triple}_LINKER"), &lld);

    let mut extra: Vec<String> = Vec::new();

    if runtime {
        // Explicit fullrust-runtime crate (#![no_std] + entry!).
        if toolchain == "nightly" {
            cmd.env(
                format!("CARGO_TARGET_{env_triple}_RUSTFLAGS"),
                "-C link-args=-static",
            );
            extra.push("--target".into());
            extra.push(target_path.to_string_lossy().into_owned());
            extra.push("-Zbuild-std=core,alloc,compiler_builtins".into());
            extra.push("-Zjson-target-spec".into());
        } else {
            // stable: precompiled core/alloc for the gnu triple.
            let stable_triple = env_triple_of(&host);
            cmd.env(format!("CARGO_TARGET_{stable_triple}_LINKER"), &lld);
            cmd.env(
                format!("CARGO_TARGET_{stable_triple}_RUSTFLAGS"),
                "-C relocation-model=static -C linker-flavor=ld -C link-args=-static -C panic=abort",
            );
            extra.push("--target".into());
            extra.push(host.clone());
        }
    } else {
        // Zero-touch: build/cache the sysroot, compile against it (no build-std).
        if toolchain != "nightly" {
            fail("zero-touch mode requires nightly (use --stable --runtime for the precompiled path)");
        }
        let sysroot = ensure_sysroot(&host, &lld, &target_path);
        cmd.env(
            format!("CARGO_TARGET_{env_triple}_RUSTFLAGS"),
            format!("--sysroot {} -C link-args=-static", sysroot.display()),
        );
        extra.push("--target".into());
        extra.push(target_path.to_string_lossy().into_owned());
        extra.push("-Zjson-target-spec".into());
    }

    // Order: `<subcommand> <user flags> --target … -Z… [-- prog args]`.
    cmd.args(&cargo_args).args(&extra).args(&prog_args);

    match cmd.status() {
        Ok(s) => exit(s.code().unwrap_or(1)),
        Err(e) => fail(&format!("failed to run cargo: {e}")),
    }
}

// ---- sysroot construction ----

/// Build (once) and return the path to a sysroot whose `std` is fullrust's.
fn ensure_sysroot(host: &str, lld: &Path, target_path: &Path) -> PathBuf {
    let real = rustc_out("nightly", &["--print", "sysroot"]);
    let real = PathBuf::from(real.trim());
    // Cache key: rustc version + cargo-fullrust version.
    let rv = rustc_out("nightly", &["--version"]);
    let key = sanitize(&format!("{}-{}", rv.trim(), VERSION));
    let base = cache_dir().join("sysroot").join(key);
    let sysroot = base.join("root");
    if sysroot.join(".ok").exists() {
        return sysroot;
    }

    eprintln!("cargo-fullrust: building the fullrust sysroot (one-time)...");
    // 1. Generate the std-crate project.
    let proj = base.join("std-src");
    let _ = std::fs::create_dir_all(proj.join("src"));
    let deps = match std::env::var("FULLRUST_DEV_REPO") {
        Ok(repo) if !repo.is_empty() => format!(
            "fullrust = {{ path = \"{repo}/crates/fullrust\", default-features = false }}\n\
             fullrust-std = {{ path = \"{repo}/crates/fullrust-std\" }}\n"
        ),
        _ => "fullrust = { version = \"0.1\", default-features = false }\n\
              fullrust-std = \"0.1\"\n"
            .to_string(),
    };
    write(
        &proj.join("Cargo.toml"),
        &format!(
            "[package]\nname = \"fullrust-sysroot-std\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\
             [lib]\nname = \"std\"\n[dependencies]\n{deps}"
        ),
    );
    write(&proj.join("src/lib.rs"), SYSROOT_STD);

    // 2. Build it for the target with build-std.
    let env_triple = env_triple_of(TARGET_STEM);
    let status = Command::new("cargo")
        .current_dir(&proj)
        .env("RUSTUP_TOOLCHAIN", "nightly")
        .env(format!("CARGO_TARGET_{env_triple}_LINKER"), lld)
        .env(
            format!("CARGO_TARGET_{env_triple}_RUSTFLAGS"),
            "-C link-args=-static",
        )
        .args([
            "build",
            "--release",
            "--target",
            &target_path.to_string_lossy(),
            "-Zbuild-std=core,alloc,compiler_builtins",
            "-Zjson-target-spec",
        ])
        .status();
    match status {
        Ok(s) if s.success() => {}
        _ => fail("failed to build the fullrust sysroot std"),
    }

    // 3. Assemble the sysroot: our target rlibs + symlinked host pieces.
    let lib = sysroot.join("lib");
    let target_lib = lib.join("rustlib").join(TARGET_STEM).join("lib");
    let _ = std::fs::create_dir_all(&target_lib);
    let built = proj
        .join("target")
        .join(TARGET_STEM)
        .join("release")
        .join("deps");
    let mut copied = 0;
    if let Ok(entries) = std::fs::read_dir(&built) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("rlib") {
                let _ = std::fs::copy(&p, target_lib.join(p.file_name().unwrap()));
                copied += 1;
            }
        }
    }
    if copied == 0 {
        fail("sysroot build produced no rlibs");
    }
    // Symlink the host target dir (proc-macros / build scripts) and the
    // compiler's own libs, so the toolchain keeps working.
    symlink(
        &real.join("lib/rustlib").join(host),
        &lib.join("rustlib").join(host),
    );
    if let Ok(entries) = std::fs::read_dir(real.join("lib")) {
        for e in entries.flatten() {
            let name = e.file_name();
            if name == "rustlib" {
                continue;
            }
            symlink(&e.path(), &lib.join(&name));
        }
    }
    write(&sysroot.join(".ok"), "");
    sysroot
}

// ---- helpers ----

fn resolve_lld(toolchain: &str, host: &str) -> PathBuf {
    let sysroot = rustc_out(toolchain, &["--print", "sysroot"]);
    let lld = PathBuf::from(sysroot.trim())
        .join("lib/rustlib")
        .join(host)
        .join("bin/gcc-ld/ld.lld");
    if !lld.exists() {
        fail(&format!("ld.lld not found at {}", lld.display()));
    }
    lld
}

fn cache_dir() -> PathBuf {
    if let Ok(d) = std::env::var("XDG_CACHE_HOME") {
        if !d.is_empty() {
            return PathBuf::from(d).join("cargo-fullrust");
        }
    }
    if let Ok(h) = std::env::var("HOME") {
        return PathBuf::from(h).join(".cache/cargo-fullrust");
    }
    std::env::temp_dir().join("cargo-fullrust")
}

fn write_cache_file(name: &str, contents: &str) -> PathBuf {
    let dir = cache_dir();
    let _ = std::fs::create_dir_all(&dir);
    let p = dir.join(name);
    write(&p, contents);
    p
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(path, contents) {
        fail(&format!("cannot write {}: {e}", path.display()));
    }
}

fn symlink(src: &Path, dst: &Path) {
    use std::os::unix::fs::symlink as ln;
    let _ = std::fs::remove_file(dst);
    if let Err(e) = ln(src, dst) {
        fail(&format!(
            "cannot symlink {} -> {}: {e}",
            dst.display(),
            src.display()
        ));
    }
}

fn run(toolchain: &str, bin: &str, args: &[&str]) -> std::process::Output {
    Command::new(bin)
        .env("RUSTUP_TOOLCHAIN", toolchain)
        .args(args)
        .output()
        .unwrap_or_else(|e| {
            fail(&format!(
                "could not run `{bin}` (is rustup installed?): {e}"
            ))
        })
}

fn rustc_out(toolchain: &str, args: &[&str]) -> String {
    let o = run(toolchain, "rustc", args);
    if !o.status.success() {
        fail(&format!(
            "`rustc {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&o.stderr).trim()
        ));
    }
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn rustc_field(toolchain: &str, key: &str) -> String {
    for line in rustc_out(toolchain, &["-vV"]).lines() {
        if let Some(v) = line.strip_prefix(&format!("{key}: ")) {
            return v.trim().to_string();
        }
    }
    fail(&format!("could not find `{key}` in rustc -vV"))
}

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

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn print_help() {
    println!(
        "cargo fullrust — build a crate as a libc-free, fully-static binary\n\n\
         USAGE:\n    cargo fullrust [--runtime] [--stable] <cargo-subcommand> [args] [-- prog args]\n\n\
         MODES:\n\
         \x20   (default)   zero-touch: build an unmodified crate via a fullrust sysroot\n\
         \x20   --runtime   crate uses the fullrust runtime directly (#![no_std] + entry!)\n\
         \x20   --stable    precompiled core/alloc (implies --runtime; no nightly)\n\n\
         EXAMPLES:\n    cargo fullrust build --release\n    cargo fullrust run -- --flag\n\n\
         Zero-touch needs a nightly toolchain with rust-src."
    );
}

fn fail(msg: &str) -> ! {
    eprintln!("cargo-fullrust: {msg}");
    exit(1);
}
