# fullrust

**Fully-static, libc-free, pure-Rust Linux binaries.**

`fullrust` is a `no_std` runtime plus its own standard library
([`fullrust-std`](crates/fullrust-std/)) that let you write ordinary-looking Rust
programs — with `println!`, `Vec`, `String`, `format!`, command-line arguments,
files, threads and sockets — and compile them into Linux ELF executables that
link **no libc and no C runtime at all**. Like the Go runtime, it talks to the
kernel directly through raw `syscall` instructions (see
[Why a standalone `std`](#why-a-standalone-std-the-go-no-cgo-model)), and is
linked with the Rust-bundled LLVM linker, so **no C toolchain is involved** in
the build.

An ordinary crate — **no fullrust dependency, no attributes, plain `fn main`** —
builds into a libc-free static binary:

```rust
// src/main.rs
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
$ cargo install cargo-fullrust
$ cargo fullrust build --release
$ file    target/x86_64-fullrust-linux/release/wordcount
ELF 64-bit LSB executable, x86-64, statically linked
$ ldd     target/x86_64-fullrust-linux/release/wordcount
        not a dynamic executable
$ readelf -d target/x86_64-fullrust-linux/release/wordcount
There is no dynamic section in this file.
```

There is no interpreter (`PT_INTERP`), no `.dynamic` section, and zero `NEEDED`
libraries. The only thing the binary needs to run is the Linux kernel.

A direct consequence — and an intended guard-rail — is that **any program that
tries to FFI into a dynamic library, or call an unprovided C symbol, fails at
link time**. Pure-Rust code links; anything reaching for libc does not.

---

## Quick start

The default (zero-touch) path needs a nightly toolchain with `rust-src`; the
`--stable` explicit-runtime path needs only stable:

```console
rustup toolchain install nightly
rustup component add rust-src --toolchain nightly
```

### Zero-touch: `cargo fullrust` on an unmodified crate

Install the subcommand once, then build a **completely ordinary** crate — plain
`fn main`, `use std::…`, no dependency on fullrust, no attributes — into a
libc-free static binary:

```console
cargo install cargo-fullrust
```
```rust
// src/main.rs — nothing fullrust-specific
use std::collections::BTreeMap;
fn main() {
    let mut m = BTreeMap::new();
    *m.entry("hi").or_insert(0) += 1;
    println!("{m:?}");
}
```
```console
cargo fullrust build --release       # -> target/x86_64-fullrust-linux/release/<bin>
cargo fullrust run -- arg1
```

How: `cargo fullrust` builds (and caches) a **sysroot whose `std` is fullrust's
own standard library**, then compiles your crate against it. Your `use std::…`
resolves to the syscall-backed std, and the binary links no libc. Needs a
nightly toolchain with `rust-src`
(`rustup component add rust-src --toolchain nightly`). Coverage is whatever
[`fullrust-std`](crates/fullrust-std/) implements — gaps surface as ordinary
"not found in `std`" errors as the stdlib grows.

### GitHub Action

Build static artifacts in CI with the reusable action (see
[`action.yml`](action.yml)). To keep them as workflow artifacts:

```yaml
- uses: KarpelesLab/fullrust@master
  with:
    working-directory: .       # your crate
    args: --release
- uses: actions/upload-artifact@v4
  with:
    path: target/x86_64-fullrust-linux/release/<your-bin>
```

#### Attach a static binary to a GitHub Release

On a tag (or a published release), build with the action and upload the binary
as a release asset. Note the **`contents: write`** permission:

```yaml
name: Release
on:
  push:
    tags: ["v*"]            # or: release: { types: [published] }

permissions:
  contents: write           # required to upload release assets

jobs:
  static-linux:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6

      - name: Build the libc-free static binary
        uses: KarpelesLab/fullrust@master
        with:
          args: --release --bin myapp     # your binary

      - name: Package as a .tar.gz
        run: |
          name="myapp-${{ github.ref_name }}-x86_64-linux-static"
          mkdir -p "dist/$name"
          cp target/x86_64-fullrust-linux/release/myapp "dist/$name/"
          cp README.md LICENSE* "dist/$name/" 2>/dev/null || true
          tar -C dist -czf "dist/$name.tar.gz" "$name"

      - name: Attach to the release
        uses: softprops/action-gh-release@v2
        with:
          files: dist/*.tar.gz
```

`softprops/action-gh-release` creates/updates the release for the tag and
uploads the archive; with the `release: published` trigger it attaches to the
existing release instead. (Equivalent with the CLI:
`gh release upload "$TAG" dist/*.tar.gz`.)

The output path is `target/x86_64-fullrust-linux/release/<bin>` relative to the
crate's workspace root (so if you set `working-directory:`, the binary still
lands under that workspace's `target/`).

### Explicit-runtime crates: `--runtime`

If you'd rather opt in directly (smaller surface, works without the sysroot),
write a `no_std` crate against the runtime and pass `--runtime`:

```toml
[dependencies]
fullrust = "0.1"
```
```rust
#![no_std]
#![no_main]
use fullrust::prelude::*;
fn main() { println!("hello from libc-free rust"); }
fullrust::entry!(main);
```
```console
cargo fullrust --runtime build --release   # build-std (nightly)
cargo fullrust --stable build              # precompiled core/alloc, no nightly
```

### Working inside this repo: `./x`

The `./x` wrapper is the in-repo equivalent of `cargo fullrust` (it predates the
subcommand and is what CI uses):

```console
./x build                      # all examples, stable path
./x run -p hello               # build + run one example
./x run -p args -- a b c       # pass arguments
./x build --nightly --release  # smaller binaries via build-std
```

Examples live in [`examples/`](examples/): `hello`, `args`, `alloc-demo`,
`panic-demo`, `std-smoke`.

---

## Why a standalone `std` (the Go "no cgo" model)

fullrust ships **its own standard library** — [`fullrust-std`](crates/fullrust-std/) —
implemented directly on Linux syscalls, instead of reusing the platform libc or
the upstream `std` that sits on top of it. This is the same architecture as the
**Go runtime**: Go issues syscalls itself and only touches libc when you opt into
cgo. fullrust is, in effect, *Rust with cgo off* — programs are written against
fullrust's std, and the binary's only boundary to the outside world is the
`syscall` instruction.

There are three ways one could get a libc-free Rust, and we deliberately chose
the first:

1. **Own std (what fullrust does).** A bespoke standard library on raw syscalls.
   Crates opt in (`#![no_std]` + the `std` alias; `cargo fullrust` drives the
   build). Purest binaries, full control.
2. **Port upstream `std`'s platform backend.** Add a freestanding `sys` backend
   inside Rust's own `library/std` (as the SGX/UEFI/Hermit targets do) and
   `build-std` the real std. Truly "it *is* `std`," but means maintaining a fork
   of the standard library pinned to a nightly — large and perpetual.
3. **A pure-Rust libc (the Eyra / `c-scape` model).** Keep the real `std` and
   satisfy the libc symbols it calls with Rust implementations.

### Why own std, and explicitly *not* a libc shim

The deciding principle is **keeping Rust's guarantees end-to-end, all the way to
the kernel.** Routing the standard library through a libc-shaped ABI (option 3)
gives that up at a C-shaped wall in the middle of every OS interaction:

- **Zero-cost abstraction survives only without the wall.** In own-std,
  `io::Write::write` → `syscall` *inlines and monomorphizes end to end* — Rust's
  signature zero-cost-abstraction property, intact on the hot path. An
  `extern "C"` libc seam is **opaque to the optimizer**: it cannot be inlined or
  specialized across, so you forfeit exactly that benefit at exactly the wrong
  place.
- **Rust types all the way down.** Our I/O returns `Result` / `Errno`; buffers
  are slices with lengths; errors are values. A libc seam forces C shapes — raw
  `*mut u8`, `c_int`, NUL-terminated strings, `struct stat`, and `errno` as a
  thread-local int — with a double-translation tax on every call.
- **A tiny `unsafe` surface instead of a vast one.** A pure-Rust libc is almost
  entirely `unsafe`: it *implements* a raw-pointer ABI and must reproduce
  glibc's quirks exactly — symbol versioning (`memcpy@GLIBC_2.14`), struct
  layouts, `__errno_location`, the TLS model, `environ`, the `__libc_start_main`
  handshake. Any subtle mismatch is undefined behavior. own-std's only `unsafe`
  is the handful of `syscall` sites plus a few compiler-mandated leaf symbols
  (`memcpy`, `_start`) — not an entire operating-system interface.

A clarification, to be fair to option 3: the `std → libc` boundary *already
exists* in normal Rust (std reaches glibc through `unsafe extern "C"`). So a libc
shim would not add unsafe FFI to *your* code, and a plain `extern "C"` call into
a Rust function is cheap (nothing like cgo's stack-switch cost). What you lose is
narrower but real: **the optimizer goes blind at the seam, the API degrades to C
shapes there, and you take on a large unsafe ABI layer to build and maintain.**
For "the purest possible binaries" on code you control, that trade isn't worth
it — so fullrust keeps Rust idioms and optimization unbroken from `main` down to
the `syscall`, and accepts the one cost of the own-std model: like Go, programs
target fullrust's std rather than running arbitrary upstream-`std` crates
unmodified. (Pure `no_std + alloc` libraries from crates.io still work as-is;
only crates that need OS services *through* `std` need to target fullrust's.)

---

## The method

A normal Rust binary starts life in libc's `crt0`: the kernel jumps to libc's
`_start`, which sets up the C runtime and eventually calls your `main`. Removing
libc means we have to supply everything that machinery provided. There turn out
to be only a handful of pieces.

### 1. The entry point (`_start`)

On `execve`, the Linux kernel transfers control to the ELF entry symbol
`_start` with the stack laid out as:

```text
rsp ->  argc
        argv[0] … argv[argc-1], NULL
        envp[0] … envp[m-1],   NULL
        auxv …
```

There is no return address. fullrust defines `_start` as a *naked* function
(no prologue/epilogue) that captures `rsp`, aligns the stack as the SysV ABI
requires, parses `argc`/`argv`/`envp`, and calls `main` — see
[`arch/x86_64.rs`](crates/fullrust/src/arch/x86_64.rs) and
[`start.rs`](crates/fullrust/src/start.rs).

There are two ways `main` gets wired up:

* **Zero-touch (default).** The sysroot `std` provides the `start` lang item, so
  an ordinary `fn main` links exactly as it would under real std — the
  compiler-generated entry calls our `lang_start`.
* **`--runtime`.** A `no_std` crate exports its `main` as `__fullrust_main` via
  the [`entry!`](crates/fullrust/src/lib.rs) macro, which `_start` calls
  directly.

Either way the process ends with `exit_group`.

### 2. Syscalls instead of libc

Every interaction with the outside world is a raw `syscall`. The instruction
wrappers (`syscall0`…`syscall6`) and the syscall-number table are the only
architecture-specific code; everything else builds on the arch-neutral,
`Result`-returning wrappers in [`syscall.rs`](crates/fullrust/src/syscall.rs)
(`read`, `write`, `open`, `close`, `mmap`, `munmap`, `getrandom`,
`exit_group`, …).

### 3. Symbols the compiler still expects

Even pure Rust expects a few free-standing symbols that libc normally provides.
fullrust supplies them in [`intrinsics.rs`](crates/fullrust/src/intrinsics.rs):

* **`memcpy` / `memmove` / `memset` / `memcmp` / `bcmp`** — the compiler lowers
  struct copies, slice fills, comparisons, etc. to these. (They are simple
  byte loops; LLVM's loop-idiom pass deliberately won't rewrite a loop *inside*
  `memcpy` into a call to `memcpy`, so there is no self-recursion.)
* **`strlen`** — used by `core::ffi::CStr::from_ptr`, which we use to read the
  NUL-terminated `argv`/`envp` strings.
* **`rust_eh_personality` / `_Unwind_Resume`** — unwinding hooks. We build with
  `panic = "abort"`, so unwinding never happens and these are abort-stubs that
  are never actually executed (see the panic discussion below).

### 4. A heap (`alloc`)

To get `Box`, `Vec`, `String` and `format!`, fullrust provides a
`#[global_allocator]`: a small **mmap-backed segregated free-list** allocator
(see [`allocator.rs`](crates/fullrust/src/allocator.rs)). Requests up to 64 KiB
are rounded to a size class and served from per-class free lists carved out of
1 MiB `mmap` arenas; larger requests get their own `mmap`/`munmap`. It is
`Sync` via a spinlock guarding the arena state.

### 5. Panics

The binary needs exactly one `#[panic_handler]`. fullrust's
(in [`panic.rs`](crates/fullrust/src/panic.rs)) prints the message and source
location to stderr and calls `exit_group(134)` (mimicking `128 + SIGABRT`).
Because we compile with `panic = "abort"`, panicking never unwinds, so the
unwind stubs from (3) are never reached.

> The binary-policy symbols — `_start`, the `#[panic_handler]`, and the
> `#[global_allocator]` static — are gated behind fullrust's default `rt`
> feature. The `--runtime`/`entry!` model leaves `rt` on so they come from
> `fullrust`; the zero-touch sysroot `std` turns `rt` off and supplies them
> itself (you can only have one of each per binary). The *mechanisms* — the
> syscalls, the `Allocator` type, the mem intrinsics — are always present.

### 6. Linking with no C runtime

The build invokes the LLVM linker (`rust-lld`) **directly** as `ld.lld`, rather
than through a C compiler driver. That means no `crt0`, no implicit `-lc`, no
`cc` — just our object files plus `core`/`alloc`. Combined with
`-relocation-model=static` and `-static`, the result is a position-dependent,
fully static ELF with no dynamic section. (`rust-lld`'s generic driver picks
GNU/ELF mode from the name `ld.lld`; the bare name `rust-lld` does not, which is
why `./x` points at the `gcc-ld/ld.lld` shim.)

---

## Build modes

`cargo fullrust` has three ways to produce a binary; all are genuinely libc-free
and fully static.

| Mode | Crate looks like | Toolchain | How |
|---|---|---|---|
| **zero-touch** (default) | unmodified `fn main`, `use std`, no deps | nightly + `rust-src` | sysroot whose `std` is fullrust's, built once and cached; your crate compiles with `--sysroot` |
| **`--runtime`** | `#![no_std]` + `fullrust::entry!` | nightly + `rust-src` | `-Z build-std` recompiles core/alloc; `fullrust` supplies the runtime |
| **`--stable`** | `#![no_std]` + `fullrust::entry!` | stable | precompiled core/alloc from the sysroot; our abort-stubs satisfy `alloc`'s unwind references |

The `--runtime`/`--stable` paths use a **different target triple per path** so
the freestanding linker flags never touch host build scripts (`./x` mirrors this
in-repo). On a release build a trivial program is well under 10 KiB.

### Why a custom target on nightly?

`-Z build-std` recompiles `compiler_builtins`, whose **build script** must
compile for the *host* (with the system `cc` and libc). If our freestanding
crates and that host build script shared a target triple, cargo would apply the
`-static`/`ld.lld` flags to the build script too and it would fail to link.
Giving the nightly path its own triple (`x86_64-fullrust-linux`) keeps host
artifacts on the normal `-gnu` host triple with the system linker, while our
code builds freestanding. The stable path has no build scripts in its graph, so
it can stay on the plain `-gnu` triple.

---

## Extending

### Add a syscall

1. Add its number to `pub mod nr` in
   [`arch/x86_64.rs`](crates/fullrust/src/arch/x86_64.rs).
2. Add a safe wrapper in [`syscall.rs`](crates/fullrust/src/syscall.rs) using
   the right `syscallN` and `from_ret` for error handling.

### Add an architecture

All CPU/ABI-specific code is confined to `arch/`. To port to, say, `aarch64`:

1. Add `crates/fullrust/src/arch/aarch64.rs` providing `syscall0…6` (the
   `svc #0` sequence, args in `x0…x5`, number in `x8`), the naked `_start`
   (stack pointer arrives in `x0`/`sp`), and the `nr` table for that ABI.
2. Wire it up with a `#[cfg(target_arch = "aarch64")]` arm in
   [`arch/mod.rs`](crates/fullrust/src/arch/mod.rs).
3. Add an `aarch64-fullrust-linux.json` target spec (next to the x86_64 one in
   `crates/cargo-fullrust/`) for the nightly path.

The rest of the crate is written against `arch::syscallN` and `arch::nr` only.

---

## Layout

```
crates/fullrust/         the runtime
  src/arch/x86_64.rs       syscall asm, naked _start, syscall numbers  (only arch-specific file)
  src/syscall.rs           arch-neutral Result-returning syscall wrappers
  src/start.rs             argc/argv/envp parsing -> user main -> exit
  src/intrinsics.rs        mem*/strlen + unwind abort-stubs
  src/allocator.rs         mmap-backed segregated free-list global allocator
  src/panic.rs             the #[panic_handler]
  src/io.rs                Fd, fmt::Write, print!/println!/eprint!/eprintln!
  src/env.rs               args()/vars()/var()
  src/rt.rs                Termination, exit/abort
  src/prelude.rs           glob import for programs
  src/lib.rs               crate root + entry!() macro
crates/fullrust-std/     the standard library (own-std, on syscalls)
  src/{io,fs,net,time,env,process,thread,sync,...}.rs
                           std-shaped modules backed by fullrust syscalls
crates/cargo-fullrust/   the `cargo fullrust` subcommand
  src/main.rs              builds/caches the sysroot, drives cargo
  sysroot/std_lib.rs       the sysroot `std` (re-exports fullrust-std + policy)
  x86_64-fullrust-linux.json   freestanding target spec (cargo-fullrust + ./x)
examples/                hello, args, alloc-demo, panic-demo, std-smoke (entry!);
                           plain (zero-touch demo: ordinary crate, no fullrust dep)
action.yml               reusable GitHub Action (build static artifacts in CI)
x                        in-repo build wrapper (linker resolution + path selection)
.cargo/config.toml       intentionally minimal — see ./x
```

---

## Limitations

* **Linux + x86-64 only** today (the design isolates arch-specific code; see
  [Extending](#extending)).
* **Threads work** (`clone`-backed, futex `join`), but the standard library is
  still maturing toward production quality — current rough edges: `Mutex`/
  `RwLock` are spinlocks rather than futex-blocking, `thread_local!` is a single
  shared slot rather than real per-thread TLS, there's no `Condvar` or
  `process::Command` yet, and the DNS resolver does plain DNS + `/etc/hosts`
  (no NSS). These are being filled in.
* **No dynamic linking, by design.** FFI into a `.so` cannot link.
* **`panic = "abort"` is mandatory** — it's what lets us drop the unwinder.
* The allocator is deliberately simple (no cross-class coalescing). It is
  correct and fine for typical workloads, not a general-purpose `malloc`.
* Build through `cargo fullrust` (or `./x` in-repo); a bare `cargo build` won't
  have the linker/target wiring.

## License

MIT OR Apache-2.0.
