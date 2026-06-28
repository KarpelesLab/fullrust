# fullrust toolchain — forking Rust for a libc-free static target

This directory builds a **fork of the Rust toolchain** that adds a built-in
target, `x86_64-unknown-linux-fullrust`, whose `std` is implemented on raw Linux
syscalls (no libc, no C runtime, no dynamic linker). With this toolchain an
**unmodified** crate (`fn main`, `use std::…`, no attributes, no deps changes)
compiles to a single statically-linked, libc-free ELF — on a *pinned released
version* of Rust, not a floating nightly.

This is the "ship a toolchain fork" path (Approach A): a real `std::sys::pal`
backend, which is also the artifact that could eventually be upstreamed as a
Tier-3 target.

## Why a distinct `target_os = "fullrust"`

std selects its platform layer in `library/std/src/sys/pal/mod.rs` with
`#[cfg(unix)]` **first**. Any `unix`-family target is forced onto the libc-based
`unix` pal. To own our pal we must therefore be a *non-unix* OS — exactly how
`hermit`/`wasi`/`uefi`/`sgx` do it. So:

- Triple name: `x86_64-unknown-linux-fullrust` (the `linux` signals the kernel ABI).
- `os = "fullrust"`, **not** in the `unix` family → std falls through to our pal,
  and to the `unsupported` shims for everything we haven't implemented yet.
- `llvm_target = "x86_64-unknown-linux-gnu"` → codegen, TLS model and syscall ABI
  are byte-for-byte the normal Linux x86-64 ones.

The kernel-facing code is reused from [`purestd`](../../purestd) (the raw-syscall
layer + arch files); std's `File`/`TcpStream`/etc. are built on top of the pal
primitives, so we port the *bottom* of purestd, not its high-level modules.

## Changes applied to a clean Rust checkout

Compiler (target registration):
- `compiler/rustc_target/src/spec/base/fullrust.rs` — **new**: base `TargetOptions`
  (libc-free, static, `panic=abort`, `rust-lld`, not unix family).
- `compiler/rustc_target/src/spec/base/mod.rs` — register `mod fullrust;`.
- `compiler/rustc_target/src/spec/targets/x86_64_unknown_linux_fullrust.rs` — **new**: the target.
- `compiler/rustc_target/src/spec/mod.rs` — list it in `supported_targets!`.

Bootstrap:
- `src/bootstrap/src/utils/cc_detect.rs` — probe C compilers for the fullrust
  target using the equivalent `…-linux-gnu` triple (cc-rs can't parse the
  `fullrust` env). The compiler is recorded but never used — the target is C-free.
- `bootstrap.toml` — build config (see below).

std (the pal backend) — *in progress*:
- `library/std/src/sys/pal/fullrust/` — **new**: the platform layer (`_start`,
  process exit/abort, stdio, time, args, …), delegating to raw syscalls.
- `library/std/src/sys/pal/mod.rs` and the `sys/{stdio,args,alloc,random,…}`
  dispatchers — add a `fullrust` branch (real where implemented, else `unsupported`).

## bootstrap.toml notes

- `download-ci-llvm = false` — CI LLVM artifacts expire for old release tags
  (404), so LLVM is built from source once (`targets = "X86"` to keep it lean).
- `optimized-compiler-builtins = false` — use pure-Rust intrinsics; avoids cc-rs
  and keeps the target genuinely C-free.
- `rust.lld = true`, `llvm-tools = true` — ship `rust-lld` (the target's linker).

## Building

The fork's source changes are kept **out of tree** as a patch overlay so they
survive independently of the (gitignored) checkout:

- `patches/fullrust-<minor>.patch` — the full source overlay (target spec, pal,
  and the `build.rs`/`cc_detect`/`compile.rs`/dispatcher edits).
- `bootstrap-<minor>.toml` — the build config.

Automated flow (clone → apply overlay → build):

```console
./build-fork.sh 1.88.0          # clones rust-1.88, applies overlay, builds
./link-and-test.sh 1.88         # links the toolchain, builds + runs test-hello
```

Manual equivalent:

```console
cd rust-1.88
BOOTSTRAP_SKIP_TARGET_SANITY=1 \
  python3 x.py build library --target x86_64-unknown-linux-fullrust --stage 1
rustup toolchain link fullrust-1.88 build/x86_64-unknown-linux-gnu/stage1
cargo +fullrust-1.88 build --target x86_64-unknown-linux-fullrust --release
```

`BOOTSTRAP_SKIP_TARGET_SANITY=1` is needed because the downloaded stage0 compiler
predates the new target.

To regenerate the overlay after editing the rust tree:

```console
cd rust-1.88 && git add -A -- compiler/rustc_target library/std src/bootstrap \
  && git diff --cached HEAD > ../patches/fullrust-1.88.patch && git reset -q
```

## Status

**Rust 1.88 — milestone 1 + threads + native TLS + fast allocator.** An
unmodified crate (`fn main`, `use std`, `println!`, `BTreeMap`, `env::args`)
builds with the fork and runs as a **statically-linked, libc-free ELF** (~44 KB):
no `INTERP` segment, no `NEEDED` entries, no libc/GLIBC symbols; `strace` shows
raw `write`/`exit_group` syscalls.

**Threads work too.** `thread::spawn`/`join`, `Arc<Mutex>` under contention,
and `thread_local!` all run correctly on a static libc-free binary: an 8-thread
× 100k-iteration contended-counter test lands 800000/800000 with per-thread TLS
verified, `available_parallelism()` reports the real CPU count.

**Native `%fs` TLS.** Thread-locals are real ELF `#[thread_local]` (local-exec
model), not a registry: the pal sets up a variant-II TCB and installs `%fs` for
the main thread (`arch_prctl` after parsing `PT_TLS` from the auxv) and for
spawned threads (`CLONE_SETTLS`), so a thread-local access is a single `mov`.
`has_thread_local = true`; the old `(tid,key)` `BTreeMap` registry is gone.
Thread-local **destructors now run** at thread exit (the pal calls
`destructors::run` + `rt::thread_cleanup`, hermit-style). See
`sys/pal/fullrust/tls.rs`.

**mimalloc-class allocator.** The `System` allocator is a per-thread, free-list-
sharded heap (`sys/pal/fullrust/heap.rs`): 4 MiB aligned segments sliced into
64 KiB size-class pages, a lock-/atomic-free owner fast path reached in two loads
off `%fs`, atomic `thread_free` lists for cross-thread frees, abandoned-segment
reclamation on thread exit, and OS purging via `madvise(MADV_FREE)`/`munmap`.
Free is routed entirely by `Layout` (no per-allocation header). Versus the old
page-per-mmap allocator on a 32-core box: **~235× faster** single-thread
(2879 → 12 ns/op), near-linear multicore scaling (0.16 → ~1800 M ops/s at 32
threads), and fragmentation overhead cut 1.43× → 1.08×. A debug-assertion-gated
integrity check rejects double/foreign/use-after-frees at zero release cost.

Implemented `std::sys::pal::fullrust`: `_start`, raw x86-64 syscalls, process
exit/abort, `clock_gettime` time, sleep/yield, args (lossy-UTF8), stdio
read/write, `getrandom`, the **mimalloc-class allocator** above, **threads**
(`clone(2)` musl-style trampoline + `CLONE_CHILD_CLEARTID` join handshake),
**futex-based sync** (Mutex/Condvar/RwLock/Once/parking), and **native `%fs`
thread-locals**. `strlen` is provided in the pal; `mem*` come from
`compiler-builtins-mem` (enabled for the target in `std_cargo`).

**Full std subsystems on raw syscalls** (no libc): `std::env` (loader-`envp`
backed vars + cwd + `current_exe`), `std::fs` (`File`/metadata via `statx`/dir
iteration via `getdents64`/the `*at` family — read, write, seek, permissions,
symlinks, hard links, `canonicalize`, copy, file locks, recursive removal),
`std::process` (`Command` via `fork`+`execve` with `PATH` search, pipe stdio,
concurrent stdout/stderr capture, `wait4` exit status, signals), and `std::net`
(TCP/UDP on the socket syscalls + a self-contained DNS resolver: `/etc/hosts` +
`/etc/resolv.conf` + UDP A/AAAA queries). Each has a dedicated `test-*` crate.

Extra changes beyond the target spec / pal: `build.rs` (check-cfg + restricted_std
allowlist), `cc_detect.rs` (probe via the gnu triple), `compile.rs`
(`compiler-builtins-mem`), the `sys/{stdio,args,random,alloc,thread_local}`
dispatchers, `fullrust` branches in the `sys/{fs,net,process,env,path}` +
`sys/sync/*` + `thread_local` guards, and a real fd-backed `pal::pipe`.

### Known limitations (next steps)
- **Allocator**: large (>8 KiB) and over-aligned allocations take a dedicated
  `mmap` each (correct, but not cached); a medium/large segment tier and finer
  decommit hysteresis are future work. Abandoned segments are reclaimed on
  demand; never-reclaimed ones stay mapped (bounded, mimalloc-style).

## Version matrix

The goal is to track `1.88 … 1.96` and observe how `std::sys::pal` shifts across
versions (the pal interface is deliberately unstable upstream). Each `rust-<v>`
checkout is gitignored; the overlay of source changes above is what we carry
forward and re-apply per version.
