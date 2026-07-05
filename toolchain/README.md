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

**Polish closing the remaining gaps:** `strerror`-quality `io::Error` messages
(errno→text table, since there's no libc `strerror`), `is_terminal` via
`ioctl(TCGETS)`, `env::split_paths`/`join_paths`/`home_dir` (`$HOME`),
`thread::set_name` via `prctl(PR_SET_NAME)`, **real vectored I/O** (`readv`/
`writev` for `File`/`TcpStream`/pipes — `IoSlice` now lowers to a kernel `iovec`),
and **proper `SIGPIPE` handling**: startup honors the compiler's
`-Zon-broken-pipe` directive (default `SIG_IGN` so a write to a closed peer
returns `EPIPE`; `kill`→`SIG_DFL`, `inherit`, `error` all respected), and the
`fork`+`execve` child restores `SIG_DFL` before exec so spawned Unix tools
terminate on a broken pipe as expected. Covered by `test-osextra`.

Extra changes beyond the target spec / pal: `build.rs` (check-cfg + restricted_std
allowlist), `cc_detect.rs` (probe via the gnu triple), `compile.rs`
(`compiler-builtins-mem`), the `sys/{stdio,args,random,alloc,thread_local}`
dispatchers, `fullrust` branches in the `sys/{fs,net,process,env,path,io}` +
`sys/sync/*` + `thread_local` guards, and a real fd-backed `pal::pipe`.

The allocator now has **three paged tiers** plus a huge fallback: small (≤8 KiB,
64 KiB pages), **medium** (8–128 KiB, 512 KiB pages) and **large** (128–512 KiB,
whole-4 MiB page), all reusing the same sharded free-list / cross-thread / reclaim
machinery parameterized by segment *kind*, with a committed empty-segment cache
(≈14–24 ns/op alloc+free churn vs ~5 µs for the old per-op `mmap`). **Over-aligned**
requests (`align > 16`) route to power-of-two classes; only `>512 KiB` or
un-fittable over-alignment take a dedicated `mmap`. Free stays header-free (masks
to the segment and reads its stored kind). A thread-exit scavenge folds and unmaps
abandoned segments emptied by cross-thread frees. Design chosen and reviewed via
multi-agent design panel + adversarial correctness review.

### Known limitations (next steps)
- **Allocator**: `>512 KiB` and un-fittable over-aligned allocations still take a
  dedicated `mmap` each (a cached-mmap magazine for that band is future work).
  Abandoned segments are reclaimed on demand or at any thread exit; a segment of a
  kind that is never re-allocated *and* with no further thread exits can linger
  (bounded, mimalloc-style).
**Diagnostics.** Despite `panic = abort` and no unwinder, panic **backtraces**
work: a frame-pointer stack walker (`backtrace/src/backtrace/frameptr.rs`, the
target forces `frame_pointer: Always`; `_start` and the thread trampoline zero
`%rbp` to terminate the chain) feeds gimli/DWARF symbolization that reads
`/proc/self/exe` (a `native_libraries` loader returns the static non-PIE image at
bias 0; the `std::fs`-based `mmap_fake` is used since there is no libc `mmap`), so
`RUST_BACKTRACE=1` and `std::backtrace::Backtrace` yield names + file:line. A
**stack-overflow handler** (`sys::pal::fullrust::stack_overflow`) installs a
`SIGSEGV`/`SIGBUS` handler on a per-thread alternate signal stack (raw
`rt_sigaction` with a restorer trampoline + `sigaltstack`); spawned-thread stacks
get an `mprotect(PROT_NONE)` guard page and the main thread's guard is derived
from `RLIMIT_STACK`, so an overflow prints `thread '…' has overflowed its stack`
and aborts while a genuine segfault passes through unchanged.

## Version matrix

The goal is to track `1.88 … 1.96` and observe how `std::sys::pal` shifts across
versions (the pal interface is deliberately unstable upstream). Each `rust-<v>`
checkout is gitignored; the overlay of source changes above is what we carry
forward and re-apply per version.
