# fullrust

**Fully-static, libc-free, pure-Rust Linux binaries.**

`fullrust` is a small `no_std` runtime that lets you write ordinary-looking Rust
programs — with `println!`, `Vec`, `String`, `format!`, command-line arguments
and environment variables — and compile them into Linux ELF executables that
link **no libc and no C runtime at all**. The program talks to the kernel
directly through raw `syscall` instructions, and is linked with the
Rust-bundled LLVM linker, so **no C toolchain is involved** in the build.

```rust
#![no_std]
#![no_main]

use fullrust::prelude::*;

fn main() {
    println!("hello from libc-free rust");
}
fullrust::entry!(main);
```

```console
$ ./x run -p hello
hello from libc-free rust

$ file target/x86_64-unknown-linux-gnu/debug/hello
ELF 64-bit LSB executable, x86-64, statically linked
$ ldd  target/x86_64-unknown-linux-gnu/debug/hello
        not a dynamic executable
$ readelf -d target/x86_64-unknown-linux-gnu/debug/hello
There is no dynamic section in this file.
```

There is no interpreter (`PT_INTERP`), no `.dynamic` section, and zero `NEEDED`
libraries. The only thing the binary needs to run is the Linux kernel.

A direct consequence — and an intended guard-rail — is that **any program that
tries to FFI into a dynamic library, or call an unprovided C symbol, fails at
link time**. Pure-Rust code links; anything reaching for libc does not.

---

## Quick start

You need a recent stable Rust toolchain. The nightly path additionally needs a
nightly toolchain with the `rust-src` component:

```console
rustup component add rust-src --toolchain nightly
```

Build and run through the `./x` wrapper (it resolves the linker and selects the
build strategy — see [Two build paths](#two-build-paths)):

```console
./x build                      # all examples, stable path
./x run -p hello               # build + run one example
./x run -p args -- a b c       # pass arguments
./x build --nightly --release  # smaller binaries via build-std
```

Examples live in [`examples/`](examples/): `hello`, `args`, `alloc-demo`,
`panic-demo`.

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
requires, and calls into Rust — see
[`arch/x86_64.rs`](crates/fullrust/src/arch/x86_64.rs). The Rust bootstrap in
[`start.rs`](crates/fullrust/src/start.rs) parses `argc`/`argv`/`envp`, then
calls your `main` (exported as `__fullrust_main` by the
[`entry!`](crates/fullrust/src/lib.rs) macro) and finally `exit_group`s with the
returned code.

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

To get `Box`, `Vec`, `String` and `format!`, fullrust installs a
`#[global_allocator]`: a small **mmap-backed segregated free-list** allocator
(see [`allocator.rs`](crates/fullrust/src/allocator.rs)). Requests up to 64 KiB
are rounded to a size class and served from per-class free lists carved out of
1 MiB `mmap` arenas; larger requests get their own `mmap`/`munmap`. It is
`Sync` via a spinlock (fullrust programs are single-threaded, so it is
effectively uncontended).

### 5. Panics

A `no_std` crate must define exactly one `#[panic_handler]`. fullrust's
(in [`panic.rs`](crates/fullrust/src/panic.rs)) prints the message and source
location to stderr and calls `exit_group(134)` (mimicking `128 + SIGABRT`).
Because we compile with `panic = "abort"`, panicking never unwinds, so the
unwind stubs from (3) are never reached.

### 6. Linking with no C runtime

The build invokes the LLVM linker (`rust-lld`) **directly** as `ld.lld`, rather
than through a C compiler driver. That means no `crt0`, no implicit `-lc`, no
`cc` — just our object files plus `core`/`alloc`. Combined with
`-relocation-model=static` and `-static`, the result is a position-dependent,
fully static ELF with no dynamic section. (`rust-lld`'s generic driver picks
GNU/ELF mode from the name `ld.lld`; the bare name `rust-lld` does not, which is
why `./x` points at the `gcc-ld/ld.lld` shim.)

---

## Two build paths

The same source compiles two ways. `./x` picks one; they use **different target
triples** so the freestanding linker flags never touch host build scripts.

| | `--stable` (default) | `--nightly` |
|---|---|---|
| Toolchain | stable | nightly + `rust-src` |
| `core`/`alloc` | precompiled, from the sysroot | recompiled from source (`-Z build-std`) |
| Target triple | `x86_64-unknown-linux-gnu` | `x86_64-fullrust-linux` (custom JSON) |
| Unwinding | precompiled `alloc` carries unwind tables (unused under `panic=abort`); our abort-stubs satisfy the references | none emitted at all |
| `mem*` intrinsics | supplied by `fullrust` | supplied by `fullrust` |
| Binary size | small (release strips the unused tables); larger in debug | smallest |
| Best for | zero extra setup, works anywhere stable does | minimal binaries |

Both produce genuinely libc-free static binaries. On a release build a trivial
program is well under 10 KiB either way.

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
3. Add a `targets/aarch64-fullrust-linux.json` for the nightly path.

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
examples/                hello, args, alloc-demo, panic-demo
targets/                 custom freestanding target spec(s) for the nightly path
x                        build wrapper (linker resolution + path selection)
.cargo/config.toml       intentionally minimal — see ./x
```

---

## Limitations

* **Linux + x86-64 only** today (the design isolates arch-specific code; see
  [Extending](#extending)).
* **No threads, no TLS.** The allocator's lock and the single-threaded
  assumption would need revisiting for `clone`-based threads.
* **No dynamic linking, by design.** FFI into a `.so` cannot link.
* **`panic = "abort"` is mandatory** — it's what lets us drop the unwinder.
* The allocator is deliberately simple (no cross-class coalescing). It is
  correct and fine for typical workloads, not a general-purpose `malloc`.
* Build through `./x`; a bare `cargo build` won't have the linker wiring.

## License

MIT OR Apache-2.0.
