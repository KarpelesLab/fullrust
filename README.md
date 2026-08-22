# fullrust

**Compile unmodified Rust crates into fully-static, libc-free Linux binaries.**

fullrust is a patched Rust toolchain whose standard library talks to the Linux
kernel through raw `syscall` instructions instead of libc. An **ordinary crate —
no code changes, no attributes, no `fullrust` dependency** — compiles to a static
`x86_64-unknown-linux-fullrust` ELF that links **no libc and no C runtime**: no
interpreter (`PT_INTERP`), no `.dynamic` section, zero `NEEDED` libraries. The
only thing the binary needs to run is the Linux kernel.

You use it as a **GitHub Action** or a **Docker image** — no local toolchain to
install, no nightly, no `-Z build-std`, no target JSON.

```rust
// src/main.rs — a completely ordinary program
use std::collections::BTreeMap;
fn main() {
    let mut counts = BTreeMap::new();
    for w in "the quick brown fox the fox".split_whitespace() {
        *counts.entry(w).or_insert(0) += 1;
    }
    println!("{counts:?}");
}
```

```console
$ file    target/x86_64-unknown-linux-fullrust/release/wordcount
ELF 64-bit LSB executable, x86-64, statically linked
$ ldd     target/x86_64-unknown-linux-fullrust/release/wordcount
        not a dynamic executable
$ readelf -d target/x86_64-unknown-linux-fullrust/release/wordcount
There is no dynamic section in this file.
```

Anything that reaches for a C library fails at **link** time — pure-Rust code
links, FFI into a `.so` does not. That's an intended guard-rail, not a bug.

---

## Use it in a GitHub workflow

One step. It runs the fullrust toolchain image against your checked-out crate and
leaves the binary under `target/x86_64-unknown-linux-fullrust/release/`.

```yaml
- uses: actions/checkout@v6
- uses: KarpelesLab/fullrust@master
  with:
    args: --release --bin myapp
```

### Attach a static binary to a GitHub Release

```yaml
name: Release
on:
  push:
    tags: ["v*"]          # or: release: { types: [published] }

permissions:
  contents: write          # required to upload release assets

jobs:
  linux-x86_64-static:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6

      - name: Build the libc-free static binary
        uses: KarpelesLab/fullrust@master
        with:
          args: --release --bin myapp

      - name: Package + attach
        run: |
          tar -C target/x86_64-unknown-linux-fullrust/release \
              -czf "myapp-${{ github.ref_name }}-linux-x86_64-static.tar.gz" myapp
      - uses: softprops/action-gh-release@v2
        with:
          files: myapp-*-linux-x86_64-static.tar.gz
```

### Action inputs

| input | default | meaning |
|---|---|---|
| `command` | `build` | cargo subcommand: `build`, `test`, `run`, `clippy`, … |
| `args` | `--release` | extra cargo args, e.g. `--release --bin myapp --no-default-features` |
| `working-directory` | `.` | the crate to build |
| `image` | `ghcr.io/karpeleslab/fullrust:1.88` | pin a Rust version (see [Versions](#versions)) |
| `no-ecosystem` | `false` | skip the `getrandom`/`socket2` `[patch.crates-io]` injection |

---

## Use it as a `container:` job

The image bakes the toolchain into its environment (`RUSTC`, `CARGO_BUILD_TARGET`,
a `CARGO_HOME` carrying the ecosystem `[patch]`), so a **plain `cargo build` is
already a fullrust build** — the same static, libc-free binary the action
produces:

```yaml
jobs:
  build:
    runs-on: ubuntu-latest
    container: ghcr.io/karpeleslab/fullrust:latest
    steps:
      - uses: actions/checkout@v6
      - run: cargo build --release
```

## Build locally with Docker

No toolchain install — mount your crate and build:

```console
docker run --rm -v "$PWD":/src ghcr.io/karpeleslab/fullrust:latest build --release
# `test`, `run`, `clippy` work the same way; run `bash` for an interactive shell.
```

The binary lands in your crate's `target/x86_64-unknown-linux-fullrust/release/`.

---

## Versions

Images are published per Rust minor, plus `latest` (the newest):

```
ghcr.io/karpeleslab/fullrust:1.88   …   :1.95   :latest
```

They're public — no login to pull. Pin one via the action's `image:` input, the
`container:` image, or the `docker run` tag. The action currently defaults to
`:1.88`; for the newest Rust, set
`image: ghcr.io/karpeleslab/fullrust:1.95` (or `:latest`).

---

## What works

An unmodified crate gets the **real** standard library, with its platform backend
rewritten on raw syscalls:

- **Threads & sync** — `std::thread` (`clone`-backed, per-thread stacks), futex
  `Mutex`/`Condvar`/`RwLock`, native `thread_local!` (`%fs`/TCB TLS).
- **Files & directories**, and **`std::process::Command`** (`fork`+`execve`,
  pipes, `wait`).
- **Networking** — TCP/UDP with a self-contained DNS resolver (`/etc/hosts` +
  `/etc/resolv.conf`, no NSS).
- **Panics unwind** (`catch_unwind`, `Drop` during unwind), and
  `RUST_BACKTRACE=1` prints **symbolized backtraces** — with no libunwind and no
  libc, via an in-tree pure-Rust unwinder.
- **`std::os::fd`** (`AsFd`/`OwnedFd`/`AsRawFd`/…) for the fd-interop ecosystem.
- **Dependencies** — pure-Rust crates work as-is (e.g. `serde`/`serde_json`). The
  image auto-injects a `[patch.crates-io]` so the common not-quite-pure gateways
  build libc-free too: `getrandom` (and thus `rand`, `uuid`), `socket2` (and thus
  `mio`/async stacks). Opt out with `no-ecosystem: true`.

## Limitations

- **Linux + x86-64 only** today.
- **Static only** — no dynamic linking; FFI into a `.so` cannot link (by design).
- **Not the `unix` target family.** `cfg(unix)` is false — that's exactly what
  keeps the build graph libc-free — so `std::os::unix` is absent (`std::os::fd`
  is provided instead). A crate whose `unix`-only path is load-bearing may need
  the ecosystem `[patch]` or a small fix; in practice most pure-Rust crates need
  nothing.

---

## Why a standalone `std` (the Go "no-cgo" model)

fullrust gives the standard library its **own platform backend on Linux
syscalls**, instead of routing it through the platform libc. This is the same
architecture as the **Go runtime**: Go issues syscalls itself and only touches
libc when you opt into cgo. fullrust is, in effect, *Rust with cgo off* — the
binary's only boundary to the outside world is the `syscall` instruction. It is
still the real `std`, so ordinary crates compile unchanged.

The deciding principle is **keeping Rust's guarantees end-to-end, all the way to
the kernel** — which a pure-Rust *libc shim* (the Eyra / `c-scape` model) gives
up at a C-shaped wall in the middle of every OS interaction:

- **Zero-cost abstraction survives only without the wall.** `io::Write::write` →
  `syscall` inlines and monomorphizes end to end. An `extern "C"` libc seam is
  opaque to the optimizer — it can't be inlined or specialized across, so you
  forfeit exactly that benefit at exactly the wrong place.
- **Rust types all the way down.** I/O returns `Result`; buffers are slices;
  errors are values — not raw `*mut u8`, `c_int`, NUL-terminated strings and a
  thread-local `errno`, with a double-translation tax on every call.
- **A tiny `unsafe` surface instead of a vast one.** The `unsafe` is the handful
  of `syscall` sites, not an entire glibc-shaped ABI (symbol versioning, struct
  layouts, the TLS model, the `__libc_start_main` handshake) that a shim must
  reproduce exactly or hit UB.

The one accepted cost, like Go: programs target fullrust's std rather than being
able to link arbitrary C libraries. (Pure `no_std + alloc` crates still work
as-is; only code that needs OS services *through* libc is excluded — on purpose.)

---

## How it's built (internals)

fullrust is a source overlay on the Rust compiler that adds the built-in
`x86_64-unknown-linux-fullrust` target and a `std::sys` backend on raw syscalls
(the allocator, native TLS, threads, fs/net/process, the pure-Rust unwinder and
backtraces), packaged as a thin Docker image around the prebuilt toolchain. It
tracks released Rust versions (currently 1.88–1.95).

See [`toolchain/README.md`](toolchain/README.md) for the design, the
syscall-backed platform layer, and how the overlay is built and ported across
Rust versions.

## License

MIT OR Apache-2.0.
